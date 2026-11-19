use anchor_lang::prelude::*;

#[error_code]
pub enum StakingError {
    #[msg("Unauthorized")]
    Unauthorized = 0,

    #[msg("Invalid input")]
    InvalidInput = 1,

    #[msg("Arithmetic overflow")]
    Overflow = 2,

    #[msg("The pool is paused")]
    PoolPaused = 3,

    #[msg("Invalid pool configuration")]
    InvalidConfig = 4,

    #[msg("Commission is above the maximum")]
    CommissionTooHigh = 5,

    #[msg("Self-stake is below the pool minimum")]
    StakeBelowMinimum = 6,

    #[msg("The operator cannot accept that much delegation")]
    DelegationCapacityExceeded = 7,

    #[msg("The operator is not active")]
    OperatorInactive = 8,

    #[msg("The operator still has delegations")]
    OperatorHasDelegations = 9,

    #[msg("Not enough shares")]
    InsufficientShares = 10,

    #[msg("Not enough stake")]
    InsufficientStake = 11,

    #[msg("Too many unbonding requests in flight")]
    UnbondingQueueFull = 12,

    #[msg("Nothing to withdraw yet")]
    NothingUnlocked = 13,

    #[msg("The feed is not stale")]
    FeedNotStale = 14,

    #[msg("The feed has no rounds")]
    FeedEmpty = 15,

    #[msg("The operator was already slashed for this round")]
    AlreadySlashed = 16,

    #[msg("Not enough slashed tokens in reserve")]
    InsufficientReserve = 17,

    #[msg("Invalid token account")]
    InvalidTokenAccount = 18,
}
