# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Disciplinator is a Solana/Anchor-based smart contract platform for habit tracking with financial incentives. Users deposit USDT tokens as collateral for completing personal challenges, with penalties redistributed as rewards to successful participants.

## Common Commands

### Build and Test
```bash
anchor build        # Build the Rust program
anchor test         # Run tests (includes build + deploy + test)
anchor deploy       # Deploy to configured cluster
anchor clean        # Clean build artifacts
```

### Code Quality
```bash
yarn lint          # Check TypeScript/JavaScript formatting
yarn lint:fix      # Auto-fix formatting issues
```

### Development Workflow
```bash
# Complete development cycle
anchor build                    # Build program and generate types
anchor test                     # Full test suite (build + deploy + test)
anchor deploy                   # Deploy to configured cluster

# Specific test commands
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts              # All tests
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/disciplinator.ts     # Main test file
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/disciplinator.ts -g "pattern"  # Specific tests
```

## Architecture Overview

### Technology Stack
- **Smart Contract**: Rust + Anchor framework (v0.31.1)
- **Tests/Client**: TypeScript with Mocha/Chai
- **Blockchain**: Solana
- **Token**: USDT (SPL Token 2022, 6 decimals)

### Core Components

**Smart Contract (`programs/disciplinator/src/lib.rs`)**:
- Challenge creation and management
- Session tracking with IPFS proof storage
- Financial penalty/reward distribution system
- User statistics and progress tracking

**Key Data Structures**:
- `Config`: Global protocol settings
- `Challenge`: Individual user challenges with deposit and progress
- `Session`: Individual session records with verification
- `UserStats`: User performance metrics
- `RewardState`: Global reward distribution state

### Project Structure
```
programs/disciplinator/     # Rust smart contract
  └── src/lib.rs           # Main program file with all instructions and accounts
tests/                      # TypeScript test suite
  └── disciplinator.ts     # Comprehensive test file covering all functionality
migrations/                 # Deployment scripts
  └── deploy.ts            # Anchor deployment script
target/                     # Build artifacts and generated types
  ├── deploy/              # Compiled .so file
  ├── idl/                 # Interface definitions (disciplinator.json)
  └── types/               # Generated TypeScript bindings (disciplinator.ts)
app/                        # Frontend directory (currently empty)
disciplinator-bot/          # Telegram bot integration (staged but not committed)
```

## Key Configuration

**Anchor.toml**: 
- Program ID: `Em4efpnH5X51Gr5hSKKWwJ4K2ktgcKDh5qgqr2w54WSH`
- Configured for localnet development
- Uses yarn as package manager

**Token Economics**:
- Deposit range: 5-10,000 USDT
- Completion-based refunds
- Configurable penalty distribution (fees, rewards, charity)

## Development Notes

- Generated TypeScript types are available at `target/types/disciplinator.ts`
- IDL file is generated at `target/idl/disciplinator.json`
- Tests require a running Solana validator (handled by `anchor test`)
- Tests use Mocha/Chai with TypeScript compilation via ts-mocha
- All tests are in single file `tests/disciplinator.ts` for comprehensive coverage
- Frontend application directory (`app/`) exists but is currently empty
- Telegram bot integration code exists in `disciplinator-bot/` directory (staged but not committed)

### Key Instructions and Functions

The smart contract (`programs/disciplinator/src/lib.rs`) implements these main instructions:
- `initialize`: Set up protocol configuration and token acceptance
- `create_challenge`: Create new habit tracking challenge with deposit
- `complete_session`: Mark session completion with optional IPFS proof
- `finalize_challenge`: Complete challenge and distribute rewards/refunds
- `distribute_rewards`: Distribute penalty pool to successful participants
- `claim_reward`: Allow users to claim distributed rewards
- `extend_grace_period`: Admin function to extend challenge deadlines
- `update_config`: Admin function to modify protocol settings
- `pause_protocol`/`unpause_protocol`: Emergency pause functionality

### Testing Patterns

Tests cover full workflow scenarios including setup, challenge lifecycle, and edge cases:
```bash
# Run all tests (recommended for full validation)
anchor test

# Run specific test patterns
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/disciplinator.ts -g "initialize"
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/disciplinator.ts -g "challenge"
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/disciplinator.ts -g "reward"
```

### Working with PDAs (Program Derived Addresses)

The contract uses several PDA seeds:
- Config: `[b"config"]`
- Challenge: `[b"challenge", user.key(), challenge_id.to_le_bytes()]`
- Session: `[b"session", challenge.key(), session_id.to_le_bytes()]`
- UserStats: `[b"user_stats", user.key()]`
- RewardState: `[b"reward_state"]`

### Contract Security Features

- Only accepts specific USDT mint addresses (configured per environment)
- Authority-based access control for admin functions
- Pausable protocol for emergency situations
- Validated deposit ranges (5-10,000 USDT)
- Grace period extensions limited to 3 per challenge

### Important Build Features

- **Test Mode**: Use `anchor build --features test-mode` for testing (bypasses USDT validation)
- **Type Generation**: `anchor build` automatically generates TypeScript types in `target/types/`
- **IDL Generation**: Interface definition in `target/idl/disciplinator.json` for client integration
- **Package Manager**: Project uses yarn (configured in Anchor.toml)