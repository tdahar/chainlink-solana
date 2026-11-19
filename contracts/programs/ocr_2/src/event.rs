use anchor_lang::prelude::*;

// use crate::state::MAX_ORACLES;

#[event]
pub struct SetConfig {
    pub config_digest: [u8; 32],
    pub f: u8,
    pub signers: Vec<[u8; 20]>,
}

#[event]
pub struct SetBilling {
    pub observation_payment_gjuels: u32,
    pub transmission_payment_gjuels: u32,
}

#[event]
pub struct RoundRequested {
    pub config_digest: [u8; 32],
    pub requester: Pubkey,
    pub epoch: u32,
    pub round: u8,
}

#[event]
pub struct NewTransmission {
    pub round_id: u32,
    pub config_digest: [u8; 32],
    pub answer: i128,
    pub transmitter: u8,
    pub observations_timestamp: u32,
    pub observer_count: u8,
    pub observers: [u8; 19], // Can't use MAX_ORACLES because of IDL parsing issues
    pub juels_per_lamport: u64,
    pub reimbursement_gjuels: u64,
}

// -- Reward pool

#[event]
pub struct RewardPoolInitialized {
    pub aggregator: Pubkey,
    pub pool: Pubkey,
    pub admin: Pubkey,
    pub reward_per_round_gjuels: u64,
    pub withdrawal_delay_secs: i64,
}

#[event]
pub struct RewardRateSet {
    pub aggregator: Pubkey,
    pub reward_per_round_gjuels: u64,
}

#[event]
pub struct RewardsDistributed {
    pub aggregator: Pubkey,
    pub round_id: u32,
    pub rounds: u32,
    pub registered_oracles: u32,
    pub allocated_gjuels: u64,
}

#[event]
pub struct RewardsDeposited {
    pub aggregator: Pubkey,
    pub sponsor: Pubkey,
    pub amount_gjuels: u64,
}

#[event]
pub struct SponsorWithdrawalRequested {
    pub aggregator: Pubkey,
    pub sponsor: Pubkey,
    pub amount_gjuels: u64,
    pub unlock_ts: i64,
}

#[event]
pub struct SponsorWithdrawalCancelled {
    pub aggregator: Pubkey,
    pub sponsor: Pubkey,
    pub amount_gjuels: u64,
}

#[event]
pub struct SponsorWithdrawn {
    pub aggregator: Pubkey,
    pub sponsor: Pubkey,
    pub recipient: Pubkey,
    pub amount_gjuels: u64,
}

#[event]
pub struct OracleRegistered {
    pub aggregator: Pubkey,
    pub transmitter: Pubkey,
    pub payee: Pubkey,
}

#[event]
pub struct OracleDeregistered {
    pub aggregator: Pubkey,
    pub transmitter: Pubkey,
    pub accrued_gjuels: u64,
}

#[event]
pub struct OracleRewardsClaimed {
    pub aggregator: Pubkey,
    pub transmitter: Pubkey,
    pub payee: Pubkey,
    pub amount_gjuels: u64,
}

#[event]
pub struct RewardDustSwept {
    pub aggregator: Pubkey,
    pub recipient: Pubkey,
    pub amount_gjuels: u64,
}

#[event]
pub struct OracleRecordClosed {
    pub aggregator: Pubkey,
    pub transmitter: Pubkey,
    pub claimed_gjuels: u64,
}
