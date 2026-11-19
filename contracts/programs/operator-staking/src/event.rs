use anchor_lang::prelude::*;

use crate::state::PoolConfig;

#[event]
pub struct PoolInitialized {
    pub pool: Pubkey,
    pub feed: Pubkey,
    pub admin: Pubkey,
    pub config: PoolConfig,
}

#[event]
pub struct PoolConfigUpdated {
    pub pool: Pubkey,
    pub config: PoolConfig,
}

#[event]
pub struct PoolPausedSet {
    pub pool: Pubkey,
    pub paused: bool,
}

#[event]
pub struct RewardRateSet {
    pub pool: Pubkey,
    pub reward_rate_per_sec: u64,
}

#[event]
pub struct RewardsFunded {
    pub pool: Pubkey,
    pub funder: Pubkey,
    pub amount: u64,
    pub reward_reserve: u64,
}

#[event]
pub struct OperatorRegistered {
    pub pool: Pubkey,
    pub operator: Pubkey,
    pub authority: Pubkey,
    pub commission_bps: u16,
    pub self_stake: u64,
}

#[event]
pub struct CommissionSet {
    pub operator: Pubkey,
    pub commission_bps: u16,
}

#[event]
pub struct OperatorStakeAdded {
    pub operator: Pubkey,
    pub amount: u64,
    pub self_stake: u64,
}

#[event]
pub struct UnbondingRequested {
    pub operator: Pubkey,
    /// The delegation account, or the operator itself for self-stake.
    pub owner: Pubkey,
    pub amount: u64,
    pub unlock_ts: i64,
}

#[event]
pub struct Unbonded {
    pub owner: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}

#[event]
pub struct OperatorDeactivated {
    pub operator: Pubkey,
}

#[event]
pub struct Delegated {
    pub operator: Pubkey,
    pub delegator: Pubkey,
    pub amount: u64,
    pub shares: u64,
}

#[event]
pub struct RewardsClaimed {
    /// The delegation account, or the operator for operator rewards.
    pub owner: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}

#[event]
pub struct AlertRaised {
    pub pool: Pubkey,
    pub operator: Pubkey,
    pub alerter: Pubkey,
    pub round_id: u32,
    pub round_timestamp: u32,
    pub slashed_self: u64,
    pub slashed_delegated: u64,
    pub alerter_reward: u64,
}

#[event]
pub struct SlashedWithdrawn {
    pub pool: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}

#[event]
pub struct Redelegated {
    pub from_operator: Pubkey,
    pub to_operator: Pubkey,
    pub delegator: Pubkey,
    pub amount: u64,
    pub shares_burned: u64,
    pub shares_minted: u64,
}
