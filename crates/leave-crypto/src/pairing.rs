//! Adding a new phone to a workspace.
//!
//! Pairing has to work over a relay that is not trusted, before the new device
//! is in the group and can be spoken to privately. Two separate secrets do
//! that job:
//!
//! - The **route token** lets an endpoint attach to the relay at all. It is
//!   transport authorization, and by itself it must not be enough to join.
//! - The **pairing secret** proves the phone saw the host's screen. It is
//!   shown once, in the QR code, and authorizes exactly one device to join.
//!
//! A phone tags its key package with the pairing secret. The host checks the
//! tag before it commits anything, so an attacker who obtained only the route
//! token cannot get itself added. The key package and the welcome are both
//! safe for the relay to carry: a key package is public, and a welcome is
//! encrypted to the invited device alone.

use crate::{
    codec::{put_bytes, put_header, take_bytes, take_header},
    error::{CryptoError, Result},
    session::{Invitation, WorkspaceSession},
    vault::subtle_eq,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length of a pairing secret in bytes.
pub const PAIRING_SECRET_BYTES: usize = 32;
/// Frame kind carrying a device's request to join.
const KIND_REQUEST: u8 = 1;
/// Frame kind carrying the group's answer.
const KIND_WELCOME: u8 = 2;
/// Domain separator so a pairing tag cannot be reused for anything else.
const TAG_CONTEXT: &str = "leave pairing tag v1";

/// The one-time secret displayed by the host and scanned by the phone.
///
/// It is wiped on drop and redacts itself when printed, so it cannot reach a
/// log line by accident.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PairingSecret([u8; PAIRING_SECRET_BYTES]);

impl PairingSecret {
    /// Draw a fresh pairing secret.
    #[must_use]
    pub fn generate() -> Self {
        let key = crate::vault::StateKey::generate();
        Self(*key.expose())
    }

    /// Rebuild a secret the phone read out of a QR code.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Identity`] when the value is the wrong length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; PAIRING_SECRET_BYTES] = bytes
            .try_into()
            .map_err(|_| CryptoError::Identity("pairing secret is the wrong length".into()))?;
        Ok(Self(bytes))
    }

    /// The raw secret, for encoding into the pairing QR code.
    #[must_use]
    pub fn expose(&self) -> &[u8; PAIRING_SECRET_BYTES] {
        &self.0
    }

    /// Authentication tag binding a key package to this secret.
    fn tag(&self, key_package: &[u8]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new_keyed(&self.0);
        hasher.update(TAG_CONTEXT.as_bytes());
        hasher.update(key_package);
        *hasher.finalize().as_bytes()
    }
}

impl core::fmt::Debug for PairingSecret {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("PairingSecret(redacted)")
    }
}

/// A phone's request to join, ready to publish to the relay.
#[must_use]
pub fn pairing_request(key_package: &[u8], secret: &PairingSecret) -> Vec<u8> {
    let mut frame = Vec::new();
    put_header(&mut frame);
    frame.push(KIND_REQUEST);
    put_bytes(&mut frame, key_package);
    put_bytes(&mut frame, &secret.tag(key_package));
    frame
}

/// Accept a pairing request and produce the welcome that admits the device.
///
/// The tag is checked before the group is touched, so a request that did not
/// come from the phone holding the pairing secret changes nothing.
///
/// # Errors
///
/// Returns [`CryptoError::KeyPackage`] when the frame is malformed or the tag
/// does not match, and [`CryptoError::Group`] when the commit fails.
pub fn accept_pairing(
    session: &mut WorkspaceSession,
    request: &[u8],
    secret: &PairingSecret,
) -> Result<Invitation> {
    let mut cursor = request;
    take_header(&mut cursor)?;
    let (kind, rest) = cursor
        .split_first()
        .ok_or_else(|| CryptoError::KeyPackage("pairing request is truncated".into()))?;
    if *kind != KIND_REQUEST {
        return Err(CryptoError::KeyPackage(
            "that frame is not a pairing request".into(),
        ));
    }
    cursor = rest;
    let key_package = take_bytes(&mut cursor)?;
    let presented = take_bytes(&mut cursor)?;
    if !subtle_eq(&secret.tag(key_package), presented) {
        return Err(CryptoError::KeyPackage(
            "this device did not present the pairing secret".into(),
        ));
    }
    session.add_device(key_package)
}

/// Wrap a welcome for the relay.
#[must_use]
pub fn pairing_welcome(invitation: &Invitation) -> Vec<u8> {
    let mut frame = Vec::new();
    put_header(&mut frame);
    frame.push(KIND_WELCOME);
    put_bytes(&mut frame, &invitation.welcome);
    frame
}

/// Read a welcome frame back out.
///
/// # Errors
///
/// Returns [`CryptoError::UnexpectedMessage`] when the frame is not a welcome.
pub fn read_pairing_welcome(frame: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = frame;
    take_header(&mut cursor)?;
    let (kind, rest) = cursor.split_first().ok_or(CryptoError::UnexpectedMessage)?;
    if *kind != KIND_WELCOME {
        return Err(CryptoError::UnexpectedMessage);
    }
    cursor = rest;
    Ok(take_bytes(&mut cursor)?.to_vec())
}

