# Threat model

## Assets

Leave protects repository contents, prompts and model output, local session
history, terminal data, browser frames, secrets, rules, skills, device keys,
workspace membership, and approval decisions.

## Adversaries

- A curious or compromised hosted relay operator
- A network attacker who can delay, duplicate, reorder, or drop frames
- A stolen or revoked phone
- A malicious workspace collaborator with a limited role
- A tailnet member or externally shared device that can reach Tailscale Serve
  but is not the Leave host owner
- A repository containing symlinks, hostile filenames, ANSI control sequences,
  Markdown links, or executable customization
- A compromised local account, which sits outside Leave's containment boundary

## Controls

The status column separates controls enforced by this alpha from controls that
remain release gates for the hosted design.

| Threat | Control | Status |
|---|---|---|
| Relay reads content | MLS application encryption; plaintext schema has no content fields | Hosted release gate |
| Frame replay | UUID event IDs, monotonic cursors, command claims, expiring approvals | Local event and approval paths enforced; hosted replay tests pending |
| Path escape | Owner registration, canonical roots, traversal rejection, symlink-write denial | Enforced |
| Stale edit overwrites agent work | Serialized atomic write, BLAKE3 base hash, explicit conflict result | Enforced |
| Viewer sends commands | Host-side RBAC after decryption | Protocol policy implemented; hosted transport pending |
| Operator opens shell | Independent raw-PTY grant, off by default | Enforced |
| Revoked device reads new work | MLS removal commit and forward key rotation | Hosted release gate |
| Secret leaks through cache | Encrypted-envelope-only IndexedDB; no terminal scrollback | Hosted encrypted cache pending; terminal cache exclusion enforced |
| PWA runs destructive work in background | No queued mutations; push contains generic text | Enforced |
| Browser preview reaches the LAN | Loopback-origin allowlist and isolated profile | Enforced for managed preview |
| Non-owner reaches private away URL | Loopback-only backend plus exact `Tailscale-User-Login` comparison | Enforced; live Tailscale qualification pending |

Private away access trusts Tailscale Serve for tailnet TLS and identity
headers. The Leave backend stays on loopback and compares the normalized login
to the host owner discovered through `tailscale status --json`. Tailscale
Funnel is unsupported because it intentionally removes the tailnet-only
boundary.

## Metadata leakage

The hosted relay can observe account and organization membership, device and
host identifiers, workspace routing identifiers, online state, timing, IP
addresses, frame size, retention use, and coarse operational metrics. Product
copy must state this plainly. "Zero knowledge" remains prohibited until the
external review confirms the implementation and wording.

## Unresolved release risks

- OpenMLS browser persistence and spent-key deletion
- The upstream provider advisory graph
- Passkey account recovery without weakening device approval
- Windows junction and filesystem replacement behavior
- Chromium supply-chain verification and CDP input isolation
- Credentialed compatibility coverage for Cognition-specific ACP extensions
