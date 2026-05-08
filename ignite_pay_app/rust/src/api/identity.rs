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

use anyhow::Result;
use ignite_pay_core::didcomm;
use ignite_pay_core::identity::{load_identity, save_identity};
use ignite_pay_core::{build_did_document, generate_ignite_did};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Manages the phone's DID identity, persisted in sled.
pub struct IdentityManager {
    did: String,
    did_doc: Value,
    agent: Arc<Mutex<affinidi_messaging_didcomm::DIDCommAgent>>,
    db: sled::Db,
    signing_private: [u8; 32],
}

impl IdentityManager {
    /// Get or create a DID identity, persisting to sled.
    pub fn new(db_path: &str) -> Result<Self> {
        let db = sled::open(db_path)?;

        let (identity, did) = match load_identity(&db)? {
            Some(loaded) => {
                let did = loaded.did.clone();
                (loaded, did)
            }
            None => {
                let (id, did) = generate_ignite_did();
                save_identity(&db, &id, &did)?;
                (id, did)
            }
        };

        let signing_private = identity.signing_private
            .ok_or_else(|| anyhow::anyhow!("no signing key in identity"))?;

        let did_doc = build_did_document(&did, &identity);
        let (agent, _) = didcomm::create_agent(identity);

        Ok(Self {
            did,
            did_doc,
            agent: Arc::new(Mutex::new(agent)),
            db,
            signing_private,
        })
    }

    /// Get the phone's DID string.
    pub fn did(&self) -> &str {
        &self.did
    }

    /// Get the DID document JSON.
    pub fn did_doc(&self) -> &Value {
        &self.did_doc
    }

    /// Get a reference to the DIDComm agent.
    pub fn agent(&self) -> Arc<Mutex<affinidi_messaging_didcomm::DIDCommAgent>> {
        self.agent.clone()
    }

    /// Get a reference to the sled database.
    pub fn db(&self) -> &sled::Db {
        &self.db
    }

    /// Sign a message with the Ed25519 signing key, returning base64-no-pad encoded signature.
    pub fn sign(&self, message: &[u8]) -> String {
        ignite_pay_core::sign_message(&self.signing_private, message)
    }

    /// Get the raw Ed25519 signing private key bytes.
    pub fn signing_key(&self) -> &[u8; 32] {
        &self.signing_private
    }
}
