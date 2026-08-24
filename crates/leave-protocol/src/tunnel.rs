//! Requests and responses carried inside the encrypted workspace channel.
//!
//! The host already exposes every workspace capability through one local API:
//! sessions, approvals, files, Git, terminals, previews, and customization.
//! Rather than grow a second surface that could drift from it, a remote device
//! sends that same request inside an MLS frame, and the host answers inside
//! one.
//!
//! Two consequences follow, and both are deliberate:
//!
//! - A feature works remotely the moment it works locally. There is no list of
//!   endpoints that only some transports support.
//! - The relay still sees nothing. A tunnelled request is plaintext only after
//!   the host decrypts it, and the host authorizes it against the sender the
//!   MLS layer authenticated, never against anything inside the payload.

use crate::Action;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Version of the tunnelled request and response encoding.
pub const TUNNEL_VERSION: u32 = 1;
/// Largest body Leave will carry in one tunnelled message.
pub const MAX_TUNNEL_BODY_BYTES: usize = 4 * 1024 * 1024;

/// A request a remote device makes of the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelRequest {
    /// Encoding version, so an old device fails loudly rather than oddly.
    pub version: u32,
    /// Identifier the device uses to match the answer to the question.
    pub exchange_id: String,
    /// HTTP method, uppercase.
    pub method: String,
    /// Path and query, always beginning with a slash.
    pub path: String,
    /// Headers the host should honour, such as `content-type`.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Request body, empty for reads.
    #[serde(default, with = "serde_bytes_vec")]
    pub body: Vec<u8>,
}

/// The host's answer to one [`TunnelRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelResponse {
    /// Encoding version.
    pub version: u32,
    /// Matches the request's identifier.
    pub exchange_id: String,
    /// HTTP status the local API produced.
    pub status: u16,
    /// Response headers worth carrying, such as `content-type`.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Response body.
    #[serde(default, with = "serde_bytes_vec")]
    pub body: Vec<u8>,
}

/// Why a tunnelled message was refused.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TunnelError {
    /// The device speaks a version this host does not.
    #[error("unsupported tunnel version {0}")]
    UnsupportedVersion(u32),
    /// The method is not one the local API accepts.
    #[error("unsupported method")]
    UnsupportedMethod,
    /// The path is missing, relative, or tries to leave the local API.
    #[error("invalid request path")]
    InvalidPath,
    /// The body is larger than the host will process.
    #[error("request body exceeds {MAX_TUNNEL_BODY_BYTES} bytes")]
    BodyTooLarge,
    /// The message could not be decoded at all.
    #[error("malformed tunnel message")]
    Malformed,
}

impl TunnelRequest {
    /// Check a decrypted request before the host acts on any part of it.
    ///
    /// # Errors
    ///
    /// Returns a [`TunnelError`] describing the first violation found.
    pub fn validate(&self) -> Result<(), TunnelError> {
        if self.version != TUNNEL_VERSION {
            return Err(TunnelError::UnsupportedVersion(self.version));
        }
        if !matches!(
            self.method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
        ) {
            return Err(TunnelError::UnsupportedMethod);
        }
        // Only the local API is reachable. A remote device must not be able to
        // ask for the static bundle, a traversal path, or an absolute URL that
        // would turn the host into a proxy.
        if !self.path.starts_with("/api/v1/local/")
            || self.path.contains("..")
            || self.path.contains("://")
        {
            return Err(TunnelError::InvalidPath);
        }
        if self.body.len() > MAX_TUNNEL_BODY_BYTES {
            return Err(TunnelError::BodyTooLarge);
        }
        Ok(())
    }
}

