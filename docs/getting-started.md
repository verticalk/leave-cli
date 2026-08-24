# Getting started

## 1. Install Leave

For a source checkout on macOS, Linux, or WSL2:

```bash
./infra/install-local.sh
```

On native Windows, use PowerShell:

```powershell
.\infra\install-local.ps1
```

The scripts build the PWA and Rust host, then place them under
`$LEAVE_INSTALL_PREFIX`. The Unix default is `$HOME/.local`; the Windows
default is `%LOCALAPPDATA%\Leave`. Candidate archives produced by CI have the
same `bin/` and `share/leave/web/` layout.

Public signed installers are not published until the legal and cryptography
release gates pass. Local candidate packages do not bypass those gates.

## 2. Open Leave Setup

Open **Leave Setup** from the Linux applications menu, macOS Applications
folder, or Windows Start menu. The private local wizard checks this computer,
opens Devin's official login when needed, lets you choose a workspace with the
native folder picker, and shows optional access before anything starts.

If the desktop shortcut is unavailable, use:

```bash
leave setup
```

Leave searches `LEAVE_DEVIN_BIN`, `PATH`, and supported Devin Desktop bundle
locations. If the official CLI is logged out, the **Connect Devin** button runs
the documented `devin auth login` flow and checks its result. The selected
folder is canonicalized before Leave registers it and starts `devin acp`.

Leave does not copy Desktop tokens, read private Desktop databases, or parse
the interactive Devin TUI.

## 3. Open it from a phone

1. Install Tailscale on the computer and phone.
2. Sign both devices into the same account or an ACL policy that lets the phone
   reach the computer.
3. In Leave Setup, enable **Open from my phone** and **Keep Leave running**.
4. Start Leave, then open the displayed `https://...ts.net` URL on the phone
   and install the PWA.

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
