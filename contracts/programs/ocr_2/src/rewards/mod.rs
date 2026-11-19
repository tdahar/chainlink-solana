//! Sponsor-funded oracle rewards.
//!
//! Lets third parties (typically protocols consuming a feed) top up the rewards of the oracles
//! serving an aggregator without touching the aggregator's own billing. See
//! `docs/ocr2-reward-pool/README.md` for the full flow.

use anchor_lang::prelude::*;
use anchor_spl::token;

pub mod context;
pub mod math;
pub mod state;

use self::context::*;
use self::state::*;
use crate::event;
use crate::state::State;
use crate::ErrorCode;

fn latest_round_id(state: &AccountLoader<State>) -> Result<u32> {
    Ok(state.load()?.config.latest_aggregator_round_id)
}

/// Returns the payee of `transmitter` if it is part of the current aggregator config.
fn current_payee(state: &AccountLoader<State>, transmitter: &Pubkey) -> Result<Option<Pubkey>> {
    let state = state.load()?;
    Ok(state
        .oracles
        .iter()
        .find(|oracle| &oracle.transmitter == transmitter)
        .map(|oracle| oracle.payee))
}

fn validate_withdrawal_delay(withdrawal_delay_secs: i64) -> Result<()> {
    require!(
        (0..=MAX_WITHDRAWAL_DELAY_SECS).contains(&withdrawal_delay_secs),
        ErrorCode::InvalidWithdrawalDelay
    );
    Ok(())
}

/// Releases rewards for the rounds transmitted since the last distribution to the oracles
/// registered right now. Must run before anything that changes the rate, the registered oracle
/// count or the distributable balance, so past rounds are paid with the parameters they ran under.
fn distribute(pool: &mut RewardPool, latest_round_id: u32) -> Result<()> {
    let rounds = latest_round_id
        .checked_sub(pool.last_distributed_round_id)
        .ok_or(ErrorCode::Overflow)?;
    if rounds == 0 {
        return Ok(());
    }
    pool.last_distributed_round_id = latest_round_id;

    // Rounds transmitted while the pool is paused are not rewarded.
    if pool.paused {
        return Ok(());
    }

    let distributable = pool.distributable_gjuels().ok_or(ErrorCode::Overflow)?;
    let distribution = math::distribution(
        pool.reward_per_round_gjuels,
        rounds,
        pool.registered_oracles,
        distributable,
    )
    .ok_or(ErrorCode::Overflow)?;

    if distribution.allocated_gjuels == 0 {
        return Ok(());
    }

    pool.acc_reward_per_oracle = pool
        .acc_reward_per_oracle
        .checked_add(distribution.acc_increase)
        .ok_or(ErrorCode::Overflow)?;
    pool.total_allocated_gjuels = pool
        .total_allocated_gjuels
        .checked_add(distribution.allocated_gjuels)
        .ok_or(ErrorCode::Overflow)?;

    emit!(event::RewardsDistributed {
        aggregator: pool.aggregator,
        round_id: latest_round_id,
        rounds,
        registered_oracles: pool.registered_oracles,
        allocated_gjuels: distribution.allocated_gjuels,
    });
    Ok(())
}

/// Moves the rewards accrued since `record.reward_debt` into `record.accrued_gjuels`.
fn settle(record: &mut OracleRewardRecord, acc_reward_per_oracle: u128) -> Result<()> {
    let pending = math::pending_reward(acc_reward_per_oracle, record.reward_debt)
        .ok_or(ErrorCode::Overflow)?;
    record.accrued_gjuels = record
        .accrued_gjuels
        .checked_add(pending)
        .ok_or(ErrorCode::Overflow)?;
    record.reward_debt = acc_reward_per_oracle;
    Ok(())
}

// -- Admin

