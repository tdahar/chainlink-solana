use anchor_lang::prelude::*;

pub const POOL_VERSION: u8 = 1;

pub const POOL_SEED: &[u8] = b"staking_pool";
pub const VAULT_SEED: &[u8] = b"staking_vault";
pub const OPERATOR_SEED: &[u8] = b"operator";
pub const DELEGATION_SEED: &[u8] = b"delegation";

/// Basis points denominator.
pub const BPS: u64 = 10_000;
/// Highest commission an operator may charge its delegators (50%).
pub const MAX_COMMISSION_BPS: u16 = 5_000;
/// Highest share of an operator's stake a single alert may slash (20%).
pub const MAX_SLASH_BPS: u16 = 2_000;
/// Highest share of a slash paid to the alerter (50%).
pub const MAX_ALERTER_REWARD_BPS: u16 = 5_000;
/// Longest unbonding period the admin may configure (60 days).
pub const MAX_UNBONDING_PERIOD_SECS: i64 = 60 * 24 * 60 * 60;
/// Unbonding requests a single operator or delegation can have in flight.
pub const MAX_UNBONDING_REQUESTS: usize = 8;

/// One staking pool per OCR2 feed.
///
/// Node operators serving the feed stake LINK into the pool and token holders delegate to them. The pool
/// emits rewards at a fixed rate per second, split pro rata to stake, and slashes operators when anyone
/// proves that the feed went stale (see `raise_alert`).
#[account]
#[derive(InitSpace)]
pub struct StakingPool {
    pub version: u8,
    pub bump: u8,
    pub vault_authority_bump: u8,
    pub paused: bool,

    pub admin: Pubkey,
    pub proposed_admin: Pubkey,

    /// The store `Transmissions` account of the feed the operators serve.
    pub feed: Pubkey,
    pub token_mint: Pubkey,
    pub token_vault: Pubkey,

    pub config: PoolConfig,

    // -- Rewards
    /// Rewards emitted per second, split pro rata to `total_staked`.
    pub reward_rate_per_sec: u64,
    /// Unix timestamp rewards were last emitted up to.
    pub last_reward_ts: i64,
    /// Cumulative reward per staked token, scaled by `math::PRECISION`.
    pub acc_reward_per_stake: u128,
    /// Tokens funded for rewards and not emitted yet.
    pub reward_reserve: u64,

    // -- Totals
    /// Sum of every operator's `total_stake()`.
    pub total_staked: u64,
    /// Tokens in unbonding queues (no longer staked, not withdrawn yet).
    pub total_unbonding: u64,
    /// Slashed tokens not paid to alerters, waiting for `withdraw_slashed`.
    pub slashed_reserve: u64,
    pub operator_count: u32,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, Debug, PartialEq, Eq)]
pub struct PoolConfig {
    /// Minimum self-stake of an active operator.
    pub min_operator_stake: u64,
    /// Delegations an operator may accept, as a multiple of its self-stake.
    pub max_delegation_ratio: u16,
    /// Time between an unstake request and the withdrawal of the tokens.
    pub unbonding_period_secs: i64,
    /// Age of the latest feed round after which the feed is considered stale.
    pub alert_staleness_secs: i64,
    /// Share of an operator's total stake slashed by one alert.
    pub slash_bps: u16,
    /// Share of a slash paid to the alerter.
    pub alerter_reward_bps: u16,
}

impl StakingPool {
    pub fn seeds(&self) -> [&[u8]; 2] {
        [POOL_SEED, self.feed.as_ref()]
    }
}

/// A pending withdrawal: tokens leave the stake at request time and can be withdrawn at `unlock_ts`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, Debug, PartialEq, Eq)]
pub struct UnbondingRequest {
    pub amount: u64,
    pub unlock_ts: i64,
}

#[account]
#[derive(InitSpace)]
pub struct Operator {
    pub bump: u8,
    pub active: bool,
    pub pool: Pubkey,
    /// Signs operator instructions and owns the operator's token accounts.
    pub authority: Pubkey,

    /// Commission taken from the rewards of the operator's delegators.
    pub commission_bps: u16,

    /// The operator's own stake.
    pub self_stake: u64,
    /// Tokens delegated to the operator, after slashing.
    pub delegated_amount: u64,
    /// Shares issued to delegators; a share is worth `delegated_amount / delegated_shares` tokens.
    pub delegated_shares: u64,
    pub delegator_count: u32,

    /// `pool.acc_reward_per_stake` the operator was last settled at.
    pub reward_debt: u128,
    /// Rewards owed to the operator itself (self-stake rewards plus commission), not claimed yet.
    pub operator_rewards: u64,
    /// Cumulative delegator reward per share, scaled by `math::PRECISION`.
    pub acc_reward_per_share: u128,

    /// Latest feed round id this operator was slashed for, so one stale round slashes it once.
    pub last_slashed_round_id: u32,
    pub total_slashed: u64,

    #[max_len(MAX_UNBONDING_REQUESTS)]
    pub unbonding: Vec<UnbondingRequest>,
}

impl Operator {
    pub fn total_stake(&self) -> Option<u64> {
        self.self_stake.checked_add(self.delegated_amount)
    }

    /// Delegations the operator can still accept with its current self-stake.
    pub fn delegation_capacity(&self, max_delegation_ratio: u16) -> Option<u64> {
        self.self_stake
            .checked_mul(u64::from(max_delegation_ratio))?
            .checked_sub(self.delegated_amount)
            .or(Some(0))
    }
}

#[account]
#[derive(InitSpace)]
pub struct Delegation {
    pub bump: u8,
    pub pool: Pubkey,
    pub operator: Pubkey,
    pub delegator: Pubkey,

    pub shares: u64,
    /// `operator.acc_reward_per_share` the delegation was last settled at.
    pub reward_debt: u128,
    /// Rewards settled and not claimed yet.
    pub pending_rewards: u64,

    #[max_len(MAX_UNBONDING_REQUESTS)]
    pub unbonding: Vec<UnbondingRequest>,
}
