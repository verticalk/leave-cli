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

## Install and set up

Leave installs into your own user account and never needs an administrator.

### The short version

Open a terminal, then run:

```bash
git clone https://github.com/verticalk/leave-cli.git
cd leave-cli
./infra/bootstrap.sh
```

On Windows, use PowerShell:

```powershell
git clone https://github.com/verticalk/leave-cli.git
cd leave-cli
.\infra\bootstrap.ps1
```

The bootstrap script checks this computer, installs anything missing for your
user account only (the official Rust toolchain and Node.js), builds Leave,
adds **Leave Setup** to your applications menu, and opens it. It asks before
each download; pass `--yes` to skip the questions.

After that, the terminal is done. Leave Setup walks through the rest:

1. **Devin** — if the official CLI is missing, Leave runs Cognition's published
   installer for you; if it is signed out, Leave opens Devin's own sign-in.
   Leave never reads or copies Devin credentials.
2. **Workspace** — pick the one folder Leave may access, with your desktop's
   native folder picker.
3. **Access** — private phone access and optional capabilities, each off until
   you turn it on. Leave can start the Tailscale sign-in from this screen.
4. **Start** — Leave runs, and shows a QR code your phone camera can open.

Public signed installers are not published until the cryptography release
gate passes, so Leave builds from this checkout on your own computer.

### Requirements the script handles for you

- Rust 1.98 (installed through the official rustup installer)
- Node 22.12 and pnpm 10.15 (installed through the official Node.js release)

You only need to install Devin yourself if you would rather follow Cognition's
[official CLI quickstart](https://docs.devin.ai/cli) than let Leave do it:

```bash
curl -fsSL https://cli.devin.ai/install.sh | bash
devin auth login
```

If Devin Desktop is installed, its command palette may offer **Install Devin
CLI**. Leave also checks the standard Devin Desktop bundle locations and the
paths the official installer uses. Set `LEAVE_DEVIN_BIN` to an explicit
official CLI path for a nonstandard installation.

### Using Leave from your phone

Phone access runs over your own private Tailscale network. Leave stays bound to
`127.0.0.1` and accepts only the tailnet identity of the computer's owner.

1. Install Tailscale on your phone and sign in with the same account.
2. In Leave Setup, turn on **Open from my phone**. If Tailscale is not
   connected yet, choose **Connect Tailscale** and Leave opens its sign-in.
3. Scan the QR code Leave shows when it finishes, then use Add to Home Screen.

Do not use Tailscale Funnel or expose port 8788 through a router.

### Doing it from the command line instead

```bash
corepack enable
pnpm install --frozen-lockfile
pnpm build
cargo build -p leave
./target/debug/leave setup
```

Advanced users can skip the wizard:

```bash
./target/debug/leave connect /absolute/path/to/repository \
  --away --background --grant-terminal --grant-preview
./target/debug/leave service status
./target/debug/leave away status
```

Add `--expose-global-customization` only when this workspace may read or change
user-global skills, plugins, and MCP configuration. `--background` installs a
macOS LaunchAgent, Linux systemd user unit, or Windows per-user Scheduled Task;
remove it with `leave service uninstall` and drop the tailnet mapping with
`leave away disable`.

Native Windows source installs use `infra/install-local.ps1`. Use WSL2 when you
need the Linux sandbox boundary recommended for autonomous Devin work.

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

## Release gate

Hosted operation requires the security evidence listed in
[the crypto release gate](docs/crypto-release-gate.md).

The project will not hide that gate behind an environment variable.

## License

Leave is licensed under the GNU Affero General Public License v3.0 only.
Contributions use Developer Certificate of Origin sign-off; the project has no
CLA. See [CONTRIBUTING.md](CONTRIBUTING.md).
