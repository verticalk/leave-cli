# Privacy model

Leave's hosted design separates routing metadata from encrypted workspace
content. The alpha does not operate a hosted service.

Private Tailscale access does not use the Leave relay. Tailscale processes the
network and account metadata required to connect the user's devices. Leave
receives the identity headers added by Serve and compares the login to the
locally discovered host owner. As with any encrypted transport, the transport
provider can observe connection metadata and traffic sizes.

## Intended hosted metadata

The service may process account identifiers, organization membership, device
and host identifiers, workspace routing IDs, IP address, online state, frame
timing and size, quota use, and coarse operational diagnostics. Default
ciphertext retention will be seven days or 250 MB per workspace.

## Content the relay must not receive in plaintext

Prompts, responses, repository contents, paths, diffs, Git data, commands,
terminal output, browser frames, rules, skills, secrets, model choices, and
approval details belong inside the encrypted workspace channel.

Product diagnostics are opt-out. The accepted fields are locked in
`leave-protocol::TELEMETRY_ALLOWLIST`; the schema rejects additional keys.
Availability and abuse controls may retain routing metadata needed to keep the
service safe.

Revoking a device stops future access after key rotation. Leave cannot erase
plaintext that an authorized device already displayed or exported.
