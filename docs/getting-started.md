# Getting started

## 1. Install Leave

One command from a fresh checkout on macOS, Linux, or WSL2:

```bash
./infra/bootstrap.sh
```

On native Windows, use PowerShell:

```powershell
.\infra\bootstrap.ps1
```

The bootstrap script installs anything missing into your user account only —
the official Rust toolchain and the pinned Node.js release — then builds the
PWA and Rust host, installs them under `$LEAVE_INSTALL_PREFIX`, adds the
**Leave Setup** launcher, and opens it. The Unix default prefix is
`$HOME/.local`; the Windows default is `%LOCALAPPDATA%\Leave`. It asks before
each download. Use `--yes` for an unattended install and `--no-setup` to skip
opening the wizard.

If Rust and Node are already set up the way you want them, run the build steps
directly instead:

```bash
./infra/install-local.sh
```

Candidate archives produced by CI have the same `bin/` and `share/leave/web/`
layout. Public signed installers are not published until the legal and
cryptography release gates pass. Local candidate packages do not bypass those
gates.

## 2. Finish setup in the browser

Leave Setup opens on its own after the bootstrap. Later, open **Leave Setup**
from the Linux applications menu, macOS Applications folder, or Windows Start
menu, or run `leave setup`.

The private local wizard checks this computer and offers to fix whatever is
missing:

- **Devin is not installed** — Leave runs Cognition's published installer
  (`curl -fsSL https://cli.devin.ai/install.sh | bash`) for your user account.
  The wizard shows that exact command before it runs. On Windows, follow
  Cognition's PowerShell quickstart and choose **Check again**.
- **Devin is signed out** — **Sign in to Devin** runs the documented
  `devin auth login` flow, opens or shows the sign-in page Devin asks for,
  and checks the result. The card always shows the exact Devin command
  Leave checked — as a full-path command you can paste into a terminal —
  so you can finish the same sign-in there when nothing opens. Leave does
  not copy Desktop tokens, read private Desktop databases, or parse the
  interactive Devin TUI.
- **Tailscale is signed out** — **Connect Tailscale** runs `tailscale up`,
  opens the sign-in link Tailscale prints, and waits for you to finish.
- **Something failed** — the wizard shows one sentence about what to do next,
  with the tool's raw output behind **What the tool reported**.

Leave searches `LEAVE_DEVIN_BIN`, `PATH`, supported Devin Desktop bundle
locations, and the paths the official installer writes to, so a fresh install
is found without opening a new terminal.

The selected folder is canonicalized before Leave registers it and starts
`devin acp`.

## 3. Open it from a phone

1. Install Tailscale on the computer and phone.
2. Sign both devices into the same account or an ACL policy that lets the phone
   reach the computer.
3. In Leave Setup, enable **Open from my phone** and **Keep Leave running**.
4. Start Leave. The final screen shows a QR code for the private
   `https://...ts.net` address, the tailnet account allowed to open it, and the
   phone steps below.
5. Point the phone camera at the code and tap the link that appears (or copy
   the address and open it in the phone's browser).
6. Install the PWA on the home screen. iPhone or iPad: in Safari, tap Share,
   then **Add to Home Screen**. Android: in Chrome, tap the three-dot menu,
   then **Add to Home screen** or **Install app**.

Skipped phone access during setup? Install Tailscale on both devices, then run
`leave connect . --away` inside the workspace (or run `leave setup` again and
tick **Open from my phone**).

Leave remains bound to `127.0.0.1`. Tailscale terminates HTTPS and adds the
signed-in identity headers. Leave permits the exact host-owner login and denies
other tailnet or shared-device identities. `leave away disable` removes Serve.

Use Tailscale Serve, not Funnel. Funnel is intentionally public and is not a
supported Leave transport.

## 4. Choose sensitive capabilities explicitly

The Access step keeps Terminal, Browser preview, and user-global customization
off by default. Turn on only what the selected workspace needs. The equivalent
advanced command is:

```bash
leave connect /path/to/repo --away \
  --grant-terminal \
  --grant-preview
```

- `--grant-terminal` enables a raw PTY as the logged-in host user for that host
  run. Terminal scrollback is not retained by the PWA.
- `--grant-preview` enables an ephemeral Chromium profile and CDP frames. URLs
  must be HTTP loopback origins. Requests leaving the approved origin are
  blocked in Chromium's Fetch domain.
- Global rules, skills, plugins, hooks, or user-scope MCP changes still require
  `leave connect /path/to/repo --expose-global-customization`. Project skills
  remain limited to Devin's documented project skill directories.

Plugin and MCP mutations require a review checkbox plus an exact typed phrase.
Git mutations are structured commands; Leave never accepts raw Git arguments.

## 5. Keep it running

Enable **Keep Leave running** during setup. Leave installs and starts the native
per-user service for the detected platform:

- Linux: systemd user service
- macOS: LaunchAgent
- Windows: per-user Scheduled Task

The equivalent advanced command is:

```bash
leave connect /path/to/repo --away --background
```

The same command installs a per-user service and starts it. Useful operations:

```bash
leave service status
leave service uninstall
leave away status
leave away disable
leave doctor
```

On native Windows, keep autonomous use attended. WSL2 remains the recommended
boundary for stronger Linux sandbox support.
