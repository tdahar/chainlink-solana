use anchor_lang::prelude::*;
use anchor_spl::token;

use super::{now, sync};
use crate::context::*;
use crate::error::StakingError;
use crate::event;
use crate::math;
use crate::state::*;

/// Slashes `operator` when the feed's latest round is older than the pool's staleness threshold, and pays
/// the alerter part of the slash. Each operator is slashed at most once per feed round, so a feed that
/// stays stale on the same round cannot be used to slash the same operator twice; it can be slashed again
/// only after the feed moved on and went stale again.
pub fn raise_alert(ctx: Context<RaiseAlert>) -> Result<()> {
    require!(!ctx.accounts.pool.paused, StakingError::PoolPaused);
    require!(ctx.accounts.operator.active, StakingError::OperatorInactive);
    let now = now()?;

    let round = store::accessors::latest_round_data(
        ctx.accounts.store_program.to_account_info(),
        ctx.accounts.feed.to_account_info(),
    )?;
    require!(round.round_id > 0, StakingError::FeedEmpty);
    let age = now
        .checked_sub(i64::from(round.timestamp))
        .ok_or(StakingError::Overflow)?;
    require!(
        age > ctx.accounts.pool.config.alert_staleness_secs,
        StakingError::FeedNotStale
    );

    let slash = {
        let pool = &mut ctx.accounts.pool;
        let operator = &mut ctx.accounts.operator;
        require!(
            operator.last_slashed_round_id != round.round_id,
            StakingError::AlreadySlashed
        );

        // Rewards earned before the alert are paid on the stake before the slash.
        sync(pool, operator, now)?;

        let slash = math::slash(
            operator.self_stake,
            operator.delegated_amount,
            pool.config.slash_bps,
            pool.config.alerter_reward_bps,
        )
        .ok_or(StakingError::Overflow)?;
        let total = slash
            .from_self
            .checked_add(slash.from_delegated)
            .ok_or(StakingError::Overflow)?;

        operator.self_stake = operator
            .self_stake
            .checked_sub(slash.from_self)
            .ok_or(StakingError::Overflow)?;
        operator.delegated_amount = operator
            .delegated_amount
            .checked_sub(slash.from_delegated)
            .ok_or(StakingError::Overflow)?;
        operator.total_slashed = operator
            .total_slashed
            .checked_add(total)
            .ok_or(StakingError::Overflow)?;
        operator.last_slashed_round_id = round.round_id;

        pool.total_staked = pool
            .total_staked
            .checked_sub(total)
            .ok_or(StakingError::Overflow)?;
        pool.slashed_reserve = pool
            .slashed_reserve
            .checked_add(slash.to_reserve)
            .ok_or(StakingError::Overflow)?;

        emit!(event::AlertRaised {
            pool: pool.key(),
            operator: operator.key(),
            alerter: ctx.accounts.alerter.key(),
            round_id: round.round_id,
            round_timestamp: round.timestamp,
            slashed_self: slash.from_self,
            slashed_delegated: slash.from_delegated,
            alerter_reward: slash.alerter_reward,
        });
        slash
    };

    if slash.alerter_reward == 0 {
        return Ok(());
    }
    let pool_key = ctx.accounts.pool.key();
    let bump = ctx.accounts.pool.vault_authority_bump;
    token::transfer(
        ctx.accounts
            .transfer_ctx()
            .with_signer(&[&[VAULT_SEED, pool_key.as_ref(), &[bump]]]),
        slash.alerter_reward,
    )
}
