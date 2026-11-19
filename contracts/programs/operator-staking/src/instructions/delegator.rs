use anchor_lang::prelude::*;
use anchor_spl::token;

use super::{
    add_staked, emit_unbonding, finish_unbonding, now, settle_delegation, start_unbonding, sync,
};
use crate::context::*;
use crate::error::StakingError;
use crate::event;
use crate::math;
use crate::state::*;

pub fn open_delegation(ctx: Context<OpenDelegation>) -> Result<()> {
    require!(ctx.accounts.operator.active, StakingError::OperatorInactive);

    let operator = &mut ctx.accounts.operator;
    let delegation = &mut ctx.accounts.delegation;
    delegation.bump = ctx.bumps.delegation;
    delegation.pool = ctx.accounts.pool.key();
    delegation.operator = operator.key();
    delegation.delegator = ctx.accounts.delegator.key();
    delegation.reward_debt = operator.acc_reward_per_share;

    operator.delegator_count = operator
        .delegator_count
        .checked_add(1)
        .ok_or(StakingError::Overflow)?;
    Ok(())
}

/// Delegates `amount` to the operator in exchange for shares of its delegated pool.
pub fn delegate(ctx: Context<Delegate>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::InvalidInput);
    require!(!ctx.accounts.pool.paused, StakingError::PoolPaused);
    require!(ctx.accounts.operator.active, StakingError::OperatorInactive);

    let capacity = ctx
        .accounts
        .operator
        .delegation_capacity(ctx.accounts.pool.config.max_delegation_ratio)
        .ok_or(StakingError::Overflow)?;
    require!(amount <= capacity, StakingError::DelegationCapacityExceeded);

    sync(&mut ctx.accounts.pool, &mut ctx.accounts.operator, now()?)?;
    settle_delegation(&ctx.accounts.operator, &mut ctx.accounts.delegation)?;

    token::transfer(ctx.accounts.transfer_ctx(), amount)?;

    let operator = &mut ctx.accounts.operator;
    let delegation = &mut ctx.accounts.delegation;
    operator.delegated_amount = operator
        .delegated_amount
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;
    let shares =
        math::shares_for_amount(amount, operator.delegated_amount, operator.delegated_shares)
            .ok_or(StakingError::Overflow)?;
    require!(shares > 0, StakingError::InvalidInput);
    operator.delegated_shares = operator
        .delegated_shares
        .checked_add(shares)
        .ok_or(StakingError::Overflow)?;
    delegation.shares = delegation
        .shares
        .checked_add(shares)
        .ok_or(StakingError::Overflow)?;
    add_staked(&mut ctx.accounts.pool, amount)?;

    emit!(event::Delegated {
        operator: operator.key(),
        delegator: delegation.delegator,
        amount,
        shares,
    });
    Ok(())
}

/// Burns `shares` and starts unbonding the tokens they are worth now.
pub fn request_undelegate(ctx: Context<DelegationUpdate>, shares: u64) -> Result<()> {
    require!(shares > 0, StakingError::InvalidInput);
    let now = now()?;
    let pool = &mut ctx.accounts.pool;
    let operator = &mut ctx.accounts.operator;
    let delegation = &mut ctx.accounts.delegation;
    require!(
        shares <= delegation.shares,
        StakingError::InsufficientShares
    );

    sync(pool, operator, now)?;
    settle_delegation(operator, delegation)?;

    let amount =
        math::amount_for_shares(shares, operator.delegated_amount, operator.delegated_shares)
            .ok_or(StakingError::Overflow)?;
    operator.delegated_amount = operator
        .delegated_amount
        .checked_sub(amount)
        .ok_or(StakingError::Overflow)?;
    operator.delegated_shares = operator
        .delegated_shares
        .checked_sub(shares)
        .ok_or(StakingError::Overflow)?;
    delegation.shares = delegation
        .shares
        .checked_sub(shares)
        .ok_or(StakingError::Overflow)?;

    let unlock_ts = start_unbonding(pool, &mut delegation.unbonding, amount, now)?;
    emit_unbonding(operator.key(), delegation.key(), amount, unlock_ts);
    Ok(())
}

pub fn withdraw_delegation_unbonded(ctx: Context<DelegationPayout>) -> Result<()> {
    let amount = finish_unbonding(
        &mut ctx.accounts.pool,
        &mut ctx.accounts.delegation.unbonding,
        now()?,
    )?;
    emit!(event::Unbonded {
        owner: ctx.accounts.delegation.key(),
        recipient: ctx.accounts.recipient.key(),
        amount,
    });
    pay(&ctx, amount)
}

pub fn claim_delegation_rewards(ctx: Context<DelegationPayout>) -> Result<()> {
    let amount = {
        let pool = &mut ctx.accounts.pool;
        let operator = &mut ctx.accounts.operator;
        let delegation = &mut ctx.accounts.delegation;
        sync(pool, operator, now()?)?;
        settle_delegation(operator, delegation)?;
        std::mem::take(&mut delegation.pending_rewards)
    };
    if amount == 0 {
        return Ok(());
    }
    emit!(event::RewardsClaimed {
        owner: ctx.accounts.delegation.key(),
        recipient: ctx.accounts.recipient.key(),
        amount,
    });
    pay(&ctx, amount)
}

fn pay(ctx: &Context<DelegationPayout>, amount: u64) -> Result<()> {
    let pool_key = ctx.accounts.pool.key();
    let bump = ctx.accounts.pool.vault_authority_bump;
    token::transfer(
        ctx.accounts
            .transfer_ctx()
            .with_signer(&[&[VAULT_SEED, pool_key.as_ref(), &[bump]]]),
        amount,
    )
}