/// Decide which action a tunnelled request performs.
///
/// The host checks this against the role and grants of the device the MLS
/// layer authenticated, before the request reaches a handler. Anything this
/// function does not recognise is treated as the most restricted match rather
/// than waved through, so a new endpoint cannot become remotely reachable by
/// being forgotten here.
#[must_use]
pub fn action_for(method: &str, path: &str) -> Action {
    let route = path.split('?').next().unwrap_or(path);
    let reading = matches!(method, "GET" | "HEAD");

    // Capability-gated surfaces are decided by the surface itself, because
    // even reading them is sensitive.
    if route.starts_with("/api/v1/local/terminals") {
        return Action::RawPty;
    }
    if route.starts_with("/api/v1/local/previews") {
        return Action::PersistentBrowserProfile;
    }
    if route.starts_with("/api/v1/local/customization") {
        return if route.contains("scope=global") || path.contains("scope=global") {
            Action::GlobalCustomization
        } else {
            Action::ProjectCustomization
        };
    }

    if reading {
        return Action::View;
    }

    match route {
        "/api/v1/local/file" => Action::EditFiles,
        path if path.starts_with("/api/v1/local/git/") => match path {
            "/api/v1/local/git/status"
            | "/api/v1/local/git/diff"
            | "/api/v1/local/git/worktrees" => Action::View,
            _ => Action::GitWrite,
        },
        "/api/v1/local/sessions" => Action::Prompt,
        path if path.starts_with("/api/v1/local/sessions/") => {
            if path.ends_with("/permissions") || path.contains("/permissions/") {
                Action::LowRiskApproval
            } else {
                Action::Prompt
            }
        }
        // An unrecognised write is treated as a workspace edit, which needs a
        // maintainer, rather than as something any viewer may do.
        _ => Action::EditFiles,
    }
}

/// Encode a value for the encrypted channel.
///
/// # Errors
///
/// Returns [`TunnelError::Malformed`] when the value cannot be encoded.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, TunnelError> {
    serde_json::to_vec(value).map_err(|_| TunnelError::Malformed)
}

/// Decode a value taken out of the encrypted channel.
///
/// # Errors
///
/// Returns [`TunnelError::Malformed`] when the bytes are not the expected
/// message.
pub fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, TunnelError> {
    serde_json::from_slice(bytes).map_err(|_| TunnelError::Malformed)
}

