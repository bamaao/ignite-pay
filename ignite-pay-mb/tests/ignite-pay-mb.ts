// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

import * as anchor from "@anchor-lang/core";
import { Program } from "@anchor-lang/core";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  SYSVAR_INSTRUCTIONS_PUBKEY,
  Transaction,
  TransactionInstruction,
  Ed25519Program,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import { expect } from "chai";
import * as nacl from "tweetnacl";
import { createHash } from "crypto";

// ── Constants ───────────────────────────────────────────────

const GLOBAL_STATE_SEED = Buffer.from("global_state");
const GLOBAL_VAULT_SEED = Buffer.from("global_buyer_vault");
const CHANNEL_SEED = Buffer.from("channel");
const SETTLEMENT_SEED = Buffer.from("settlement");

// ── Helpers ─────────────────────────────────────────────────

function expectError(tx: Promise<any>, code: number) {
  return tx.then(
    () => { throw new Error("Expected transaction to fail"); },
    (err: any) => {
      const anchorErr = err?.error?.errorCode?.number
        ?? err?.error?.errorCode?.code
        ?? extractAnchorError(err);
      const expected = 6000 + code;
      if (typeof anchorErr === "number") {
        expect(anchorErr).to.eq(expected);
      } else {
        const msg = err?.message ?? String(err);
        expect(msg).to.include("custom program error");
      }
    }
  );
}

function extractAnchorError(err: any): number | null {
  const msg = err?.message ?? "";
  const m = msg.match(/custom program error: 0x([0-9a-f]+)/i);
  if (m) return parseInt(m[1], 16);
  return null;
}

function deriveGlobalStatePda(
  programId: PublicKey,
  buyer: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [GLOBAL_STATE_SEED, buyer.toBuffer()],
    programId,
  );
}

function deriveGlobalVaultPda(
  programId: PublicKey,
  buyer: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [GLOBAL_VAULT_SEED, buyer.toBuffer()],
    programId,
  );
}

function deriveChannelPda(
  programId: PublicKey,
  buyer: PublicKey,
  merchant: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [CHANNEL_SEED, buyer.toBuffer(), merchant.toBuffer()],
    programId,
  );
}

function deriveSettlementPda(
  programId: PublicKey,
  channel: PublicKey,
  nonce: number,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SETTLEMENT_SEED, channel.toBuffer(), new anchor.BN(nonce).toArrayLike(Buffer, "le", 8)],
    programId,
  );
}

async function airdrop(
  provider: anchor.Provider,
  pubkey: PublicKey,
  sol: number,
) {
  const sig = await provider.connection.requestAirdrop(pubkey, sol * LAMPORTS_PER_SOL);
  await provider.connection.confirmTransaction(sig, "confirmed");
}

async function sendAndConfirmRaw(
  provider: anchor.Provider,
  tx: Transaction,
): Promise<string> {
  const raw = tx.serialize();
  const sig = await provider.connection.sendRawTransaction(raw, { skipPreflight: false });
  await provider.connection.confirmTransaction(sig, "confirmed");
  return sig;
}

function buildSettleBatchEd25519Ix(
  buyer: Keypair,
  merchant: Keypair,
  merkleRoot: Buffer,
  totalAmount: anchor.BN,
  channelKey: PublicKey,
  nonce: anchor.BN,
): { buyerSig: Buffer; merchantSig: Buffer; instructions: TransactionInstruction[] } {
  const msgPreimage = Buffer.concat([
    merkleRoot,
    totalAmount.toArrayLike(Buffer, "le", 8),
    channelKey.toBuffer(),
    nonce.toArrayLike(Buffer, "le", 8),
  ]);
  const msgHash = createHash("sha256").update(msgPreimage).digest();
  const buyerSig = Buffer.from(nacl.sign.detached(msgHash, buyer.secretKey));
  const merchantSig = Buffer.from(nacl.sign.detached(msgHash, merchant.secretKey));

  const ix1 = Ed25519Program.createInstructionWithPublicKey({
    publicKey: buyer.publicKey.toBuffer(),
    message: msgHash,
    signature: buyerSig,
  });
  const ix2 = Ed25519Program.createInstructionWithPublicKey({
    publicKey: merchant.publicKey.toBuffer(),
    message: msgHash,
    signature: merchantSig,
  });

  return { buyerSig, merchantSig, instructions: [ix1, ix2] };
}

