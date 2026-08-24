//! Versioned wire types and authorization policy shared by Leave endpoints.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use uuid::Uuid;

/// Current Leave envelope and secure-message schema version.
pub const PROTOCOL_VERSION: u32 = 1;
/// Hard upper bound for an encrypted WebSocket application frame.
pub const MAX_CIPHERTEXT_BYTES: usize = 8 * 1024 * 1024;

pub mod tunnel;

/// Generated Protobuf messages. Inner messages only appear inside MLS ciphertext.
#[allow(missing_docs)]
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/leave.protocol.v1.rs"));
}

/// Errors detected before a relay envelope enters the routing layer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnvelopeError {
    /// The sender speaks an unsupported protocol version.
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u32),
    /// The route is blank or too long.
    #[error("invalid route identifier")]
    InvalidRoute,
    /// The message identifier is not a UUID.
    #[error("invalid message identifier")]
    InvalidMessageId,
    /// The encrypted body exceeds the relay limit.
    #[error("ciphertext exceeds {MAX_CIPHERTEXT_BYTES} bytes")]
    CiphertextTooLarge,
    /// Empty encrypted frames are never routable application messages.
    #[error("ciphertext is empty")]
    EmptyCiphertext,
}

/// Validate metadata visible to the blind relay without inspecting ciphertext.
///
/// # Errors
///
/// Returns an [`EnvelopeError`] when visible metadata violates the protocol.
pub fn validate_envelope(envelope: &wire::RelayEnvelope) -> Result<(), EnvelopeError> {
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(EnvelopeError::UnsupportedVersion(envelope.protocol_version));
    }
    if envelope.route_id.is_empty() || envelope.route_id.len() > 128 {
        return Err(EnvelopeError::InvalidRoute);
    }
    if Uuid::parse_str(&envelope.message_id).is_err() {
        return Err(EnvelopeError::InvalidMessageId);
    }
    if envelope.ciphertext.is_empty() {
        return Err(EnvelopeError::EmptyCiphertext);
    }
    if envelope.ciphertext.len() > MAX_CIPHERTEXT_BYTES {
        return Err(EnvelopeError::CiphertextTooLarge);
    }
    Ok(())
}

/// Stable workspace collaboration roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full workspace administration and key lifecycle control.
    Owner,
    /// Code, Git, project customization, and ordinary approvals.
    Maintainer,
    /// Agent prompts and low-risk approvals.
    Operator,
    /// Encrypted read-only access.
    Viewer,
}

/// Sensitive actions granted independently of a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Create and interact with a raw operating-system terminal.
    RawPty,
    /// Approve actions classified as destructive.
    DangerousApproval,
    /// Reuse a browser profile that may contain credentials.
    PersistentBrowserProfile,
    /// Read or change global Devin customization.
    GlobalCustomization,
}

/// Actions which the host authorizes after decrypting a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read encrypted workspace content.
    View,
    /// Send a prompt or cancel an active turn.
    Prompt,
    /// Approve a command classified below the destructive threshold.
    LowRiskApproval,
    /// Write files inside a registered workspace.
    EditFiles,
    /// Perform a structured mutating Git operation.
    GitWrite,
    /// Change project-scoped rules, skills, hooks, or MCP configuration.
    ProjectCustomization,
    /// Invite, revoke, or change the role of workspace members.
    ManageMembers,
    /// Create or interact with an unrestricted PTY.
    RawPty,
    /// Approve a command classified as destructive.
    DangerousApproval,
    /// Reuse browser state that may contain credentials.
    PersistentBrowserProfile,
    /// Read or change global Devin customization.
    GlobalCustomization,
}

/// Decide whether a role and its explicit grants authorize an action.
#[must_use]
pub fn authorize(role: Role, grants: &BTreeSet<Capability>, action: Action) -> bool {
    let role_allows = match action {
        Action::View => true,
        Action::Prompt
        | Action::LowRiskApproval
        | Action::RawPty
        | Action::DangerousApproval
        | Action::PersistentBrowserProfile
        | Action::GlobalCustomization => !matches!(role, Role::Viewer),
        Action::EditFiles | Action::GitWrite | Action::ProjectCustomization => {
            matches!(role, Role::Owner | Role::Maintainer)
        }
        Action::ManageMembers => matches!(role, Role::Owner),
    };
    if !role_allows {
        return false;
    }

    match action {
        Action::RawPty => grants.contains(&Capability::RawPty),
        Action::DangerousApproval => grants.contains(&Capability::DangerousApproval),
        Action::PersistentBrowserProfile => grants.contains(&Capability::PersistentBrowserProfile),
        Action::GlobalCustomization => grants.contains(&Capability::GlobalCustomization),
        _ => true,
    }
}

/// The only diagnostic fields accepted by hosted telemetry.
pub const TELEMETRY_ALLOWLIST: &[&str] = &[
    "app_version",
    "byte_count_bucket",
    "coarse_platform",
    "connection_state",
    "error_code",
    "latency_bucket_ms",
    "protocol_version",
];

/// Reject telemetry fields that could carry workspace content.
#[must_use]
pub fn telemetry_fields_allowed<'a>(fields: impl IntoIterator<Item = &'a str>) -> bool {
    fields
        .into_iter()
        .all(|field| TELEMETRY_ALLOWLIST.binary_search(&field).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_terminal_needs_role_and_explicit_grant() {
        let mut grants = BTreeSet::new();
        assert!(!authorize(Role::Owner, &grants, Action::RawPty));
        grants.insert(Capability::RawPty);
        assert!(authorize(Role::Owner, &grants, Action::RawPty));
        assert!(!authorize(Role::Viewer, &grants, Action::RawPty));
    }

    #[test]
    fn relay_rejects_plain_or_oversized_payloads() {
        let mut envelope = wire::RelayEnvelope {
            protocol_version: PROTOCOL_VERSION,
            route_id: "workspace-1".into(),
            message_id: Uuid::now_v7().to_string(),
            ciphertext: Vec::new(),
        };
        assert_eq!(
            validate_envelope(&envelope),
            Err(EnvelopeError::EmptyCiphertext)
        );
        envelope.ciphertext = vec![0; MAX_CIPHERTEXT_BYTES + 1];
        assert_eq!(
            validate_envelope(&envelope),
            Err(EnvelopeError::CiphertextTooLarge)
        );
    }

    #[test]
    fn telemetry_rejects_content_fields() {
        assert!(telemetry_fields_allowed(["app_version", "error_code"]));
        assert!(!telemetry_fields_allowed(["app_version", "prompt"]));
    }
}
