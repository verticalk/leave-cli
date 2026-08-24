//! MLS boundary for Leave native and browser endpoints.
//!
//! [`WorkspaceSession`] implements the group: application messages, member
//! addition, and removal with forward key rotation.
//!
//! Production remote access remains deliberately disabled until the remaining
//! release-gate evidence is recorded.

mod codec;
pub mod error;
pub mod identity;
mod provider;
pub mod session;
pub mod vault;

pub use error::CryptoError;
pub use identity::{CIPHERSUITE, DeviceIdentity};
pub use session::{Invitation, OpenedMessage, WorkspaceSession};
pub use vault::{STATE_KEY_BYTES, StateKey, open_state, seal_state, subtle_eq};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The dependency and review conditions which protect remote releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoReleaseStatus {
    /// The audited `OpenMLS` feature graph is present in this build.
    pub openmls_compiled: GateCheck,
    /// Cargo audit reports no advisory in the selected provider graph.
    pub advisory_graph_clean: GateCheck,
    /// Native and browser implementations pass shared golden vectors.
    pub native_wasm_vectors_pass: GateCheck,
    /// Browser persistence and spent-key deletion tests pass.
    pub browser_persistence_pass: GateCheck,
    /// Maintainers attached the independent review record to the release.
    pub external_review_recorded: GateCheck,
}

/// One independently reviewed remote-release condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateCheck {
    /// Evidence for the condition has been recorded.
    Passed,
    /// The condition prevents remote operation.
    Blocked,
}

impl CryptoReleaseStatus {
    /// Current repository status. Review fields require a deliberate code change.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            // No OpenMLS provider is integrated yet, so this cannot be earned
            // by a build flag. It changes when the transport lands.
            openmls_compiled: GateCheck::Blocked,
            advisory_graph_clean: GateCheck::Blocked,
            native_wasm_vectors_pass: GateCheck::Blocked,
            browser_persistence_pass: GateCheck::Blocked,
            external_review_recorded: GateCheck::Blocked,
        }
    }

    /// Return true only when every remote-release condition passes.
    #[must_use]
    pub const fn allows_remote_release(self) -> bool {
        matches!(self.openmls_compiled, GateCheck::Passed)
            && matches!(self.advisory_graph_clean, GateCheck::Passed)
            && matches!(self.native_wasm_vectors_pass, GateCheck::Passed)
            && matches!(self.browser_persistence_pass, GateCheck::Passed)
            && matches!(self.external_review_recorded, GateCheck::Passed)
    }
}

/// Refuse hosted or internet-routed operation until every crypto gate passes.
///
/// # Errors
///
/// Returns [`CryptoGateError::Blocked`] while any recorded check is blocked.
pub fn require_remote_release() -> Result<(), CryptoGateError> {
    let status = CryptoReleaseStatus::current();
    if status.allows_remote_release() {
        Ok(())
    } else {
        Err(CryptoGateError::Blocked(status))
    }
}

/// Error returned when a build has not passed the remote crypto release gate.
#[derive(Debug, Error)]
pub enum CryptoGateError {
    /// Remote startup was attempted before all evidence was recorded.
    #[error("remote access is blocked by the OpenMLS release gate: {0:?}")]
    Blocked(CryptoReleaseStatus),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_release_fails_closed() {
        assert!(require_remote_release().is_err());
    }
}
