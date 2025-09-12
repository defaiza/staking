use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface, transfer_checked, TransferChecked};

declare_id!("2TLhCW35y5jcuoKtfwTx7H5EPMqUtCf3UQhYKdKKg3Hq");

// Constants for sustainable economics
pub const GOLD_MIN: u64 = 10_000_000 * 10u64.pow(6);      // 10M DEFAI
pub const GOLD_MAX: u64 = 99_999_999 * 10u64.pow(6);      // 99.99M DEFAI
pub const GOLD_APY_BPS: u16 = 50;                         // 0.5% = 50 basis points

pub const TITANIUM_MIN: u64 = 100_000_000 * 10u64.pow(6); // 100M DEFAI
pub const TITANIUM_MAX: u64 = 999_999_999 * 10u64.pow(6); // 999.99M DEFAI
pub const TITANIUM_APY_BPS: u16 = 75;                     // 0.75% = 75 basis points

pub const INFINITE_MIN: u64 = 1_000_000_000 * 10u64.pow(6); // 1B DEFAI
pub const INFINITE_APY_BPS: u16 = 100;                      // 1% = 100 basis points

pub const SECONDS_PER_YEAR: u64 = 31_536_000;
pub const BASIS_POINTS: u64 = 10_000;

// Timelock duration for admin actions
pub const ADMIN_TIMELOCK_DURATION: i64 = 48 * 60 * 60; // 48 hours

// TODO: Replace with actual program initializer authority pubkey for production
// Example: pub const PROGRAM_INITIALIZER: Pubkey = pubkey!("ActualInitializerPubkeyHere");
// For now, we'll add logging to track who initializes the program

#[program]
pub mod defai_staking {
    use super::*;

    pub fn initialize_program(
        ctx: Context<InitializeProgram>,
        defai_mint: Pubkey,
    ) -> Result<()> {
        // TODO: For production, uncomment and implement authority check:
        // require_keys_eq!(
        //     ctx.accounts.authority.key(),
        //     PROGRAM_INITIALIZER,
        //     StakingError::InvalidAuthority
        // );
        
        // Log the initializer for audit trail
        msg!("Program initialized by authority: {}", ctx.accounts.authority.key());
        
        let program_state = &mut ctx.accounts.program_state;
        
        program_state.authority = ctx.accounts.authority.key();
        program_state.defai_mint = defai_mint;
        program_state.total_staked = 0;
        program_state.total_users = 0;
        program_state.paused = false;
        program_state.vault_bump = ctx.bumps.stake_vault;
        program_state.reward_escrow_bump = 0; // Will be set in initialize_escrow
        program_state.escrow_vault_bump = 0;  // Will be set in initialize_escrow
        program_state.pending_authority = None;
        program_state.authority_change_timestamp = 0;
        
        Ok(())
    }

    pub fn initialize_escrow(
        ctx: Context<InitializeEscrow>,
    ) -> Result<()> {
        // Verify the caller is the program authority
        require_keys_eq!(
            ctx.accounts.authority.key(),
            ctx.accounts.program_state.authority,
            StakingError::InvalidAuthority
        );
        
        // Log the escrow initialization
        msg!("Escrow initialized by authority: {}", ctx.accounts.authority.key());
        
        let program_state = &mut ctx.accounts.program_state;
        program_state.reward_escrow_bump = ctx.bumps.reward_escrow;
        program_state.escrow_vault_bump = ctx.bumps.escrow_token_account;
        
        let escrow = &mut ctx.accounts.reward_escrow;
        escrow.authority = program_state.key();
        escrow.total_balance = 0;
        escrow.total_distributed = 0;
        escrow.bump = ctx.bumps.reward_escrow;
        
        Ok(())
    }

