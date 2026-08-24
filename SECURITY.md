# Security policy

## Supported versions

No public production version exists. Security reports still help the private
alpha and receive priority.

## Reporting

Do not open a public issue for a suspected vulnerability. Send a private GitHub
security advisory to the repository maintainers and include affected commit,
reproduction steps, impact, and any proposed mitigation. Do not include real
credentials, source code, prompts, or terminal output from another person.

The project will acknowledge a complete report within three business days. It
will publish a timeline only after affected users have a safe upgrade path.

## Security invariants

- Remote operation remains disabled until the MLS release gate passes.
- The host never accepts a public inbound connection.
- Devin credentials stay inside the official local credential store.
- The relay never receives a deliberate plaintext content field.
- The host authorizes every decrypted command inside a registered root.
- Raw PTY and global customization require separate, revocable grants.
- Approval IDs expire and cannot be replayed.
- Telemetry accepts only the field allowlist in `leave-protocol`.

Changing an invariant requires a threat-model update and security review in the
same pull request.
