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

#[cfg(feature = "state-channel")]
pub mod channel;
#[cfg(feature = "zk-compression")]
pub mod compression;
#[cfg(not(feature = "zk-compression"))]
pub mod pda_did;
pub mod error;
pub mod payment;
pub mod session;
pub mod session_program;
pub mod types;

pub use error::SolanaError;

// Re-export solana_sdk for downstream crates
pub use solana_sdk;

// Alias: downstream crates use `ignite_pay_solana::compression::DidService`
#[cfg(not(feature = "zk-compression"))]
pub use pda_did as compression;
