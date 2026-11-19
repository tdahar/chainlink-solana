use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount, Transfer};

use crate::error::StakingError;
use crate::state::*;
use store::{Store, Transmissions};

// -- Pool administration

#[derive(Accounts)]
pub struct InitializePool<'info> {
    /// The feed the pool's operators serve.
    pub feed: Box<Account<'info, Transmissions>>,
    #[account(
        init,
        payer = payer,
        space = 8 + StakingPool::INIT_SPACE,
        seeds = [POOL_SEED, feed.key().as_ref()],
        bump,
    )]
    pub pool: Box<Account<'info, StakingPool>>,
    pub admin: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,

    pub token_mint: Box<Account<'info, Mint>>,
    /// CHECK: This is a PDA
    #[account(seeds = [VAULT_SEED, pool.key().as_ref()], bump)]
    pub vault_authority: AccountInfo<'info>,
    // Created by the caller beforehand.
    #[account(
        associated_token::mint = token_mint,
        associated_token::authority = vault_authority,
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    pub system_program: Program<'info, System>,
}

/// Admin-only pool settings.
#[derive(Accounts)]
pub struct AdminUpdate<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(address = pool.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptAdmin<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(address = pool.proposed_admin @ StakingError::Unauthorized)]
    pub proposed_admin: Signer<'info>,
}

/// Anyone can fund rewards.
#[derive(Accounts)]
pub struct FundRewards<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    pub funder: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint, token::authority = funder)]
    pub funder_token: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

impl<'info> FundRewards<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        deposit_ctx(
            &self.token_program,
            &self.funder_token,
            &self.token_vault,
            &self.funder,
        )
    }
}

#[derive(Accounts)]
pub struct WithdrawSlashed<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(address = pool.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint)]
    pub recipient: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: This is a PDA
    #[account(seeds = [VAULT_SEED, pool.key().as_ref()], bump = pool.vault_authority_bump)]
    pub vault_authority: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> WithdrawSlashed<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        payout_ctx(
            &self.token_program,
            &self.token_vault,
            &self.recipient,
            &self.vault_authority,
        )
    }
}

// -- Operators

#[derive(Accounts)]
pub struct RegisterOperator<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(
        init,
        payer = authority,
        space = 8 + Operator::INIT_SPACE,
        seeds = [OPERATOR_SEED, pool.key().as_ref(), authority.key().as_ref()],
        bump,
    )]
    pub operator: Box<Account<'info, Operator>>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint, token::authority = authority)]
    pub authority_token: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

impl<'info> RegisterOperator<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        deposit_ctx(
            &self.token_program,
            &self.authority_token,
            &self.token_vault,
            &self.authority,
        )
    }
}

/// Operator instructions that move no tokens.
#[derive(Accounts)]
pub struct OperatorUpdate<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(
        mut,
        seeds = [OPERATOR_SEED, pool.key().as_ref(), authority.key().as_ref()],
        bump = operator.bump,
        has_one = pool,
        has_one = authority,
    )]
    pub operator: Box<Account<'info, Operator>>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct AddOperatorStake<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(
        mut,
        seeds = [OPERATOR_SEED, pool.key().as_ref(), authority.key().as_ref()],
        bump = operator.bump,
        has_one = pool,
        has_one = authority,
    )]
    pub operator: Box<Account<'info, Operator>>,
    pub authority: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint, token::authority = authority)]
    pub authority_token: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

impl<'info> AddOperatorStake<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        deposit_ctx(
            &self.token_program,
            &self.authority_token,
            &self.token_vault,
            &self.authority,
        )
    }
}

/// Used by claim_operator_rewards and withdraw_operator_unbonded.
#[derive(Accounts)]
pub struct OperatorPayout<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(
        mut,
        seeds = [OPERATOR_SEED, pool.key().as_ref(), authority.key().as_ref()],
        bump = operator.bump,
        has_one = pool,
        has_one = authority,
    )]
    pub operator: Box<Account<'info, Operator>>,
    pub authority: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint)]
    pub recipient: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: This is a PDA
    #[account(seeds = [VAULT_SEED, pool.key().as_ref()], bump = pool.vault_authority_bump)]
    pub vault_authority: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> OperatorPayout<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        payout_ctx(
            &self.token_program,
            &self.token_vault,
            &self.recipient,
            &self.vault_authority,
        )
    }
}

// -- Delegators

