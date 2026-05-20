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

use crate::error::{Result, SolanaError};
use crate::session::{SessionKeypair, SessionManager};
use crate::session_program::{self as session_prog, build_execute_payment_ix, build_execute_spl_payment_ix, build_withdraw_remaining_ix, derive_session_pda};
use crate::types::{PayMode, PaymentResult, SplPaymentParams};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::transaction::Transaction;
use spl_associated_token_account::get_associated_token_address;

#[derive(serde::Deserialize)]
struct SponsorResponse {
    signature: String,
}

/// Main client for executing on-chain payments.
pub struct IgnitePayClient {
    pub rpc_client: RpcClient,
    pub session_manager: SessionManager,
    pub mode: PayMode,
    pub relayer_url: Option<String>,
}

impl std::fmt::Debug for IgnitePayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IgnitePayClient")
            .field("mode", &self.mode)
            .field("relayer_url", &self.relayer_url)
            .finish()
    }
}

/// Helper to extract slot from a confirmed transaction signature.
fn get_slot_for_signature(rpc_client: &RpcClient, sig: solana_sdk::signature::Signature) -> u64 {
    rpc_client
        .get_signature_statuses(&[sig])
        .ok()
        .and_then(|resp| {
            resp.value
                .first()
                .and_then(|opt_status| opt_status.as_ref().map(|s| s.slot))
        })
        .unwrap_or(0)
}

