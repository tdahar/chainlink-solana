use anchor_lang::prelude::*;

pub const REWARD_POOL_VERSION: u8 = 1;

pub const REWARD_POOL_SEED: &[u8] = b"reward_pool";
pub const REWARD_VAULT_SEED: &[u8] = b"reward_vault";
pub const SPONSOR_SEED: &[u8] = b"sponsor";
pub const ORACLE_REWARD_SEED: &[u8] = b"oracle_reward";

/// Upper bound for the sponsor withdrawal delay (30 days).
pub const MAX_WITHDRAWAL_DELAY_SECS: i64 = 30 * 24 * 60 * 60;

/// Sponsor-funded reward pool attached to a single OCR2 aggregator.
///
/// Sponsors (protocols consuming the feed) deposit LINK into the pool vault. The pool releases
/// `reward_per_round_gjuels` for every round transmitted on the aggregator and splits it evenly
/// between the oracles that registered with the pool. Accrual uses a cumulative per-oracle index
/// (`acc_reward_per_oracle`), so distributing is O(1) regardless of the number of oracles.
#[account]
#[derive(InitSpace)]
pub struct RewardPool {
    pub version: u8,
    pub bump: u8,
    pub vault_authority_bump: u8,
    pub paused: bool,

    /// The aggregator `State` account this pool rewards.
    pub aggregator: Pubkey,
    /// Pool admin, initialised to the aggregator owner.
    pub admin: Pubkey,
    pub proposed_admin: Pubkey,

    pub token_mint: Pubkey,
    pub token_vault: Pubkey,

    /// Amount released for every round transmitted on the aggregator.
    pub reward_per_round_gjuels: u64,
    /// Minimum time between a sponsor withdrawal request and its execution.
    pub withdrawal_delay_secs: i64,

    /// Last aggregator round id rewards were distributed up to.
    pub last_distributed_round_id: u32,
    /// Number of oracles currently registered with the pool.
    pub registered_oracles: u32,
    /// Cumulative reward per registered oracle, scaled by `math::ACC_PRECISION`.
    pub acc_reward_per_oracle: u128,

    /// Sum of all sponsor deposits.
    pub total_deposited_gjuels: u64,
    /// Sum of all rewards released to oracles.
    pub total_allocated_gjuels: u64,
    /// Sum of all rewards paid out to oracles.
    pub total_claimed_gjuels: u64,
    /// Sum of all deposits returned to sponsors.
    pub total_withdrawn_gjuels: u64,
    /// Sum of withdrawal requests that are waiting for their delay to pass.
    pub total_pending_withdrawal_gjuels: u64,
}

impl RewardPool {
    /// Deposits that have not been released to oracles nor returned to sponsors.
    pub fn unallocated_gjuels(&self) -> Option<u64> {
        self.total_deposited_gjuels
            .checked_sub(self.total_allocated_gjuels)?
            .checked_sub(self.total_withdrawn_gjuels)
    }

    /// Unallocated deposits that are not reserved by a pending sponsor withdrawal. Only this
    /// amount can still be released to oracles.
    pub fn distributable_gjuels(&self) -> Option<u64> {
        self.unallocated_gjuels()?
            .checked_sub(self.total_pending_withdrawal_gjuels)
    }
}

/// Per-sponsor accounting of deposits and withdrawals.
#[account]
#[derive(InitSpace)]
pub struct SponsorPosition {
    pub bump: u8,
    pub pool: Pubkey,
    pub sponsor: Pubkey,

    pub deposited_gjuels: u64,
    pub withdrawn_gjuels: u64,

    /// Amount reserved by an outstanding withdrawal request, 0 if none.
    pub pending_withdrawal_gjuels: u64,
    /// Unix timestamp after which the pending withdrawal can be executed.
    pub withdrawal_unlock_ts: i64,
}

impl SponsorPosition {
    /// Deposits the sponsor could still ask back, ignoring pool-wide availability.
    pub fn outstanding_gjuels(&self) -> Option<u64> {
        self.deposited_gjuels
            .checked_sub(self.withdrawn_gjuels)?
            .checked_sub(self.pending_withdrawal_gjuels)
    }
}

/// Per-oracle accounting, keyed by the oracle transmitter.
#[account]
#[derive(InitSpace)]
pub struct OracleRewardRecord {
    pub bump: u8,
    pub active: bool,
    pub pool: Pubkey,
    pub transmitter: Pubkey,
    /// Payee of the oracle the last time it was seen on the aggregator. Used to pay out
    /// rewards that were settled before the oracle left the aggregator.
    pub payee: Pubkey,

    /// Value of `acc_reward_per_oracle` the oracle was last settled at.
    pub reward_debt: u128,
    /// Rewards settled but not claimed yet.
    pub accrued_gjuels: u64,
    /// Lifetime rewards claimed.
    pub claimed_gjuels: u64,
}