pub fn init_reward_pool(
    ctx: Context<InitRewardPool>,
    reward_per_round_gjuels: u64,
    withdrawal_delay_secs: i64,
) -> Result<()> {
    validate_withdrawal_delay(withdrawal_delay_secs)?;
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;

    let pool = &mut ctx.accounts.pool;
    pool.version = REWARD_POOL_VERSION;
    pool.bump = ctx.bumps.pool;
    pool.vault_authority_bump = ctx.bumps.vault_authority;
    pool.paused = false;
    pool.aggregator = ctx.accounts.state.key();
    pool.admin = ctx.accounts.authority.key();
    pool.proposed_admin = Pubkey::default();
    pool.token_mint = ctx.accounts.token_mint.key();
    pool.token_vault = ctx.accounts.token_vault.key();
    pool.reward_per_round_gjuels = reward_per_round_gjuels;
    pool.withdrawal_delay_secs = withdrawal_delay_secs;
    // Only rounds transmitted after the pool exists are rewarded.
    pool.last_distributed_round_id = latest_round_id;

    emit!(event::RewardPoolInitialized {
        aggregator: pool.aggregator,
        pool: pool.key(),
        admin: pool.admin,
        reward_per_round_gjuels,
        withdrawal_delay_secs,
    });
    Ok(())
}

pub fn set_reward_rate(
    ctx: Context<ConfigureRewardPool>,
    reward_per_round_gjuels: u64,
) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let pool = &mut ctx.accounts.pool;

    // Pay out the rounds so far at the old rate.
    distribute(pool, latest_round_id)?;
    pool.reward_per_round_gjuels = reward_per_round_gjuels;

    emit!(event::RewardRateSet {
        aggregator: pool.aggregator,
        reward_per_round_gjuels,
    });
    Ok(())
}

pub fn set_reward_pool_paused(ctx: Context<ConfigureRewardPool>, paused: bool) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let pool = &mut ctx.accounts.pool;

    // Rounds before the switch are paid (or not) according to the previous setting.
    distribute(pool, latest_round_id)?;
    pool.paused = paused;
    Ok(())
}

pub fn set_withdrawal_delay(
    ctx: Context<UpdateRewardPool>,
    withdrawal_delay_secs: i64,
) -> Result<()> {
    validate_withdrawal_delay(withdrawal_delay_secs)?;
    // Requests already pending keep the unlock time they were given.
    ctx.accounts.pool.withdrawal_delay_secs = withdrawal_delay_secs;
    Ok(())
}

pub fn transfer_reward_pool_admin(
    ctx: Context<UpdateRewardPool>,
    proposed_admin: Pubkey,
) -> Result<()> {
    ctx.accounts.pool.proposed_admin = proposed_admin;
    Ok(())
}

pub fn accept_reward_pool_admin(ctx: Context<AcceptRewardPoolAdmin>) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    pool.admin = std::mem::take(&mut pool.proposed_admin);
    Ok(())
}

pub fn distribute_rewards(ctx: Context<DistributeRewards>) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    distribute(&mut ctx.accounts.pool, latest_round_id)
}

// -- Sponsors

pub fn open_sponsor_position(ctx: Context<OpenSponsorPosition>) -> Result<()> {
    let position = &mut ctx.accounts.position;
    position.bump = ctx.bumps.position;
    position.pool = ctx.accounts.pool.key();
    position.sponsor = ctx.accounts.sponsor.key();
    Ok(())
}

pub fn deposit_rewards(ctx: Context<DepositRewards>, amount_gjuels: u64) -> Result<()> {
    require!(amount_gjuels > 0, ErrorCode::InvalidInput);
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;

    // Rounds before the deposit are paid with the balance that existed when they ran.
    distribute(&mut ctx.accounts.pool, latest_round_id)?;

    token::transfer(ctx.accounts.transfer_ctx(), amount_gjuels)?;

    let pool = &mut ctx.accounts.pool;
    pool.total_deposited_gjuels = pool
        .total_deposited_gjuels
        .checked_add(amount_gjuels)
        .ok_or(ErrorCode::Overflow)?;
    let position = &mut ctx.accounts.position;
    position.deposited_gjuels = position
        .deposited_gjuels
        .checked_add(amount_gjuels)
        .ok_or(ErrorCode::Overflow)?;

    emit!(event::RewardsDeposited {
        aggregator: pool.aggregator,
        sponsor: position.sponsor,
        amount_gjuels,
    });
    Ok(())
}

