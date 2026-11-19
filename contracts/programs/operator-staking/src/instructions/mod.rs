//! Instruction handlers, and the reward accounting they share.
//!
//! Rewards use two accumulators. The pool's `acc_reward_per_stake` grows with every emission, pro rata to
//! `total_staked`. Settling an operator splits what its total stake earned since its `reward_debt` between
//! the operator (self-stake rewards plus commission) and its delegators, whose share grows the operator's
//! `acc_reward_per_share`; settling a delegation then pays it per share. Every handler that changes a
//! stake settles first, so past rewards are paid on the stake that earned them.

use anchor_lang::prelude::*;

use crate::error::StakingError;
use crate::event;
use crate::math;
use crate::state::*;

pub mod admin;
pub mod alert;
pub mod delegator;
pub mod operator;

pub(crate) fn now() -> Result<i64> {
    Ok(Clock::get()?.unix_timestamp)
}

/// Emits the rewards due since `pool.last_reward_ts`. Nothing is emitted while the pool is paused.
pub(crate) fn update_pool(pool: &mut StakingPool, now: i64) -> Result<()> {
    if now <= pool.last_reward_ts {
        return Ok(());
    }
    let elapsed = u64::try_from(now - pool.last_reward_ts).map_err(|_| StakingError::Overflow)?;
    pool.last_reward_ts = now;
    if pool.paused {
        return Ok(());
    }

    let emission = math::emission(
        pool.reward_rate_per_sec,
        elapsed,
        pool.reward_reserve,
        pool.total_staked,
    )
    .ok_or(StakingError::Overflow)?;
    pool.acc_reward_per_stake = pool
        .acc_reward_per_stake
        .checked_add(emission.acc_increase)
        .ok_or(StakingError::Overflow)?;
    pool.reward_reserve = pool
        .reward_reserve
        .checked_sub(emission.emitted)
        .ok_or(StakingError::Overflow)?;
    Ok(())
}

/// Credits the operator and its delegators with what the operator's stake earned since it was last
/// settled.
pub(crate) fn settle_operator(pool: &StakingPool, operator: &mut Operator) -> Result<()> {
    let total = operator.total_stake().ok_or(StakingError::Overflow)?;
    let earned = math::accrued(total, pool.acc_reward_per_stake, operator.reward_debt)
        .ok_or(StakingError::Overflow)?;
    operator.reward_debt = pool.acc_reward_per_stake;
    if earned == 0 {
        return Ok(());
    }

    let split = math::split_rewards(
        earned,
        operator.self_stake,
        operator.delegated_amount,
        operator.commission_bps,
    )
    .ok_or(StakingError::Overflow)?;
    operator.operator_rewards = operator
        .operator_rewards
        .checked_add(split.operator)
        .ok_or(StakingError::Overflow)?;
    let increase = math::per_share_increase(split.delegators, operator.delegated_shares)
        .ok_or(StakingError::Overflow)?;
    operator.acc_reward_per_share = operator
        .acc_reward_per_share
        .checked_add(increase)
        .ok_or(StakingError::Overflow)?;
    Ok(())
}

/// Emits pending rewards and settles `operator`: what every stake-changing handler does first.
pub(crate) fn sync(pool: &mut StakingPool, operator: &mut Operator, now: i64) -> Result<()> {
    update_pool(pool, now)?;
    settle_operator(pool, operator)
}

/// Credits the delegation with what its shares earned since it was last settled.
pub(crate) fn settle_delegation(operator: &Operator, delegation: &mut Delegation) -> Result<()> {
    let earned = math::accrued(
        delegation.shares,
        operator.acc_reward_per_share,
        delegation.reward_debt,
    )
    .ok_or(StakingError::Overflow)?;
    delegation.reward_debt = operator.acc_reward_per_share;
    delegation.pending_rewards = delegation
        .pending_rewards
        .checked_add(earned)
        .ok_or(StakingError::Overflow)?;
    Ok(())
}

/// Moves `amount` from the stake into an unbonding request of `queue` that unlocks after the pool's
/// unbonding period.
pub(crate) fn start_unbonding(
    pool: &mut StakingPool,
    queue: &mut Vec<UnbondingRequest>,
    amount: u64,
    now: i64,
) -> Result<i64> {
    require!(
        queue.len() < MAX_UNBONDING_REQUESTS,
        StakingError::UnbondingQueueFull
    );
    let unlock_ts = now
        .checked_add(pool.config.unbonding_period_secs)
        .ok_or(StakingError::Overflow)?;
    queue.push(UnbondingRequest { amount, unlock_ts });

    pool.total_staked = pool
        .total_staked
        .checked_sub(amount)
        .ok_or(StakingError::Overflow)?;
    pool.total_unbonding = pool
        .total_unbonding
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;
    Ok(unlock_ts)
}

/// Removes the unlocked requests of `queue` and returns their amount, which the caller pays out.
pub(crate) fn finish_unbonding(
    pool: &mut StakingPool,
    queue: &mut Vec<UnbondingRequest>,
    now: i64,
) -> Result<u64> {
    let amount = math::take_unlocked(queue, now).ok_or(StakingError::Overflow)?;
    require!(amount > 0, StakingError::NothingUnlocked);
    pool.total_unbonding = pool
        .total_unbonding
        .checked_sub(amount)
        .ok_or(StakingError::Overflow)?;
    Ok(amount)
}

pub(crate) fn add_staked(pool: &mut StakingPool, amount: u64) -> Result<()> {
    pool.total_staked = pool
        .total_staked
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;
    Ok(())
}

pub(crate) fn emit_unbonding(operator: Pubkey, owner: Pubkey, amount: u64, unlock_ts: i64) {
    emit!(event::UnbondingRequested {
        operator,
        owner,
        amount,
        unlock_ts,
    });
}
