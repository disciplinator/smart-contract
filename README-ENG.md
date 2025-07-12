# Disciplinator - Habit Tracking Smart Contract

Disciplinator is a Solana/Anchor-based smart contract platform for habit tracking with financial incentives. Users deposit USDT tokens as collateral for completing personal challenges, with penalties redistributed as rewards to successful participants.

## 🎯 What is this contract?

Disciplinator is a habit formation platform with economic incentives where:

- **Users create challenges** with financial collateral in USDT
- **External verifiers** confirm session completion
- **Penalties for non-completion** are distributed among successful participants
- **Reward system** motivates long-term participation

## 🔧 Why is this contract needed?

### Problem
Many people struggle to maintain beneficial habits due to lack of motivation and accountability.

### Solution
Disciplinator solves this problem through:

1. **Financial motivation**: Users risk their own money
2. **External verification**: Independent verifiers prevent fraud
3. **Positive reinforcement**: Successful participants receive additional rewards
4. **Flexibility**: Support for various challenge types (fitness, education, meditation, custom)

## 📊 Core Entities

### 1. Config (Protocol Configuration)
```rust
pub struct Config {
    pub authority: Pubkey,          // Protocol administrator
    pub treasury: Pubkey,           // Treasury account
    pub accepted_mint: Pubkey,      // Accepted token (USDT)
    pub fee_percentage: u8,         // Protocol fee percentage
    pub reward_percentage: u8,      // Reward pool percentage
    pub charity_percentage: u8,     // Charity percentage
    pub total_challenges: u64,      // Total number of challenges
    pub total_volume: u64,          // Total deposit volume
    pub paused: bool,               // Protocol pause status
    pub min_deposit: u64,           // Minimum deposit (5 USDT)
    pub max_deposit: u64,           // Maximum deposit (10,000 USDT)
}
```

### 2. Challenge
```rust
pub struct Challenge {
    pub participant: Pubkey,        // Participant
    pub deposit_amount: u64,        // Deposit amount
    pub total_sessions: u32,        // Total number of sessions
    pub completed_sessions: u32,    // Completed sessions
    pub start_time: i64,            // Start time
    pub end_time: i64,              // End time
    pub status: ChallengeStatus,    // Status: Active/Completed/Failed/etc.
    pub verifier: Option<Pubkey>,   // Verifier (optional)
    pub challenge_type: ChallengeType, // Type: Fitness/Education/Meditation/Custom
    pub grace_periods_used: u8,     // Used grace periods (max 3)
}
```

### 3. Session
```rust
pub struct Session {
    pub challenge: Pubkey,          // Associated challenge
    pub session_number: u32,        // Session number
    pub timestamp: i64,             // Completion time
    pub proof_ipfs_hash: String,    // IPFS proof hash
    pub verified_by: Pubkey,        // Verifier
    pub metadata: SessionMetadata,  // Metadata (duration, location, notes)
    pub auto_verified: bool,        // Automatic verification
}
```

### 4. UserStats (User Statistics)
```rust
pub struct UserStats {
    pub total_challenges: u32,           // Total challenges
    pub challenges_completed: u32,       // Fully completed
    pub challenges_partial: u32,         // Partially completed (80%+)
    pub challenges_failed: u32,          // Failed
    pub perfect_completions: u32,        // Perfect completions (100%)
    pub total_sessions_completed: u32,   // Total sessions completed
    pub current_streak: u32,            // Current streak
    pub best_streak: u32,               // Best streak
    pub total_deposited: u64,           // Total deposited amount
    pub total_refunded: u64,            // Total refunded amount
    pub total_penalties: u64,           // Total penalty amount
    pub total_rewards_claimed: u64,     // Total rewards claimed
}
```