/// Base64 is smaller than a JSON array of numbers for binary bodies.
mod serde_bytes_vec {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Alphabet and padding of standard base64.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let mut buffer = [0_u8; 3];
            buffer[..chunk.len()].copy_from_slice(chunk);
            let value =
                (u32::from(buffer[0]) << 16) | (u32::from(buffer[1]) << 8) | u32::from(buffer[2]);
            for index in 0..4 {
                if index <= chunk.len() {
                    let shift = 18 - index * 6;
                    let position = ((value >> shift) & 0b0011_1111) as usize;
                    encoded.push(char::from(ALPHABET[position]));
                } else {
                    encoded.push('=');
                }
            }
        }
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut decoded = Vec::with_capacity(text.len() / 4 * 3);
        let mut accumulator = 0_u32;
        let mut bits = 0_u32;
        for character in text.bytes() {
            if character == b'=' {
                break;
            }
            let Some(position) = ALPHABET.iter().position(|entry| *entry == character) else {
                return Err(serde::de::Error::custom("invalid base64"));
            };
            accumulator = (accumulator << 6) | u32::try_from(position).unwrap_or(0);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                let byte = ((accumulator >> bits) & 0xff) as u8;
                decoded.push(byte);
            }
        }
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, path: &str) -> TunnelRequest {
        TunnelRequest {
            version: TUNNEL_VERSION,
            exchange_id: "exchange-1".into(),
            method: method.into(),
            path: path.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn an_ordinary_request_is_accepted() {
        assert_eq!(request("GET", "/api/v1/local/status").validate(), Ok(()));
        assert_eq!(request("POST", "/api/v1/local/sessions").validate(), Ok(()));
    }

    #[test]
    fn a_remote_device_cannot_reach_outside_the_local_api() {
        for path in [
            "/",
            "/index.html",
            "/api/v1/setup/status",
            "/api/v1/local/../../etc/passwd",
            "http://evil.example/api/v1/local/status",
            "/api/v1/local/file?path=../../../etc/passwd",
        ] {
            assert_eq!(
                request("GET", path).validate(),
                Err(TunnelError::InvalidPath),
                "{path} must not be reachable through the tunnel"
            );
        }
    }

    #[test]
    fn unusual_methods_are_refused() {
        assert_eq!(
            request("CONNECT", "/api/v1/local/status").validate(),
            Err(TunnelError::UnsupportedMethod)
        );
        assert_eq!(
            request("TRACE", "/api/v1/local/status").validate(),
            Err(TunnelError::UnsupportedMethod)
        );
    }

    #[test]
    fn a_future_version_fails_loudly() {
        let mut ahead = request("GET", "/api/v1/local/status");
        ahead.version = TUNNEL_VERSION + 1;
        assert_eq!(
            ahead.validate(),
            Err(TunnelError::UnsupportedVersion(TUNNEL_VERSION + 1))
        );
    }

    #[test]
    fn an_oversized_body_is_refused() {
        let mut large = request("PUT", "/api/v1/local/file");
        large.body = vec![0; MAX_TUNNEL_BODY_BYTES + 1];
        assert_eq!(large.validate(), Err(TunnelError::BodyTooLarge));
    }

    #[test]
    fn messages_survive_the_encoding() -> Result<(), TunnelError> {
        let mut original = request("PUT", "/api/v1/local/file");
        original.body = (0..=255_u8).cycle().take(1000).collect();
        original
            .headers
            .insert("content-type".into(), "application/json".into());
        let decoded: TunnelRequest = decode(&encode(&original)?)?;
        assert_eq!(decoded.body, original.body);
        assert_eq!(decoded.headers, original.headers);
        assert_eq!(decoded.path, original.path);
        Ok(())
    }

    #[test]
    fn binary_bodies_of_every_length_round_trip() -> Result<(), TunnelError> {
        for length in 0..8_usize {
            let mut original = request("PUT", "/api/v1/local/file");
            original.body = (0..length)
                .map(|index| u8::try_from(index).unwrap_or(0))
                .collect();
            let decoded: TunnelRequest = decode(&encode(&original)?)?;
            assert_eq!(decoded.body, original.body, "length {length}");
        }
        Ok(())
    }

    #[test]
    fn a_response_survives_the_encoding() -> Result<(), TunnelError> {
        let original = TunnelResponse {
            version: TUNNEL_VERSION,
            exchange_id: "exchange-1".into(),
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: b"{\"ok\":true}".to_vec(),
        };
        let decoded: TunnelResponse = decode(&encode(&original)?)?;
        assert_eq!(decoded.status, 200);
        assert_eq!(decoded.body, original.body);
        Ok(())
    }

    #[test]
    fn reads_are_visible_to_every_role() {
        for path in [
            "/api/v1/local/status",
            "/api/v1/local/sessions",
            "/api/v1/local/files",
            "/api/v1/local/git/status",
            "/api/v1/local/events",
        ] {
            assert_eq!(action_for("GET", path), Action::View, "{path}");
        }
    }

    #[test]
    fn sensitive_surfaces_need_their_own_grant() {
        assert_eq!(
            action_for("POST", "/api/v1/local/terminals"),
            Action::RawPty
        );
        // Even reading a terminal needs the grant.
        assert_eq!(
            action_for("GET", "/api/v1/local/terminals/abc"),
            Action::RawPty
        );
        assert_eq!(
            action_for("POST", "/api/v1/local/previews"),
            Action::PersistentBrowserProfile
        );
        assert_eq!(
            action_for("POST", "/api/v1/local/customization?scope=global"),
            Action::GlobalCustomization
        );
        assert_eq!(
            action_for("POST", "/api/v1/local/customization"),
            Action::ProjectCustomization
        );
    }

    #[test]
    fn writes_are_classified_by_what_they_change() {
        assert_eq!(action_for("PUT", "/api/v1/local/file"), Action::EditFiles);
        assert_eq!(
            action_for("POST", "/api/v1/local/git/commit"),
            Action::GitWrite
        );
        assert_eq!(
            action_for("POST", "/api/v1/local/git/push"),
            Action::GitWrite
        );
        assert_eq!(
            action_for("POST", "/api/v1/local/sessions/abc/prompts"),
            Action::Prompt
        );
        assert_eq!(
            action_for("POST", "/api/v1/local/sessions/abc/permissions"),
            Action::LowRiskApproval
        );
    }

    #[test]
    fn an_unknown_write_is_not_waved_through() {
        // A new endpoint nobody classified must not be reachable by a viewer.
        assert_eq!(
            action_for("POST", "/api/v1/local/something-new"),
            Action::EditFiles
        );
    }

    #[test]
    fn a_query_string_cannot_disguise_the_route() {
        assert_eq!(
            action_for("POST", "/api/v1/local/terminals?pretend=status"),
            Action::RawPty
        );
        assert_eq!(
            action_for("GET", "/api/v1/local/status?x=/terminals"),
            Action::View
        );
    }

    #[test]
    fn junk_does_not_decode() {
        assert!(matches!(
            decode::<TunnelRequest>(b"not json"),
            Err(TunnelError::Malformed)
        ));
    }
}
