//! Encryption for session state that has to rest on disk.
//!
//! [`crate::WorkspaceSession::export_state`] produces this device's signing key
//! and the group's ratchet secrets. Writing that to a file in the clear would
//! hand every workspace secret to anything that can read the data directory, so
//! the host seals it under a key held in the operating system's credential
//! store and writes only the sealed form.

use crate::{
    codec::{put_bytes, put_header, take_bytes, take_header},
    error::{CryptoError, Result},
};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, OsRng, rand_core::RngCore},
};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length of a state key in bytes.
pub const STATE_KEY_BYTES: usize = 32;
/// Length of the random nonce prefixed to every sealed blob.
const NONCE_BYTES: usize = 24;

/// A symmetric key protecting saved session state.
///
/// The bytes are wiped when the key is dropped. Keep it in the operating
/// system credential store, never beside the file it protects.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct StateKey([u8; STATE_KEY_BYTES]);

impl StateKey {
    /// Draw a new key from the operating system random source.
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0_u8; STATE_KEY_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Rebuild a key from stored bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Identity`] when the stored value is the wrong
    /// length, which usually means the credential store entry was replaced.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; STATE_KEY_BYTES] = bytes
            .try_into()
            .map_err(|_| CryptoError::Identity("stored state key is the wrong length".into()))?;
        Ok(Self(bytes))
    }

    /// The raw key, for handing to the operating system credential store.
    #[must_use]
    pub fn expose(&self) -> &[u8; STATE_KEY_BYTES] {
        &self.0
    }
}

impl core::fmt::Debug for StateKey {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print key material, not even in a panic or a log line.
        formatter.write_str("StateKey(redacted)")
    }
}

/// Encrypt session state for storage.
///
/// # Errors
///
/// Returns [`CryptoError::Seal`] when encryption fails.
pub fn seal_state(key: &StateKey, state: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.expose().into());
    let mut nonce = [0_u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), state)
        .map_err(|_| CryptoError::Seal("could not encrypt the saved session".into()))?;
    let mut sealed = Vec::with_capacity(ciphertext.len() + NONCE_BYTES + 16);
    put_header(&mut sealed);
    put_bytes(&mut sealed, &nonce);
    put_bytes(&mut sealed, &ciphertext);
    Ok(sealed)
}

/// Decrypt session state read back from storage.
///
/// # Errors
///
/// Returns [`CryptoError::Open`] when the blob was truncated, tampered with,
/// or sealed under a different key.
pub fn open_state(key: &StateKey, sealed: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = sealed;
    take_header(&mut cursor)?;
    let nonce = take_bytes(&mut cursor)?;
    if nonce.len() != NONCE_BYTES {
        return Err(CryptoError::Open(
            "saved session has a malformed nonce".into(),
        ));
    }
    let ciphertext = take_bytes(&mut cursor)?;
    let cipher = XChaCha20Poly1305::new(key.expose().into());
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| CryptoError::Open("saved session did not decrypt".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_survives_a_seal_and_open() -> Result<()> {
        let key = StateKey::generate();
        let sealed = seal_state(&key, b"session state")?;
        assert_eq!(open_state(&key, &sealed)?, b"session state");
        Ok(())
    }

    #[test]
    fn a_sealed_blob_hides_the_state() -> Result<()> {
        let key = StateKey::generate();
        let secret = b"ed25519-private-key-material";
        let sealed = seal_state(&key, secret)?;
        assert!(
            !sealed.windows(secret.len()).any(|window| window == secret),
            "sealed state must not contain the plaintext"
        );
        Ok(())
    }

    #[test]
    fn another_key_does_not_open_it() -> Result<()> {
        let sealed = seal_state(&StateKey::generate(), b"session state")?;
        assert!(open_state(&StateKey::generate(), &sealed).is_err());
        Ok(())
    }

    #[test]
    fn tampering_is_detected() -> Result<()> {
        let key = StateKey::generate();
        let mut sealed = seal_state(&key, b"session state")?;
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(open_state(&key, &sealed).is_err());
        Ok(())
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce() -> Result<()> {
        let key = StateKey::generate();
        assert_ne!(
            seal_state(&key, b"same input")?,
            seal_state(&key, b"same input")?,
            "reusing a nonce would leak that two saves were identical"
        );
        Ok(())
    }

    #[test]
    fn a_key_of_the_wrong_length_is_refused() {
        assert!(StateKey::from_bytes(b"short").is_err());
        assert!(StateKey::from_bytes(&[0_u8; STATE_KEY_BYTES]).is_ok());
    }

    #[test]
    fn the_key_never_prints_itself() {
        let key = StateKey::generate();
        assert_eq!(format!("{key:?}"), "StateKey(redacted)");
    }
}