/// Whether a relay frame belongs to the pairing exchange rather than the group.
///
/// Endpoints share one route for both, so a receiver needs to tell them apart
/// before handing bytes to the MLS layer.
#[must_use]
pub fn is_pairing_frame(frame: &[u8]) -> bool {
    let mut cursor = frame;
    take_header(&mut cursor).is_ok()
        && cursor
            .first()
            .is_some_and(|kind| *kind == KIND_REQUEST || *kind == KIND_WELCOME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;

    fn host() -> Result<WorkspaceSession> {
        WorkspaceSession::create(DeviceIdentity::generate("host")?, "workspace-1")
    }

    #[test]
    fn a_phone_that_scanned_the_code_joins_the_workspace() -> Result<()> {
        let mut host = host()?;
        let secret = PairingSecret::generate();

        let phone = DeviceIdentity::generate("phone")?;
        let request = pairing_request(&phone.publish_key_package()?, &secret);
        let invitation = accept_pairing(&mut host, &request, &secret)?;

        let welcome = read_pairing_welcome(&pairing_welcome(&invitation))?;
        let mut phone = WorkspaceSession::join(phone, &welcome)?;

        let frame = host.seal(b"paired")?;
        assert_eq!(phone.open(&frame)?.plaintext, b"paired");
        let mut members = host.member_device_ids()?;
        members.sort();
        assert_eq!(members, ["host", "phone"]);
        Ok(())
    }

    #[test]
    fn the_route_token_alone_does_not_get_a_device_in() -> Result<()> {
        let mut host = host()?;
        let secret = PairingSecret::generate();

        // The attacker is on the relay and can publish, but never saw the code.
        let attacker = DeviceIdentity::generate("attacker")?;
        let guessed = PairingSecret::generate();
        let request = pairing_request(&attacker.publish_key_package()?, &guessed);

        assert!(matches!(
            accept_pairing(&mut host, &request, &secret),
            Err(CryptoError::KeyPackage(_))
        ));
        assert_eq!(
            host.member_device_ids()?,
            ["host"],
            "a rejected request must not change the group"
        );
        Ok(())
    }

    #[test]
    fn a_tag_from_another_key_package_is_refused() -> Result<()> {
        let mut host = host()?;
        let secret = PairingSecret::generate();
        let honest = DeviceIdentity::generate("phone")?;
        let attacker = DeviceIdentity::generate("attacker")?;

        // Replaying an honest device's tag against a different key package
        // must not work: the tag covers the key package it was made for.
        let honest_package = honest.publish_key_package()?;
        let mut request = pairing_request(&honest_package, &secret);
        let tag_start = request.len() - 32;
        let tag: Vec<u8> = request[tag_start..].to_vec();
        request = pairing_request(&attacker.publish_key_package()?, &secret);
        let tag_start = request.len() - 32;
        request[tag_start..].copy_from_slice(&tag);

        assert!(accept_pairing(&mut host, &request, &secret).is_err());
        Ok(())
    }

    #[test]
    fn a_damaged_or_foreign_request_is_refused() -> Result<()> {
        let mut host = host()?;
        let secret = PairingSecret::generate();
        assert!(accept_pairing(&mut host, b"", &secret).is_err());
        assert!(accept_pairing(&mut host, b"not a leave frame", &secret).is_err());

        let phone = DeviceIdentity::generate("phone")?;
        let mut request = pairing_request(&phone.publish_key_package()?, &secret);
        request.truncate(request.len() / 2);
        assert!(accept_pairing(&mut host, &request, &secret).is_err());
        Ok(())
    }

    #[test]
    fn a_welcome_frame_is_not_mistaken_for_a_request() -> Result<()> {
        let mut host = host()?;
        let secret = PairingSecret::generate();
        let phone = DeviceIdentity::generate("phone")?;
        let request = pairing_request(&phone.publish_key_package()?, &secret);
        let invitation = accept_pairing(&mut host, &request, &secret)?;
        let welcome = pairing_welcome(&invitation);

        assert!(matches!(
            accept_pairing(&mut host, &welcome, &secret),
            Err(CryptoError::KeyPackage(_))
        ));
        assert!(read_pairing_welcome(&request).is_err());
        Ok(())
    }

    #[test]
    fn pairing_frames_are_distinguishable_from_group_traffic() -> Result<()> {
        let mut host = host()?;
        let secret = PairingSecret::generate();
        let phone = DeviceIdentity::generate("phone")?;
        let request = pairing_request(&phone.publish_key_package()?, &secret);
        let invitation = accept_pairing(&mut host, &request, &secret)?;

        assert!(is_pairing_frame(&request));
        assert!(is_pairing_frame(&pairing_welcome(&invitation)));
        assert!(!is_pairing_frame(&host.seal(b"ordinary work")?));
        assert!(!is_pairing_frame(b""));
        Ok(())
    }

    #[test]
    fn the_secret_never_prints_itself() {
        let secret = PairingSecret::generate();
        assert_eq!(format!("{secret:?}"), "PairingSecret(redacted)");
        assert_ne!(secret.expose(), PairingSecret::generate().expose());
    }

    #[test]
    fn a_secret_of_the_wrong_length_is_refused() {
        assert!(PairingSecret::from_bytes(b"short").is_err());
        assert!(PairingSecret::from_bytes(&[7_u8; PAIRING_SECRET_BYTES]).is_ok());
    }
}