pub fn request_sponsor_withdrawal(
    ctx: Context<UpdateSponsorWithdrawal>,
    amount_gjuels: u64,
) -> Result<()> {
    require!(amount_gjuels > 0, ErrorCode::InvalidInput);
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let now = Clock::get()?.unix_timestamp;

    let pool = &mut ctx.accounts.pool;
    let position = &mut ctx.accounts.position;
    require!(
        position.pending_withdrawal_gjuels == 0,
        ErrorCode::WithdrawalPending
    );

    // Rewards already earned by oracles are not withdrawable.
    distribute(pool, latest_round_id)?;

    let available = math::withdrawable(
        position.outstanding_gjuels().ok_or(ErrorCode::Overflow)?,
        pool.distributable_gjuels().ok_or(ErrorCode::Overflow)?,
    );
    require!(
        amount_gjuels <= available,
        ErrorCode::InsufficientUnallocated
    );

    // Reserve the amount now so it is no longer released to oracles while the delay runs.
    pool.total_pending_withdrawal_gjuels = pool
        .total_pending_withdrawal_gjuels
        .checked_add(amount_gjuels)
        .ok_or(ErrorCode::Overflow)?;
    position.pending_withdrawal_gjuels = amount_gjuels;
    position.withdrawal_unlock_ts = now
        .checked_add(pool.withdrawal_delay_secs)
        .ok_or(ErrorCode::Overflow)?;

    emit!(event::SponsorWithdrawalRequested {
        aggregator: pool.aggregator,
        sponsor: position.sponsor,
        amount_gjuels,
        unlock_ts: position.withdrawal_unlock_ts,
    });
    Ok(())
}

pub fn cancel_sponsor_withdrawal(ctx: Context<UpdateSponsorWithdrawal>) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let pool = &mut ctx.accounts.pool;
    let position = &mut ctx.accounts.position;

    let amount_gjuels = position.pending_withdrawal_gjuels;
    require!(amount_gjuels > 0, ErrorCode::NoPendingWithdrawal);

    // The reserved amount becomes distributable again from the next round on.
    distribute(pool, latest_round_id)?;

    pool.total_pending_withdrawal_gjuels = pool
        .total_pending_withdrawal_gjuels
        .checked_sub(amount_gjuels)
        .ok_or(ErrorCode::Overflow)?;
    position.pending_withdrawal_gjuels = 0;
    position.withdrawal_unlock_ts = 0;

    emit!(event::SponsorWithdrawalCancelled {
        aggregator: pool.aggregator,
        sponsor: position.sponsor,
        amount_gjuels,
    });
    Ok(())
}

pub fn execute_sponsor_withdrawal(ctx: Context<ExecuteSponsorWithdrawal>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    let amount_gjuels = {
        let pool = &mut ctx.accounts.pool;
        let position = &mut ctx.accounts.position;

        require!(
            position.pending_withdrawal_gjuels > 0,
            ErrorCode::NoPendingWithdrawal
        );
        require!(
            now >= position.withdrawal_unlock_ts,
            ErrorCode::WithdrawalLocked
        );

        let amount_gjuels = std::mem::take(&mut position.pending_withdrawal_gjuels);
        position.withdrawal_unlock_ts = 0;
        position.withdrawn_gjuels = position
            .withdrawn_gjuels
            .checked_add(amount_gjuels)
            .ok_or(ErrorCode::Overflow)?;

        pool.total_pending_withdrawal_gjuels = pool
            .total_pending_withdrawal_gjuels
            .checked_sub(amount_gjuels)
            .ok_or(ErrorCode::Overflow)?;
        pool.total_withdrawn_gjuels = pool
            .total_withdrawn_gjuels
            .checked_add(amount_gjuels)
            .ok_or(ErrorCode::Overflow)?;

        emit!(event::SponsorWithdrawn {
            aggregator: pool.aggregator,
            sponsor: position.sponsor,
            recipient: ctx.accounts.recipient.key(),
            amount_gjuels,
        });
        amount_gjuels
    };

    let pool_key = ctx.accounts.pool.key();
    let vault_authority_bump = ctx.accounts.pool.vault_authority_bump;
    token::transfer(
        ctx.accounts.transfer_ctx().with_signer(&[&[
            REWARD_VAULT_SEED,
            pool_key.as_ref(),
            &[vault_authority_bump],
        ]]),
        amount_gjuels,
    )?;
    Ok(())
}

// -- Oracles

