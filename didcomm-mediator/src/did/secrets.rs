//! The mediator's DIDCommAgent already manages its own keys via
//! `PrivateIdentity` stored in the `DIDCommStore`. No separate
//! secrets resolver is needed — the agent's `unpack()` method
//! automatically matches incoming JWE recipient headers against
//! local identities.
//!
//! This module re-exports the key types for convenience.

pub use affinidi_messaging_didcomm::identity::PrivateIdentity;
pub use affinidi_messaging_didcomm::DIDCommAgent;
