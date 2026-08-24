//! Print a vector produced by the WebAssembly build.
//!
//! Run this to regenerate `fixtures/wasm_vector.bin`, which the native test
//! suite then verifies. It is behind a feature because it reports the vector
//! through a deliberate failure: the WebAssembly test runner discards ordinary
//! output, and a panic message is the one channel that always reaches stdout.
//!
//! ```text
//! cargo test -p leave-crypto --target wasm32-unknown-unknown \
//!   --features emit-vectors --test wasm_emit_vector
//! ```
//!
//! Copy the hex between the markers and decode it into the fixture.
#![cfg(all(target_arch = "wasm32", feature = "emit-vectors"))]

use leave_crypto::InteropVector;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn emit_a_vector_for_the_native_suite() {
    let vector = InteropVector::produce("browser-host", "native-phone", b"cross build payload")
        .unwrap_or_else(|error| panic!("could not produce a vector: {error}"));
    vector
        .verify()
        .unwrap_or_else(|error| panic!("this build cannot open its own vector: {error}"));
    let encoded = vector
        .encode()
        .unwrap_or_else(|error| panic!("could not encode: {error}"));
    let hex: String = encoded.iter().map(|byte| format!("{byte:02x}")).collect();
    panic!("WASM_VECTOR_BEGIN{hex}WASM_VECTOR_END");
}