function buildOptimisticSettleEd25519Ix(
  merchant: Keypair,
  merkleRoot: Buffer,
  totalAmount: anchor.BN,
  channelKey: PublicKey,
  nonce: anchor.BN,
): { merchantSig: Buffer; instructions: TransactionInstruction[] } {
  const msgPreimage = Buffer.concat([
    merkleRoot,
    totalAmount.toArrayLike(Buffer, "le", 8),
    channelKey.toBuffer(),
    nonce.toArrayLike(Buffer, "le", 8),
  ]);
  const msgHash = createHash("sha256").update(msgPreimage).digest();
  const merchantSig = Buffer.from(nacl.sign.detached(msgHash, merchant.secretKey));

  const ix = Ed25519Program.createInstructionWithPublicKey({
    publicKey: merchant.publicKey.toBuffer(),
    message: msgHash,
    signature: merchantSig,
  });

  return { merchantSig, instructions: [ix] };
}

function buildSingleLeafTree(
  channelKey: PublicKey,
  seq: number,
  amount: number,
  buyerPubkey: PublicKey,
  buyerVoucherSig: Buffer,
): { merkleRoot: Buffer; leafHash: Buffer; siblingHashes: Buffer[]; siblingSums: anchor.BN[] } {
  const leafPreimage = Buffer.concat([
    Buffer.from([0x00]),
    channelKey.toBuffer(),
    new anchor.BN(seq).toArrayLike(Buffer, "le", 8),
    new anchor.BN(amount).toArrayLike(Buffer, "le", 8),
    buyerPubkey.toBuffer(),
    buyerVoucherSig,
  ]);
  const leafHash = createHash("sha256").update(leafPreimage).digest();
  return { merkleRoot: leafHash, leafHash, siblingHashes: [], siblingSums: [] };
}

function buildTwoLeafTree(
  channelKey: PublicKey,
  leaf1Seq: number, leaf1Amount: number,
  leaf2Seq: number, leaf2Amount: number,
  buyerPubkey: PublicKey,
  buyerSig1: Buffer, buyerSig2: Buffer,
) {
  const leaf1Preimage = Buffer.concat([
    Buffer.from([0x00]),
    channelKey.toBuffer(),
    new anchor.BN(leaf1Seq).toArrayLike(Buffer, "le", 8),
    new anchor.BN(leaf1Amount).toArrayLike(Buffer, "le", 8),
    buyerPubkey.toBuffer(),
    buyerSig1,
  ]);
  const leaf1Hash = createHash("sha256").update(leaf1Preimage).digest();

  const leaf2Preimage = Buffer.concat([
    Buffer.from([0x00]),
    channelKey.toBuffer(),
    new anchor.BN(leaf2Seq).toArrayLike(Buffer, "le", 8),
    new anchor.BN(leaf2Amount).toArrayLike(Buffer, "le", 8),
    buyerPubkey.toBuffer(),
    buyerSig2,
  ]);
  const leaf2Hash = createHash("sha256").update(leaf2Preimage).digest();

  const loHash = Buffer.compare(leaf1Hash, leaf2Hash) <= 0 ? leaf1Hash : leaf2Hash;
  const loSum = Buffer.compare(leaf1Hash, leaf2Hash) <= 0 ? leaf1Amount : leaf2Amount;
  const hiHash = Buffer.compare(leaf1Hash, leaf2Hash) <= 0 ? leaf2Hash : leaf1Hash;
  const hiSum = Buffer.compare(leaf1Hash, leaf2Hash) <= 0 ? leaf2Amount : leaf1Amount;

  const internalPreimage = Buffer.concat([
    Buffer.from([0x01]),
    loHash,
    new anchor.BN(loSum).toArrayLike(Buffer, "le", 8),
    hiHash,
    new anchor.BN(hiSum).toArrayLike(Buffer, "le", 8),
  ]);
  const merkleRoot = createHash("sha256").update(internalPreimage).digest();

  return {
    merkleRoot,
    leaf1Hash,
    leaf2Hash,
    leaf1SiblingHashes: [leaf2Hash],
    leaf1SiblingSums: [new anchor.BN(leaf2Amount)],
    leaf2SiblingHashes: [leaf1Hash],
    leaf2SiblingSums: [new anchor.BN(leaf1Amount)],
  };
}

