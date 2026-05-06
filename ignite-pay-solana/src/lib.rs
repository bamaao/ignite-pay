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