impl IgnitePayClient {
    /// Derive the Associated Token Account address for an owner and mint.
    pub fn derive_ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        get_associated_token_address(owner, mint)
    }

    /// Create a new IgnitePayClient.
    pub fn new(
        rpc_url: &str,
        db: sled::Db,
        mode: PayMode,
        relayer_url: Option<String>,
    ) -> Result<Self> {
        Ok(Self {
            rpc_client: RpcClient::new(rpc_url.to_string()),
            session_manager: SessionManager::new(db)?,
            mode,
            relayer_url,
        })
    }

    /// Execute a SOL transfer using a session key via the on-chain session program.
    pub async fn execute_sol_transfer(
        &self,
        session: &SessionKeypair,
        recipient: &Pubkey,
        amount_lamports: u64,
    ) -> Result<PaymentResult> {
        if self.session_manager.is_expired(&session.session_data) {
            return Err(SolanaError::SessionExpired);
        }
        if !self
            .session_manager
            .check_spending_limit(&session.session_data, amount_lamports)
        {
            return Err(SolanaError::SpendingLimitExceeded {
                current: session.session_data.current_spent,
                limit: session.session_data.spending_limit,
            });
        }

        let program_id = session_prog::session_program_id();
        let (session_pda, _) = derive_session_pda(
            &session.session_data.owner,
            &session.keypair.pubkey(),
            &program_id,
        );

        let ix = build_execute_payment_ix(
            &program_id,
            &session_pda,
            &session.keypair.pubkey(),
            recipient,
            amount_lamports,
            "sol:transfer",
        );

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&session.keypair.pubkey()),
            &[&session.keypair],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        self.session_manager
            .record_spent(&session.keypair.pubkey(), amount_lamports)?;

        let slot = get_slot_for_signature(&self.rpc_client, sig);

        Ok(PaymentResult {
            signature: sig.to_string(),
            slot,
            block_time: None,
        })
    }

    /// Execute an SPL Token transfer using a session key via the on-chain session program.
    pub async fn execute_spl_transfer(
        &self,
        session: &SessionKeypair,
        source_ata: &Pubkey,
        dest_ata: &Pubkey,
        amount: u64,
        mint: &Pubkey,
    ) -> Result<PaymentResult> {
        if self.session_manager.is_expired(&session.session_data) {
            return Err(SolanaError::SessionExpired);
        }
        if !self
            .session_manager
            .check_spending_limit(&session.session_data, amount)
        {
            return Err(SolanaError::SpendingLimitExceeded {
                current: session.session_data.current_spent,
                limit: session.session_data.spending_limit,
            });
        }

        let program_id = session_prog::session_program_id();
        let (session_pda, _) = derive_session_pda(
            &session.session_data.owner,
            &session.keypair.pubkey(),
            &program_id,
        );

        let ix = build_execute_spl_payment_ix(
            &program_id,
            &session_pda,
            &session.keypair.pubkey(),
            source_ata,
            dest_ata,
            mint,
            amount,
            "spl:transfer",
        );

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&session.keypair.pubkey()),
            &[&session.keypair],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        self.session_manager
            .record_spent(&session.keypair.pubkey(), amount)?;

        let slot = get_slot_for_signature(&self.rpc_client, sig);

        Ok(PaymentResult {
            signature: sig.to_string(),
            slot,
            block_time: None,
        })
    }

    /// Unified payment entry point — dispatches based on PayMode.
    pub async fn execute_payment(
        &self,
        recipient: &str,
        amount: u64,
        token: &str,
        _network: &str,
        session: &SessionKeypair,
        spl_params: Option<&SplPaymentParams>,
    ) -> Result<PaymentResult> {
        let recipient_pubkey = recipient
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::InvalidPubkey(e.to_string()))?;

        match self.mode {
            PayMode::Sponsored => {
                let relayer_url = self.relayer_url.as_ref().ok_or_else(|| {
                    SolanaError::RelayerError("relayer_url not configured for sponsored mode".into())
                })?;

                // Fetch relayer pubkey
                let relayer_pubkey = Self::fetch_relayer_pubkey(relayer_url).await?;

                match token {
                    "SOL" | "sol" => {
                        self.execute_sol_transfer_sponsored(session, &recipient_pubkey, amount, &relayer_pubkey, relayer_url)
                            .await
                    }
                    _ => {
                        let params = spl_params.ok_or_else(|| {
                            SolanaError::Other(anyhow::anyhow!(
                                "SPL token transfers require spl_params with mint address"
                            ))
                        })?;
                        let program_id = session_prog::session_program_id();
                        let (session_pda, _) = derive_session_pda(
                            &session.session_data.owner,
                            &session.keypair.pubkey(),
                            &program_id,
                        );
                        let source_ata = params.source_ata_override
                            .unwrap_or_else(|| Self::derive_ata(&session_pda, &params.mint));
                        let dest_ata = params.dest_ata_override
                            .unwrap_or_else(|| Self::derive_ata(&recipient_pubkey, &params.mint));

                        self.execute_spl_transfer_sponsored(session, &source_ata, &dest_ata, amount, &params.mint, &relayer_pubkey, relayer_url)
                            .await
                    }
                }
            }
            PayMode::SelfFunded => {
                match token {
                    "SOL" | "sol" => {
                        self.execute_sol_transfer(session, &recipient_pubkey, amount)
                            .await
                    }
                    _ => {
                        let params = spl_params.ok_or_else(|| {
                            SolanaError::Other(anyhow::anyhow!(
                                "SPL token transfers require spl_params with mint address"
                            ))
                        })?;
                        let program_id = session_prog::session_program_id();
                        let (session_pda, _) = derive_session_pda(
                            &session.session_data.owner,
                            &session.keypair.pubkey(),
                            &program_id,
                        );
                        let source_ata = params.source_ata_override
                            .unwrap_or_else(|| Self::derive_ata(&session_pda, &params.mint));
                        let dest_ata = params.dest_ata_override
                            .unwrap_or_else(|| Self::derive_ata(&recipient_pubkey, &params.mint));

                        self.execute_spl_transfer(session, &source_ata, &dest_ata, amount, &params.mint)
                            .await
                    }
                }
            }
        }
    }

    /// Execute a payment using the relayer-sponsored path, regardless of global mode.
    /// Used when the phone user explicitly selects the "relayer" payment method.
    pub async fn execute_payment_sponsored(
        &self,
        recipient: &str,
        amount: u64,
        token: &str,
        _network: &str,
        session: &SessionKeypair,
        spl_params: Option<&SplPaymentParams>,
    ) -> Result<PaymentResult> {
        let recipient_pubkey = recipient
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::InvalidPubkey(e.to_string()))?;

        let relayer_url = self.relayer_url.as_ref().ok_or_else(|| {
            SolanaError::RelayerError("relayer_url not configured for sponsored payment".into())
        })?;

        let relayer_pubkey = Self::fetch_relayer_pubkey(relayer_url).await?;

        match token {
            "SOL" | "sol" => {
                self.execute_sol_transfer_sponsored(session, &recipient_pubkey, amount, &relayer_pubkey, relayer_url)
                    .await
            }
            _ => {
                let params = spl_params.ok_or_else(|| {
                    SolanaError::Other(anyhow::anyhow!(
                        "SPL token transfers require spl_params with mint address"
                    ))
                })?;
                let prog_id = session_prog::session_program_id();
                let (session_pda, _) = derive_session_pda(
                    &session.session_data.owner,
                    &session.keypair.pubkey(),
                    &prog_id,
                );
                let source_ata = params.source_ata_override
                    .unwrap_or_else(|| Self::derive_ata(&session_pda, &params.mint));
                let dest_ata = params.dest_ata_override
                    .unwrap_or_else(|| Self::derive_ata(&recipient_pubkey, &params.mint));

                self.execute_spl_transfer_sponsored(session, &source_ata, &dest_ata, amount, &params.mint, &relayer_pubkey, relayer_url)
                    .await
            }
        }
    }

    /// Fetch the relayer's fee-payer public key from GET /info.
    async fn fetch_relayer_pubkey(relayer_url: &str) -> Result<Pubkey> {
        let info_url = format!("{}/info", relayer_url.trim_end_matches("/sponsor"));
        let resp = reqwest::get(&info_url).await?;
        let body: serde_json::Value = resp.json().await?;
        let pk_str = body["pubkey"]
            .as_str()
            .ok_or_else(|| SolanaError::RelayerError("Missing pubkey in relayer /info response".into()))?;
        pk_str
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::RelayerError(format!("Invalid relayer pubkey: {}", e)))
    }

    /// Send a partially-signed transaction to the relayer for fee-payer signature and broadcast.
    async fn send_to_relayer(&self, tx: &Transaction, relayer_url: &str, amount: u64, session: &SessionKeypair) -> Result<PaymentResult> {
        let tx_bytes = bincode::serialize(tx)
            .map_err(|e| SolanaError::RelayerError(format!("Failed to serialize tx: {}", e)))?;
        let tx_b58 = bs58::encode(&tx_bytes).into_string();

        let sponsor_url = format!("{}/sponsor", relayer_url.trim_end_matches("/sponsor"));
        let client = reqwest::Client::new();
        let resp = client
            .post(&sponsor_url)
            .json(&serde_json::json!({ "transaction": tx_b58 }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SolanaError::RelayerError(
                format!("Relayer returned {}: {}", status, body),
            ));
        }

        let result: SponsorResponse = resp.json().await?;

        self.session_manager
            .record_spent(&session.keypair.pubkey(), amount)?;

        let sig = result.signature
            .parse::<solana_sdk::signature::Signature>()
            .map_err(|e| SolanaError::RelayerError(format!("Invalid signature from relayer: {}", e)))?;
        let slot = get_slot_for_signature(&self.rpc_client, sig);

        Ok(PaymentResult {
            signature: result.signature,
            slot,
            block_time: None,
        })
    }

    /// Execute a SOL transfer via the session program with relayer-sponsored gas.
    pub async fn execute_sol_transfer_sponsored(
        &self,
        session: &SessionKeypair,
        recipient: &Pubkey,
        amount_lamports: u64,
        relayer_pubkey: &Pubkey,
        relayer_url: &str,
    ) -> Result<PaymentResult> {
        if self.session_manager.is_expired(&session.session_data) {
            return Err(SolanaError::SessionExpired);
        }
        if !self
            .session_manager
            .check_spending_limit(&session.session_data, amount_lamports)
        {
            return Err(SolanaError::SpendingLimitExceeded {
                current: session.session_data.current_spent,
                limit: session.session_data.spending_limit,
            });
        }

        let program_id = session_prog::session_program_id();
        let (session_pda, _) = derive_session_pda(
            &session.session_data.owner,
            &session.keypair.pubkey(),
            &program_id,
        );

        let ix = build_execute_payment_ix(
            &program_id,
            &session_pda,
            &session.keypair.pubkey(),
            recipient,
            amount_lamports,
            "sol:transfer",
        );

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let mut tx = Transaction::new_with_payer(&[ix], Some(relayer_pubkey));
        tx.partial_sign(&[&session.keypair], recent_blockhash);

        self.send_to_relayer(&tx, relayer_url, amount_lamports, session).await
    }

    /// Execute an SPL Token transfer via the session program with relayer-sponsored gas.
    pub async fn execute_spl_transfer_sponsored(
        &self,
        session: &SessionKeypair,
        source_ata: &Pubkey,
        dest_ata: &Pubkey,
        amount: u64,
        mint: &Pubkey,
        relayer_pubkey: &Pubkey,
        relayer_url: &str,
    ) -> Result<PaymentResult> {
        if self.session_manager.is_expired(&session.session_data) {
            return Err(SolanaError::SessionExpired);
        }
        if !self
            .session_manager
            .check_spending_limit(&session.session_data, amount)
        {
            return Err(SolanaError::SpendingLimitExceeded {
                current: session.session_data.current_spent,
                limit: session.session_data.spending_limit,
            });
        }

        let program_id = session_prog::session_program_id();
        let (session_pda, _) = derive_session_pda(
            &session.session_data.owner,
            &session.keypair.pubkey(),
            &program_id,
        );

        let ix = build_execute_spl_payment_ix(
            &program_id,
            &session_pda,
            &session.keypair.pubkey(),
            source_ata,
            dest_ata,
            mint,
            amount,
            "spl:transfer",
        );

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let mut tx = Transaction::new_with_payer(&[ix], Some(relayer_pubkey));
        tx.partial_sign(&[&session.keypair], recent_blockhash);

        self.send_to_relayer(&tx, relayer_url, amount, session).await
    }

    /// Close a session and refund remaining SOL from PDA to the owner.
    pub async fn close_session_refund(
        &self,
        session: &SessionKeypair,
        owner: &Pubkey,
    ) -> Result<()> {
        let program_id = session_prog::session_program_id();
        let (session_pda, _) = derive_session_pda(
            &session.session_data.owner,
            &session.keypair.pubkey(),
            &program_id,
        );

        let pda_balance = self
            .rpc_client
            .get_balance(&session_pda)
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        if pda_balance > 0 {
            let recent_blockhash = self
                .rpc_client
                .get_latest_blockhash()
                .map_err(|e| SolanaError::RpcError(e.to_string()))?;

            // Use the on-chain withdraw_remaining instruction to move SOL from PDA to owner
            let withdraw_ix = build_withdraw_remaining_ix(&program_id, &session_pda, owner, owner);

            let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                &[withdraw_ix],
                Some(&session.keypair.pubkey()),
                &[&session.keypair],
                recent_blockhash,
            );

            self.rpc_client
                .send_and_confirm_transaction(&tx)
                .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;
        }

        self.session_manager
            .close_session(&session.keypair.pubkey())?;
        Ok(())
    }

    /// Get a reference to the session manager.
    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }

    /// Register a session key on-chain via the session program.
    /// Creates the on-chain PDA and returns the session PDA address and tx signature.
    pub async fn register_session_on_chain(
        &self,
        owner: &SessionKeypair,
        ephemeral: &SessionKeypair,
        target_program: &Pubkey,
        expires_at: i64,
        spending_limit: u64,
        scopes: Vec<String>,
        token_mint: &Pubkey,
        per_tx_limit: u64,
        daily_tx_count_limit: u32,
    ) -> Result<(Pubkey, String)> {
        let program_id = session_prog::session_program_id();
        let (session_pda, _) = derive_session_pda(
            &owner.keypair.pubkey(),
            &ephemeral.keypair.pubkey(),
            &program_id,
        );

        let ix = session_prog::build_register_session_ix(
            &program_id,
            &session_pda,
            &owner.keypair.pubkey(),
            &ephemeral.keypair.pubkey(),
            target_program,
            expires_at,
            spending_limit,
            scopes,
            token_mint,
            per_tx_limit,
            daily_tx_count_limit,
        );

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner.keypair.pubkey()),
            &[&owner.keypair, &ephemeral.keypair],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok((session_pda, sig.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> sled::Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    #[test]
    fn test_client_new() {
        let db = temp_db();
        let client = IgnitePayClient::new(
            "https://api.devnet.solana.com",
            db,
            PayMode::SelfFunded,
            None,
        );
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_new_sponsored() {
        let db = temp_db();
        let client = IgnitePayClient::new(
            "https://api.devnet.solana.com",
            db,
            PayMode::Sponsored,
            Some("http://localhost:3001/sponsor".into()),
        );
        assert!(client.is_ok());
    }
}
