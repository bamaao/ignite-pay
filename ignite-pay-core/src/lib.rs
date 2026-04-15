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
