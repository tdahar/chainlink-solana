use anchor_lang::prelude::*;
use anchor_spl::token;

use super::{now, update_pool};
use crate::context::*;
use crate::error::StakingError;
use crate::event;
use crate::state::*;

fn validate_config(config: &PoolConfig) -> Result<()> {
    require!(config.min_operator_stake > 0, StakingError::InvalidConfig);
    require!(
        (1..=100).contains(&config.max_delegation_ratio),
        StakingError::InvalidConfig
    );
    require!(
        (0..=MAX_UNBONDING_PERIOD_SECS).contains(&config.unbonding_period_secs),
        StakingError::InvalidConfig
    );
    require!(config.alert_staleness_secs > 0, StakingError::InvalidConfig);
    require!(
        config.slash_bps <= MAX_SLASH_BPS,
        StakingError::InvalidConfig
    );
    require!(
        config.alerter_reward_bps <= MAX_ALERTER_REWARD_BPS,
        StakingError::InvalidConfig
    );
    Ok(())
}

pub fn initialize_pool(
    ctx: Context<InitializePool>,
    config: PoolConfig,
    reward_rate_per_sec: u64,
) -> Result<()> {
    validate_config(&config)?;

    let pool = &mut ctx.accounts.pool;
    pool.version = POOL_VERSION;
    pool.bump = ctx.bumps.pool;
    pool.vault_authority_bump = ctx.bumps.vault_authority;
    pool.paused = false;
    pool.admin = ctx.accounts.admin.key();
    pool.proposed_admin = Pubkey::default();
    pool.feed = ctx.accounts.feed.key();
    pool.token_mint = ctx.accounts.token_mint.key();
    pool.token_vault = ctx.accounts.token_vault.key();
    pool.config = config;
    pool.reward_rate_per_sec = reward_rate_per_sec;
    pool.last_reward_ts = now()?;

    emit!(event::PoolInitialized {
        pool: pool.key(),
        feed: pool.feed,
        admin: pool.admin,
        config,
    });
    Ok(())
}

/// Changes the pool settings. Unbonding requests already made keep their unlock time; a lower minimum
/// stake or delegation ratio only applies to later stake changes.
pub fn set_pool_config(ctx: Context<AdminUpdate>, config: PoolConfig) -> Result<()> {
    validate_config(&config)?;
    let pool = &mut ctx.accounts.pool;
    pool.config = config;
    emit!(event::PoolConfigUpdated {
        pool: pool.key(),
        config,
    });
    Ok(())
}

pub fn set_paused(ctx: Context<AdminUpdate>, paused: bool) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    // Emit up to now under the previous setting.
    update_pool(pool, now()?)?;
    pool.paused = paused;
    emit!(event::PoolPausedSet {
        pool: pool.key(),
        paused,
    });
    Ok(())
}

pub fn set_reward_rate(ctx: Context<AdminUpdate>, reward_rate_per_sec: u64) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    // Emit up to now at the old rate.
    update_pool(pool, now()?)?;
    pool.reward_rate_per_sec = reward_rate_per_sec;
    emit!(event::RewardRateSet {
        pool: pool.key(),
        reward_rate_per_sec,
    });
    Ok(())
}

pub fn transfer_admin(ctx: Context<AdminUpdate>, proposed_admin: Pubkey) -> Result<()> {
    ctx.accounts.pool.proposed_admin = proposed_admin;
    Ok(())
}

pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    pool.admin = std::mem::take(&mut pool.proposed_admin);
    Ok(())
}

pub fn fund_rewards(ctx: Context<FundRewards>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::InvalidInput);
    // Emit with the reserve as it was, so the new funds only pay future rewards.
    update_pool(&mut ctx.accounts.pool, now()?)?;

    token::transfer(ctx.accounts.transfer_ctx(), amount)?;

    let pool = &mut ctx.accounts.pool;
    pool.reward_reserve = pool
        .reward_reserve
        .checked_add(amount)
        .ok_or(StakingError::Overflow)?;
    emit!(event::RewardsFunded {
        pool: pool.key(),
        funder: ctx.accounts.funder.key(),
        amount,
        reward_reserve: pool.reward_reserve,
    });
    Ok(())
}

/// Sends slashed tokens that were not paid to alerters to a recipient of the admin's choice.
pub fn withdraw_slashed(ctx: Context<WithdrawSlashed>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::InvalidInput);
    {
        let pool = &mut ctx.accounts.pool;
        pool.slashed_reserve = pool
            .slashed_reserve
            .checked_sub(amount)
            .ok_or(StakingError::InsufficientReserve)?;
        emit!(event::SlashedWithdrawn {
            pool: pool.key(),
            recipient: ctx.accounts.recipient.key(),
            amount,
        });
    }

    let pool_key = ctx.accounts.pool.key();
    let bump = ctx.accounts.pool.vault_authority_bump;
    token::transfer(
        ctx.accounts
            .transfer_ctx()
            .with_signer(&[&[VAULT_SEED, pool_key.as_ref(), &[bump]]]),
        amount,
    )?;
    Ok(())
}
