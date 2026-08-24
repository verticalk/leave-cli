//! The WebAssembly build must open a vector the native build produced.
//!
//! This is one half of the release gate's interoperability evidence. The other
//! half lives in `interop.rs`, where the native build opens a vector this build
//! produced.
#![cfg(target_arch = "wasm32")]

use leave_crypto::InteropVector;
use wasm_bindgen_test::wasm_bindgen_test;

/// A vector produced by the native build, checked into the repository.
const NATIVE_VECTOR: &[u8] = include_bytes!("fixtures/native_vector.bin");

#[wasm_bindgen_test]
fn the_browser_build_opens_a_natively_produced_vector() {
    let vector = InteropVector::decode(NATIVE_VECTOR)
        .unwrap_or_else(|error| panic!("could not decode the native vector: {error}"));
    vector
        .verify()
        .unwrap_or_else(|error| panic!("the native vector failed in WebAssembly: {error}"));
}

#[wasm_bindgen_test]
fn the_browser_build_pairs_and_exchanges_work() {
    let mut host = leave_crypto::WorkspaceSession::create(
        leave_crypto::DeviceIdentity::generate("host")
            .unwrap_or_else(|error| panic!("host identity: {error}")),
        "workspace-1",
    )
    .unwrap_or_else(|error| panic!("host session: {error}"));
    let secret = leave_crypto::PairingSecret::generate();

    let mut pending = leave_crypto::browser::start_pairing("browser-phone", secret.expose())
        .unwrap_or_else(|_| panic!("the browser could not start pairing"));
    let invitation = leave_crypto::accept_pairing(&mut host, &pending.request_frame(), &secret)
        .unwrap_or_else(|error| panic!("the host refused the browser: {error}"));
    let mut phone = pending
        .complete(&leave_crypto::pairing_welcome(&invitation))
        .unwrap_or_else(|_| panic!("the browser could not finish pairing"));

    let frame = host
        .seal(b"approve the deploy")
        .unwrap_or_else(|error| panic!("seal: {error}"));
    let opened = phone
        .open(&frame)
        .unwrap_or_else(|_| panic!("the browser could not open workspace traffic"));
    assert_eq!(opened.plaintext(), b"approve the deploy");
    assert_eq!(opened.sender_device_id(), "host");
}

#[wasm_bindgen_test]
fn a_browser_session_survives_being_stored_and_reloaded() {
    let mut host = leave_crypto::WorkspaceSession::create(
        leave_crypto::DeviceIdentity::generate("host")
            .unwrap_or_else(|error| panic!("host identity: {error}")),
        "workspace-1",
    )
    .unwrap_or_else(|error| panic!("host session: {error}"));
    let secret = leave_crypto::PairingSecret::generate();
    let mut pending = leave_crypto::browser::start_pairing("browser-phone", secret.expose())
        .unwrap_or_else(|_| panic!("start pairing"));
    let invitation = leave_crypto::accept_pairing(&mut host, &pending.request_frame(), &secret)
        .unwrap_or_else(|error| panic!("accept pairing: {error}"));
    let phone = pending
        .complete(&leave_crypto::pairing_welcome(&invitation))
        .unwrap_or_else(|_| panic!("complete pairing"));

    let state = phone
        .export_state()
        .unwrap_or_else(|_| panic!("export state"));
    drop(phone);

    // A reload of the installed page must not require pairing again.
    let mut restored = leave_crypto::browser::BrowserSession::restore(&state)
        .unwrap_or_else(|_| panic!("restore state"));
    let frame = host
        .seal(b"still paired after a reload")
        .unwrap_or_else(|error| panic!("seal: {error}"));
    let opened = restored
        .open(&frame)
        .unwrap_or_else(|_| panic!("the reloaded session could not read the workspace"));
    assert_eq!(opened.plaintext(), b"still paired after a reload");
}