pub fn register_oracle(ctx: Context<RegisterOracle>) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let transmitter = ctx.accounts.transmitter.key();
    let payee = current_payee(&ctx.accounts.state, &transmitter)?
        .ok_or(ErrorCode::UnauthorizedTransmitter)?;

    let pool = &mut ctx.accounts.pool;
    // Pay out the rounds so far to the oracles already registered, before this one counts.
    distribute(pool, latest_round_id)?;

    let record = &mut ctx.accounts.record;
    record.bump = ctx.bumps.record;
    record.pool = pool.key();
    record.transmitter = transmitter;
    record.payee = payee;
    // Start accruing from the current index.
    settle(record, pool.acc_reward_per_oracle)?;
    record.active = true;

    pool.registered_oracles = pool
        .registered_oracles
        .checked_add(1)
        .ok_or(ErrorCode::Overflow)?;

    emit!(event::OracleRegistered {
        aggregator: pool.aggregator,
        transmitter,
        payee,
    });
    Ok(())
}

pub fn reactivate_oracle(ctx: Context<ReactivateOracle>) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let transmitter = ctx.accounts.transmitter.key();
    let payee = current_payee(&ctx.accounts.state, &transmitter)?
        .ok_or(ErrorCode::UnauthorizedTransmitter)?;

    let pool = &mut ctx.accounts.pool;
    let record = &mut ctx.accounts.record;
    require!(!record.active, ErrorCode::OracleAlreadyActive);

    distribute(pool, latest_round_id)?;

    // Rewards released while the oracle was inactive belong to the other oracles.
    record.reward_debt = pool.acc_reward_per_oracle;
    record.payee = payee;
    record.active = true;

    pool.registered_oracles = pool
        .registered_oracles
        .checked_add(1)
        .ok_or(ErrorCode::Overflow)?;

    emit!(event::OracleRegistered {
        aggregator: pool.aggregator,
        transmitter,
        payee,
    });
    Ok(())
}

pub fn deregister_oracle(ctx: Context<DeregisterOracle>) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let transmitter = ctx.accounts.record.transmitter;
    require!(
        current_payee(&ctx.accounts.state, &transmitter)?.is_none(),
        ErrorCode::OracleStillActive
    );

    let pool = &mut ctx.accounts.pool;
    let record = &mut ctx.accounts.record;
    require!(record.active, ErrorCode::OracleNotRegistered);

    // Rounds up to now still count for this oracle; it keeps what it earned and can claim it later.
    distribute(pool, latest_round_id)?;
    settle(record, pool.acc_reward_per_oracle)?;
    record.active = false;

    pool.registered_oracles = pool
        .registered_oracles
        .checked_sub(1)
        .ok_or(ErrorCode::Overflow)?;

    emit!(event::OracleDeregistered {
        aggregator: pool.aggregator,
        transmitter,
        accrued_gjuels: record.accrued_gjuels,
    });
    Ok(())
}

pub fn claim_oracle_rewards(ctx: Context<ClaimOracleRewards>) -> Result<()> {
    let latest_round_id = latest_round_id(&ctx.accounts.state)?;
    let payee = current_payee(&ctx.accounts.state, &ctx.accounts.record.transmitter)?;

    let amount_gjuels = {
        let pool = &mut ctx.accounts.pool;
        let record = &mut ctx.accounts.record;

        distribute(pool, latest_round_id)?;
        if record.active {
            settle(record, pool.acc_reward_per_oracle)?;
        }
        // Keep the last known payee for oracles that later leave the aggregator.
        if let Some(payee) = payee {
            record.payee = payee;
        }

        // Validate that the instruction was signed by the same authority as the token account
        require!(
            ctx.accounts.recipient.owner == ctx.accounts.authority.key(),
            ErrorCode::Unauthorized
        );

        let amount_gjuels = std::mem::take(&mut record.accrued_gjuels);
        if amount_gjuels == 0 {
            return Ok(());
        }
        record.claimed_gjuels = record
            .claimed_gjuels
            .checked_add(amount_gjuels)
            .ok_or(ErrorCode::Overflow)?;
        pool.total_claimed_gjuels = pool
            .total_claimed_gjuels
            .checked_add(amount_gjuels)
            .ok_or(ErrorCode::Overflow)?;

        emit!(event::OracleRewardsClaimed {
            aggregator: pool.aggregator,
            transmitter: record.transmitter,
            payee: record.payee,
            amount_gjuels,
        });
        amount_gjuels
    };

    let pool_key = ctx.accounts.pool.key();
    let vault_authority_bump = ctx.accounts.pool.vault_authority_bump;
    token::transfer(
        ctx.accounts.transfer_ctx().with_signer(&[&[
            REWARD_VAULT_SEED,
            pool_key.as_ref(),
            &[vault_authority_bump],
        ]]),
        amount_gjuels,
    )?;
    Ok(())
}
