#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_2022::{self};
use anchor_spl::token_interface::{TokenAccount, TokenInterface, TransferChecked};
use anchor_spl::token::Mint;

declare_id!("Em4efpnH5X51Gr5hSKKWwJ4K2ktgcKDh5qgqr2w54WSH");

#[program]
pub mod disciplinator {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>, 
        fee_percentage: u8,
        reward_percentage: u8,
        charity_percentage: u8,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        
        require!(
            fee_percentage + reward_percentage + charity_percentage == 100,
            ErrorCode::InvalidPercentageDistribution
        );
        
        // Validate that the mint is USDT
        #[cfg(not(feature = "test-mode"))]
        {
            let mint_key = ctx.accounts.accepted_mint.key();
            let valid_mints = [
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", // Official USDT
                "Dn4noZ5jgGfkntzcQSUZ8czkreiZ1ForXYoV2H8Dm7S1", // Wormhole USDT
            ];
            
            let is_valid_usdt = valid_mints.iter().any(|&mint| {
                mint.parse::<Pubkey>().map_or(false, |parsed_mint| parsed_mint == mint_key)
            });
            
            require!(is_valid_usdt, ErrorCode::InvalidMint);
        }
        require!(ctx.accounts.accepted_mint.decimals == 6, ErrorCode::InvalidDecimals);
        
        config.authority = ctx.accounts.authority.key();
        config.fee_percentage = fee_percentage;
        config.reward_percentage = reward_percentage;
        config.charity_percentage = charity_percentage;
        config.treasury = ctx.accounts.treasury.key();
        config.accepted_mint = ctx.accounts.accepted_mint.key();
        config.total_challenges = 0;
        config.total_volume = 0;
        config.paused = false;
        config.min_deposit = 5_000_000; // 5 USDT minimum
        config.max_deposit = 10_000_000_000; // 10,000 USDT maximum
        
