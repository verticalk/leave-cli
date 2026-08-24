//! Errors raised by the MLS session layer.

use thiserror::Error;

/// A failure inside Leave's MLS boundary.
///
/// Variants stay coarse on purpose. A remote peer learns only that its frame
/// was rejected, never which internal check rejected it.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// A signature key pair or credential could not be created.
    #[error("could not create device identity: {0}")]
    Identity(String),
    /// A key package could not be produced or read.
    #[error("could not use key package: {0}")]
    KeyPackage(String),
    /// A group could not be created, joined, or updated.
    #[error("could not update the workspace group: {0}")]
    Group(String),
    /// A frame could not be encrypted.
    #[error("could not encrypt the message: {0}")]
    Seal(String),
    /// A frame could not be decrypted or authenticated.
    #[error("could not decrypt the message: {0}")]
    Open(String),
    /// A frame decrypted but was not the expected kind of message.
    #[error("unexpected message type on the workspace channel")]
    UnexpectedMessage,
    /// The named device is not a member of this group.
    #[error("device is not a member of this workspace")]
    UnknownDevice,
    /// A credential did not carry a usable device identity.
    #[error("device identity is not valid UTF-8")]
    MalformedIdentity,
    /// Serialization of an MLS structure failed.
    #[error("could not encode an MLS structure: {0}")]
    Encode(String),
}

/// Result alias for the MLS session layer.
pub type Result<T> = core::result::Result<T, CryptoError>;
