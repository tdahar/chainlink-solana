import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import {
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
} from "@solana/web3.js";
import {
  createMint,
  getAccount,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { assert } from "chai";

import { OperatorStaking } from "../target/types/operator_staking";

const header = 8 + 192; // account discriminator + header
const transmissionSize = 48;

const POOL_SEED = Buffer.from("staking_pool");
const VAULT_SEED = Buffer.from("staking_vault");
const OPERATOR_SEED = Buffer.from("operator");
const DELEGATION_SEED = Buffer.from("delegation");

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe("operator-staking", () => {
  const provider = anchor.AnchorProvider.local();
  anchor.setProvider(provider);

  const workspace = anchor.workspace;
  const program: Program<OperatorStaking> = workspace.OperatorStaking;

  const admin = provider.wallet;
  const feed = Keypair.generate();
  const operatorAuthority = Keypair.generate();
  const delegator = Keypair.generate();
  const alerter = Keypair.generate();

  const config = {
    minOperatorStake: new BN(1_000),
    maxDelegationRatio: 10,
    unbondingPeriodSecs: new BN(2),
    alertStalenessSecs: new BN(60),
    slashBps: 1_000, // 10%
    alerterRewardBps: 2_000, // 20% of the slash
  };
  const rewardRatePerSec = new BN(100);

  let mint: PublicKey;
  let pool: PublicKey;
  let vaultAuthority: PublicKey;
  let tokenVault: PublicKey;
  let operator: PublicKey;
  let delegation: PublicKey;

  let adminToken: PublicKey;
  let operatorToken: PublicKey;
  let delegatorToken: PublicKey;
  let alerterToken: PublicKey;

  const balance = async (account: PublicKey): Promise<bigint> =>
    (await getAccount(provider.connection, account)).amount;

  const airdrop = async (to: PublicKey) => {
    const sig = await provider.connection.requestAirdrop(
      to,
      10 * LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(sig, "confirmed");
  };

  const tokenAccount = async (owner: PublicKey, amount: number) => {
    const account = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      admin.payer,
      mint,
      owner
    );
    if (amount > 0) {
      await mintTo(
        provider.connection,
        admin.payer,
        mint,
        account.address,
        admin.payer,
        amount
      );
    }
    return account.address;
  };

  const submitRound = async (timestamp: number, answer: number) => {
    await workspace.Store.methods
      .submit({ timestamp: new BN(timestamp), answer: new BN(answer) })
      .accounts({ feed: feed.publicKey, authority: admin.publicKey })
      .rpc();
  };

  before(async () => {
    await Promise.all(
      [operatorAuthority, delegator, alerter].map((k) => airdrop(k.publicKey))
    );

    mint = await createMint(
      provider.connection,
      admin.payer,
      admin.publicKey,
      null,
      9
    );

    // A feed written by the admin, so the test controls its round timestamps.
    const liveLength = 3;
    const historicalLength = 3;
    await workspace.Store.methods
      .createFeed("ETH/USD", 8, 30, liveLength)
      .accounts({ feed: feed.publicKey, authority: admin.publicKey })
      .signers([feed])
      .preInstructions([
        await workspace.Store.account.transmissions.createInstruction(
          feed,
          header + (liveLength + historicalLength) * transmissionSize
        ),
      ])
      .rpc();
    await workspace.Store.methods
      .setWriter(admin.publicKey)
      .accounts({
        feed: feed.publicKey,
        owner: admin.publicKey,
        authority: admin.publicKey,
      })
      .rpc();

    [pool] = PublicKey.findProgramAddressSync(
      [POOL_SEED, feed.publicKey.toBuffer()],
      program.programId
    );
    [vaultAuthority] = PublicKey.findProgramAddressSync(
      [VAULT_SEED, pool.toBuffer()],
      program.programId
    );
    tokenVault = (
      await getOrCreateAssociatedTokenAccount(
        provider.connection,
        admin.payer,
        mint,
        vaultAuthority,
        true
      )
    ).address;
    [operator] = PublicKey.findProgramAddressSync(
      [OPERATOR_SEED, pool.toBuffer(), operatorAuthority.publicKey.toBuffer()],
      program.programId
    );
    [delegation] = PublicKey.findProgramAddressSync(
      [DELEGATION_SEED, operator.toBuffer(), delegator.publicKey.toBuffer()],
      program.programId
    );

    adminToken = await tokenAccount(admin.publicKey, 1_000_000);
    operatorToken = await tokenAccount(operatorAuthority.publicKey, 100_000);
    delegatorToken = await tokenAccount(delegator.publicKey, 100_000);
    alerterToken = await tokenAccount(alerter.publicKey, 0);
  });

  it("Initializes the pool", async () => {
    await program.methods
      .initializePool(config, rewardRatePerSec)
      .accounts({
        feed: feed.publicKey,
        pool,
        admin: admin.publicKey,
        payer: admin.publicKey,
        tokenMint: mint,
        vaultAuthority,
        tokenVault,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const state = await program.account.stakingPool.fetch(pool);
    assert.ok(state.feed.equals(feed.publicKey));
    assert.ok(state.admin.equals(admin.publicKey));
    assert.equal(state.config.slashBps, config.slashBps);
    assert.equal(state.totalStaked.toNumber(), 0);
  });

  it("Rejects an invalid configuration", async () => {
    try {
      await program.methods
        .setPoolConfig({ ...config, slashBps: 9_000 })
        .accounts({ pool, admin: admin.publicKey })
        .rpc();
      assert.fail("expected the config to be rejected");
    } catch (err) {
      assert.include(err.toString(), "InvalidConfig");
    }
  });

  it("Funds rewards", async () => {
    await program.methods
      .fundRewards(new BN(500_000))
      .accounts({
        pool,
        funder: admin.publicKey,
        funderToken: adminToken,
        tokenVault,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();
    const state = await program.account.stakingPool.fetch(pool);
    assert.equal(state.rewardReserve.toNumber(), 500_000);
  });

  it("Rejects an operator below the minimum stake", async () => {
    try {
      await program.methods
        .registerOperator(1_000, new BN(999))
        .accounts({
          pool,
          operator,
          authority: operatorAuthority.publicKey,
          authorityToken: operatorToken,
          tokenVault,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([operatorAuthority])
        .rpc();
      assert.fail("expected the registration to be rejected");
    } catch (err) {
      assert.include(err.toString(), "StakeBelowMinimum");
    }
  });

  it("Rejects a commission above the maximum", async () => {
    try {
      await program.methods
        .registerOperator(6_000, new BN(10_000))
        .accounts({
          pool,
          operator,
          authority: operatorAuthority.publicKey,
          authorityToken: operatorToken,
          tokenVault,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([operatorAuthority])
        .rpc();
      assert.fail("expected the registration to be rejected");
    } catch (err) {
      assert.include(err.toString(), "CommissionTooHigh");
    }
  });

  it("Registers an operator", async () => {
    await program.methods
      .registerOperator(1_000, new BN(10_000))
      .accounts({
        pool,
        operator,
        authority: operatorAuthority.publicKey,
        authorityToken: operatorToken,
        tokenVault,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([operatorAuthority])
      .rpc();

    const state = await program.account.operator.fetch(operator);
    assert.ok(state.active);
    assert.equal(state.selfStake.toNumber(), 10_000);
    assert.equal(state.commissionBps, 1_000);
    const poolState = await program.account.stakingPool.fetch(pool);
    assert.equal(poolState.totalStaked.toNumber(), 10_000);
    assert.equal(poolState.operatorCount, 1);
  });

  it("Delegates to the operator", async () => {
    await program.methods
      .openDelegation()
      .accounts({
        pool,
        operator,
        delegation,
        delegator: delegator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([delegator])
      .rpc();

    await program.methods
      .delegate(new BN(30_000))
      .accounts({
        pool,
        operator,
        delegation,
        delegator: delegator.publicKey,
        delegatorToken,
        tokenVault,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([delegator])
      .rpc();

    const state = await program.account.delegation.fetch(delegation);
    // first delegation: one share per token
    assert.equal(state.shares.toNumber(), 30_000);
    const op = await program.account.operator.fetch(operator);
    assert.equal(op.delegatedAmount.toNumber(), 30_000);
    assert.equal(op.delegatorCount, 1);
  });

  it("Rejects a delegation above the operator's capacity", async () => {
    try {
      await program.methods
        .delegate(new BN(70_001))
        .accounts({
          pool,
          operator,
          delegation,
          delegator: delegator.publicKey,
          delegatorToken,
          tokenVault,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([delegator])
        .rpc();
      assert.fail("expected the delegation to be rejected");
    } catch (err) {
      assert.include(err.toString(), "DelegationCapacityExceeded");
    }
  });

  it("Rejects an unstake that leaves the operator below the minimum", async () => {
    try {
      await program.methods
        .requestOperatorUnstake(new BN(9_500))
        .accounts({ pool, operator, authority: operatorAuthority.publicKey })
        .signers([operatorAuthority])
        .rpc();
      assert.fail("expected the unstake to be rejected");
    } catch (err) {
      assert.include(err.toString(), "StakeBelowMinimum");
    }
  });

  it("Rejects an exit while delegators remain", async () => {
    try {
      await program.methods
        .requestOperatorUnstake(new BN(10_000))
        .accounts({ pool, operator, authority: operatorAuthority.publicKey })
        .signers([operatorAuthority])
        .rpc();
      assert.fail("expected the exit to be rejected");
    } catch (err) {
      assert.include(err.toString(), "OperatorHasDelegations");
    }
  });

  it("Stops new stake while paused", async () => {
    await program.methods
      .setPaused(true)
      .accounts({ pool, admin: admin.publicKey })
      .rpc();
    try {
      await program.methods
        .addOperatorStake(new BN(1_000))
        .accounts({
          pool,
          operator,
          authority: operatorAuthority.publicKey,
          authorityToken: operatorToken,
          tokenVault,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([operatorAuthority])
        .rpc();
      assert.fail("expected the stake to be rejected");
    } catch (err) {
      assert.include(err.toString(), "PoolPaused");
    } finally {
      await program.methods
        .setPaused(false)
        .accounts({ pool, admin: admin.publicKey })
        .rpc();
    }
  });

  it("Pays rewards to the operator and the delegator", async () => {
    await sleep(3_000);

    const operatorBefore = await balance(operatorToken);
    await program.methods
      .claimOperatorRewards()
      .accounts({
        pool,
        operator,
        authority: operatorAuthority.publicKey,
        recipient: operatorToken,
        tokenVault,
        vaultAuthority,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([operatorAuthority])
      .rpc();
    const operatorEarned = (await balance(operatorToken)) - operatorBefore;

    const delegatorBefore = await balance(delegatorToken);
    await program.methods
      .claimDelegationRewards()
      .accounts({
        pool,
        operator,
        delegation,
        delegator: delegator.publicKey,
        recipient: delegatorToken,
        tokenVault,
        vaultAuthority,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([delegator])
      .rpc();
    const delegatorEarned = (await balance(delegatorToken)) - delegatorBefore;

    assert.ok(operatorEarned > 0n, "operator earned nothing");
    assert.ok(delegatorEarned > 0n, "delegator earned nothing");
    // 1/4 of the stake plus 10% of the delegated rewards vs 90% of 3/4
    assert.ok(delegatorEarned > operatorEarned);
  });

  it("Does not slash while the feed is fresh", async () => {
    await submitRound(Math.floor(Date.now() / 1000), 1_000);
    try {
      await program.methods
        .raiseAlert()
        .accounts({
          pool,
          operator,
          feed: feed.publicKey,
          storeProgram: workspace.Store.programId,
          alerter: alerter.publicKey,
          alerterToken,
          tokenVault,
          vaultAuthority,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([alerter])
        .rpc();
      assert.fail("expected the alert to be rejected");
    } catch (err) {
      assert.include(err.toString(), "FeedNotStale");
    }
  });

  it("Slashes the operator once the feed is stale", async () => {
    // a round from 10 minutes ago
    await submitRound(Math.floor(Date.now() / 1000) - 600, 1_001);

    const before = await program.account.operator.fetch(operator);
    await program.methods
      .raiseAlert()
      .accounts({
        pool,
        operator,
        feed: feed.publicKey,
        storeProgram: workspace.Store.programId,
        alerter: alerter.publicKey,
        alerterToken,
        tokenVault,
        vaultAuthority,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([alerter])
      .rpc();
    const after = await program.account.operator.fetch(operator);

    assert.equal(
      after.selfStake.toNumber(),
      before.selfStake.toNumber() * 0.9
    );
    assert.equal(
      after.delegatedAmount.toNumber(),
      before.delegatedAmount.toNumber() * 0.9
    );
    // 20% of the 4_000 slashed
    assert.equal(Number(await balance(alerterToken)), 800);
    const poolState = await program.account.stakingPool.fetch(pool);
    assert.equal(poolState.slashedReserve.toNumber(), 3_200);
  });

  it("Does not slash twice for the same round", async () => {
    try {
      await program.methods
        .raiseAlert()
        .accounts({
          pool,
          operator,
          feed: feed.publicKey,
          storeProgram: workspace.Store.programId,
          alerter: alerter.publicKey,
          alerterToken,
          tokenVault,
          vaultAuthority,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([alerter])
        .rpc();
      assert.fail("expected the alert to be rejected");
    } catch (err) {
      assert.include(err.toString(), "AlreadySlashed");
    }
  });

  it("Undelegates after the unbonding period", async () => {
    const state = await program.account.delegation.fetch(delegation);
    await program.methods
      .requestUndelegate(state.shares)
      .accounts({
        pool,
        operator,
        delegation,
        delegator: delegator.publicKey,
      })
      .signers([delegator])
      .rpc();

    const withdraw = () =>
      program.methods
        .withdrawDelegationUnbonded()
        .accounts({
          pool,
          operator,
          delegation,
          delegator: delegator.publicKey,
          recipient: delegatorToken,
          tokenVault,
          vaultAuthority,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([delegator])
        .rpc();

    try {
      await withdraw();
      assert.fail("expected the withdrawal to wait for the unbonding period");
    } catch (err) {
      assert.include(err.toString(), "NothingUnlocked");
    }

    await sleep(4_000);
    const before = await balance(delegatorToken);
    await withdraw();
    // 30_000 delegated, 10% slashed
    assert.equal(Number((await balance(delegatorToken)) - before), 27_000);

    const op = await program.account.operator.fetch(operator);
    assert.equal(op.delegatedAmount.toNumber(), 0);
    assert.equal(op.delegatedShares.toNumber(), 0);
  });

  it("Lets the operator exit once delegators left", async () => {
    const op = await program.account.operator.fetch(operator);
    await program.methods
      .requestOperatorUnstake(op.selfStake)
      .accounts({ pool, operator, authority: operatorAuthority.publicKey })
      .signers([operatorAuthority])
      .rpc();

    const after = await program.account.operator.fetch(operator);
    assert.ok(!after.active);
    assert.equal(after.selfStake.toNumber(), 0);
    assert.equal(after.unbonding.length, 1);
    const poolState = await program.account.stakingPool.fetch(pool);
    assert.equal(poolState.operatorCount, 0);
    assert.equal(poolState.totalStaked.toNumber(), 0);
  });

  it("Withdraws slashed tokens to the treasury", async () => {
    const before = await balance(adminToken);
    await program.methods
      .withdrawSlashed(new BN(3_200))
      .accounts({
        pool,
        admin: admin.publicKey,
        recipient: adminToken,
        tokenVault,
        vaultAuthority,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();
    assert.equal(Number((await balance(adminToken)) - before), 3_200);
    const poolState = await program.account.stakingPool.fetch(pool);
    assert.equal(poolState.slashedReserve.toNumber(), 0);
  });
});