        Ok(())
    }

    pub fn create_challenge(
        ctx: Context<CreateChallenge>,
        deposit_amount: u64,
        total_sessions: u32,
        duration_days: u32,
        verifier: Option<Pubkey>,
        challenge_type: ChallengeType,
    ) -> Result<()> {
        let challenge = &mut ctx.accounts.challenge;
        let config = &mut ctx.accounts.config;
        let clock = Clock::get()?;
        
        // Validate inputs
        require!(!config.paused, ErrorCode::ProtocolPaused);
        require!(deposit_amount >= config.min_deposit, ErrorCode::DepositTooSmall); // Min from config
        require!(deposit_amount <= config.max_deposit, ErrorCode::DepositTooLarge); // Max from config
        require!(total_sessions > 0 && total_sessions <= 365, ErrorCode::InvalidSessionCount);
        require!(duration_days >= 7 && duration_days <= 365, ErrorCode::InvalidDuration);
        
        // Initialize challenge
        challenge.participant = ctx.accounts.participant.key();
        challenge.deposit_amount = deposit_amount;
        challenge.total_sessions = total_sessions;
        challenge.completed_sessions = 0;
        challenge.start_time = clock.unix_timestamp;
        challenge.end_time = clock.unix_timestamp + (duration_days as i64 * 86400);
        challenge.status = ChallengeStatus::Active;
        challenge.verifier = verifier;
        challenge.challenge_id = config.total_challenges;
        challenge.last_session_time = 0;
        challenge.challenge_type = challenge_type;
        challenge.minimum_interval_hours = calculate_minimum_interval(total_sessions, duration_days);
        challenge.grace_periods_used = 0;
        challenge.max_grace_periods = 3; // Allow 3 grace periods per challenge
        
        // Update global stats
        config.total_challenges += 1;
        config.total_volume += deposit_amount;
        
        // Transfer tokens to vault
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.participant_token_account.to_account_info(),
            mint: ctx.accounts.accepted_mint.to_account_info(),
            to: ctx.accounts.vault.to_account_info(),
            authority: ctx.accounts.participant.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        
        token_2022::transfer_checked(
            cpi_ctx,
            deposit_amount,
            ctx.accounts.accepted_mint.decimals,
        )?;
        
        emit!(ChallengeCreated {
            participant: challenge.participant,
            challenge_id: challenge.challenge_id,
            deposit_amount,
            total_sessions,
            end_time: challenge.end_time,
            challenge_type: challenge.challenge_type.clone(),
        });
        
        Ok(())
    }

    pub fn mark_session_complete(
        ctx: Context<MarkSession>,
        proof_ipfs_hash: String,
        session_metadata: SessionMetadata,
    ) -> Result<()> {
        let challenge_key = ctx.accounts.challenge.key();
        let challenge = &mut ctx.accounts.challenge;
        let clock = Clock::get()?;
        
        // Validate challenge status
        require!(challenge.status == ChallengeStatus::Active, ErrorCode::ChallengeNotActive);
        require!(clock.unix_timestamp < challenge.end_time, ErrorCode::ChallengeExpired);
        require!(challenge.completed_sessions < challenge.total_sessions, ErrorCode::AllSessionsCompleted);
        
        // Verify authorization - only verifier can mark sessions complete
        // This prevents participants from self-verifying and gaming the system
        require!(
            challenge.verifier.is_some(), 
            ErrorCode::NoVerifierSet
        );
        require!(
            challenge.verifier.map_or(false, |v| ctx.accounts.signer.key() == v),
            ErrorCode::UnauthorizedVerifier
        );
        
        // Validate IPFS hash format
        validate_ipfs_hash(&proof_ipfs_hash)?;
        
        // Check minimum interval between sessions
        if challenge.last_session_time > 0 {
            let hours_passed = (clock.unix_timestamp - challenge.last_session_time) / 3600;
            require!(
                hours_passed >= challenge.minimum_interval_hours as i64,
                ErrorCode::SessionTooSoon
            );
        }
        
        // Validate session metadata based on challenge type
        validate_session_metadata(&challenge.challenge_type, &session_metadata)?;
        
        // Update challenge
        challenge.completed_sessions += 1;
        challenge.last_session_time = clock.unix_timestamp;
        
        // Store session record
        let session = &mut ctx.accounts.session;
        session.challenge = challenge_key;
        session.session_number = challenge.completed_sessions;
        session.timestamp = clock.unix_timestamp;
        session.proof_ipfs_hash = proof_ipfs_hash;
        session.verified_by = ctx.accounts.signer.key();
        session.metadata = session_metadata;
        session.auto_verified = false; // Always false since only verifiers can mark sessions
        
        // Update user stats
        let user_stats = &mut ctx.accounts.user_stats;
        user_stats.total_sessions_completed += 1;
        user_stats.last_activity = clock.unix_timestamp;
        
        emit!(SessionCompleted {
            challenge_id: challenge.challenge_id,
            session_number: challenge.completed_sessions,
            timestamp: clock.unix_timestamp,
            verified_by: ctx.accounts.signer.key(),
        });
        
        // Auto-finalize if all sessions completed
        if challenge.completed_sessions == challenge.total_sessions {
            msg!("All sessions completed, auto-finalizing challenge");
        }
        
        Ok(())
    }

    pub fn finalize_challenge(ctx: Context<FinalizeChallenge>) -> Result<()> {
        let challenge_key = ctx.accounts.challenge.key();
        let challenge = &mut ctx.accounts.challenge;
        let config = &ctx.accounts.config;
        let clock = Clock::get()?;
        
        // Validate finalization conditions
        require!(
            challenge.status == ChallengeStatus::Active,
            ErrorCode::ChallengeNotActive
        );
        require!(
            clock.unix_timestamp >= challenge.end_time || 
            challenge.completed_sessions == challenge.total_sessions,
            ErrorCode::CannotFinalizeYet
        );
        
        // Calculate completion and amounts using safe integer arithmetic
        let refund_amount = challenge.deposit_amount
            .checked_mul(challenge.completed_sessions as u64)
            .and_then(|x| x.checked_div(challenge.total_sessions as u64))
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        let penalty_amount = challenge.deposit_amount.checked_sub(refund_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        
        // Store completion rate as percentage (0-10000 for 0.00%-100.00%)
        let completion_rate_percentage = (challenge.completed_sessions as u64 * 10000) / challenge.total_sessions as u64;
        
        // Calculate distribution
        let protocol_fee = (penalty_amount * config.fee_percentage as u64) / 100;
        let reward_pool_amount = (penalty_amount * config.reward_percentage as u64) / 100;
        let _charity_amount = penalty_amount - protocol_fee - reward_pool_amount;
        
        // Transfer refund to participant
        if refund_amount > 0 {
            transfer_from_vault(
                &ctx.accounts.vault.to_account_info(),
                &ctx.accounts.participant_token_account.to_account_info(),
                &ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.accepted_mint,
                refund_amount,
                &[
                    b"vault",
                    config.key().as_ref(),
                    &[ctx.bumps.vault],
                ],
            )?;
        }
        
        // Transfer protocol fee
        if protocol_fee > 0 {
            transfer_from_vault(
                &ctx.accounts.vault.to_account_info(),
                &ctx.accounts.treasury_token_account.to_account_info(),
                &ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.accepted_mint,
                protocol_fee,
                &[
                    b"vault",
                    config.key().as_ref(),
                    &[ctx.bumps.vault],
                ],
            )?;
        }
        
        // Keep rewards and charity in vault for later distribution
        
        // Update challenge status (using percentage: 10000 = 100%, 8000 = 80%)
        challenge.status = if completion_rate_percentage >= 10000 {
            ChallengeStatus::Completed
        } else if completion_rate_percentage >= 8000 {
            ChallengeStatus::PartiallyCompleted
        } else {
            ChallengeStatus::Failed
        };
        
        // Update user stats
        let user_stats = &mut ctx.accounts.user_stats;
        user_stats.total_challenges += 1;
        user_stats.total_deposited += challenge.deposit_amount;
        user_stats.total_refunded += refund_amount;
        user_stats.total_penalties += penalty_amount;
        
        match challenge.status {
            ChallengeStatus::Completed => {
                user_stats.challenges_completed += 1;
                user_stats.current_streak += 1;
                user_stats.perfect_completions += 1;
                if user_stats.current_streak > user_stats.best_streak {
                    user_stats.best_streak = user_stats.current_streak;
                }
            },
            ChallengeStatus::PartiallyCompleted => {
                user_stats.challenges_partial += 1;
                // Maintain streak for 80%+ completion
                user_stats.current_streak += 1;
            },
            ChallengeStatus::Failed => {
                user_stats.challenges_failed += 1;
                user_stats.current_streak = 0;
            },
            _ => {}
        }
        
        // Record finalization for rewards
        let finalization = &mut ctx.accounts.finalization_record;
        finalization.challenge = challenge_key;
        finalization.participant = challenge.participant;
        finalization.completion_rate_percentage = completion_rate_percentage;
        finalization.penalty_amount = penalty_amount;
        finalization.reward_pool_contribution = reward_pool_amount;
        finalization.timestamp = clock.unix_timestamp;
        finalization.rewarded = false;
        
        emit!(ChallengeFinalized {
            challenge_id: challenge.challenge_id,
            participant: challenge.participant,
            refund_amount,
            penalty_amount,
            completion_rate_percentage,
            status: challenge.status.clone(),
        });
        
        Ok(())
    }

    pub fn use_grace_period(ctx: Context<UseGracePeriod>, reason: String) -> Result<()> {
        let challenge = &mut ctx.accounts.challenge;
        let clock = Clock::get()?;
        
        require!(challenge.status == ChallengeStatus::Active, ErrorCode::ChallengeNotActive);
        require!(challenge.grace_periods_used < challenge.max_grace_periods, ErrorCode::NoGracePeriodsLeft);
        require!(clock.unix_timestamp < challenge.end_time, ErrorCode::ChallengeExpired);
        
        // Extend challenge by 3 days with overflow protection
        challenge.end_time = challenge.end_time
            .checked_add(3 * 86400)
            .ok_or(ErrorCode::TimeOverflow)?;
        challenge.grace_periods_used += 1;
        
        // Record grace period usage
        let grace_record = &mut ctx.accounts.grace_record;
        let challenge_key = challenge.key();
        grace_record.challenge = challenge_key;
        grace_record.used_at = clock.unix_timestamp;
        grace_record.reason = reason;
        grace_record.new_end_time = challenge.end_time;
        
        emit!(GracePeriodUsed {
            challenge_id: challenge.challenge_id,
            grace_periods_remaining: challenge.max_grace_periods - challenge.grace_periods_used,
            new_end_time: challenge.end_time,
        });
        
        Ok(())
    }

    pub fn distribute_rewards(ctx: Context<DistributeRewards>, epoch: u64) -> Result<()> {
        let clock = Clock::get()?;
        let reward_state = &mut ctx.accounts.reward_state;
        
        // Ensure epoch hasn't been processed
        require!(reward_state.last_epoch_processed < epoch, ErrorCode::EpochAlreadyProcessed);
        require!(clock.unix_timestamp >= reward_state.next_epoch_time, ErrorCode::EpochNotReady);
        
        // Calculate total rewards to distribute
        let vault_balance = ctx.accounts.vault.amount;
        let reserved_amount = ctx.accounts.vault_reserve.amount;
        let available_rewards = vault_balance.saturating_sub(reserved_amount);
        
        // Update reward state
        reward_state.last_epoch_processed = epoch;
        reward_state.next_epoch_time = clock.unix_timestamp + (7 * 86400); // Weekly
        reward_state.total_distributed += available_rewards;
        
        emit!(RewardsDistributed {
            epoch,
            amount: available_rewards,
            timestamp: clock.unix_timestamp,
        });
        
        Ok(())
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let user_stats = &ctx.accounts.user_stats;
        let reward_state = &ctx.accounts.reward_state;
        let _clock = Clock::get()?;
        
        // Check eligibility
        require!(
            user_stats.perfect_completions > 0,
            ErrorCode::NotEligibleForRewards
        );
        require!(
            user_stats.last_claim_epoch < reward_state.last_epoch_processed,
            ErrorCode::AlreadyClaimedThisEpoch
        );
        
        // Calculate reward amount based on performance score
        let performance_score = calculate_performance_score(user_stats);
        let epoch_records = &ctx.remaining_accounts; // Finalization records for the epoch
        let total_epoch_score = calculate_total_epoch_score(epoch_records)?;
        
        // Calculate reward amount using safe integer arithmetic
        let reward_amount = if total_epoch_score > 0 {
            ctx.accounts.vault_rewards.amount
                .checked_mul(performance_score)
                .and_then(|x| x.checked_div(total_epoch_score))
                .unwrap_or(0)
        } else {
            0
        };
        
        // Verify sufficient funds before transfer
        require!(
            ctx.accounts.vault_rewards.amount >= reward_amount,
            ErrorCode::InsufficientRewards
        );
        
        // Transfer rewards
        if reward_amount > 0 {
            transfer_from_vault(
                &ctx.accounts.vault_rewards.to_account_info(),
                &ctx.accounts.participant_token_account.to_account_info(),
                &ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.accepted_mint,
                reward_amount,
                &[
                    b"vault_rewards",
                    ctx.accounts.config.key().as_ref(),
                    &[ctx.bumps.vault_rewards],
                ],
            )?;
        }
        
        // Update user stats
        let user_stats = &mut ctx.accounts.user_stats;
        user_stats.total_rewards_claimed += reward_amount;
        user_stats.last_claim_epoch = reward_state.last_epoch_processed;
        
        emit!(RewardsClaimed {
            participant: ctx.accounts.participant.key(),
            amount: reward_amount,
            epoch: reward_state.last_epoch_processed,
            performance_score,
        });
        
        Ok(())
    }

    pub fn pause_protocol(ctx: Context<PauseProtocol>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.paused = true;
        
        emit!(ProtocolPaused {
            authority: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        Ok(())
    }

    pub fn unpause_protocol(ctx: Context<PauseProtocol>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.paused = false;
        
        emit!(ProtocolUnpaused {
            authority: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        Ok(())
    }
}

// Helper functions
fn validate_ipfs_hash(hash: &str) -> Result<()> {
    // IPFS hash validation: should be 46 characters and start with "Qm"
    require!(
        hash.len() == 46 && hash.starts_with("Qm"),
        ErrorCode::InvalidIPFSHash
    );
    
    // Additional validation: check if it contains only valid base58 characters
    let valid_chars = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    require!(
        hash.chars().all(|c| valid_chars.contains(c)),
        ErrorCode::InvalidIPFSHash
    );
    
    Ok(())
}

fn calculate_minimum_interval(total_sessions: u32, duration_days: u32) -> u16 {
    let total_hours = duration_days as f64 * 24.0;
    let interval = total_hours / total_sessions as f64;
    // Minimum 12 hours, maximum 48 hours between sessions
    (interval.max(12.0).min(48.0)) as u16
}

fn validate_session_metadata(
    challenge_type: &ChallengeType,
    metadata: &SessionMetadata,
) -> Result<()> {
    match challenge_type {
        ChallengeType::Fitness => {
            require!(
                metadata.duration_minutes.unwrap_or(0) >= 20,
                ErrorCode::InvalidSessionDuration
            );
        },
        ChallengeType::Education => {
            require!(
                metadata.duration_minutes.unwrap_or(0) >= 30,
                ErrorCode::InvalidSessionDuration
            );
        },
        ChallengeType::Meditation => {
            require!(
                metadata.duration_minutes.unwrap_or(0) >= 10,
                ErrorCode::InvalidSessionDuration
            );
        },
        ChallengeType::Custom => {
            // Custom challenges have flexible requirements
        },
    }
    Ok(())
}

fn calculate_performance_score(stats: &UserStats) -> u64 {
    let base_score = stats.perfect_completions as u64 * 100;
    let streak_bonus = stats.best_streak as u64 * 10;
    let consistency_bonus = if stats.challenges_failed == 0 { 50 } else { 0 };
    
    base_score + streak_bonus + consistency_bonus
}

fn calculate_total_epoch_score(_records: &[AccountInfo]) -> Result<u64> {
    // Sum up all performance scores from finalization records
    // Implementation depends on how you want to iterate through accounts
    Ok(1000) // Placeholder
}

fn transfer_from_vault<'info>(
    vault: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    token_program: &AccountInfo<'info>,
    mint: &Account<'info, Mint>,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> Result<()> {
    let cpi_accounts = TransferChecked {
        from: vault.clone(),
        mint: mint.to_account_info(),
        to: to.clone(),
        authority: vault.clone(),
    };
    let seeds_slice = &[signer_seeds];
    let cpi_ctx = CpiContext::new_with_signer(
        token_program.clone(),
        cpi_accounts,
        seeds_slice,
    );
    
    token_2022::transfer_checked(cpi_ctx, amount, mint.decimals)
}

// Account structures
#[account]
pub struct Config {
    pub authority: Pubkey,
    pub treasury: Pubkey,
    pub accepted_mint: Pubkey,
    pub fee_percentage: u8,
    pub reward_percentage: u8,
    pub charity_percentage: u8,
    pub total_challenges: u64,
    pub total_volume: u64,
    pub paused: bool,
    pub min_deposit: u64,
    pub max_deposit: u64,
}

#[account]
pub struct Challenge {
    pub participant: Pubkey,
    pub deposit_amount: u64,
    pub total_sessions: u32,
    pub completed_sessions: u32,
    pub start_time: i64,
    pub end_time: i64,
    pub last_session_time: i64,
    pub status: ChallengeStatus,
    pub verifier: Option<Pubkey>,
    pub challenge_id: u64,
    pub challenge_type: ChallengeType,
    pub minimum_interval_hours: u16,
    pub grace_periods_used: u8,
    pub max_grace_periods: u8,
}

#[account]
pub struct Session {
    pub challenge: Pubkey,
    pub session_number: u32,
    pub timestamp: i64,
    pub proof_ipfs_hash: String,
    pub verified_by: Pubkey,
    pub metadata: SessionMetadata,
    pub auto_verified: bool,
}

#[account]
pub struct UserStats {
    pub user: Pubkey,
    pub total_challenges: u32,
    pub challenges_completed: u32,
    pub challenges_partial: u32,
    pub challenges_failed: u32,
    pub perfect_completions: u32,
    pub total_sessions_completed: u32,
    pub total_deposited: u64,
    pub total_refunded: u64,
    pub total_penalties: u64,
    pub total_rewards_claimed: u64,
    pub current_streak: u32,
    pub best_streak: u32,
    pub last_activity: i64,
    pub last_claim_epoch: u64,
}

#[account]
pub struct FinalizationRecord {
    pub challenge: Pubkey,
    pub participant: Pubkey,
    pub completion_rate_percentage: u64,
    pub penalty_amount: u64,
    pub reward_pool_contribution: u64,
    pub timestamp: i64,
    pub rewarded: bool,
}

#[account]
pub struct RewardState {
    pub last_epoch_processed: u64,
    pub next_epoch_time: i64,
    pub total_distributed: u64,
}

#[account]
pub struct GracePeriodRecord {
    pub challenge: Pubkey,
    pub used_at: i64,
    pub reason: String,
    pub new_end_time: i64,
}

// Enums and types
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum ChallengeStatus {
    Active,
    Completed,
    PartiallyCompleted,
    Failed,
    Cancelled,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum ChallengeType {
    Fitness,
    Education,
    Meditation,
    Custom,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SessionMetadata {
    pub duration_minutes: Option<u16>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

// Contexts
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Config::INIT_SPACE,
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, Config>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    /// CHECK: Treasury account
    pub treasury: AccountInfo<'info>,
    
    pub accepted_mint: Account<'info, Mint>,
    
    #[account(
        init,
        payer = authority,
        seeds = [b"vault", config.key().as_ref()],
        bump,
        token::mint = accepted_mint,
        token::authority = vault,
        token::token_program = token_program,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        init,
        payer = authority,
        seeds = [b"vault_rewards", config.key().as_ref()],
        bump,
        token::mint = accepted_mint,
        token::authority = vault_rewards,
        token::token_program = token_program,
    )]
    pub vault_rewards: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        init,
        payer = authority,
        seeds = [b"vault_reserve", config.key().as_ref()],
        bump,
        token::mint = accepted_mint,
        token::authority = vault_reserve,
        token::token_program = token_program,
    )]
    pub vault_reserve: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        init,
        payer = authority,
        space = 8 + RewardState::INIT_SPACE,
        seeds = [b"reward_state"],
        bump
    )]
    pub reward_state: Account<'info, RewardState>,
    
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct CreateChallenge<'info> {
    #[account(
        init,
        payer = participant,
        space = 8 + Challenge::INIT_SPACE,
        seeds = [
            b"challenge", 
            participant.key().as_ref(), 
            &config.total_challenges.to_le_bytes()
        ],
        bump
    )]
    pub challenge: Account<'info, Challenge>,
    
    #[account(mut)]
    pub participant: Signer<'info>,
    
    #[account(
        mut,
        constraint = participant_token_account.owner == participant.key(),
        constraint = participant_token_account.mint == config.accepted_mint,
    )]
    pub participant_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, Config>,
    
    pub accepted_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        seeds = [b"vault", config.key().as_ref()],
        bump,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = participant,
        space = 8 + UserStats::INIT_SPACE,
        seeds = [b"user_stats", participant.key().as_ref()],
        bump
    )]
    pub user_stats: Account<'info, UserStats>,
    
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MarkSession<'info> {
    #[account(
        mut,
        constraint = challenge.verifier == Some(signer.key())
    )]
    pub challenge: Account<'info, Challenge>,
    
    /// CHECK: Participant account
    pub participant: AccountInfo<'info>,
    
    #[account(mut)]
    pub signer: Signer<'info>,
    
    #[account(
        init,
        payer = signer,
        space = 8 + Session::INIT_SPACE,
        seeds = [
            b"session", 
            challenge.key().as_ref(),
            &challenge.completed_sessions.to_le_bytes()
        ],
        bump
    )]
    pub session: Account<'info, Session>,
    
    #[account(
        mut,
        seeds = [b"user_stats", participant.key().as_ref()],
        bump
    )]
    pub user_stats: Account<'info, UserStats>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FinalizeChallenge<'info> {
    #[account(
        mut,
        constraint = challenge.participant == participant.key()
    )]
    pub challenge: Account<'info, Challenge>,
    
    #[account(mut)]
    pub participant: Signer<'info>,
    
    #[account(
        mut,
        constraint = participant_token_account.owner == participant.key(),
        constraint = participant_token_account.mint == config.accepted_mint,
    )]
    pub participant_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, Config>,
    
    pub accepted_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        seeds = [b"vault", config.key().as_ref()],
        bump,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = treasury_token_account.owner == config.treasury
    )]
    pub treasury_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"user_stats", participant.key().as_ref()],
        bump
    )]
    pub user_stats: Account<'info, UserStats>,
    
    #[account(
        init,
        payer = participant,
        space = 8 + FinalizationRecord::INIT_SPACE,
        seeds = [
            b"finalization",
            challenge.key().as_ref()
        ],
        bump
    )]
    pub finalization_record: Account<'info, FinalizationRecord>,
    
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UseGracePeriod<'info> {
    #[account(
        mut,
        constraint = challenge.participant == participant.key()
    )]
    pub challenge: Account<'info, Challenge>,
    
    #[account(mut)]
    pub participant: Signer<'info>,
    
    #[account(
        init,
        payer = participant,
        space = 8 + GracePeriodRecord::INIT_SPACE,
        seeds = [
            b"grace",
            challenge.key().as_ref(),
            &challenge.grace_periods_used.to_le_bytes()
        ],
        bump
    )]
    pub grace_record: Account<'info, GracePeriodRecord>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DistributeRewards<'info> {
    #[account(
        mut,
        seeds = [b"reward_state"],
        bump
    )]
    pub reward_state: Account<'info, RewardState>,
    
    #[account(
        seeds = [b"config"],
        bump,
        constraint = config.authority == authority.key()
    )]
    pub config: Account<'info, Config>,
    
    pub authority: Signer<'info>,
    
    #[account(
        seeds = [b"vault", config.key().as_ref()],
        bump,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        seeds = [b"vault_reserve", config.key().as_ref()],
        bump,
    )]
    pub vault_reserve: InterfaceAccount<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub participant: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"user_stats", participant.key().as_ref()],
        bump
    )]
    pub user_stats: Account<'info, UserStats>,
    
    #[account(
        seeds = [b"reward_state"],
        bump
    )]
    pub reward_state: Account<'info, RewardState>,
    
    #[account(
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, Config>,
    
    pub accepted_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        constraint = participant_token_account.owner == participant.key(),
        constraint = participant_token_account.mint == config.accepted_mint,
    )]
    pub participant_token_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"vault_rewards", config.key().as_ref()],
        bump,
    )]
    pub vault_rewards: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct PauseProtocol<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump,
        constraint = config.authority == authority.key()
    )]
    pub config: Account<'info, Config>,
    
    pub authority: Signer<'info>,
}

