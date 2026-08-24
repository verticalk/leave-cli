# Crypto release gate

Leave's remote transport is disabled in this alpha. The repository does not
claim end-to-end or zero-knowledge encryption yet.

The `leave-crypto` crate reserves the native and WASM boundary for an RFC 9420
OpenMLS implementation; the dependency is added back when that implementation
starts. A maintainer may change the hard-coded release status only after all of
these checks are attached to a signed release record:

1. OpenMLS ships a provider graph without the advisories tracked in upstream
   issue 2126.
2. `cargo audit` and `cargo deny check advisories` pass for the exact lockfile.
3. Native and WASM implementations pass the same golden vectors and interoperate.
4. Browser reload, upgrade, IndexedDB durability, and spent-key deletion tests pass.
5. An independent cryptography review covers protocol use and persistence.

Until then, Leave does not allow its hosted transport. Personal Tailscale Serve
access is a separate tailnet-only path and does not claim MLS or relay
end-to-end encryption. Do not add a temporary cipher, plaintext relay mode, or
a command-line override.

## Current audit evidence

`cargo audit` and `cargo deny check` pass on the current lockfile, so the
second gate condition holds today.

Until 2026-08-24 they did not. The audit reported six high-severity
vulnerabilities, and two of them had no published fix:

| Advisory | Crate | Version | Fixed version |
|---|---|---:|---:|
| RUSTSEC-2026-0209 | `libcrux-aesgcm` | 0.0.7 | No fix published |
| RUSTSEC-2026-0211 | `libcrux-aesgcm` | 0.0.7 | No fix published |
| RUSTSEC-2026-0124 | `libcrux-chacha20poly1305` | 0.0.7 | 0.0.8 or later |
| RUSTSEC-2026-0212 | `libcrux-secrets` | 0.0.5 | 0.0.6 or later |
| RUSTSEC-2026-0207 | `libcrux-sha3` | 0.0.8 | 0.0.10 or later |
| RUSTSEC-2026-0208 | `libcrux-sha3` | 0.0.8 | 0.0.10 or later |

Every one of them arrived through the optional OpenMLS dependency that
`leave-crypto` declared before any code used it. No source file in the
workspace imported `openmls`, and three of the six crates were never compiled
in any configuration: `openmls`'s `libcrux-provider` feature and `hpke-rs`'s
`libcrux` feature are both opt-in, and `openmls_rust_crypto` takes `hpke-rs`
with the RustCrypto backend. They reached `cargo audit` only because the
lockfile records the union of every optional dependency.

The three that did compile under `--features openmls-experimental` could not
have been upgraded from here: `libcrux-traits` 0.0.6 requires
`libcrux-secrets` at exactly `=0.0.5`, so `cargo update --precise` is refused.

Dropping the unused declaration removed 79 packages from the lockfile,
including all six advisories, without changing a line of compiled behaviour.
Re-add the OpenMLS dependency in the same change that starts using it, and
audit the graph as it stands then. That is a hygiene fix, not an exception
list: no advisory was ignored, suppressed, or waived.

## What is missing before the gate matters

The gate protects a transport that does not exist yet. `leave-crypto` contains
the release-status enum and no cryptography. The host has no relay client, the
relay's `hosts`, `workspaces`, `devices`, and `organizations` endpoints are
fixed `503` responses, its WebSocket route serves loopback demo mode only, and
`leave login` and `leave pair` return an error instead of enrolling anything.

Changing the release status to `Passed` would therefore not enable remote
access. It would start a relay that answers every request with `503`, while
asserting evidence that no test or review has produced.
