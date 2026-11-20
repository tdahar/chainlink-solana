//! Read-only views for off-chain clients and other programs. They simulate the reward accounting on copies
//! of the accounts, so what they return is what the matching instruction would pay at `now`, and they never
//! change an account.

use anchor_lang::prelude::*;

use crate::error::StakingError;
use crate::instructions::{settle_delegation, settle_operator, update_pool};
use crate::math;
use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct PoolSummary {
    pub total_staked: u64,
    pub total_unbonding: u64,
    pub reward_reserve: u64,
    pub reward_rate_per_sec: u64,
    /// Seconds until the reward reserve runs out at the current rate; `u64::MAX` when the rate is zero.
    pub reward_runway_secs: u64,
    pub operator_count: u32,
    pub paused: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct OperatorSummary {
    pub active: bool,
    pub commission_bps: u16,
    pub self_stake: u64,
    pub delegated_amount: u64,
    pub delegated_shares: u64,
    /// Further delegations the operator can accept now.
    pub delegation_capacity: u64,
    /// Rewards `claim_operator_rewards` would pay now.
    pub claimable_rewards: u64,
    /// Tokens in the operator's unbonding queue, and the part of them unlocked now.
    pub unbonding: u64,
    pub unlocked: u64,
    pub total_slashed: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct DelegationSummary {
    pub shares: u64,
    /// What the shares are worth now, after any slashing.
    pub value: u64,
    /// Rewards `claim_delegation_rewards` would pay now.
    pub claimable_rewards: u64,
    /// Tokens in the delegation's unbonding queue, and the part of them unlocked now.
    pub unbonding: u64,
    pub unlocked: u64,
}

// https://github.com/coral-xyz/anchor/pull/2770
macro_rules! try_from {
    ($ty: ty, $acc: expr) => {{
        let acc_ref = $acc.as_ref();
        <$ty>::try_from(unsafe {
            core::mem::transmute::<&AccountInfo<'_>, &AccountInfo<'_>>(acc_ref)
        })
    }};
}

fn queue_totals(queue: &[UnbondingRequest], now: i64) -> Result<(u64, u64)> {
    let mut total = 0u64;
    let mut unlocked = 0u64;
    for request in queue {
        total = total
            .checked_add(request.amount)
            .ok_or(StakingError::Overflow)?;
        if request.unlock_ts <= now {
            unlocked = unlocked
                .checked_add(request.amount)
                .ok_or(StakingError::Overflow)?;
        }
    }
    Ok((total, unlocked))
}

pub fn pool_summary(pool: &AccountInfo, now: i64) -> Result<PoolSummary> {
    let account = try_from!(Account<StakingPool>, pool)?;
    let mut pool = (*account).clone();
    update_pool(&mut pool, now)?;

    let reward_runway_secs = if pool.reward_rate_per_sec == 0 {
        u64::MAX
    } else {
        pool.reward_reserve / pool.reward_rate_per_sec
    };
    Ok(PoolSummary {
        total_staked: pool.total_staked,
        total_unbonding: pool.total_unbonding,
        reward_reserve: pool.reward_reserve,
        reward_rate_per_sec: pool.reward_rate_per_sec,
        reward_runway_secs,
        operator_count: pool.operator_count,
        paused: pool.paused,
    })
}

pub fn operator_summary(
    pool: &AccountInfo,
    operator: &AccountInfo,
    now: i64,
) -> Result<OperatorSummary> {
    let pool_account = try_from!(Account<StakingPool>, pool)?;
    let operator_account = try_from!(Account<Operator>, operator)?;
    require_keys_eq!(
        operator_account.pool,
        pool_account.key(),
        StakingError::InvalidInput
    );

    let mut pool = (*pool_account).clone();
    let mut operator = (*operator_account).clone();
    update_pool(&mut pool, now)?;
    settle_operator(&pool, &mut operator)?;

    let delegation_capacity = operator
        .delegation_capacity(pool.config.max_delegation_ratio)
        .ok_or(StakingError::Overflow)?;
    let (unbonding, unlocked) = queue_totals(&operator.unbonding, now)?;
    Ok(OperatorSummary {
        active: operator.active,
        commission_bps: operator.commission_bps,
        self_stake: operator.self_stake,
        delegated_amount: operator.delegated_amount,
        delegated_shares: operator.delegated_shares,
        delegation_capacity,
        claimable_rewards: operator.operator_rewards,
        unbonding,
        unlocked,
        total_slashed: operator.total_slashed,
    })
}

pub fn delegation_summary(
    pool: &AccountInfo,
    operator: &AccountInfo,
    delegation: &AccountInfo,
    now: i64,
) -> Result<DelegationSummary> {
    let pool_account = try_from!(Account<StakingPool>, pool)?;
    let operator_account = try_from!(Account<Operator>, operator)?;
    let delegation_account = try_from!(Account<Delegation>, delegation)?;
    require_keys_eq!(
        operator_account.pool,
        pool_account.key(),
        StakingError::InvalidInput
    );
    require_keys_eq!(
        delegation_account.operator,
        operator_account.key(),
        StakingError::InvalidInput
    );

    let mut pool = (*pool_account).clone();
    let mut operator = (*operator_account).clone();
    let mut delegation = (*delegation_account).clone();
    update_pool(&mut pool, now)?;
    settle_operator(&pool, &mut operator)?;
    settle_delegation(&operator, &mut delegation)?;

    let value = math::amount_for_shares(
        delegation.shares,
        operator.delegated_amount,
        operator.delegated_shares,
    )
    .ok_or(StakingError::Overflow)?;
    let (unbonding, unlocked) = queue_totals(&delegation.unbonding, now)?;
    Ok(DelegationSummary {
        shares: delegation.shares,
        value,
        claimable_rewards: delegation.pending_rewards,
        unbonding,
        unlocked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_totals_split_locked_and_unlocked() {
        let queue = vec![
            UnbondingRequest {
                amount: 10,
                unlock_ts: 100,
            },
            UnbondingRequest {
                amount: 20,
                unlock_ts: 200,
            },
        ];
        assert_eq!(queue_totals(&queue, 150).unwrap(), (30, 10));
        assert_eq!(queue_totals(&queue, 50).unwrap(), (30, 0));
        assert_eq!(queue_totals(&[], 50).unwrap(), (0, 0));
    }
}
