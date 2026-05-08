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

pub mod didcomm;
pub mod identity;
pub mod ipfs;
pub mod list_store;
pub mod types;
pub mod vc;

pub mod audit_merkle;
pub mod log_crypto;
pub mod log_chunk;
pub mod log_sync;

// Re-export protobuf types
pub mod audit_proto {
    include!(concat!(env!("OUT_DIR"), "/ignite_pay.audit.v1.rs"));
}

#[cfg(feature = "solana")]
pub mod solana_did;

// Re-export key types for convenience
pub use didcomm::*;
pub use identity::*;
pub use types::*;
pub use vc::*;
