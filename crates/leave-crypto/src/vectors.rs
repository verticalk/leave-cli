//! Golden vectors shared by the native and WebAssembly builds.
//!
//! The release gate asks for evidence that both builds interpret the same
//! bytes the same way. A vector is a saved session plus a frame sealed for it,
//! so either build can be handed the same input and must produce the same
//! plaintext and the same authenticated sender.
//!
//! Sealing is randomized, so vectors pin down the direction that can be
//! checked exactly: opening. `tests/interop.rs` runs them natively and
//! `tests/wasm_interop.rs` runs the identical assertions in WebAssembly, each
//! against a vector the *other* build produced.

use crate::{
    codec::{put_bytes, put_header, take_bytes, take_header},
    error::{CryptoError, Result},
    identity::DeviceIdentity,
    session::WorkspaceSession,
};

/// One interoperability vector.
pub struct InteropVector {
    /// Saved session state for the receiving device.
    pub receiver_state: Vec<u8>,
    /// A frame sealed by the other member of that group.
    pub sealed_frame: Vec<u8>,
    /// Plaintext the receiver must recover.
    pub expected_plaintext: Vec<u8>,
    /// Device identifier the receiver must authenticate as the sender.
    pub expected_sender: String,
}

impl InteropVector {
    /// Produce a vector on this build, for the other build to check.
    ///
    /// # Errors
    ///
    /// Returns an error when the group or the frame cannot be created.
    pub fn produce(sender_id: &str, receiver_id: &str, plaintext: &[u8]) -> Result<Self> {
        let mut sender =
            WorkspaceSession::create(DeviceIdentity::generate(sender_id)?, "interop-workspace")?;
        let receiver = DeviceIdentity::generate(receiver_id)?;
        let invitation = sender.add_device(&receiver.publish_key_package()?)?;
        let receiver = WorkspaceSession::join(receiver, &invitation.welcome)?;
        Ok(Self {
            receiver_state: receiver.export_state()?,
            sealed_frame: sender.seal(plaintext)?,
            expected_plaintext: plaintext.to_vec(),
            expected_sender: sender_id.to_owned(),
        })
    }

    /// Check this vector on the current build.
    ///
    /// # Errors
    ///
    /// Returns an error when the session cannot be restored, the frame does
    /// not open, or it opens to something the vector did not predict.
    pub fn verify(&self) -> Result<()> {
        let mut receiver = WorkspaceSession::restore(&self.receiver_state)?;
        let opened = receiver.open(&self.sealed_frame)?;
        if opened.plaintext != self.expected_plaintext {
            return Err(CryptoError::Open(
                "the vector opened to different plaintext on this build".into(),
            ));
        }
        if opened.sender_device_id != self.expected_sender {
            return Err(CryptoError::Open(
                "the vector authenticated a different sender on this build".into(),
            ));
        }
        Ok(())
    }

    /// Encode a vector so the other build can load it.
    ///
    /// # Errors
    ///
    /// Returns an error when the vector cannot be encoded.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        put_header(&mut buffer);
        put_bytes(&mut buffer, &self.receiver_state);
        put_bytes(&mut buffer, &self.sealed_frame);
        put_bytes(&mut buffer, &self.expected_plaintext);
        put_bytes(&mut buffer, self.expected_sender.as_bytes());
        Ok(buffer)
    }

    /// Decode a vector produced by the other build.
    ///
    /// # Errors
    ///
    /// Returns an error when the bytes are not a vector.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = bytes;
        take_header(&mut cursor)?;
        let receiver_state = take_bytes(&mut cursor)?.to_vec();
        let sealed_frame = take_bytes(&mut cursor)?.to_vec();
        let expected_plaintext = take_bytes(&mut cursor)?.to_vec();
        let expected_sender = String::from_utf8(take_bytes(&mut cursor)?.to_vec())
            .map_err(|_| CryptoError::MalformedIdentity)?;
        Ok(Self {
            receiver_state,
            sealed_frame,
            expected_plaintext,
            expected_sender,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_verifies_on_the_build_that_made_it() -> Result<()> {
        let vector = InteropVector::produce("host", "phone", b"interop payload")?;
        vector.verify()?;
        Ok(())
    }

    #[test]
    fn a_vector_survives_encoding() -> Result<()> {
        let vector = InteropVector::produce("host", "phone", b"interop payload")?;
        let decoded = InteropVector::decode(&vector.encode()?)?;
        decoded.verify()?;
        assert_eq!(decoded.expected_sender, "host");
        assert_eq!(decoded.expected_plaintext, b"interop payload");
        Ok(())
    }

    #[test]
    fn a_damaged_vector_does_not_quietly_pass() -> Result<()> {
        let vector = InteropVector::produce("host", "phone", b"interop payload")?;
        let mut encoded = vector.encode()?;
        let last = encoded.len() - 1;
        encoded[last] ^= 0xff;
        // Either the vector fails to decode, or it decodes and fails to
        // verify. What must never happen is a silent pass.
        let outcome = InteropVector::decode(&encoded).and_then(|vector| vector.verify());
        assert!(outcome.is_err());
        Ok(())
    }
}