// Space implementations
impl Config {
    pub const INIT_SPACE: usize = 32 + 32 + 32 + 1 + 1 + 1 + 8 + 8 + 1 + 8 + 8;
}

impl Challenge {
    pub const INIT_SPACE: usize = 32 + 8 + 4 + 4 + 8 + 8 + 8 + 1 + 33 + 8 + 1 + 2 + 1 + 1;
}

impl Session {
    pub const INIT_SPACE: usize = 32 + 4 + 8 + 64 + 32 + 100 + 1; // Assuming metadata ~100 bytes
}

impl UserStats {
    pub const INIT_SPACE: usize = 32 + 4 + 4 + 4 + 4 + 4 + 4 + 8 + 8 + 8 + 8 + 4 + 4 + 8 + 8;
}

impl FinalizationRecord {
    pub const INIT_SPACE: usize = 32 + 32 + 8 + 8 + 8 + 8 + 1;
}

impl RewardState {
    pub const INIT_SPACE: usize = 8 + 8 + 8;
}

impl GracePeriodRecord {
    pub const INIT_SPACE: usize = 32 + 8 + 256 + 8; // 256 bytes for reason string
}

// Events
#[event]
pub struct ChallengeCreated {
    pub participant: Pubkey,
    pub challenge_id: u64,
    pub deposit_amount: u64,
    pub total_sessions: u32,
    pub end_time: i64,
    pub challenge_type: ChallengeType,
}