// ── Tests ────────────────────────────────────────────────────

describe("ignite-pay-mb", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  let program: any;
  let buyer: Keypair;
  let merchant: Keypair;

  before(async () => {
    const workspace = anchor.workspace as any;
    program = workspace.IgnitePayMb;
    if (!program) {
      throw new Error("Program not found in workspace. Run `anchor build` first.");
    }

    buyer = Keypair.generate();
    merchant = Keypair.generate();

    await airdrop(provider, buyer.publicKey, 50);
    await airdrop(provider, merchant.publicKey, 10);
  });

  // ── initialize_global ─────────────────────────────────────

  describe("initialize_global", () => {
    it("creates global state and vault", async () => {
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [vault] = deriveGlobalVaultPda(program.programId, buyer.publicKey);

      await program.methods
        .initializeGlobal()
        .accounts({
          globalState,
          vault,
          buyer: buyer.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([buyer])
        .rpc();

      const gs = await program.account.globalState.fetch(globalState);
      expect(gs.buyer.toBase58()).to.eq(buyer.publicKey.toBase58());
      expect(gs.totalDeposited.toNumber()).to.eq(0);
      expect(gs.totalAllocated.toNumber()).to.eq(0);
    });
  });

  // ── deposit ────────────────────────────────────────────────

  describe("deposit", () => {
    it("deposits SOL into the global vault", async () => {
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [vault] = deriveGlobalVaultPda(program.programId, buyer.publicKey);

      const depositAmount = 10 * LAMPORTS_PER_SOL;

      await program.methods
        .deposit(new anchor.BN(depositAmount))
        .accounts({
          globalState,
          vault,
          buyer: buyer.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([buyer])
        .rpc();

      const gs = await program.account.globalState.fetch(globalState);
      expect(gs.totalDeposited.toNumber()).to.eq(depositAmount);

      const vaultBalance = await provider.connection.getBalance(vault);
      expect(vaultBalance).to.eq(depositAmount);
    });
  });

  // ── initialize_channel ────────────────────────────────────

  describe("initialize_channel", () => {
    it("creates a channel with correct initial state", async () => {
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [channelPda] = deriveChannelPda(program.programId, buyer.publicKey, merchant.publicKey);

      await program.methods
        .initializeChannel(
          new anchor.BN(5 * LAMPORTS_PER_SOL), // spending_cap
          new anchor.BN(86400),                   // challenge_period
          new anchor.BN(259200),                  // dispute_period
        )
        .accounts({
          globalState,
          channel: channelPda,
          buyer: buyer.publicKey,
          merchant: merchant.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([buyer])
        .rpc();

      const channel = await program.account.channel.fetch(channelPda);
      expect(channel.buyer.toBase58()).to.eq(buyer.publicKey.toBase58());
      expect(channel.merchant.toBase58()).to.eq(merchant.publicKey.toBase58());
      expect(channel.spendingCap.toNumber()).to.eq(5 * LAMPORTS_PER_SOL);
      expect(channel.settledAmount.toNumber()).to.eq(0);
      expect(channel.nonce.toNumber()).to.eq(0);

      const gs = await program.account.globalState.fetch(globalState);
      expect(gs.totalAllocated.toNumber()).to.eq(5 * LAMPORTS_PER_SOL);
    });

    it("fails if spending cap exceeds deposit", async () => {
      const merchant2 = Keypair.generate();
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [channelPda] = deriveChannelPda(program.programId, buyer.publicKey, merchant2.publicKey);

      // AllocationExceedsDeposit = error index 18
      await expectError(
        program.methods
          .initializeChannel(
            new anchor.BN(100 * LAMPORTS_PER_SOL), // way more than deposited
            new anchor.BN(86400),
            new anchor.BN(259200),
          )
          .accounts({
            globalState,
            channel: channelPda,
            buyer: buyer.publicKey,
            merchant: merchant2.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([buyer])
          .rpc(),
        18, // AllocationExceedsDeposit
      );
    });
  });

  // ── update_spending_cap ────────────────────────────────────

  describe("update_spending_cap", () => {
    it("increases spending cap", async () => {
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [channelPda] = deriveChannelPda(program.programId, buyer.publicKey, merchant.publicKey);

      await program.methods
        .updateSpendingCap(new anchor.BN(7 * LAMPORTS_PER_SOL))
        .accounts({
          globalState,
          channel: channelPda,
          buyer: buyer.publicKey,
        })
        .signers([buyer])
        .rpc();

      const channel = await program.account.channel.fetch(channelPda);
      expect(channel.spendingCap.toNumber()).to.eq(7 * LAMPORTS_PER_SOL);

      const gs = await program.account.globalState.fetch(globalState);
      expect(gs.totalAllocated.toNumber()).to.eq(7 * LAMPORTS_PER_SOL);
    });
  });

  // ── settle_batch (dual-sig) ────────────────────────────────

  describe("settle_batch", () => {
    it("settles a batch with valid dual signatures", async () => {
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [vault] = deriveGlobalVaultPda(program.programId, buyer.publicKey);
      const [channelPda] = deriveChannelPda(program.programId, buyer.publicKey, merchant.publicKey);
      const [settlementPda] = deriveSettlementPda(program.programId, channelPda, 0);

      const totalAmount = new anchor.BN(1 * LAMPORTS_PER_SOL);
      const merkleRoot = Buffer.alloc(32, 0xab);
      const nonce = new anchor.BN(0);

      const { buyerSig, merchantSig, instructions } = buildSettleBatchEd25519Ix(
        buyer, merchant, merkleRoot, totalAmount, channelPda, nonce,
      );

      // Build transaction manually to ensure Ed25519 instructions come before settle_batch
      const settleIx = await program.methods
        .settleBatch(
          Array.from(merkleRoot),
          totalAmount,
          Array.from(buyerSig),
          Array.from(merchantSig),
        )
        .accounts({
          globalState,
          vault,
          channel: channelPda,
          settlementEscrow: settlementPda,
          merchant: merchant.publicKey,
          instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          systemProgram: SystemProgram.programId,
        })
        .instruction();

      const tx = new Transaction();
      tx.add(...instructions, settleIx);
      tx.recentBlockhash = (await provider.connection.getLatestBlockhash()).blockhash;
      tx.feePayer = merchant.publicKey;
      tx.partialSign(merchant);

      await sendAndConfirmRaw(provider, tx);

      const channel = await program.account.channel.fetch(channelPda);
      expect(channel.settledAmount.toNumber()).to.eq(totalAmount.toNumber());
      expect(channel.nonce.toNumber()).to.eq(1);

      const escrow = await program.account.settlementEscrow.fetch(settlementPda);
      expect(escrow.amount.toNumber()).to.eq(totalAmount.toNumber());
      expect(escrow.optimistic).to.be.false;
    });

    it("fails when spending cap exceeded", async () => {
      const [globalState] = deriveGlobalStatePda(program.programId, buyer.publicKey);
      const [vault] = deriveGlobalVaultPda(program.programId, buyer.publicKey);
      const [channelPda] = deriveChannelPda(program.programId, buyer.publicKey, merchant.publicKey);
      const [settlementPda] = deriveSettlementPda(program.programId, channelPda, 1);

      const totalAmount = new anchor.BN(10 * LAMPORTS_PER_SOL);
      const merkleRoot = Buffer.alloc(32, 0xcd);
      const nonce = new anchor.BN(1);

      const { buyerSig, merchantSig, instructions } = buildSettleBatchEd25519Ix(
        buyer, merchant, merkleRoot, totalAmount, channelPda, nonce,
      );

      // SpendingCapExceeded = error index 0
      const settleIx = await program.methods
        .settleBatch(
          Array.from(merkleRoot), totalAmount,
          Array.from(buyerSig), Array.from(merchantSig),
        )
        .accounts({
          globalState, vault,
          channel: channelPda,
          settlementEscrow: settlementPda,
          merchant: merchant.publicKey,
          instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          systemProgram: SystemProgram.programId,
        })
        .instruction();

      const capTx = new Transaction();
      capTx.add(...instructions, settleIx);
      capTx.recentBlockhash = (await provider.connection.getLatestBlockhash()).blockhash;
      capTx.feePayer = merchant.publicKey;
      capTx.partialSign(merchant);

      await expectError(
        sendAndConfirmRaw(provider, capTx),
        0,
      );
    });
  });

  // ── optimistic_settle ─────────────────────────────────────

  describe("optimistic_settle", () => {
    const optBuyer = Keypair.generate();
    const optMerchant = Keypair.generate();

    before(async () => {
      await airdrop(provider, optBuyer.publicKey, 20);
      await airdrop(provider, optMerchant.publicKey, 5);

      const [gs] = deriveGlobalStatePda(program.programId, optBuyer.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, optBuyer.publicKey);

      await program.methods.initializeGlobal()
        .accounts({ globalState: gs, vault: v, buyer: optBuyer.publicKey, systemProgram: SystemProgram.programId })
        .signers([optBuyer]).rpc();

      await program.methods.deposit(new anchor.BN(10 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: optBuyer.publicKey, systemProgram: SystemProgram.programId })
        .signers([optBuyer]).rpc();

      const [ch] = deriveChannelPda(program.programId, optBuyer.publicKey, optMerchant.publicKey);
      await program.methods.initializeChannel(
        new anchor.BN(10 * LAMPORTS_PER_SOL),
        new anchor.BN(86400),
        new anchor.BN(259200),
      )
        .accounts({ globalState: gs, channel: ch, buyer: optBuyer.publicKey, merchant: optMerchant.publicKey, systemProgram: SystemProgram.programId })
        .signers([optBuyer]).rpc();
    });

    it("settles with merchant-only signature", async () => {
      const [gs] = deriveGlobalStatePda(program.programId, optBuyer.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, optBuyer.publicKey);
      const [ch] = deriveChannelPda(program.programId, optBuyer.publicKey, optMerchant.publicKey);
      const [esc] = deriveSettlementPda(program.programId, ch, 0);

      const totalAmount = new anchor.BN(2 * LAMPORTS_PER_SOL);
      const merkleRoot = Buffer.alloc(32, 0xdd);
      const nonce = new anchor.BN(0);

      const { merchantSig, instructions } = buildOptimisticSettleEd25519Ix(
        optMerchant, merkleRoot, totalAmount, ch, nonce,
      );

      const optIx = await program.methods
        .optimisticSettle(Array.from(merkleRoot), totalAmount, Array.from(merchantSig))
        .accounts({
          globalState: gs,
          vault: v,
          channel: ch,
          settlementEscrow: esc,
          merchant: optMerchant.publicKey,
          instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          systemProgram: SystemProgram.programId,
        })
        .instruction();

      const optTx = new Transaction();
      optTx.add(...instructions, optIx);
      optTx.recentBlockhash = (await provider.connection.getLatestBlockhash()).blockhash;
      optTx.feePayer = optMerchant.publicKey;
      optTx.partialSign(optMerchant);

      await sendAndConfirmRaw(provider, optTx);

      const escrow = await program.account.settlementEscrow.fetch(esc);
      expect(escrow.amount.toNumber()).to.eq(totalAmount.toNumber());
      expect(escrow.optimistic).to.be.true;
      expect(escrow.disputed).to.be.false;
    });

    it("fails: challenge_period must be > 0 for optimistic settle", async () => {
      const cpBuyer = Keypair.generate();
      const cpMerchant = Keypair.generate();
      await airdrop(provider, cpBuyer.publicKey, 10);
      await airdrop(provider, cpMerchant.publicKey, 5);

      const [gs] = deriveGlobalStatePda(program.programId, cpBuyer.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, cpBuyer.publicKey);
      const [ch] = deriveChannelPda(program.programId, cpBuyer.publicKey, cpMerchant.publicKey);

      await program.methods.initializeGlobal()
        .accounts({ globalState: gs, vault: v, buyer: cpBuyer.publicKey, systemProgram: SystemProgram.programId })
        .signers([cpBuyer]).rpc();

      await program.methods.deposit(new anchor.BN(5 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: cpBuyer.publicKey, systemProgram: SystemProgram.programId })
        .signers([cpBuyer]).rpc();

      // Channel with challenge_period = 0
      await program.methods.initializeChannel(
        new anchor.BN(5 * LAMPORTS_PER_SOL), new anchor.BN(0), new anchor.BN(259200),
      )
        .accounts({ globalState: gs, channel: ch, buyer: cpBuyer.publicKey, merchant: cpMerchant.publicKey, systemProgram: SystemProgram.programId })
        .signers([cpBuyer]).rpc();

      const [esc] = deriveSettlementPda(program.programId, ch, 0);
      const totalAmount = new anchor.BN(1 * LAMPORTS_PER_SOL);
      const merkleRoot = Buffer.alloc(32, 0xee);
      const nonce = new anchor.BN(0);

      const { merchantSig, instructions } = buildOptimisticSettleEd25519Ix(
        cpMerchant, merkleRoot, totalAmount, ch, nonce,
      );

      // ChallengePeriodRequired = error index 19
      const cpIx = await program.methods
        .optimisticSettle(Array.from(merkleRoot), totalAmount, Array.from(merchantSig))
        .accounts({
          globalState: gs, vault: v, channel: ch, settlementEscrow: esc,
          merchant: cpMerchant.publicKey,
          instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          systemProgram: SystemProgram.programId,
        })
        .instruction();

      const cpTx = new Transaction();
      cpTx.add(...instructions, cpIx);
      cpTx.recentBlockhash = (await provider.connection.getLatestBlockhash()).blockhash;
      cpTx.feePayer = cpMerchant.publicKey;
      cpTx.partialSign(cpMerchant);

      await expectError(
        sendAndConfirmRaw(provider, cpTx),
        19,
      );
    });
  });

  // ── dispute + resolve_dispute ──────────────────────────────

  describe("resolve_dispute", () => {
    it("buyer wins dispute with valid fraud proof", async () => {
      const b2 = Keypair.generate();
      const m2 = Keypair.generate();
      await airdrop(provider, b2.publicKey, 20);
      await airdrop(provider, m2.publicKey, 5);

      const [gs] = deriveGlobalStatePda(program.programId, b2.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, b2.publicKey);
      const [ch] = deriveChannelPda(program.programId, b2.publicKey, m2.publicKey);

      await program.methods.initializeGlobal()
        .accounts({ globalState: gs, vault: v, buyer: b2.publicKey, systemProgram: SystemProgram.programId })
        .signers([b2]).rpc();

      await program.methods.deposit(new anchor.BN(10 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: b2.publicKey, systemProgram: SystemProgram.programId })
        .signers([b2]).rpc();

      await program.methods.initializeChannel(
        new anchor.BN(10 * LAMPORTS_PER_SOL), new anchor.BN(86400), new anchor.BN(259200),
      )
        .accounts({ globalState: gs, channel: ch, buyer: b2.publicKey, merchant: m2.publicKey, systemProgram: SystemProgram.programId })
        .signers([b2]).rpc();

      // 2-leaf tree
      const sig1 = Buffer.alloc(64, 0xaa);
      const sig2 = Buffer.alloc(64, 0xbb);
      const tree = buildTwoLeafTree(ch, 0, 1 * LAMPORTS_PER_SOL, 1, 1 * LAMPORTS_PER_SOL, b2.publicKey, sig1, sig2);

      const totalAmount = new anchor.BN(2 * LAMPORTS_PER_SOL);
      const nonce = new anchor.BN(0);
      const [esc] = deriveSettlementPda(program.programId, ch, 0);

      const { buyerSig, merchantSig, instructions } = buildSettleBatchEd25519Ix(b2, m2, tree.merkleRoot, totalAmount, ch, nonce);
      const rdIx = await program.methods.settleBatch(Array.from(tree.merkleRoot), totalAmount, Array.from(buyerSig), Array.from(merchantSig))
        .accounts({ globalState: gs, vault: v, channel: ch, settlementEscrow: esc, merchant: m2.publicKey, instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY, systemProgram: SystemProgram.programId })
        .instruction();

      const rdTx = new Transaction();
      rdTx.add(...instructions, rdIx);
      rdTx.recentBlockhash = (await provider.connection.getLatestBlockhash()).blockhash;
      rdTx.feePayer = m2.publicKey;
      rdTx.partialSign(m2);

      await sendAndConfirmRaw(provider, rdTx);

      // Dispute
      await program.methods.dispute()
        .accounts({ channel: ch, settlementEscrow: esc, buyer: b2.publicKey, systemProgram: SystemProgram.programId })
        .signers([b2]).rpc();

      // Resolve: leaf 1 (1 SOL) < total (2 SOL)
      const buyerBalBefore = await provider.connection.getBalance(b2.publicKey);
      await program.methods.resolveDispute(
        new anchor.BN(0), new anchor.BN(1 * LAMPORTS_PER_SOL),
        Array.from(sig1),
        tree.leaf1SiblingHashes.map((h: Buffer) => Array.from(h)),
        tree.leaf1SiblingSums,
      )
        .accounts({ channel: ch, settlementEscrow: esc, buyer: b2.publicKey, systemProgram: SystemProgram.programId })
        .signers([b2]).rpc();

      const channel = await program.account.channel.fetch(ch);
      expect(channel.settledAmount.toNumber()).to.eq(0);

      const buyerBalAfter = await provider.connection.getBalance(b2.publicKey);
      expect(buyerBalAfter - buyerBalBefore).to.be.greaterThan(1.9 * LAMPORTS_PER_SOL);
    });
  });

  // ── release_settlement ─────────────────────────────────────

  describe("release_settlement", () => {
    it("releases settlement to merchant after challenge period", async () => {
      const b3 = Keypair.generate();
      const m3 = Keypair.generate();
      await airdrop(provider, b3.publicKey, 20);
      await airdrop(provider, m3.publicKey, 5);

      const [gs] = deriveGlobalStatePda(program.programId, b3.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, b3.publicKey);
      const [ch] = deriveChannelPda(program.programId, b3.publicKey, m3.publicKey);

      await program.methods.initializeGlobal()
        .accounts({ globalState: gs, vault: v, buyer: b3.publicKey, systemProgram: SystemProgram.programId })
        .signers([b3]).rpc();

      await program.methods.deposit(new anchor.BN(10 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: b3.publicKey, systemProgram: SystemProgram.programId })
        .signers([b3]).rpc();

      // challenge_period = 0 for immediate release
      await program.methods.initializeChannel(
        new anchor.BN(10 * LAMPORTS_PER_SOL), new anchor.BN(0), new anchor.BN(259200),
      )
        .accounts({ globalState: gs, channel: ch, buyer: b3.publicKey, merchant: m3.publicKey, systemProgram: SystemProgram.programId })
        .signers([b3]).rpc();

      const merkleRoot = Buffer.alloc(32, 0xdd);
      const totalAmount = new anchor.BN(1 * LAMPORTS_PER_SOL);
      const nonce = new anchor.BN(0);
      const [esc] = deriveSettlementPda(program.programId, ch, 0);

      const { buyerSig, merchantSig, instructions } = buildSettleBatchEd25519Ix(b3, m3, merkleRoot, totalAmount, ch, nonce);
      const relIx = await program.methods.settleBatch(Array.from(merkleRoot), totalAmount, Array.from(buyerSig), Array.from(merchantSig))
        .accounts({ globalState: gs, vault: v, channel: ch, settlementEscrow: esc, merchant: m3.publicKey, instructionSysvar: SYSVAR_INSTRUCTIONS_PUBKEY, systemProgram: SystemProgram.programId })
        .instruction();

      const relTx = new Transaction();
      relTx.add(...instructions, relIx);
      relTx.recentBlockhash = (await provider.connection.getLatestBlockhash()).blockhash;
      relTx.feePayer = m3.publicKey;
      relTx.partialSign(m3);

      await sendAndConfirmRaw(provider, relTx);

      const merchantBalBefore = await provider.connection.getBalance(m3.publicKey);

      await program.methods.releaseSettlement()
        .accounts({ channel: ch, settlementEscrow: esc, merchant: m3.publicKey, systemProgram: SystemProgram.programId })
        .signers([m3]).rpc();

      const merchantBalAfter = await provider.connection.getBalance(m3.publicKey);
      expect(merchantBalAfter - merchantBalBefore).to.be.greaterThan(0.9 * LAMPORTS_PER_SOL);

      const escrowBal = await provider.connection.getBalance(esc);
      expect(escrowBal).to.be.greaterThan(0); // rent-exempt amount remains
      expect(escrowBal).to.be.lessThan(0.01 * LAMPORTS_PER_SOL);
    });
  });

  // ── withdraw ───────────────────────────────────────────────

  describe("withdraw", () => {
    it("buyer withdraws unallocated funds", async () => {
      const b4 = Keypair.generate();
      const m4 = Keypair.generate();
      await airdrop(provider, b4.publicKey, 20);

      const [gs] = deriveGlobalStatePda(program.programId, b4.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, b4.publicKey);
      const [ch] = deriveChannelPda(program.programId, b4.publicKey, m4.publicKey);

      await program.methods.initializeGlobal()
        .accounts({ globalState: gs, vault: v, buyer: b4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      await program.methods.deposit(new anchor.BN(5 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: b4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      await program.methods.initializeChannel(
        new anchor.BN(2 * LAMPORTS_PER_SOL), new anchor.BN(86400), new anchor.BN(259200),
      )
        .accounts({ globalState: gs, channel: ch, buyer: b4.publicKey, merchant: m4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      // Withdraw 2 SOL (unallocated = 5 - 2 = 3 SOL available)
      const buyerBalBefore = await provider.connection.getBalance(b4.publicKey);

      await program.methods.withdraw(new anchor.BN(2 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: b4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      const gsAfter = await program.account.globalState.fetch(gs);
      expect(gsAfter.totalDeposited.toNumber()).to.eq(3 * LAMPORTS_PER_SOL);

      const buyerBalAfter = await provider.connection.getBalance(b4.publicKey);
      expect(buyerBalAfter - buyerBalBefore).to.be.greaterThan(1.9 * LAMPORTS_PER_SOL);
    });

    it("fails: cannot withdraw allocated funds", async () => {
      const b4 = Keypair.generate();
      await airdrop(provider, b4.publicKey, 20);

      const [gs] = deriveGlobalStatePda(program.programId, b4.publicKey);
      const [v] = deriveGlobalVaultPda(program.programId, b4.publicKey);
      const m4 = Keypair.generate();
      const [ch] = deriveChannelPda(program.programId, b4.publicKey, m4.publicKey);

      await program.methods.initializeGlobal()
        .accounts({ globalState: gs, vault: v, buyer: b4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      await program.methods.deposit(new anchor.BN(5 * LAMPORTS_PER_SOL))
        .accounts({ globalState: gs, vault: v, buyer: b4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      await program.methods.initializeChannel(
        new anchor.BN(5 * LAMPORTS_PER_SOL), new anchor.BN(86400), new anchor.BN(259200),
      )
        .accounts({ globalState: gs, channel: ch, buyer: b4.publicKey, merchant: m4.publicKey, systemProgram: SystemProgram.programId })
        .signers([b4]).rpc();

      // All deposited funds are allocated — withdraw should fail
      // InsufficientBalance = error index 1
      await expectError(
        program.methods.withdraw(new anchor.BN(1 * LAMPORTS_PER_SOL))
          .accounts({ globalState: gs, vault: v, buyer: b4.publicKey, systemProgram: SystemProgram.programId })
          .signers([b4]).rpc(),
        1,
      );
    });
  });
});
