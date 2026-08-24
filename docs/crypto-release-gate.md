# Crypto release gate

Leave's remote transport is disabled in this alpha. The repository does not
claim end-to-end or zero-knowledge encryption yet.

The `leave-crypto` crate reserves the native and WASM boundary for an RFC 9420
OpenMLS implementation. A maintainer may change the hard-coded release status
only after all of these checks are attached to a signed release record:

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

The 2026-08-23 audit of `Cargo.lock` reports six vulnerabilities in the
optional OpenMLS provider graph. Remote operation remains blocked until the
graph is upgraded and a new exact-lockfile audit passes:

| Advisory | Crate | Current version | Fixed version |
|---|---|---:|---:|
| RUSTSEC-2026-0209 | `libcrux-aesgcm` | 0.0.7 | No fix published |
| RUSTSEC-2026-0211 | `libcrux-aesgcm` | 0.0.7 | No fix published |
| RUSTSEC-2026-0124 | `libcrux-chacha20poly1305` | 0.0.7 | 0.0.8 or later |
| RUSTSEC-2026-0212 | `libcrux-secrets` | 0.0.5 | 0.0.6 or later |
| RUSTSEC-2026-0207 | `libcrux-sha3` | 0.0.8 | 0.0.10 or later |
| RUSTSEC-2026-0208 | `libcrux-sha3` | 0.0.8 | 0.0.10 or later |

RustSec also marks `libcrux-aesgcm` and `proc-macro-error2` unmaintained. The
table is an audit snapshot, not an exception list; CI must keep failing while
these records affect the release graph.
