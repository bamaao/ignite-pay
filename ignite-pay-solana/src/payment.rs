use crate::error::{Result, SolanaError};
use crate::session::{SessionKeypair, SessionManager};
use crate::types::{PayMode, PaymentResult};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
#[allow(deprecated)]
use solana_sdk::system_instruction;

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

    /// Execute a SOL transfer using a session key.
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

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix =
            system_instruction::transfer(&session.keypair.pubkey(), recipient, amount_lamports);

        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
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

    /// Execute an SPL Token transfer using a session key.
    pub async fn execute_spl_transfer(
        &self,
        session: &SessionKeypair,
        source_ata: &Pubkey,
        dest_ata: &Pubkey,
        amount: u64,
        _mint: &Pubkey,
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

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let token_program_id = spl_token::id();
        let ix = spl_token::instruction::transfer(
            &token_program_id,
            source_ata,
            dest_ata,
            &session.keypair.pubkey(),
            &[&session.keypair.pubkey()],
            amount,
        )
        .map_err(|e| SolanaError::TransactionFailed(format!("SPL transfer ix error: {}", e)))?;

        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
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

    /// Unified payment entry point — automatically selects SOL or SPL transfer.
    pub async fn execute_payment(
        &self,
        recipient: &str,
        amount: u64,
        token: &str,
        _network: &str,
        session: &SessionKeypair,
    ) -> Result<PaymentResult> {
        let recipient_pubkey = recipient
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::InvalidPubkey(e.to_string()))?;

        match token {
            "SOL" | "sol" => {
                self.execute_sol_transfer(session, &recipient_pubkey, amount)
                    .await
            }
            _ => Err(SolanaError::Other(anyhow::anyhow!(
                "SPL token transfers require source_ata, dest_ata, and mint. Use execute_spl_transfer directly."
            ))),
        }
    }

    /// Close a session and refund remaining SOL to the owner.
    pub async fn close_session_refund(
        &self,
        session: &SessionKeypair,
        owner: &Pubkey,
    ) -> Result<()> {
        let balance = self
            .rpc_client
            .get_balance(&session.keypair.pubkey())
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        if balance > 0 {
            let recent_blockhash = self
                .rpc_client
                .get_latest_blockhash()
                .map_err(|e| SolanaError::RpcError(e.to_string()))?;

            let ix = system_instruction::transfer(&session.keypair.pubkey(), owner, balance);

            let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                &[ix],
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
