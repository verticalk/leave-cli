# Architecture

## Trust boundary

The local `leave` host is the only component allowed to authorize workspace
commands or touch repository paths. It runs as the signed-in operating-system
user and launches `devin acp` over stdio. The current alpha listens on loopback.
The hosted design will add an outbound relay connection after its release gates
pass; the host will still never listen on a public interface.

Personal away access uses Tailscale Serve as a separate transport. Leave stays
on loopback, Tailscale terminates tailnet-only HTTPS, and the host accepts only
the exact owner's `Tailscale-User-Login` header. Localhost remains available to
the same operating-system user. Tailscale Funnel is not supported.

The release-gated relay is limited to account and workspace metadata plus
opaque encrypted frames. It cannot authorize a decrypted action. The future
host transport must perform that check again after decryption using the
workspace role and explicit capability grants.

The alpha PWA stores versioned static assets for offline startup. It does not
put prompts, files, terminal scrollback, or decrypted API responses into the
service-worker cache. Encrypted offline event envelopes remain part of the MLS
release work.

```text
mobile PWA
   │  MLS application messages
   ▼
blind relay  ── Postgres metadata / ciphertext retention
   │          └ Redis presence, leases, and horizontal fanout
   ▼
local leave host ── SQLite authoritative event log
   ├─ devin acp over stdio
   ├─ guarded repository filesystem and Git
   ├─ explicitly granted PTYs
   └─ isolated Chromium preview over CDP
```

## Delivery semantics

Every local event receives a monotonic sequence and UUIDv7 identifier before
fanout. Devices reconnect with `after_sequence`; duplicate event IDs are safe.
Commands carry a durable command ID which the host claims before side effects.
Leave offers at-least-once transport, never exactly-once execution.

A disconnect after prompt submission can leave the outcome uncertain. Leave
does not resend that prompt automatically. The device reloads the authoritative
session cursor and asks the user to reconcile the turn.

## Protocol

`proto/leave.proto` is the public wire contract. A relay envelope exposes only
the schema version, routing ID, message ID, and MLS ciphertext. Commands and
events exist inside that ciphertext. Text WebSocket frames are rejected.

The host treats ACP as a local adapter, not Leave's remote sync protocol. It
keeps unknown JSON fields so newer Devin extensions can reach a qualified
adapter instead of disappearing silently.

## Storage defaults

The intended hosted defaults are seven days or 250 MB of relay ciphertext per
workspace, 30 days or 1 GB in the host event log, and a 100 MB encrypted device
LRU. The alpha has no retention worker yet and must not run as a hosted service.