    pub fn fund_escrow(
        ctx: Context<FundEscrow>,
        amount: u64,
    ) -> Result<()> {
        // Transfer tokens from funder to escrow
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.funder_token_account.to_account_info(),
                to: ctx.accounts.escrow_token_account.to_account_info(),
                authority: ctx.accounts.funder.to_account_info(),
                mint: ctx.accounts.defai_mint.to_account_info(),
            },
        );
        transfer_checked(transfer_ctx, amount, ctx.accounts.defai_mint.decimals)?;
        
        // Update escrow balance
        let escrow = &mut ctx.accounts.reward_escrow;
        escrow.total_balance = escrow.total_balance.checked_add(amount).unwrap();
        
        emit!(EscrowFundedEvent {
            funder: ctx.accounts.funder.key(),
            amount,
            new_balance: escrow.total_balance,
        });
        
        Ok(())
    }

    pub fn stake_tokens(
        ctx: Context<StakeTokens>,
        amount: u64,
    ) -> Result<()> {
        let program_state = &ctx.accounts.program_state;
        
        // Check if program is paused
        require!(!program_state.paused, StakingError::ProgramPaused);
        
        // Check minimum amount for Gold tier
        require!(amount >= GOLD_MIN, StakingError::AmountTooLow);
        
        // Transfer tokens from user to stake vault
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.user_token_account.to_account_info(),
                to: ctx.accounts.stake_vault.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
                mint: ctx.accounts.defai_mint.to_account_info(),
            },
        );
        transfer_checked(transfer_ctx, amount, ctx.accounts.defai_mint.decimals)?;
        
        // Create or update user stake account
        let user_stake = &mut ctx.accounts.user_stake;
        let clock = Clock::get()?;
        
        if user_stake.owner == Pubkey::default() {
            // New stake
            user_stake.owner = ctx.accounts.user.key();
            user_stake.staked_amount = amount;
            user_stake.stake_timestamp = clock.unix_timestamp;
            user_stake.last_stake_timestamp = clock.unix_timestamp;  // Set both timestamps for new stake
            user_stake.last_claim_timestamp = clock.unix_timestamp;
            user_stake.locked_until = clock.unix_timestamp + 7 * 24 * 60 * 60; // 7 day initial lock
            user_stake.rewards_earned = 0;
            user_stake.rewards_claimed = 0;
            
            // Update global stats
            let program_state = &mut ctx.accounts.program_state;
            program_state.total_users += 1;
        } else {
            // Calculate pending rewards before adding new stake
            let pending_rewards = calculate_rewards(
                user_stake.staked_amount,
                get_tier_apy(user_stake.staked_amount)?,
                user_stake.last_claim_timestamp,
                clock.unix_timestamp,
            )?;
            
            user_stake.rewards_earned = user_stake.rewards_earned.checked_add(pending_rewards).unwrap();
            user_stake.staked_amount = user_stake.staked_amount.checked_add(amount).unwrap();
            user_stake.last_claim_timestamp = clock.unix_timestamp;
            user_stake.last_stake_timestamp = clock.unix_timestamp;  // Update last stake timestamp on additional stakes
            user_stake.locked_until = clock.unix_timestamp + 7 * 24 * 60 * 60; // Extend lock period for additional stakes
        }
        
        // Update tier based on new total
        user_stake.tier = get_tier(user_stake.staked_amount)?;
        
        // Update total staked
        let program_state = &mut ctx.accounts.program_state;
        program_state.total_staked = program_state.total_staked.checked_add(amount).unwrap();
        
        emit!(StakeEvent {
            user: ctx.accounts.user.key(),
            amount,
            tier: user_stake.tier,
            total_staked: user_stake.staked_amount,
        });
        
        Ok(())
    }

    pub fn unstake_tokens(
        ctx: Context<UnstakeTokens>,
        amount: u64,
    ) -> Result<()> {
        // Enforce pause
        require!(!ctx.accounts.program_state.paused, StakingError::ProgramPaused);
        let user_stake = &mut ctx.accounts.user_stake;
        let clock = Clock::get()?;
        
        // Check if tokens are locked
        require!(
            clock.unix_timestamp >= user_stake.locked_until,
            StakingError::TokensLocked
        );
        
        // Check sufficient balance
        require!(
            user_stake.staked_amount >= amount,
            StakingError::InsufficientStake
        );
        
        // Calculate pending rewards before unstaking
        let pending_rewards = calculate_rewards(
            user_stake.staked_amount,
            get_tier_apy(user_stake.staked_amount)?,
            user_stake.last_claim_timestamp,
            clock.unix_timestamp,
        )?;
        user_stake.rewards_earned = user_stake.rewards_earned.checked_add(pending_rewards).unwrap();
        user_stake.last_claim_timestamp = clock.unix_timestamp;
        
        // Calculate unstaking penalty using last stake timestamp
        let penalty = calculate_unstake_penalty(
            user_stake.last_stake_timestamp,
            clock.unix_timestamp,
            amount,
        )?;
        
        let amount_after_penalty = amount.checked_sub(penalty).unwrap();
        
        // Transfer tokens back to user (minus penalty)
        let program_state_key = ctx.accounts.program_state.key();
        let seeds = &[
            b"stake-vault",
            program_state_key.as_ref(),
            &[ctx.accounts.program_state.vault_bump],
        ];
        let signer = &[&seeds[..]];
        
        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.stake_vault.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.stake_vault.to_account_info(),
                mint: ctx.accounts.defai_mint.to_account_info(),
            },
            signer,
        );
        transfer_checked(transfer_ctx, amount_after_penalty, ctx.accounts.defai_mint.decimals)?;
        
        // If there's a penalty, transfer it to escrow
        if penalty > 0 {
            let transfer_penalty_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.stake_vault.to_account_info(),
                    to: ctx.accounts.escrow_token_account.to_account_info(),
                    authority: ctx.accounts.stake_vault.to_account_info(),
                    mint: ctx.accounts.defai_mint.to_account_info(),
                },
                signer,
            );
            transfer_checked(transfer_penalty_ctx, penalty, ctx.accounts.defai_mint.decimals)?;
            
            // Update escrow balance
            let escrow = &mut ctx.accounts.reward_escrow;
            escrow.total_balance = escrow.total_balance.checked_add(penalty).unwrap();
        }
        
        // Update user stake
        user_stake.staked_amount = user_stake.staked_amount.checked_sub(amount).unwrap();
        
        // Update tier
        if user_stake.staked_amount > 0 {
            user_stake.tier = get_tier(user_stake.staked_amount)?;
        } else {
            user_stake.tier = 0;
        }
        
        // Update global stats
        let program_state = &mut ctx.accounts.program_state;
        program_state.total_staked = program_state.total_staked.checked_sub(amount).unwrap();
        
        emit!(UnstakeEvent {
            user: ctx.accounts.user.key(),
            amount,
            penalty,
            remaining_stake: user_stake.staked_amount,
            new_tier: user_stake.tier,
        });
        
        Ok(())
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        // Enforce pause
        require!(!ctx.accounts.program_state.paused, StakingError::ProgramPaused);
        let user_stake = &mut ctx.accounts.user_stake;
        let clock = Clock::get()?;
        
        // Calculate pending rewards
        let pending_rewards = calculate_rewards(
            user_stake.staked_amount,
            get_tier_apy(user_stake.staked_amount)?,
            user_stake.last_claim_timestamp,
            clock.unix_timestamp,
        )?;
        
        let total_claimable = user_stake.rewards_earned
            .checked_add(pending_rewards).unwrap()
            .checked_sub(user_stake.rewards_claimed).unwrap();
        
        require!(total_claimable > 0, StakingError::NoRewards);
        
        // Check escrow has sufficient balance
        require!(
            ctx.accounts.reward_escrow.total_balance >= total_claimable,
            StakingError::InsufficientEscrowBalance
        );
        
        // Transfer rewards from escrow to user
        let program_state_key = ctx.accounts.program_state.key();
        let escrow_seeds = &[
            b"reward-escrow",
            program_state_key.as_ref(),
            &[ctx.accounts.program_state.reward_escrow_bump],
        ];
        let escrow_signer = &[&escrow_seeds[..]];
        
        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.escrow_token_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.reward_escrow.to_account_info(),
                mint: ctx.accounts.defai_mint.to_account_info(),
            },
            escrow_signer,
        );
        transfer_checked(transfer_ctx, total_claimable, ctx.accounts.defai_mint.decimals)?;
        
        // Update user stake
        user_stake.rewards_earned = user_stake.rewards_earned.checked_add(pending_rewards).unwrap();
        user_stake.rewards_claimed = user_stake.rewards_claimed.checked_add(total_claimable).unwrap();
        user_stake.last_claim_timestamp = clock.unix_timestamp;
        
        // Update escrow
        let escrow = &mut ctx.accounts.reward_escrow;
        escrow.total_balance = escrow.total_balance.checked_sub(total_claimable).unwrap();
        escrow.total_distributed = escrow.total_distributed.checked_add(total_claimable).unwrap();
        
        emit!(RewardsClaimedEvent {
            user: ctx.accounts.user.key(),
            amount: total_claimable,
            total_distributed: escrow.total_distributed,
        });
        
        Ok(())
    }

    pub fn propose_authority_change(
        ctx: Context<UpdateAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let program_state = &mut ctx.accounts.program_state;
        program_state.pending_authority = Some(new_authority);
        program_state.authority_change_timestamp = Clock::get()?.unix_timestamp + ADMIN_TIMELOCK_DURATION;
        
        emit!(AuthorityUpdatedEvent {
            old_authority: program_state.authority,
            new_authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Authority change proposed. Can be executed after {}", program_state.authority_change_timestamp);
        
        Ok(())
    }
    
    pub fn accept_authority_change(
        ctx: Context<UpdateAuthority>,
    ) -> Result<()> {
        let program_state = &mut ctx.accounts.program_state;
        
        require!(
            program_state.pending_authority.is_some(),
            StakingError::NoPendingAuthorityChange
        );
        require!(
            Clock::get()?.unix_timestamp >= program_state.authority_change_timestamp,
            StakingError::TimelockNotExpired
        );
        
        let old_authority = program_state.authority;
        let new_authority = program_state.pending_authority.unwrap();
        program_state.authority = new_authority;
        program_state.pending_authority = None;
        program_state.authority_change_timestamp = 0;
        
        emit!(AuthorityUpdatedEvent {
            old_authority,
            new_authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Authority changed from {} to {}", old_authority, new_authority);
        
        Ok(())
    }

    pub fn pause_program(ctx: Context<PauseProgram>, paused: bool) -> Result<()> {
        let program_state = &mut ctx.accounts.program_state;
        program_state.paused = paused;
        
        emit!(ProgramPausedEvent {
            authority: ctx.accounts.authority.key(),
            paused,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        Ok(())
    }
    
    pub fn compound_rewards(ctx: Context<CompoundRewards>) -> Result<()> {
        // Enforce pause
        require!(!ctx.accounts.program_state.paused, StakingError::ProgramPaused);
        let user_stake = &mut ctx.accounts.user_stake;
        let clock = Clock::get()?;
        
        // Calculate pending rewards
        let pending_rewards = calculate_rewards(
            user_stake.staked_amount,
            get_tier_apy(user_stake.staked_amount)?,
            user_stake.last_claim_timestamp,
            clock.unix_timestamp,
        )?;
        
        let total_unclaimed = user_stake.rewards_earned
            .checked_add(pending_rewards).unwrap()
            .checked_sub(user_stake.rewards_claimed).unwrap();
        
        require!(total_unclaimed > 0, StakingError::NoRewards);
        
        // Check escrow has sufficient balance
        require!(
            ctx.accounts.reward_escrow.total_balance >= total_unclaimed,
            StakingError::InsufficientEscrowBalance
        );
        
        // Update stake amount by adding rewards
        let old_staked = user_stake.staked_amount;
        user_stake.staked_amount = user_stake.staked_amount
            .checked_add(total_unclaimed).unwrap();
        
        // Update tier based on new amount
        let old_tier = user_stake.tier;
        user_stake.tier = get_tier(user_stake.staked_amount)?;
        
        // Update reward tracking
        user_stake.rewards_earned = user_stake.rewards_earned.checked_add(pending_rewards).unwrap();
        user_stake.rewards_claimed = user_stake.rewards_earned; // Mark all as claimed since compounded
        user_stake.last_claim_timestamp = clock.unix_timestamp;
        
        // Reduce escrow balance (rewards stay in vault as part of stake)
        let escrow = &mut ctx.accounts.reward_escrow;
        escrow.total_balance = escrow.total_balance.checked_sub(total_unclaimed).unwrap();
        escrow.total_distributed = escrow.total_distributed.checked_add(total_unclaimed).unwrap();
        
        // Update global staked amount
        let program_state = &mut ctx.accounts.program_state;
        program_state.total_staked = program_state.total_staked.checked_add(total_unclaimed).unwrap();
        
        msg!(
            "Compounded {} rewards. Stake: {} -> {}, Tier: {} -> {}",
            total_unclaimed,
            old_staked,
            user_stake.staked_amount,
            old_tier,
            user_stake.tier
        );
        
        emit!(RewardsCompoundedEvent {
            user: ctx.accounts.user.key(),
            amount_compounded: total_unclaimed,
            new_stake_amount: user_stake.staked_amount,
            old_tier,
            new_tier: user_stake.tier,
            timestamp: clock.unix_timestamp,
        });
        
        Ok(())
    }
}

// Account structures
#[account]
pub struct ProgramState {
    pub authority: Pubkey,
    pub defai_mint: Pubkey,
    pub total_staked: u64,
    pub total_users: u64,
    pub paused: bool,
    pub vault_bump: u8,              // Bump for stake-vault PDA
    pub reward_escrow_bump: u8,      // Bump for reward-escrow PDA
    pub escrow_vault_bump: u8,       // Bump for escrow-vault PDA (token account)
    pub pending_authority: Option<Pubkey>,
    pub authority_change_timestamp: i64,
}

#[account]
pub struct RewardEscrow {
    pub authority: Pubkey,
    pub total_balance: u64,
    pub total_distributed: u64,
    pub bump: u8,
}

#[account]
pub struct UserStake {
    pub owner: Pubkey,
    pub staked_amount: u64,
    pub rewards_earned: u64,
    pub rewards_claimed: u64,
    pub tier: u8,
    pub stake_timestamp: i64,         // Initial stake timestamp
    pub last_stake_timestamp: i64,    // Most recent stake timestamp for penalty calculation
    pub last_claim_timestamp: i64,
    pub locked_until: i64,
}

// Context structs
#[derive(Accounts)]
pub struct InitializeProgram<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 32 + 8 + 8 + 1 + 1 + 1 + 1 + 33 + 8,  // Added 1 byte for escrow_vault_bump
        seeds = [b"program-state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        init,
        payer = authority,
        seeds = [b"stake-vault", program_state.key().as_ref()],
        bump,
        token::mint = defai_mint,
        token::authority = stake_vault,
    )]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub defai_mint: InterfaceAccount<'info, Mint>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct InitializeEscrow<'info> {
    #[account(
        mut,
        seeds = [b"program-state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 8 + 8 + 1,
        seeds = [b"reward-escrow", program_state.key().as_ref()],
        bump
    )]
    pub reward_escrow: Account<'info, RewardEscrow>,
    
    #[account(
        init,
        payer = authority,
        seeds = [b"escrow-vault", program_state.key().as_ref()],
        bump,
        token::mint = defai_mint,
        token::authority = reward_escrow,
    )]
    pub escrow_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub defai_mint: InterfaceAccount<'info, Mint>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct FundEscrow<'info> {
    // Bring in ProgramState to access authoritative addresses
    #[account(
        seeds = [b"program-state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        mut,
        // Ensure reward_escrow is the correct PDA
        seeds = [b"reward-escrow", program_state.key().as_ref()],
        bump = program_state.reward_escrow_bump
    )]
    pub reward_escrow: Account<'info, RewardEscrow>,
    
    #[account(
        mut,
        // Ensure escrow_token_account is the correct ATA and is owned by reward_escrow
        seeds = [b"escrow-vault", program_state.key().as_ref()],
        bump = program_state.escrow_vault_bump,
        token::authority = reward_escrow,
        token::mint = defai_mint,  // Ensure the ATA's mint matches the provided mint
    )]
    pub escrow_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub funder_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub funder: Signer<'info>,
    
    #[account(
        // Ensure the provided mint is the official one from ProgramState
        constraint = defai_mint.key() == program_state.defai_mint @ StakingError::InvalidMint
    )]
    pub defai_mint: InterfaceAccount<'info, Mint>,
    
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct StakeTokens<'info> {
    #[account(mut)]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + 32 + 8 + 8 + 8 + 1 + 8 + 8 + 8 + 8,  // Added 8 bytes for last_stake_timestamp
        seeds = [b"user-stake", user.key().as_ref()],
        bump
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(
        mut,
        seeds = [b"stake-vault", program_state.key().as_ref()],
        bump = program_state.vault_bump,
        token::authority = stake_vault,
        token::mint = defai_mint
    )]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        constraint = defai_mint.key() == program_state.defai_mint @ StakingError::InvalidMint
    )]
    pub defai_mint: InterfaceAccount<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UnstakeTokens<'info> {
    #[account(mut)]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        mut,
        seeds = [b"user-stake", user.key().as_ref()],
        bump,
        has_one = owner @ StakingError::InvalidOwner
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(
        mut,
        seeds = [b"stake-vault", program_state.key().as_ref()],
        bump = program_state.vault_bump,
        token::authority = stake_vault,
        token::mint = defai_mint
    )]
    pub stake_vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"reward-escrow", program_state.key().as_ref()],
        bump = program_state.reward_escrow_bump
    )]
    pub reward_escrow: Account<'info, RewardEscrow>,
    
    #[account(
        mut,
        seeds = [b"escrow-vault", program_state.key().as_ref()],
        bump = program_state.escrow_vault_bump,
        token::authority = reward_escrow,
        token::mint = defai_mint
    )]
    pub escrow_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        constraint = defai_mint.key() == program_state.defai_mint @ StakingError::InvalidMint
    )]
    pub defai_mint: InterfaceAccount<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    pub owner: SystemAccount<'info>,
    
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        mut,
        seeds = [b"user-stake", user.key().as_ref()],
        bump,
        has_one = owner @ StakingError::InvalidOwner
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(
        mut,
        seeds = [b"reward-escrow", program_state.key().as_ref()],
        bump = program_state.reward_escrow_bump
    )]
    pub reward_escrow: Account<'info, RewardEscrow>,
    
    #[account(
        mut,
        seeds = [b"escrow-vault", program_state.key().as_ref()],
        bump = program_state.escrow_vault_bump,
        token::authority = reward_escrow,
        token::mint = defai_mint
    )]
    pub escrow_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        constraint = defai_mint.key() == program_state.defai_mint @ StakingError::InvalidMint
    )]
    pub defai_mint: InterfaceAccount<'info, Mint>,
    
    pub user: Signer<'info>,
    pub owner: SystemAccount<'info>,
    
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct UpdateAuthority<'info> {
    #[account(
        mut,
        has_one = authority @ StakingError::InvalidAuthority
    )]
    pub program_state: Account<'info, ProgramState>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct PauseProgram<'info> {
    #[account(
        mut,
        has_one = authority @ StakingError::InvalidAuthority
    )]
    pub program_state: Account<'info, ProgramState>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct CompoundRewards<'info> {
    #[account(mut)]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        mut,
        seeds = [b"user-stake", user.key().as_ref()],
        bump,
        has_one = owner @ StakingError::InvalidOwner
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(
        mut,
        // Add this constraint to ensure it's the official PDA
        seeds = [b"reward-escrow", program_state.key().as_ref()],
        bump = program_state.reward_escrow_bump
    )]
    pub reward_escrow: Account<'info, RewardEscrow>,
    
    pub user: Signer<'info>,
    pub owner: SystemAccount<'info>,
}

