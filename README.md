# Leave CLI

Leave is a mobile workspace for the local Devin agent running on your computer.
It uses Cognition's supported CLI and ACP surfaces, keeps repository access on
the host, and gives phones and tablets a purpose-built workspace for sessions,
approvals, files, Git, terminals, and app previews.

This repository is an early personal-workspace alpha. Public internet relay,
pairing, and account enrollment fail closed until the cryptography and legal
release gates pass. Private away access is available through Tailscale Serve:
Leave still binds to loopback and accepts the host owner's injected Tailscale
identity. The PWA is backed by the host's real SQLite event log and ACP
connection; it does not use demo workspace or session data.

Leave is an independent project. It is not affiliated with, endorsed by, or
sponsored by Cognition. Devin is a trademark of its respective owner.

## What works today

- `leave workspace add/list/remove` with canonical path containment
- Optimistic BLAKE3 file-write primitives with conflict detection
- A durable SQLite event log with replay cursors and command deduplication
- A supervised `devin acp` process using the official Rust ACP SDK over stdio
- Real ACP session creation and resume, streamed messages, tool lifecycle, and
  permission choices
- Append-before-broadcast event streaming with cursor recovery after reconnect
- A loopback-only same-origin REST and WebSocket API
- A responsive installable PWA for live sessions, plans, approvals, and
  durable local activity
- Guarded file browsing and CodeMirror editing with atomic BLAKE3 conflicts
- Structured Git status, diff, stage, unstage, commit, push, branches, and a
  read-only safe worktree inventory
- Persistent cross-platform PTYs behind an explicit runtime grant, with no
  offline scrollback cache
- Ephemeral Chromium previews over CDP, restricted to one approved loopback
  origin and an isolated profile
- Documented Devin rules, skills, plugins, and MCP commands, including typed
  confirmations for executable or destructive changes
- A private browser-based setup guide for Devin login, workspace selection,
  phone access, capability grants, and per-user background installation
- Tailnet-only HTTPS away access restricted to the host owner's identity
- Explicit role and capability authorization rules
- Fail-closed OpenMLS and public-release gates

The public hosted relay is still intentionally unavailable. It is not needed
for personal phone access when the computer and phone share a Tailscale network.

See [Compatibility](docs/compatibility.md) for the tested contract and known
gaps. The checked status in that file is the product claim.

## Repository map

```text
apps/leave             local host daemon and CLI
apps/relay             blind relay and metadata API boundary
apps/web               React PWA
crates/leave-core      local event store and guarded filesystem
crates/leave-crypto    native/WASM crypto release gate
crates/leave-protocol  Protobuf wire types and authorization policy
proto                  public protocol schema
docs                   architecture, threat model, and compatibility
```

## Run the local alpha

Requirements:

- Rust 1.98 with rustfmt and Clippy
- Node 22.12+
- pnpm 10.15
- The official, locally authenticated `devin` CLI

Install Devin using Cognition's [official CLI quickstart](https://docs.devin.ai/cli).
For macOS, Linux, or WSL, the documented installer is:

```bash
curl -fsSL https://cli.devin.ai/install.sh | bash
```

Restart the terminal, then authenticate and confirm the account:

```bash
devin auth login
devin auth status
devin --version
```

Windows users should follow the PowerShell installer in the official quickstart.
Leave does not read or copy Devin credentials; `devin acp` reads the credentials
stored by Devin itself.

If Devin Desktop is installed, its command palette may offer **Install Devin
CLI**, which adds `devin` to `PATH`. Leave also checks the standard Devin
Desktop bundle locations on Linux, macOS, and Windows. Set `LEAVE_DEVIN_BIN`
to an explicit official CLI path for a nonstandard installation.

```bash
corepack enable
pnpm install --frozen-lockfile
pnpm build
cargo build -p leave
```

The easiest setup requires no terminal after installation. Open **Leave Setup**
from the Linux applications menu, macOS Applications folder, or Windows Start
menu. Leave detects the operating system and guides you through:

1. Connecting the supported Devin CLI through Devin's official login.
2. Choosing the only workspace folder Leave may access.
3. Enabling private phone access and optional capabilities.
4. Starting Leave now or installing the native per-user background service.

The setup page is bound to localhost and protected by a fresh private session.
It never reads or copies Devin credentials. The command-line fallback is:

```bash
./target/debug/leave setup
```

Advanced users can still connect directly:

```bash
./target/debug/leave connect /absolute/path/to/repository \
  --grant-terminal --grant-preview
```

Add `--expose-global-customization` only when this workspace may read or change
user-global skills, plugins, and MCP configuration.

For phone access, install and sign in to Tailscale on the computer and phone,
then enable **Open from my phone** in Leave Setup. The direct CLI equivalent is:

```bash
./target/debug/leave connect /absolute/path/to/repository --away
```

Leave configures persistent Tailscale Serve HTTPS while keeping its host on
`127.0.0.1`. It rejects tailnet requests unless Tailscale's identity header
matches the computer owner's login. Do not use Tailscale Funnel or expose port
8788 through a router.

To keep Leave running after the terminal closes and restart it at login:

```bash
./target/debug/leave connect /absolute/path/to/repository \
  --away --background --grant-terminal --grant-preview
./target/debug/leave service status
./target/debug/leave away status
```

This installs a macOS LaunchAgent, Linux systemd user unit, or Windows per-user
Scheduled Task. Remove it with `leave service uninstall`; remove the tailnet
mapping with `leave away disable`.

Native Windows source installs use `infra/install-local.ps1`. The script adds
Leave's per-user `bin` directory to the user PATH. Use WSL2 when you need the
Linux sandbox boundary recommended for autonomous Devin work.

See [Getting started](docs/getting-started.md) for the phone checklist and
capability details.

For an isolated protocol test without Devin credentials, use the deterministic
ACP fixture:

```bash
LEAVE_DATA_DIR=/tmp/leave-fixture-state \
  ./target/debug/leave workspace add /absolute/path/to/repository --json
LEAVE_DATA_DIR=/tmp/leave-fixture-state \
  LEAVE_ACP_COMMAND="node tests/fixtures/mock-acp.mjs" \
  ./target/debug/leave serve --workspace <workspace-uuid>
```

## Verify a checkout

```bash
cargo test --workspace
pnpm check
pnpm test
pnpm build
pnpm --filter @leave/web e2e
```

The browser suite expects Playwright browser binaries and their OS libraries.
Credentialed qualification against Devin latest and the previous supported
release remains a manual release check.

## Release gates

Public distribution and hosted operation require both:

1. Written Cognition approval for the intended third-party integration and
   product language.
2. The security evidence listed in [the crypto release gate](docs/crypto-release-gate.md).

The project will not hide either gate behind an environment variable.

## License

Leave is licensed under the GNU Affero General Public License v3.0 only.
Contributions use Developer Certificate of Origin sign-off; the project has no
CLA. See [CONTRIBUTING.md](CONTRIBUTING.md).