#[derive(Accounts)]
pub struct OpenDelegation<'info> {
    #[account(seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(mut, has_one = pool)]
    pub operator: Box<Account<'info, Operator>>,
    #[account(
        init,
        payer = delegator,
        space = 8 + Delegation::INIT_SPACE,
        seeds = [DELEGATION_SEED, operator.key().as_ref(), delegator.key().as_ref()],
        bump,
    )]
    pub delegation: Box<Account<'info, Delegation>>,
    #[account(mut)]
    pub delegator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Delegate<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(mut, has_one = pool)]
    pub operator: Box<Account<'info, Operator>>,
    #[account(
        mut,
        seeds = [DELEGATION_SEED, operator.key().as_ref(), delegator.key().as_ref()],
        bump = delegation.bump,
        has_one = operator,
        has_one = delegator,
    )]
    pub delegation: Box<Account<'info, Delegation>>,
    pub delegator: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint, token::authority = delegator)]
    pub delegator_token: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

impl<'info> Delegate<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        deposit_ctx(
            &self.token_program,
            &self.delegator_token,
            &self.token_vault,
            &self.delegator,
        )
    }
}

/// Delegator instructions that move no tokens.
#[derive(Accounts)]
pub struct DelegationUpdate<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(mut, has_one = pool)]
    pub operator: Box<Account<'info, Operator>>,
    #[account(
        mut,
        seeds = [DELEGATION_SEED, operator.key().as_ref(), delegator.key().as_ref()],
        bump = delegation.bump,
        has_one = operator,
        has_one = delegator,
    )]
    pub delegation: Box<Account<'info, Delegation>>,
    pub delegator: Signer<'info>,
}

/// Used by claim_delegation_rewards and withdraw_delegation_unbonded.
#[derive(Accounts)]
pub struct DelegationPayout<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(mut, has_one = pool)]
    pub operator: Box<Account<'info, Operator>>,
    #[account(
        mut,
        seeds = [DELEGATION_SEED, operator.key().as_ref(), delegator.key().as_ref()],
        bump = delegation.bump,
        has_one = operator,
        has_one = delegator,
    )]
    pub delegation: Box<Account<'info, Delegation>>,
    pub delegator: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint)]
    pub recipient: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: This is a PDA
    #[account(seeds = [VAULT_SEED, pool.key().as_ref()], bump = pool.vault_authority_bump)]
    pub vault_authority: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> DelegationPayout<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        payout_ctx(
            &self.token_program,
            &self.token_vault,
            &self.recipient,
            &self.vault_authority,
        )
    }
}

// -- Alerts

/// Permissionless: anyone who sees the feed go stale can slash an operator of the pool and is paid part
/// of the slash.
#[derive(Accounts)]
pub struct RaiseAlert<'info> {
    #[account(mut, seeds = [POOL_SEED, pool.feed.as_ref()], bump = pool.bump)]
    pub pool: Box<Account<'info, StakingPool>>,
    #[account(mut, has_one = pool)]
    pub operator: Box<Account<'info, Operator>>,
    /// The feed whose latest round proves the staleness.
    pub feed: Box<Account<'info, Transmissions>>,
    pub store_program: Program<'info, Store>,
    pub alerter: Signer<'info>,
    #[account(mut, token::mint = pool.token_mint)]
    pub alerter_token: Box<Account<'info, TokenAccount>>,
    #[account(mut, address = pool.token_vault)]
    pub token_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK: This is a PDA
    #[account(seeds = [VAULT_SEED, pool.key().as_ref()], bump = pool.vault_authority_bump)]
    pub vault_authority: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> RaiseAlert<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        payout_ctx(
            &self.token_program,
            &self.token_vault,
            &self.alerter_token,
            &self.vault_authority,
        )
    }
}

// -- Helpers

fn deposit_ctx<'a, 'b, 'c, 'info>(
    token_program: &Program<'info, Token>,
    from: &Account<'info, TokenAccount>,
    vault: &Account<'info, TokenAccount>,
    authority: &Signer<'info>,
) -> CpiContext<'a, 'b, 'c, 'info, Transfer<'info>> {
    CpiContext::new(
        token_program.to_account_info(),
        Transfer {
            from: from.to_account_info(),
            to: vault.to_account_info(),
            authority: authority.to_account_info(),
        },
    )
}

fn payout_ctx<'a, 'b, 'c, 'info>(
    token_program: &Program<'info, Token>,
    vault: &Account<'info, TokenAccount>,
    to: &Account<'info, TokenAccount>,
    vault_authority: &AccountInfo<'info>,
) -> CpiContext<'a, 'b, 'c, 'info, Transfer<'info>> {
    CpiContext::new(
        token_program.to_account_info(),
        Transfer {
            from: vault.to_account_info(),
            to: to.to_account_info(),
            authority: vault_authority.clone(),
        },
    )
}
