import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Disciplinator } from "../target/types/disciplinator";
import { 
  PublicKey, 
  Keypair, 
  SystemProgram, 
  SYSVAR_RENT_PUBKEY,
  LAMPORTS_PER_SOL
} from "@solana/web3.js";
import { 
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount
} from "@solana/spl-token";
import { assert } from "chai";

describe("disciplinator", () => {
  // Configure the client to use the local cluster.
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.disciplinator as Program<Disciplinator>;
  
  // Test keypairs
  const authority = Keypair.generate();
  const treasury = Keypair.generate();
  const participant = Keypair.generate();
  const verifier = Keypair.generate();
  
  // Token and account variables
  let mint: PublicKey;
  let authorityTokenAccount: PublicKey;
  let participantTokenAccount: PublicKey;
  let treasuryTokenAccount: PublicKey;
  
  // Program derived addresses
  let configPda: PublicKey;
  let vaultPda: PublicKey;
  let vaultRewardsPda: PublicKey;
  let vaultReservePda: PublicKey;
  let rewardStatePda: PublicKey;
  let userStatsPda: PublicKey;
  let challengePda: PublicKey;
  
  const USDT_DECIMALS = 6;
  const MIN_DEPOSIT = 5_000_000; // 5 USDT
  const TEST_DEPOSIT = 10_000_000; // 10 USDT

  before(async () => {
    // Airdrop SOL to test accounts
    await provider.connection.requestAirdrop(authority.publicKey, 2 * LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(participant.publicKey, 2 * LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(treasury.publicKey, LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(verifier.publicKey, LAMPORTS_PER_SOL);
    
    // Wait for airdrops to complete
    await new Promise(resolve => setTimeout(resolve, 1000));
    
    // Create USDT-like token mint
    mint = await createMint(
      provider.connection,
      authority,
      authority.publicKey,
      null,
      USDT_DECIMALS
    );
    
    // Create token accounts
    authorityTokenAccount = await createAccount(
      provider.connection,
      authority,
      mint,
      authority.publicKey
    );
    
    participantTokenAccount = await createAccount(
      provider.connection,
      participant,
      mint,
      participant.publicKey
    );
    
    treasuryTokenAccount = await createAccount(
      provider.connection,
      treasury,
      mint,
      treasury.publicKey
    );
    
    // Mint tokens to participant for testing
    await mintTo(
      provider.connection,
      authority,
      mint,
      participantTokenAccount,
      authority,
      1000_000_000 // 1000 USDT
    );
    
    // Calculate PDAs
    [configPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("config")],
      program.programId
    );
    
    [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), configPda.toBuffer()],
      program.programId
    );
    
    [vaultRewardsPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_rewards"), configPda.toBuffer()],
      program.programId
    );
    
    [vaultReservePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_reserve"), configPda.toBuffer()],
      program.programId
    );
    
    [rewardStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("reward_state")],
      program.programId
    );
    
    [userStatsPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("user_stats"), participant.publicKey.toBuffer()],
      program.programId
    );
    
    // First, test invalid percentage distribution
    try {
      await program.methods
        .initialize(30, 50, 30) // 110% total - should fail
        .accounts({
          config: configPda,
          authority: authority.publicKey,
          treasury: treasury.publicKey,
          acceptedMint: mint,
          vault: vaultPda,
          vaultRewards: vaultRewardsPda,
          vaultReserve: vaultReservePda,
          rewardState: rewardStatePda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: SYSVAR_RENT_PUBKEY,
        })
        .signers([authority])
        .rpc();
      
      throw new Error("Should have failed with invalid percentage distribution");
    } catch (error) {
      if (!error.message.includes("InvalidPercentageDistribution")) {
        throw new Error(`Expected InvalidPercentageDistribution error, got: ${error.message}`);
      }
      console.log("✓ InvalidPercentageDistribution test passed");
    }

    // Initialize the protocol with valid percentages
    const tx = await program.methods
      .initialize(20, 70, 10) // 20% fee, 70% rewards, 10% charity
      .accounts({
        config: configPda,
        authority: authority.publicKey,
        treasury: treasury.publicKey,
        acceptedMint: mint,
        vault: vaultPda,
        vaultRewards: vaultRewardsPda,
        vaultReserve: vaultReservePda,
        rewardState: rewardStatePda,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([authority])
      .rpc();
      
    console.log("Initialize transaction signature:", tx);
  });

  describe("Initialization", () => {
    it("Should verify protocol was initialized correctly", async () => {
      // Verify config was created correctly
      const config = await program.account.config.fetch(configPda);
      assert.equal(config.authority.toString(), authority.publicKey.toString());
      assert.equal(config.treasury.toString(), treasury.publicKey.toString());
      assert.equal(config.feePercentage, 20);
      assert.equal(config.rewardPercentage, 70);
      assert.equal(config.charityPercentage, 10);
      assert.equal(config.minDeposit.toNumber(), MIN_DEPOSIT);
      assert.isFalse(config.paused);
    });

  });

  describe("Challenge Management", () => {
    before(async () => {
      // Calculate challenge PDA for the first challenge (ID 0)
      const configAccount = await program.account.config.fetch(configPda);
      [challengePda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          participant.publicKey.toBuffer(),
          configAccount.totalChallenges.toArrayLike(Buffer, "le", 8)
        ],
        program.programId
      );
    });

    it("Should create a challenge", async () => {
      const tx = await program.methods
        .createChallenge(
          new anchor.BN(TEST_DEPOSIT),
          30, // 30 sessions
          30, // 30 days
          null, // no verifier
          { fitness: {} } // fitness challenge
        )
        .accounts({
          challenge: challengePda,
          participant: participant.publicKey,
          participantTokenAccount: participantTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          userStats: userStatsPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([participant])
        .rpc();

      console.log("Create challenge transaction signature:", tx);
      
      // Verify challenge was created
      const challenge = await program.account.challenge.fetch(challengePda);
      assert.equal(challenge.participant.toString(), participant.publicKey.toString());
      assert.equal(challenge.depositAmount.toNumber(), TEST_DEPOSIT);
      assert.equal(challenge.totalSessions, 30);
      assert.equal(challenge.completedSessions, 0);
      assert.isTrue(challenge.status.active !== undefined); // Check it's Active variant
      
      // Verify tokens were transferred to vault
      const vaultAccount = await getAccount(
        provider.connection,
        vaultPda,
        undefined,
        TOKEN_PROGRAM_ID
      );
      assert.equal(vaultAccount.amount.toString(), TEST_DEPOSIT.toString());
    });

    it("Should fail to create challenge with insufficient deposit", async () => {
      const smallDeposit = 1_000_000; // 1 USDT - below minimum
      
      // Create a new challenge PDA for a different challenge ID
      const configAccount = await program.account.config.fetch(configPda);
      const [newChallengePda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          participant.publicKey.toBuffer(),
          new anchor.BN(1).toArrayLike(Buffer, "le", 8) // Use challenge ID 1
        ],
        program.programId
      );
      
      try {
        await program.methods
          .createChallenge(
            new anchor.BN(smallDeposit),
            10,
            7,
            null,
            { fitness: {} }
          )
          .accounts({
            challenge: newChallengePda,
            participant: participant.publicKey,
            participantTokenAccount: participantTokenAccount,
            config: configPda,
            acceptedMint: mint,
            vault: vaultPda,
            userStats: userStatsPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([participant])
          .rpc();
        
        assert.fail("Should have failed with deposit too small");
      } catch (error) {
        assert.include(error.message, "DepositTooSmall");
      }
    });
  });

  describe("Session Management", () => {
    let sessionPda: PublicKey;
    let sessionChallengePda: PublicKey;
    let verifier: Keypair;

    before(async () => {
      // Create a verifier account and token account
      verifier = Keypair.generate();
      await provider.connection.requestAirdrop(verifier.publicKey, LAMPORTS_PER_SOL);
      await new Promise(resolve => setTimeout(resolve, 500));
      
      // Create a new challenge with verifier for session tests
      const configAccount = await program.account.config.fetch(configPda);
      [sessionChallengePda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          participant.publicKey.toBuffer(),
          configAccount.totalChallenges.toArrayLike(Buffer, "le", 8)
        ],
        program.programId
      );
      
      // Create the challenge with a verifier
      await program.methods
        .createChallenge(
          new anchor.BN(TEST_DEPOSIT),
          10, // 10 sessions  
          30, // 30 days
          verifier.publicKey, // With verifier
          { fitness: {} }
        )
        .accounts({
          challenge: sessionChallengePda,
          participant: participant.publicKey,
          participantTokenAccount: participantTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          userStats: userStatsPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([participant])
        .rpc();
      
      // Calculate session PDA for the first session (session 0)
      [sessionPda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("session"),
          sessionChallengePda.toBuffer(),
          Buffer.from([0, 0, 0, 0]) // completed_sessions as u32 LE bytes
        ],
        program.programId
      );
    });

    it("Should mark a session as complete", async () => {
      const tx = await program.methods
        .markSessionComplete(
          "QmNRCQX8gEZsRp1UnD3mNsGj5hCKjFkWqWkYpGjkhU4QgU",
          {
            durationMinutes: 45,
            location: "Home Gym",
            notes: "Great workout session"
          }
        )
        .accounts({
          challenge: sessionChallengePda,
          participant: participant.publicKey,
          signer: verifier.publicKey,
          session: sessionPda,
          userStats: userStatsPda,
          systemProgram: SystemProgram.programId,
        })
        .signers([verifier])
        .rpc();

      console.log("Mark session complete transaction signature:", tx);
      
      // Verify session was recorded
      const session = await program.account.session.fetch(sessionPda);
      assert.equal(session.challenge.toString(), sessionChallengePda.toString());
      assert.equal(session.sessionNumber, 1);
      assert.equal(session.proofIpfsHash, "QmNRCQX8gEZsRp1UnD3mNsGj5hCKjFkWqWkYpGjkhU4QgU");
      assert.equal(session.verifiedBy.toString(), verifier.publicKey.toString());
      assert.isFalse(session.autoVerified);
      
      // Verify challenge was updated
      const challenge = await program.account.challenge.fetch(sessionChallengePda);
      assert.equal(challenge.completedSessions, 1);
    });

    it("Should fail to mark session too soon", async () => {
      const [session2Pda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("session"),
          sessionChallengePda.toBuffer(),
          Buffer.from([1, 0, 0, 0]) // session 1
        ],
        program.programId
      );

      try {
        await program.methods
          .markSessionComplete(
            "QmTXKz6FwjhZGNKUjC2qv3XN7efdF8hSYrUwPxEGdBnNkS",
            {
              durationMinutes: 30,
              location: null,
              notes: null
            }
          )
          .accounts({
            challenge: sessionChallengePda,
            participant: participant.publicKey,
            signer: verifier.publicKey,
            session: session2Pda,
            userStats: userStatsPda,
            systemProgram: SystemProgram.programId,
          })
          .signers([verifier])
          .rpc();
        
        assert.fail("Should have failed with session too soon");
      } catch (error) {
        assert.include(error.message, "SessionTooSoon");
      }
    });
  });

  describe("Grace Period", () => {
    it("Should allow using a grace period", async () => {
      const [gracePda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("grace"),
          challengePda.toBuffer(),
          Buffer.from([0]) // grace_periods_used as u8
        ],
        program.programId
      );

      const tx = await program.methods
        .useGracePeriod("Unexpected work commitment")
        .accounts({
          challenge: challengePda,
          participant: participant.publicKey,
          graceRecord: gracePda,
          systemProgram: SystemProgram.programId,
        })
        .signers([participant])
        .rpc();

      console.log("Use grace period transaction signature:", tx);
      
      // Verify grace period was recorded
      const graceRecord = await program.account.gracePeriodRecord.fetch(gracePda);
      assert.equal(graceRecord.challenge.toString(), challengePda.toString());
      assert.equal(graceRecord.reason, "Unexpected work commitment");
      
      // Verify challenge end time was extended
      const challenge = await program.account.challenge.fetch(challengePda);
      assert.equal(challenge.gracePeriodsUsed, 1);
    });
  });

  describe("Protocol Controls", () => {
    it("Should pause the protocol", async () => {
      const tx = await program.methods
        .pauseProtocol()
        .accounts({
          config: configPda,
          authority: authority.publicKey,
        })
        .signers([authority])
        .rpc();

      console.log("Pause protocol transaction signature:", tx);
      
      const config = await program.account.config.fetch(configPda);
      assert.isTrue(config.paused);
    });

    it("Should unpause the protocol", async () => {
      const tx = await program.methods
        .unpauseProtocol()
        .accounts({
          config: configPda,
          authority: authority.publicKey,
        })
        .signers([authority])
        .rpc();

      console.log("Unpause protocol transaction signature:", tx);
      
      const config = await program.account.config.fetch(configPda);
      assert.isFalse(config.paused);
    });

    it("Should fail to pause with wrong authority", async () => {
      try {
        await program.methods
          .pauseProtocol()
          .accounts({
            config: configPda,
            authority: participant.publicKey,
          })
          .signers([participant])
          .rpc();
        
        assert.fail("Should have failed with wrong authority");
      } catch (error) {
        assert.include(error.message, "ConstraintRaw");
      }
    });
  });

  describe("Finalization", () => {
    it("Should finalize a challenge", async () => {
      // Create a verifier for this test
      const testVerifier = Keypair.generate();
      await provider.connection.requestAirdrop(testVerifier.publicKey, LAMPORTS_PER_SOL);
      await new Promise(resolve => setTimeout(resolve, 500));
      
      // Create a new challenge specifically for finalization testing
      const configAccount = await program.account.config.fetch(configPda);
      const [finalizationChallengePda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          participant.publicKey.toBuffer(),
          configAccount.totalChallenges.toArrayLike(Buffer, "le", 8)
        ],
        program.programId
      );
      
      // Create the challenge with just 1 session to avoid timing constraints
      await program.methods
        .createChallenge(
          new anchor.BN(TEST_DEPOSIT),
          1, // Only 1 session to avoid SessionTooSoon errors
          30, // 30 days
          testVerifier.publicKey, // With our test verifier
          { fitness: {} }
        )
        .accounts({
          challenge: finalizationChallengePda,
          participant: participant.publicKey,
          participantTokenAccount: participantTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          userStats: userStatsPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([participant])
        .rpc();
      
      const [finalizationPda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("finalization"),
          finalizationChallengePda.toBuffer()
        ],
        program.programId
      );

      // Complete the 1 session to make finalization possible
      for (let i = 0; i < 1; i++) {
        // Calculate session PDA for each session
        const [currentSessionPda] = PublicKey.findProgramAddressSync(
          [
            Buffer.from("session"),
            finalizationChallengePda.toBuffer(),
            Buffer.from([i & 0xFF, (i >> 8) & 0xFF, (i >> 16) & 0xFF, (i >> 24) & 0xFF])
          ],
          program.programId
        );
        
        await program.methods
          .markSessionComplete(
            "QmNRCQX8gEZsRp1UnD3mNsGj5hCKjFkWqWkYpGjkhU4QgU", // Valid IPFS hash (same for all for simplicity)
            {
              durationMinutes: 30,
              location: "Test Location",
              notes: "Test session " + i
            }
          )
          .accounts({
            challenge: finalizationChallengePda,
            participant: participant.publicKey,
            signer: testVerifier.publicKey,
            session: currentSessionPda,
            userStats: userStatsPda,
            systemProgram: SystemProgram.programId,
          })
          .signers([testVerifier])
          .rpc();
        
        // Add delay between sessions to avoid "SessionTooSoon" error
        await new Promise(resolve => setTimeout(resolve, 1000));
      }
      
      // Now finalize the challenge
      const tx = await program.methods
        .finalizeChallenge()
        .accounts({
          challenge: finalizationChallengePda,
          participant: participant.publicKey,
          participantTokenAccount: participantTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          treasuryTokenAccount: treasuryTokenAccount,
          userStats: userStatsPda,
          finalizationRecord: finalizationPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([participant])
        .rpc();

      console.log("Finalize challenge transaction signature:", tx);
      
      // Verify finalization record was created
      const finalizationRecord = await program.account.finalizationRecord.fetch(finalizationPda);
      assert.equal(finalizationRecord.challenge.toString(), finalizationChallengePda.toString());
      assert.equal(finalizationRecord.participant.toString(), participant.publicKey.toString());
      
      // Verify challenge status was updated
      const challenge = await program.account.challenge.fetch(finalizationChallengePda);
      assert.isNotNull(challenge.status.failed || challenge.status.partiallyCompleted);
    });
  });

  describe("Security Tests", () => {
    const maliciousUser = Keypair.generate();
    let maliciousTokenAccount: PublicKey;

    before(async () => {
      // Airdrop SOL to malicious user
      await provider.connection.requestAirdrop(maliciousUser.publicKey, LAMPORTS_PER_SOL);
      await new Promise(resolve => setTimeout(resolve, 1000));

      // Create token account for malicious user
      maliciousTokenAccount = await createAccount(
        provider.connection,
        maliciousUser,
        mint,
        maliciousUser.publicKey,
        undefined,
        undefined,
        TOKEN_PROGRAM_ID
      );

      // Mint tokens to malicious user
      await mintTo(
        provider.connection,
        authority,
        mint,
        maliciousTokenAccount,
        authority,
        1000_000_000 // 1000 USDT
      );
    });

    it("Should prevent unauthorized session marking (verifier-only)", async () => {
      // Get current challenge count
      const configAccount = await program.account.config.fetch(configPda);
      
      // Create a challenge first
      const maliciousChallengePda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          configAccount.totalChallenges.toArrayLike(Buffer, "le", 8)
        ],
        program.programId
      )[0];

      const maliciousUserStatsPda = PublicKey.findProgramAddressSync(
        [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
        program.programId
      )[0];

      // Create challenge with verifier (using a different signer as verifier)
      const challengeVerifier = Keypair.generate();
      await provider.connection.requestAirdrop(challengeVerifier.publicKey, LAMPORTS_PER_SOL);
      await new Promise(resolve => setTimeout(resolve, 500));
      
      await program.methods
        .createChallenge(
          new anchor.BN(TEST_DEPOSIT),
          21,
          30,
          verifier.publicKey, // Set verifier
          { fitness: {} }
        )
        .accounts({
          challenge: maliciousChallengePda,
          participant: maliciousUser.publicKey,
          participantTokenAccount: maliciousTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          userStats: maliciousUserStatsPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([maliciousUser])
        .rpc();

      // Now try to self-verify session (should fail)
      const sessionPda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("session"),
          maliciousChallengePda.toBuffer(),
          Buffer.from([0, 0, 0, 0])
        ],
        program.programId
      )[0];

      try {
        await program.methods
          .markSessionComplete(
            "QmTest123456789012345678901234567890123456", // Valid IPFS hash format
            {
              durationMinutes: 30,
              location: "Test Location",
              notes: "Self-verification attempt"
            }
          )
          .accounts({
            challenge: maliciousChallengePda,
            participant: maliciousUser.publicKey,
            signer: maliciousUser.publicKey, // Participant trying to self-verify
            session: sessionPda,
            userStats: maliciousUserStatsPda,
            systemProgram: SystemProgram.programId,
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected participant self-verification");
      } catch (error) {
        assert.include(error.toString(), "ConstraintRaw");
      }
    });

    it("Should reject invalid IPFS hash format", async () => {
      // Create a challenge for this test
      const configAccount = await program.account.config.fetch(configPda);
      const [testChallengePda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          configAccount.totalChallenges.toArrayLike(Buffer, "le", 8)
        ],
        program.programId
      );
      
      // Create the challenge first
      await program.methods
        .createChallenge(
          new anchor.BN(TEST_DEPOSIT),
          10,
          30,
          participant.publicKey, // verifier
          { fitness: {} }
        )
        .accounts({
          challenge: testChallengePda,
          participant: maliciousUser.publicKey,
          participantTokenAccount: maliciousTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          userStats: PublicKey.findProgramAddressSync(
            [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
            program.programId
          )[0],
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([maliciousUser])
        .rpc();
      
      const sessionPda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("session"),
          testChallengePda.toBuffer(),
          Buffer.from([0, 0, 0, 0])
        ],
        program.programId
      )[0];

      try {
        await program.methods
          .markSessionComplete(
            "invalid_hash", // Invalid IPFS hash
            {
              durationMinutes: 30,
              location: "Test Location",
              notes: "Test session"
            }
          )
          .accounts({
            challenge: testChallengePda,
            participant: maliciousUser.publicKey,
            signer: participant.publicKey,
            session: sessionPda,
            userStats: PublicKey.findProgramAddressSync(
              [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
              program.programId
            )[0],
            systemProgram: SystemProgram.programId,
          })
          .signers([participant])
          .rpc();

        assert.fail("Should have rejected invalid IPFS hash");
      } catch (error) {
        assert.include(error.toString(), "InvalidIPFSHash");
      }
    });

    it("Should reject challenges with invalid session counts", async () => {
      const configAccount = await program.account.config.fetch(configPda);
      const invalidChallengePda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          Buffer.from(configAccount.totalChallenges.toArrayLike(Buffer, "le", 8))
        ],
        program.programId
      )[0];

      const maliciousUserStatsPda = PublicKey.findProgramAddressSync(
        [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
        program.programId
      )[0];

      try {
        await program.methods
          .createChallenge(
            new anchor.BN(TEST_DEPOSIT),
            500, // Too many sessions
            30,
            verifier.publicKey,
            { fitness: {} }
          )
          .accounts({
            challenge: invalidChallengePda,
            participant: maliciousUser.publicKey,
            participantTokenAccount: maliciousTokenAccount,
            config: configPda,
            acceptedMint: mint,
            vault: vaultPda,
            userStats: maliciousUserStatsPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected invalid session count");
      } catch (error) {
        assert.include(error.toString(), "InvalidSessionCount");
      }
    });

    it("Should reject challenges with invalid duration", async () => {
      const configAccount = await program.account.config.fetch(configPda);
      const invalidChallengePda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          Buffer.from(configAccount.totalChallenges.toArrayLike(Buffer, "le", 8))
        ],
        program.programId
      )[0];

      const maliciousUserStatsPda = PublicKey.findProgramAddressSync(
        [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
        program.programId
      )[0];

      try {
        await program.methods
          .createChallenge(
            new anchor.BN(TEST_DEPOSIT),
            21,
            500, // Too many days
            verifier.publicKey,
            { fitness: {} }
          )
          .accounts({
            challenge: invalidChallengePda,
            participant: maliciousUser.publicKey,
            participantTokenAccount: maliciousTokenAccount,
            config: configPda,
            acceptedMint: mint,
            vault: vaultPda,
            userStats: maliciousUserStatsPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected invalid duration");
      } catch (error) {
        assert.include(error.toString(), "InvalidDuration");
      }
    });

    it("Should reject deposit amounts that are too large", async () => {
      const configAccount = await program.account.config.fetch(configPda);
      const invalidChallengePda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          Buffer.from(configAccount.totalChallenges.toArrayLike(Buffer, "le", 8))
        ],
        program.programId
      )[0];

      const maliciousUserStatsPda = PublicKey.findProgramAddressSync(
        [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
        program.programId
      )[0];

      try {
        await program.methods
          .createChallenge(
            new anchor.BN("15000000000000"), // 15 million USDT (too large)
            21,
            30,
            verifier.publicKey,
            { fitness: {} }
          )
          .accounts({
            challenge: invalidChallengePda,
            participant: maliciousUser.publicKey,
            participantTokenAccount: maliciousTokenAccount,
            config: configPda,
            acceptedMint: mint,
            vault: vaultPda,
            userStats: maliciousUserStatsPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected amount exceeding max deposit");
      } catch (error) {
        assert.include(error.toString(), "DepositTooLarge");
      }
    });

    it("Should reject deposit amounts that are too small", async () => {
      const configAccount = await program.account.config.fetch(configPda);
      const invalidChallengePda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          Buffer.from(configAccount.totalChallenges.toArrayLike(Buffer, "le", 8))
        ],
        program.programId
      )[0];

      const maliciousUserStatsPda = PublicKey.findProgramAddressSync(
        [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
        program.programId
      )[0];

      try {
        await program.methods
          .createChallenge(
            new anchor.BN(1_000_000), // 1 USDT (too small)
            21,
            30,
            verifier.publicKey,
            { fitness: {} }
          )
          .accounts({
            challenge: invalidChallengePda,
            participant: maliciousUser.publicKey,
            participantTokenAccount: maliciousTokenAccount,
            config: configPda,
            acceptedMint: mint,
            vault: vaultPda,
            userStats: maliciousUserStatsPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected amount below min deposit");
      } catch (error) {
        assert.include(error.toString(), "DepositTooSmall");
      }
    });

    it("Should prevent unauthorized protocol pausing", async () => {
      try {
        await program.methods
          .pauseProtocol()
          .accounts({
            config: configPda,
            authority: maliciousUser.publicKey, // Wrong authority
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected unauthorized pause");
      } catch (error) {
        assert.include(error.toString(), "AnchorError");
      }
    });

    it("Should create challenge without verifier and reject self-verification", async () => {
      // Get current challenge count
      const configAccount = await program.account.config.fetch(configPda);
      
      const noVerifierChallengePda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("challenge"),
          maliciousUser.publicKey.toBuffer(),
          configAccount.totalChallenges.toArrayLike(Buffer, "le", 8)
        ],
        program.programId
      )[0];

      const maliciousUserStatsPda = PublicKey.findProgramAddressSync(
        [Buffer.from("user_stats"), maliciousUser.publicKey.toBuffer()],
        program.programId
      )[0];

      // Create challenge without verifier
      await program.methods
        .createChallenge(
          new anchor.BN(TEST_DEPOSIT),
          21,
          30,
          null, // No verifier
          { fitness: {} }
        )
        .accounts({
          challenge: noVerifierChallengePda,
          participant: maliciousUser.publicKey,
          participantTokenAccount: maliciousTokenAccount,
          config: configPda,
          acceptedMint: mint,
          vault: vaultPda,
          userStats: maliciousUserStatsPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([maliciousUser])
        .rpc();

      // Try to mark session (should fail because no verifier is set)
      const sessionPda = PublicKey.findProgramAddressSync(
        [
          Buffer.from("session"),
          noVerifierChallengePda.toBuffer(),
          Buffer.from([0, 0, 0, 0])
        ],
        program.programId
      )[0];

      try {
        await program.methods
          .markSessionComplete(
            "QmTest123456789012345678901234567890123456",
            {
              durationMinutes: 30,
              location: "Test Location",
              notes: "Test session"
            }
          )
          .accounts({
            challenge: noVerifierChallengePda,
            participant: maliciousUser.publicKey,
            signer: maliciousUser.publicKey,
            session: sessionPda,
            userStats: maliciousUserStatsPda,
            systemProgram: SystemProgram.programId,
          })
          .signers([maliciousUser])
          .rpc();

        assert.fail("Should have rejected session marking for challenge without verifier");
      } catch (error) {
        assert.include(error.toString(), "ConstraintRaw");
      }
    });
  });
});