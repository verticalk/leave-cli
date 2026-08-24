//! Write a golden interoperability vector produced by the native build.
//!
//! The WebAssembly test loads the result and must open it identically. Run it
//! with the destination path:
//!
//! ```text
//! cargo run -p leave-crypto --example produce_vector -- \
//!   crates/leave-crypto/tests/fixtures/native_vector.bin
//! ```

use leave_crypto::InteropVector;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: produce_vector <path>");
        return ExitCode::FAILURE;
    };
    match write_vector(&path) {
        Ok(()) => {
            println!("wrote {path}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("could not produce a vector: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write_vector(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let vector = InteropVector::produce("native-host", "browser-phone", b"cross build payload")?;
    // Never publish a vector this build cannot itself open.
    vector.verify()?;
    std::fs::write(path, vector.encode()?)?;
    Ok(())
}
