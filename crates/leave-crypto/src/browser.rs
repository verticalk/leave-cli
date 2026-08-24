//! The WebAssembly surface the installed PWA uses.
//!
//! The phone runs the same MLS implementation as the host: one code path, one
//! ciphersuite, one set of tests. Key material stays inside the WebAssembly
//! instance, and only sealed bytes cross into JavaScript, so a bug in the page
//! cannot read a signing key out of a plain array.
//!
//! Everything here is a thin wrapper. Behaviour lives in [`crate::session`] so
//! native and browser endpoints cannot drift apart.

#![cfg(target_arch = "wasm32")]

use crate::{
    error::CryptoError,
    identity::DeviceIdentity,
    pairing::{PairingSecret, pairing_request, read_pairing_welcome},
    session::WorkspaceSession,
};
use wasm_bindgen::prelude::*;

/// Turn an internal error into one a page can display.
fn to_js(error: CryptoError) -> JsError {
    JsError::new(&error.to_string())
}

/// A device that has asked to join a workspace and is waiting for the welcome.
#[wasm_bindgen]
pub struct PendingPairing {
    identity: Option<DeviceIdentity>,
    request: Vec<u8>,
}

#[wasm_bindgen]
impl PendingPairing {
    /// The frame to publish to the relay.
    #[wasm_bindgen(getter, js_name = requestFrame)]
    #[must_use]
    pub fn request_frame(&self) -> Vec<u8> {
        self.request.clone()
    }

    /// Finish pairing with the welcome the host sent back.
    ///
    /// # Errors
    ///
    /// Returns an error when the welcome is malformed, was addressed to
    /// another device, or this pairing was already completed.
    #[wasm_bindgen(js_name = complete)]
    pub fn complete(&mut self, welcome_frame: &[u8]) -> Result<BrowserSession, JsError> {
        let identity = self
            .identity
            .take()
            .ok_or_else(|| JsError::new("this pairing was already completed"))?;
        let welcome = read_pairing_welcome(welcome_frame).map_err(to_js)?;
        let session = WorkspaceSession::join(identity, &welcome).map_err(to_js)?;
        Ok(BrowserSession { inner: session })
    }
}

/// Start pairing this browser with a workspace.
///
/// `pairing_secret` is the value scanned out of the host's QR code.
///
/// # Errors
///
/// Returns an error when the secret is the wrong length or the device's key
/// material cannot be generated.
#[wasm_bindgen(js_name = startPairing)]
pub fn start_pairing(device_id: &str, pairing_secret: &[u8]) -> Result<PendingPairing, JsError> {
    let secret = PairingSecret::from_bytes(pairing_secret).map_err(to_js)?;
    let identity = DeviceIdentity::generate(device_id).map_err(to_js)?;
    let request = pairing_request(&identity.publish_key_package().map_err(to_js)?, &secret);
    Ok(PendingPairing {
        identity: Some(identity),
        request,
    })
}

/// A decrypted frame handed back to the page.
#[wasm_bindgen]
pub struct BrowserMessage {
    sender_device_id: String,
    plaintext: Vec<u8>,
}

#[wasm_bindgen]
impl BrowserMessage {
    /// The device that actually sent this, taken from the MLS credential.
    #[wasm_bindgen(getter, js_name = senderDeviceId)]
    #[must_use]
    pub fn sender_device_id(&self) -> String {
        self.sender_device_id.clone()
    }

    /// The decrypted payload.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn plaintext(&self) -> Vec<u8> {
        self.plaintext.clone()
    }
}

/// One workspace session running inside the browser.
#[wasm_bindgen]
pub struct BrowserSession {
    inner: WorkspaceSession,
}

#[wasm_bindgen]
impl BrowserSession {
    /// Resume a session saved by [`Self::export_state`].
    ///
    /// # Errors
    ///
    /// Returns an error when the saved state is unreadable.
    #[wasm_bindgen(js_name = restore)]
    pub fn restore(state: &[u8]) -> Result<BrowserSession, JsError> {
        Ok(Self {
            inner: WorkspaceSession::restore(state).map_err(to_js)?,
        })
    }

    /// Encrypt one payload for the workspace.
    ///
    /// # Errors
    ///
    /// Returns an error when the group cannot produce a frame.
    #[wasm_bindgen(js_name = seal)]
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, JsError> {
        self.inner.seal(plaintext).map_err(to_js)
    }

    /// Decrypt one frame and report who sent it.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame does not decrypt or authenticate.
    #[wasm_bindgen(js_name = open)]
    pub fn open(&mut self, ciphertext: &[u8]) -> Result<BrowserMessage, JsError> {
        let opened = self.inner.open(ciphertext).map_err(to_js)?;
        Ok(BrowserMessage {
            sender_device_id: opened.sender_device_id,
            plaintext: opened.plaintext,
        })
    }

    /// Apply a membership change published by the host.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame is not a commit this group accepts.
    #[wasm_bindgen(js_name = applyCommit)]
    pub fn apply_commit(&mut self, commit: &[u8]) -> Result<(), JsError> {
        self.inner.apply_commit(commit).map_err(to_js)
    }

    /// Serialize the session so the page can put it in storage.
    ///
    /// The result is secret. A page must seal it before it rests anywhere a
    /// script or another origin could reach.
    ///
    /// # Errors
    ///
    /// Returns an error when the session cannot be serialized.
    #[wasm_bindgen(js_name = exportState)]
    pub fn export_state(&self) -> Result<Vec<u8>, JsError> {
        self.inner.export_state().map_err(to_js)
    }

    /// The group's current epoch.
    #[wasm_bindgen(getter, js_name = epoch)]
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.inner.epoch()
    }

    /// This device's identifier.
    #[wasm_bindgen(getter, js_name = deviceId)]
    #[must_use]
    pub fn device_id(&self) -> String {
        self.inner.device_id().to_owned()
    }
}
