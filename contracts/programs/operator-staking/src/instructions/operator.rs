use anchor_lang::prelude::*;
use anchor_spl::token;

use super::{
    add_staked, emit_unbonding, finish_unbonding, now, start_unbonding, sync, update_pool,
};
use crate::context::*;
use crate::error::StakingError;
use crate::event;
use crate::state::*;

pub fn register_operator(
    ctx: Context<RegisterOperator>,
    commission_bps: u16,
    stake: u64,
) -> Result<()> {
    require!(!ctx.accounts.pool.paused, StakingError::PoolPaused);
    require!(
        commission_bps <= MAX_COMMISSION_BPS,
        StakingError::CommissionTooHigh
    );
    require!(
        stake >= ctx.accounts.pool.config.min_operator_stake,
        StakingError::StakeBelowMinimum
    );

    // Emit before the new stake counts, so it only earns from now on.
    update_pool(&mut ctx.accounts.pool, now()?)?;

    token::transfer(ctx.accounts.transfer_ctx(), stake)?;

    let pool = &mut ctx.accounts.pool;
    let operator = &mut ctx.accounts.operator;
    operator.bump = ctx.bumps.operator;
    operator.active = true;
    operator.pool = pool.key();
    operator.authority = ctx.accounts.authority.key();
    operator.commission_bps = commission_bps;
    operator.self_stake = stake;
    operator.reward_debt = pool.acc_reward_per_stake;

    add_staked(pool, stake)?;
    pool.operator_count = pool
        .operator_count
        .checked_add(1)
        .ok_or(StakingError::Overflow)?;

    emit!(event::OperatorRegistered {
        pool: pool.key(),
        operator: operator.key(),
        authority: operator.authority,
        commission_bps,
        self_stake: stake,
    });
    Ok(())
}

/// Changes the commission taken from delegators' rewards from now on.
pub fn set_commission(ctx: Context<OperatorUpdate>, commission_bps: u16) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    let operator = &mut ctx.accounts.operator;
    require!(operator.active, StakingError::OperatorInactive);

    // Rewards earned so far are split at the old commission.
    sync(pool, operator, now()?)?;
    operator.commission_bps = commission_bps;

    emit!(event::CommissionSet {
        operator: operator.key(),
        commission_bps,
    });
    Ok(())
}

pub fn add_operator_stake(ctx: Context<AddOperatorStake>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::InvalidInput);
    require!(!ctx.accounts.pool.paused, StakingError::PoolPaused);
    require!(ctx.accounts.operator.active, StakingError::OperatorInactive);

    sync(&mut ctx.accounts.pool, &mut ctx.accounts.operator, now()?)?;

    token::transfer(ctx.accounts.transfer_ctx(), amount)?;

    let operator = &mut ctx.accounts.operator;
    operator.self_stake = operator
        .self_stake
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;
    add_staked(&mut ctx.accounts.pool, amount)?;

    emit!(event::OperatorStakeAdded {
        operator: operator.key(),
        amount,
        self_stake: operator.self_stake,
    });
    Ok(())
}

/// Starts unbonding `amount` of the operator's self-stake. The remaining self-stake must stay above the
/// pool minimum and back the current delegations; unstaking everything is only possible once every
/// delegator has left, and deactivates the operator.
pub fn request_operator_unstake(ctx: Context<OperatorUpdate>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::InvalidInput);
    let now = now()?;
    let pool = &mut ctx.accounts.pool;
    let operator = &mut ctx.accounts.operator;

    sync(pool, operator, now)?;

    let remaining = operator
        .self_stake
        .checked_sub(amount)
        .ok_or(StakingError::InsufficientStake)?;
    if remaining == 0 {
        require!(
            operator.delegated_amount == 0 && operator.delegated_shares == 0,
            StakingError::OperatorHasDelegations
        );
        if operator.active {
            operator.active = false;
            pool.operator_count = pool
                .operator_count
                .checked_sub(1)
                .ok_or(StakingError::Overflow)?;
            emit!(event::OperatorDeactivated {
                operator: operator.key(),
            });
        }
    } else {
        require!(
            remaining >= pool.config.min_operator_stake,
            StakingError::StakeBelowMinimum
        );
        let capacity = remaining
            .checked_mul(u64::from(pool.config.max_delegation_ratio))
            .ok_or(StakingError::Overflow)?;
        require!(
            operator.delegated_amount <= capacity,
            StakingError::DelegationCapacityExceeded
        );
    }
    operator.self_stake = remaining;

    let unlock_ts = start_unbonding(pool, &mut operator.unbonding, amount, now)?;
    emit_unbonding(operator.key(), operator.key(), amount, unlock_ts);
    Ok(())
}

pub fn withdraw_operator_unbonded(ctx: Context<OperatorPayout>) -> Result<()> {
    let amount = finish_unbonding(
        &mut ctx.accounts.pool,
        &mut ctx.accounts.operator.unbonding,
        now()?,
    )?;
    emit!(event::Unbonded {
        owner: ctx.accounts.operator.key(),
        recipient: ctx.accounts.recipient.key(),
        amount,
    });
    pay(&ctx, amount)
}

pub fn claim_operator_rewards(ctx: Context<OperatorPayout>) -> Result<()> {
    let amount = {
        let pool = &mut ctx.accounts.pool;
        let operator = &mut ctx.accounts.operator;
        sync(pool, operator, now()?)?;
        std::mem::take(&mut operator.operator_rewards)
    };
    if amount == 0 {
        return Ok(());
    }
    emit!(event::RewardsClaimed {
        owner: ctx.accounts.operator.key(),
        recipient: ctx.accounts.recipient.key(),
        amount,
    });
    pay(&ctx, amount)
}

fn pay(ctx: &Context<OperatorPayout>, amount: u64) -> Result<()> {
    let pool_key = ctx.accounts.pool.key();
    let bump = ctx.accounts.pool.vault_authority_bump;
    token::transfer(
        ctx.accounts
            .transfer_ctx()
            .with_signer(&[&[VAULT_SEED, pool_key.as_ref(), &[bump]]]),
        amount,
    )
}
