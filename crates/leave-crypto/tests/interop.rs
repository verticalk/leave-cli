//! The native build must open a vector the WebAssembly build produced.
//!
//! This is the other half of the release gate's interoperability evidence.
//! `wasm_interop.rs` runs the mirror image: the browser build opening a vector
//! this build produced. Together they show both endpoints read the same bytes
//! the same way, rather than each being self-consistent in isolation.

use leave_crypto::InteropVector;

/// A vector produced by the WebAssembly build, checked into the repository.
/// Regenerate with `tests/wasm_emit_vector.rs`.
const WASM_VECTOR: &[u8] = include_bytes!("fixtures/wasm_vector.bin");

/// The vector the WebAssembly suite checks, so both sides stay in step.
const NATIVE_VECTOR: &[u8] = include_bytes!("fixtures/native_vector.bin");

#[test]
fn the_native_build_opens_a_vector_produced_in_webassembly()
-> Result<(), Box<dyn std::error::Error>> {
    let vector = InteropVector::decode(WASM_VECTOR)?;
    vector.verify()?;
    assert_eq!(vector.expected_sender, "browser-host");
    assert_eq!(vector.expected_plaintext, b"cross build payload");
    Ok(())
}

#[test]
fn the_committed_native_vector_still_verifies() -> Result<(), Box<dyn std::error::Error>> {
    let vector = InteropVector::decode(NATIVE_VECTOR)?;
    vector.verify()?;
    assert_eq!(vector.expected_sender, "native-host");
    Ok(())
}

#[test]
fn the_two_vectors_are_genuinely_from_different_builds() -> Result<(), Box<dyn std::error::Error>> {
    let native = InteropVector::decode(NATIVE_VECTOR)?;
    let wasm = InteropVector::decode(WASM_VECTOR)?;
    assert_ne!(
        native.receiver_state, wasm.receiver_state,
        "the two fixtures must not be copies of one another"
    );
    assert_ne!(native.expected_sender, wasm.expected_sender);
    Ok(())
}