### 5. FinalizationRecord
```rust
pub struct FinalizationRecord {
    pub challenge: Pubkey,              // Challenge
    pub participant: Pubkey,            // Participant
    pub completion_rate_percentage: u64, // Completion percentage (0-10000)
    pub penalty_amount: u64,            // Penalty amount
    pub reward_pool_contribution: u64,  // Reward pool contribution
    pub timestamp: i64,                 // Finalization time
    pub rewarded: bool,                 // Rewards distributed
}
```

### 6. RewardState (Reward State)
```rust
pub struct RewardState {
    pub last_epoch_processed: u64,     // Last processed epoch
    pub next_epoch_time: i64,           // Next epoch time
    pub total_distributed: u64,        // Total distributed rewards
}
```

## 🔗 Entity Relationships

```
Config (1) ←→ (∞) Challenge
    ↓
  Vault (stores deposits)

Challenge (1) ←→ (∞) Session
    ↓
Participant (User)
    ↓
UserStats (1:1)

Challenge (1) → (1) FinalizationRecord
    ↓
RewardState (global)
```

### Challenge Lifecycle:
1. **Creation**: User creates challenge with deposit → tokens locked in Vault
2. **Execution**: Verifier marks completed sessions → statistics updated
3. **Grace Periods**: User can extend challenge (up to 3 times, 3 days each)
4. **Finalization**: After time expires or all sessions completed:
   - Refund proportional to completion percentage
   - Penalties distributed: protocol fee + reward pool + charity
5. **Reward Distribution**: Weekly distribution to successful participants from reward pool

## ⚠️ Limitations and Rules

### Financial Constraints
- **Minimum deposit**: 5 USDT (5,000,000 in 6-decimal format)
- **Maximum deposit**: 10,000 USDT (10,000,000,000 in 6-decimal format)
- **Accepted tokens**: Only official USDT tokens on Solana

### Time Constraints
- **Minimum challenge duration**: 7 days
- **Maximum challenge duration**: 365 days
- **Maximum number of sessions**: 365
- **Minimum interval between sessions**: 12-48 hours (depends on challenge parameters)
- **Grace periods**: Maximum 3 periods of 3 days each

### Security and Access Control
- **Session verification**: Only designated verifier can confirm sessions
- **Self-verification prohibited**: Participants cannot confirm their own sessions
- **Admin control**: Only authority can pause protocol and change settings
- **IPFS validation**: Proofs must be valid IPFS hashes (46 characters, starting with "Qm")

### Business Logic
- **Success criteria**:
  - 100% completion = Completed (full refund + eligible for rewards)
  - 80-99% completion = PartiallyCompleted (proportional refund + streak maintained)
  - <80% completion = Failed (minimal refund, streak reset)
- **Penalty distribution**: 100% = protocol fee + reward pool + charity
- **Rewards**: Distributed weekly based on user performance scores

### Challenge Types and Session Requirements
- **Fitness**: Minimum 20 minutes per session
- **Education**: Minimum 30 minutes per session
- **Meditation**: Minimum 10 minutes per session
- **Custom**: Flexible requirements

### Technical Limitations
- **Program Derived Addresses (PDA)**: Uses deterministic addresses for all accounts
- **Token Program**: Only Token Program 2022 for USDT
- **Arithmetic safety**: Overflow protection in all calculations
- **Protocol pauses**: Emergency stop capability

## 🏗️ Technical Architecture

- **Blockchain**: Solana
- **Framework**: Anchor v0.31.1
- **Contract Language**: Rust
- **Token**: USDT (SPL Token 2022, 6 decimals)
- **Tests**: TypeScript with Mocha/Chai
- **Program ID**: `Em4efpnH5X51Gr5hSKKWwJ4K2ktgcKDh5qgqr2w54WSH`

## 🚀 Deployment and Testing

```bash
# Build contract
anchor build

# Run tests (includes build, deploy, and testing)
anchor test

# Deploy to configured cluster
anchor deploy

# Clean build artifacts
anchor clean
```

The contract includes comprehensive security tests that verify protection against unauthorized access, invalid data, and various attack vectors.