//! Staking, delegation and alert-based slashing for the node operators of an OCR2 feed.
//!
//! See `docs/operator-staking/README.md` for the design.

#![allow(deprecated)] // anchor-lang 0.31 #[program] macro uses AccountInfo::realloc
use anchor_lang::prelude::*;

declare_id!("9h5WUYEfq6Fuj8wCpXYnCA6EDyzHGae1iwEWLfKJhzhY");

pub mod context;
pub mod error;
pub mod event;
pub mod instructions;
pub mod math;
pub mod query;
pub mod state;

use crate::context::*;
use crate::state::PoolConfig;

#[program]
pub mod operator_staking {
    use super::*;

    // -- Pool administration

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        config: PoolConfig,
        reward_rate_per_sec: u64,
    ) -> Result<()> {
        instructions::admin::initialize_pool(ctx, config, reward_rate_per_sec)
    }

    pub fn set_pool_config(ctx: Context<AdminUpdate>, config: PoolConfig) -> Result<()> {
        instructions::admin::set_pool_config(ctx, config)
    }

    pub fn set_paused(ctx: Context<AdminUpdate>, paused: bool) -> Result<()> {
        instructions::admin::set_paused(ctx, paused)
    }

    pub fn set_reward_rate(ctx: Context<AdminUpdate>, reward_rate_per_sec: u64) -> Result<()> {
        instructions::admin::set_reward_rate(ctx, reward_rate_per_sec)
    }

    pub fn transfer_admin(ctx: Context<AdminUpdate>, proposed_admin: Pubkey) -> Result<()> {
        instructions::admin::transfer_admin(ctx, proposed_admin)
    }

    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        instructions::admin::accept_admin(ctx)
    }

    pub fn fund_rewards(ctx: Context<FundRewards>, amount: u64) -> Result<()> {
        instructions::admin::fund_rewards(ctx, amount)
    }

    pub fn withdraw_slashed(ctx: Context<WithdrawSlashed>, amount: u64) -> Result<()> {
        instructions::admin::withdraw_slashed(ctx, amount)
    }

    // -- Operators

    pub fn register_operator(
        ctx: Context<RegisterOperator>,
        commission_bps: u16,
        stake: u64,
    ) -> Result<()> {
        instructions::operator::register_operator(ctx, commission_bps, stake)
    }

    pub fn set_commission(ctx: Context<OperatorUpdate>, commission_bps: u16) -> Result<()> {
        instructions::operator::set_commission(ctx, commission_bps)
    }

    pub fn add_operator_stake(ctx: Context<AddOperatorStake>, amount: u64) -> Result<()> {
        instructions::operator::add_operator_stake(ctx, amount)
    }

    pub fn request_operator_unstake(ctx: Context<OperatorUpdate>, amount: u64) -> Result<()> {
        instructions::operator::request_operator_unstake(ctx, amount)
    }

    pub fn withdraw_operator_unbonded(ctx: Context<OperatorPayout>) -> Result<()> {
        instructions::operator::withdraw_operator_unbonded(ctx)
    }

    pub fn claim_operator_rewards(ctx: Context<OperatorPayout>) -> Result<()> {
        instructions::operator::claim_operator_rewards(ctx)
    }

    // -- Delegators

    pub fn open_delegation(ctx: Context<OpenDelegation>) -> Result<()> {
        instructions::delegator::open_delegation(ctx)
    }

    pub fn delegate(ctx: Context<Delegate>, amount: u64) -> Result<()> {
        instructions::delegator::delegate(ctx, amount)
    }

    pub fn request_undelegate(ctx: Context<DelegationUpdate>, shares: u64) -> Result<()> {
        instructions::delegator::request_undelegate(ctx, shares)
    }

    pub fn withdraw_delegation_unbonded(ctx: Context<DelegationPayout>) -> Result<()> {
        instructions::delegator::withdraw_delegation_unbonded(ctx)
    }

    pub fn claim_delegation_rewards(ctx: Context<DelegationPayout>) -> Result<()> {
        instructions::delegator::claim_delegation_rewards(ctx)
    }

    // -- Alerts

    pub fn raise_alert(ctx: Context<RaiseAlert>) -> Result<()> {
        instructions::alert::raise_alert(ctx)
    }
}