#[event]
pub struct SessionCompleted {
    pub challenge_id: u64,
    pub session_number: u32,
    pub timestamp: i64,
    pub verified_by: Pubkey,
}

#[event]
pub struct ChallengeFinalized {
    pub challenge_id: u64,
    pub participant: Pubkey,
    pub refund_amount: u64,
    pub penalty_amount: u64,
    pub completion_rate_percentage: u64,
    pub status: ChallengeStatus,
}

#[event]
pub struct GracePeriodUsed {
    pub challenge_id: u64,
    pub grace_periods_remaining: u8,
    pub new_end_time: i64,
}

#[event]
pub struct RewardsDistributed {
    pub epoch: u64,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct RewardsClaimed {
    pub participant: Pubkey,
    pub amount: u64,
    pub epoch: u64,
    pub performance_score: u64,
}

#[event]
pub struct ProtocolPaused {
    pub authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProtocolUnpaused {
    pub authority: Pubkey,
    pub timestamp: i64,
}

// Error codes
#[error_code]
pub enum ErrorCode {
    #[msg("Protocol is currently paused")]
    ProtocolPaused,
    #[msg("Invalid deposit amount")]
    InvalidDepositAmount,
    #[msg("Deposit amount too small (min 1 USDC)")]
    DepositTooSmall,
    #[msg("Deposit amount too large (max 10k USDC)")]
    DepositTooLarge,
    #[msg("Invalid session count")]
    InvalidSessionCount,
    #[msg("Invalid challenge duration")]
    InvalidDuration,
    #[msg("Challenge is not active")]
    ChallengeNotActive,
    #[msg("Challenge has expired")]
    ChallengeExpired,
    #[msg("All sessions already completed")]
    AllSessionsCompleted,
    #[msg("Unauthorized access")]
    Unauthorized,
    #[msg("Unauthorized verifier")]
    UnauthorizedVerifier,
    #[msg("Unauthorized participant")]
    UnauthorizedParticipant,
    #[msg("Session submitted too soon")]
    SessionTooSoon,
    #[msg("Cannot finalize challenge yet")]
    CannotFinalizeYet,
    #[msg("Challenge finalization too early")]
    ChallengeTooEarly,
    #[msg("Not eligible for rewards")]
    NotEligibleForRewards,
    #[msg("Already claimed rewards for this epoch")]
    AlreadyClaimedThisEpoch,
    #[msg("Invalid percentage distribution")]
    InvalidPercentageDistribution,
    #[msg("Invalid session duration")]
    InvalidSessionDuration,
    #[msg("No grace periods remaining")]
    NoGracePeriodsLeft,
    #[msg("Epoch already processed")]
    EpochAlreadyProcessed,
    #[msg("Invalid mint - only USDT accepted")]
    InvalidMint,
    #[msg("Invalid token decimals")]
    InvalidDecimals,
    #[msg("No verifier set for challenge")]
    NoVerifierSet,
    #[msg("Arithmetic overflow occurred")]
    ArithmeticOverflow,
    #[msg("Time calculation overflow")]
    TimeOverflow,
    #[msg("Invalid IPFS hash format")]
    InvalidIPFSHash,
    #[msg("Insufficient rewards in vault")]
    InsufficientRewards,
    #[msg("Epoch not ready for processing")]
    EpochNotReady,
}