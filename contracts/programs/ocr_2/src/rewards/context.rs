use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount, Transfer};

use super::state::*;
use crate::state::State;
use crate::ErrorCode;

#[derive(Accounts)]
pub struct InitRewardPool<'info> {
    pub state: AccountLoader<'info, State>,
    #[account(
        init,
        payer = payer,
        space = 8 + RewardPool::INIT_SPACE,
        seeds = [REWARD_POOL_SEED, state.key().as_ref()],
        bump,
    )]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = state.load()?.config.owner @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(address = state.load()?.config.token_mint @ ErrorCode::InvalidTokenAccount)]
    pub token_mint: Box<Account<'info, Mint>>,
    /// CHECK: This is a PDA
    #[account(seeds = [REWARD_VAULT_SEED, pool.key().as_ref()], bump)]
    pub vault_authority: AccountInfo<'info>,
    // Created by the caller beforehand, like the aggregator vault.
    #[account(
        associated_token::mint = token_mint,
        associated_token::authority = vault_authority,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    pub system_program: Program<'info, System>,
}

/// Admin-only settings that do not depend on the aggregator state.
#[derive(Accounts)]
pub struct UpdateRewardPool<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, pool.aggregator.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.admin @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptRewardPoolAdmin<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, pool.aggregator.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.proposed_admin @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ConfigureRewardPool<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(address = pool.admin @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,
}

/// Permissionless crank releasing rewards for the rounds transmitted since the last distribution.
#[derive(Accounts)]
pub struct DistributeRewards<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
}

#[derive(Accounts)]
pub struct OpenSponsorPosition<'info> {
    #[account(seeds = [REWARD_POOL_SEED, pool.aggregator.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(
        init,
        payer = sponsor,
        space = 8 + SponsorPosition::INIT_SPACE,
        seeds = [SPONSOR_SEED, pool.key().as_ref(), sponsor.key().as_ref()],
        bump,
    )]
    pub position: Box<Account<'info, SponsorPosition>>,
    #[account(mut)]
    pub sponsor: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositRewards<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(
        mut,
        seeds = [SPONSOR_SEED, pool.key().as_ref(), sponsor.key().as_ref()],
        bump = position.bump,
        has_one = pool,
        has_one = sponsor,
    )]
    pub position: Box<Account<'info, SponsorPosition>>,
    pub sponsor: Signer<'info>,
    #[account(
        mut,
        token::mint = pool.token_mint,
        token::authority = sponsor,
    )]
    pub sponsor_token_account: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

impl<'info> DepositRewards<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.sponsor_token_account.to_account_info(),
                to: self.token_vault.to_account_info(),
                authority: self.sponsor.to_account_info(),
            },
        )
    }
}

/// Used by request_sponsor_withdrawal and cancel_sponsor_withdrawal.
#[derive(Accounts)]
pub struct UpdateSponsorWithdrawal<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(
        mut,
        seeds = [SPONSOR_SEED, pool.key().as_ref(), sponsor.key().as_ref()],
        bump = position.bump,
        has_one = pool,
        has_one = sponsor,
    )]
    pub position: Box<Account<'info, SponsorPosition>>,
    pub sponsor: Signer<'info>,
}

#[derive(Accounts)]
pub struct ExecuteSponsorWithdrawal<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, pool.aggregator.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(
        mut,
        seeds = [SPONSOR_SEED, pool.key().as_ref(), sponsor.key().as_ref()],
        bump = position.bump,
        has_one = pool,
        has_one = sponsor,
    )]
    pub position: Box<Account<'info, SponsorPosition>>,
    pub sponsor: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint)]
    pub recipient: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: This is a PDA
    #[account(seeds = [REWARD_VAULT_SEED, pool.key().as_ref()], bump = pool.vault_authority_bump)]
    pub vault_authority: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> ExecuteSponsorWithdrawal<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.token_vault.to_account_info(),
                to: self.recipient.to_account_info(),
                authority: self.vault_authority.to_account_info(),
            },
        )
    }
}

#[derive(Accounts)]
pub struct RegisterOracle<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(
        init,
        payer = payer,
        space = 8 + OracleRewardRecord::INIT_SPACE,
        seeds = [ORACLE_REWARD_SEED, pool.key().as_ref(), transmitter.key().as_ref()],
        bump,
    )]
    pub record: Box<Account<'info, OracleRewardRecord>>,
    pub transmitter: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReactivateOracle<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(
        mut,
        seeds = [ORACLE_REWARD_SEED, pool.key().as_ref(), transmitter.key().as_ref()],
        bump = record.bump,
        has_one = pool,
        has_one = transmitter,
    )]
    pub record: Box<Account<'info, OracleRewardRecord>>,
    pub transmitter: Signer<'info>,
}

/// Permissionless: anyone can deregister an oracle that left the aggregator so it stops diluting
/// the rewards of the remaining oracles.
#[derive(Accounts)]
pub struct DeregisterOracle<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(
        mut,
        seeds = [ORACLE_REWARD_SEED, pool.key().as_ref(), record.transmitter.as_ref()],
        bump = record.bump,
        has_one = pool,
    )]
    pub record: Box<Account<'info, OracleRewardRecord>>,
}

#[derive(Accounts)]
pub struct ClaimOracleRewards<'info> {
    #[account(mut, seeds = [REWARD_POOL_SEED, state.key().as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, RewardPool>>,
    #[account(address = pool.aggregator)]
    pub state: AccountLoader<'info, State>,
    #[account(
        mut,
        seeds = [ORACLE_REWARD_SEED, pool.key().as_ref(), record.transmitter.as_ref()],
        bump = record.bump,
        has_one = pool,
    )]
    pub record: Box<Account<'info, OracleRewardRecord>>,
    pub authority: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint)]
    pub recipient: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: This is a PDA
    #[account(seeds = [REWARD_VAULT_SEED, pool.key().as_ref()], bump = pool.vault_authority_bump)]
    pub vault_authority: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> ClaimOracleRewards<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.token_vault.to_account_info(),
                to: self.recipient.to_account_info(),
                authority: self.vault_authority.to_account_info(),
            },
        )
    }
}