// Events
#[event]
pub struct StakeEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub tier: u8,
    pub total_staked: u64,
}

#[event]
pub struct UnstakeEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub penalty: u64,
    pub remaining_stake: u64,
    pub new_tier: u8,
}

#[event]
pub struct RewardsClaimedEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub total_distributed: u64,
}

#[event]
pub struct EscrowFundedEvent {
    pub funder: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
}

#[event]
pub struct AuthorityUpdatedEvent {
    pub old_authority: Pubkey,
    pub new_authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProgramPausedEvent {
    pub authority: Pubkey,
    pub paused: bool,
    pub timestamp: i64,
}

#[event]
pub struct RewardsCompoundedEvent {
    pub user: Pubkey,
    pub amount_compounded: u64,
    pub new_stake_amount: u64,
    pub old_tier: u8,
    pub new_tier: u8,
    pub timestamp: i64,
}

// Error codes
#[error_code]
pub enum StakingError {
    #[msg("Amount too low for any tier")]
    AmountTooLow,
    #[msg("Insufficient stake amount")]
    InsufficientStake,
    #[msg("Tokens are still locked")]
    TokensLocked,
    #[msg("No rewards available to claim")]
    NoRewards,
    #[msg("Invalid authority")]
    InvalidAuthority,
    #[msg("Invalid owner")]
    InvalidOwner,
    #[msg("Program is paused")]
    ProgramPaused,
    #[msg("Insufficient escrow balance")]
    InsufficientEscrowBalance,
    #[msg("No pending authority change")]
    NoPendingAuthorityChange,
    #[msg("Timelock not expired")]
    TimelockNotExpired,
    #[msg("Invalid mint address")]
    InvalidMint,
}

// Helper functions
fn get_tier(amount: u64) -> Result<u8> {
    if amount >= INFINITE_MIN {
        Ok(3) // Infinite
    } else if amount >= TITANIUM_MIN {
        Ok(2) // Titanium
    } else if amount >= GOLD_MIN {
        Ok(1) // Gold
    } else {
        Ok(0) // No tier
    }
}

fn get_tier_apy(amount: u64) -> Result<u16> {
    if amount >= INFINITE_MIN {
        Ok(INFINITE_APY_BPS)
    } else if amount >= TITANIUM_MIN {
        Ok(TITANIUM_APY_BPS)
    } else if amount >= GOLD_MIN {
        Ok(GOLD_APY_BPS)
    } else {
        Err(StakingError::AmountTooLow.into())
    }
}

fn calculate_rewards(
    staked_amount: u64,
    tier_apy_bps: u16,
    last_claim_timestamp: i64,
    current_timestamp: i64,
) -> Result<u64> {
    let time_elapsed = (current_timestamp - last_claim_timestamp) as u64;
    
    // Calculate rewards: amount * apy * time / (year * basis_points)
    let rewards = (staked_amount as u128)
        .checked_mul(tier_apy_bps as u128).unwrap()
        .checked_mul(time_elapsed as u128).unwrap()
        .checked_div(SECONDS_PER_YEAR as u128).unwrap()
        .checked_div(BASIS_POINTS as u128).unwrap() as u64;
    
    Ok(rewards)
}

fn calculate_unstake_penalty(
    stake_timestamp: i64,
    current_timestamp: i64,
    amount: u64,
) -> Result<u64> {
    let days_staked = (current_timestamp - stake_timestamp) / 86400;
    
    let penalty_bps = if days_staked < 30 {
        200  // 2%
    } else if days_staked < 90 {
        100  // 1%
    } else {
        0    // No penalty
    };
    
    Ok((amount as u128 * penalty_bps as u128 / BASIS_POINTS as u128) as u64)
}
