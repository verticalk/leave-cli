//! Local Leave host daemon and owner-facing command-line interface.

mod acp;
mod away;
mod customization;
mod git;
mod local_server;
mod preview;
mod service;
mod setup_server;
mod terminal;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use leave_core::{EventStore, WorkspaceRecord, WorkspaceRoot};
use leave_crypto::CryptoReleaseStatus;
use serde::Serialize;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    process::Stdio,
};
use tokio::process::Command;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "leave",
    version,
    about = "Use your local Devin workspace from another device"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandName,
    /// Print machine-readable command results.
    #[arg(long, global = true)]
    json: bool,
    /// Override Leave's local state directory.
    #[arg(long, global = true, env = "LEAVE_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum CommandName {
    /// Authenticate a hosted account after the remote security gate passes.
    Login,
    /// Pair a trusted device after the remote security gate passes.
    Pair,
    /// Authenticate Devin, register a workspace, and start Leave in one guided command.
    Connect(ConnectArgs),
    /// Open the guided first-run setup application.
    Setup(SetupArgs),
    /// Manage locally approved workspace roots.
    Workspace(WorkspaceArgs),
    /// Run the local host and supervise Devin ACP.
    Serve(ServeArgs),
    /// Inspect or disable private Tailscale away access.
    Away(AwayArgs),
    /// Install or inspect Leave as a per-user background host service.
    Service(ServiceArgs),
    /// Show local readiness without exposing secrets.
    Status,
    /// Run installation and compatibility checks.
    Doctor,
    /// Manage the isolated browser used for local previews.
    Preview(PreviewArgs),
    /// Open an existing session in Devin for an official cloud handoff.
    Handoff(HandoffArgs),
}

#[derive(Debug, Args)]
struct SetupArgs {
    /// Loopback port for the setup application.
    #[arg(long, default_value_t = 8790)]
    port: u16,
    /// Default loopback port for the workspace host created by setup.
    #[arg(long, default_value_t = 8788)]
    host_port: u16,
    /// Production PWA build directory.
    #[arg(long, env = "LEAVE_WEB_DIR")]
    web_dir: Option<PathBuf>,
    /// Keep the default browser closed and print the setup URL instead.
    #[arg(long)]
    no_open: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
struct ConnectArgs {
    /// Repository or workspace directory to approve.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Display name for a newly registered workspace.
    #[arg(long)]
    name: Option<String>,
    /// Make the workspace reachable only inside the owner's Tailscale network.
    #[arg(long)]
    away: bool,
    /// Install and start Leave as a per-user background service after setup.
    #[arg(long)]
    background: bool,
    /// Loopback port used behind Tailscale Serve.
    #[arg(long, default_value_t = 8788)]
    port: u16,
    /// Production PWA build directory.
    #[arg(long, env = "LEAVE_WEB_DIR")]
    web_dir: Option<PathBuf>,
    /// Explicitly grant raw PTY access for this host run.
    #[arg(long)]
    grant_terminal: bool,
    /// Explicitly grant an ephemeral managed browser for this host run.
    #[arg(long)]
    grant_preview: bool,
    /// Opt in to user-global skills, plugins, and MCP configuration.
    #[arg(long)]
    expose_global_customization: bool,
}

#[derive(Debug, Args)]
struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommand,
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    /// Add one canonical local directory.
    Add {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        expose_history: bool,
        #[arg(long, default_value_t = true)]
        expose_project_customization: bool,
        #[arg(long)]
        expose_global_customization: bool,
    },
    /// List registered workspaces.
    List,
    /// Remove the registration without touching repository files.
    Remove { id: Uuid },
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
struct ServeArgs {
    /// Workspace UUID returned by `leave workspace list`.
    #[arg(long)]
    workspace: Uuid,
    /// Request internet-routed operation. This currently fails closed.
    #[arg(long)]
    remote: bool,
    /// Publish the loopback host through owner-restricted Tailscale Serve.
    #[arg(long)]
    away: bool,
    /// Loopback port for the local PWA and API.
    #[arg(long, default_value_t = 8788)]
    port: u16,
    /// Production PWA build directory.
    #[arg(long, env = "LEAVE_WEB_DIR")]
    web_dir: Option<PathBuf>,
    /// ACP agent command. Arguments are parsed without invoking a shell.
    #[arg(long, env = "LEAVE_ACP_COMMAND")]
    acp_command: Option<String>,
    /// Explicit official Devin CLI path, persisted by background setup.
    #[arg(long, env = "LEAVE_DEVIN_BIN")]
    devin_binary: Option<PathBuf>,
    /// Explicitly grant raw PTY access for this host run.
    #[arg(long)]
    grant_terminal: bool,
    /// Explicitly grant an ephemeral managed browser for this host run.
    #[arg(long)]
    grant_preview: bool,
    /// Override the managed Chromium executable.
    #[arg(long, env = "LEAVE_CHROME_BIN")]
    chrome_binary: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct AwayArgs {
    #[command(subcommand)]
    command: AwayCommand,
}

#[derive(Debug, Subcommand)]
enum AwayCommand {
    /// Show the current Tailscale Serve mapping.
    Status,
    /// Stop publishing Leave through Tailscale Serve.
    Disable,
}

#[derive(Debug, Args)]
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Install and start a per-user host service.
    Install {
        /// Registered workspace UUID.
        #[arg(long)]
        workspace: Uuid,
        /// Use owner-restricted Tailscale away access.
        #[arg(long)]
        away: bool,
        /// Persist an explicit raw PTY grant.
        #[arg(long)]
        grant_terminal: bool,
        /// Persist an explicit ephemeral browser grant.
        #[arg(long)]
        grant_preview: bool,
        /// Loopback port for the background host.
        #[arg(long, default_value_t = 8788)]
        port: u16,
        /// Packaged PWA directory.
        #[arg(long, env = "LEAVE_WEB_DIR")]
        web_dir: Option<PathBuf>,
    },
    /// Show operating-system service state.
    Status,
    /// Stop and remove the per-user service registration.
    Uninstall,
}

#[derive(Debug, Args)]
struct PreviewArgs {
    #[command(subcommand)]
    command: PreviewCommand,
}

#[derive(Debug, Subcommand)]
enum PreviewCommand {
    /// Show the pinned browser installation state.
    Install,
}

#[derive(Debug, Args)]
struct HandoffArgs {
    /// Official Devin local session identifier.
    session_id: String,
}

#[derive(Debug, Serialize)]
struct StatusReport {
    version: &'static str,
    data_directory: PathBuf,
    devin: Check,
    devin_auth: Check,
    remote_crypto_gate: CryptoReleaseStatus,
    workspace_count: usize,
}

#[derive(Debug, Serialize)]
struct Check {
    ok: bool,
    detail: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("leave=info")),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let paths = AppPaths::discover(cli.data_dir)?;
    tokio::fs::create_dir_all(&paths.data_dir).await?;
    let store = EventStore::open(&paths.database).await?;

    match cli.command {
        CommandName::Login | CommandName::Pair => {
            bail!("remote enrollment is disabled until the OpenMLS release gate passes")
        }
        CommandName::Connect(args) => connect(&store, args, &paths.data_dir).await?,
        CommandName::Setup(args) => {
            setup_server::serve(
                paths.data_dir.clone(),
                args.web_dir.unwrap_or_else(discover_web_directory),
                args.port,
                args.host_port,
                !args.no_open,
            )
            .await?;
        }
        CommandName::Workspace(args) => workspace_command(&store, args, cli.json).await?,
        CommandName::Serve(args) => serve(&store, args).await?,
        CommandName::Away(args) => away_command(args, cli.json).await?,
        CommandName::Service(args) => service_command(args, cli.json, &paths.data_dir).await?,
        CommandName::Status => print_status(&store, &paths, cli.json).await?,
        CommandName::Doctor => doctor(&store, &paths, cli.json).await?,
        CommandName::Preview(args) => preview_command(&args)?,
        CommandName::Handoff(args) => handoff(args).await?,
    }
    Ok(())
}

async fn workspace_command(
    store: &EventStore,
    args: WorkspaceArgs,
    json: bool,
) -> anyhow::Result<()> {
    match args.command {
        WorkspaceCommand::Add {
            path,
            name,
            expose_history,
            expose_project_customization,
            expose_global_customization,
        } => {
            let root = WorkspaceRoot::register(&path).await?;
            let record = WorkspaceRecord {
                id: Uuid::now_v7(),
                name: name.unwrap_or_else(|| {
                    root.as_path().file_name().map_or_else(
                        || "workspace".into(),
                        |value| value.to_string_lossy().into_owned(),
                    )
                }),
                canonical_path: root.as_path().to_path_buf(),
                expose_history,
                expose_project_customization,
                expose_global_customization,
            };
            store.upsert_workspace(&record).await?;
            print_value(&record, json)?;
        }
        WorkspaceCommand::List => print_value(&store.list_workspaces().await?, json)?,
        WorkspaceCommand::Remove { id } => {
            let removed = store.remove_workspace(id).await?;
            print_value(&serde_json::json!({ "id": id, "removed": removed }), json)?;
        }
    }
    Ok(())
}

async fn serve(store: &EventStore, args: ServeArgs) -> anyhow::Result<()> {
    if args.remote {
        leave_crypto::require_remote_release()?;
    }
    let workspace = store
        .list_workspaces()
        .await?
        .into_iter()
        .find(|workspace| workspace.id == args.workspace)
        .context("workspace is not registered")?;
    if args.remote && args.away {
        bail!("--remote and --away are different transports and cannot be combined");
    }
    let devin_binary = args
        .devin_binary
        .filter(|path| path.is_file())
        .or_else(discover_devin_binary)
        .context("Devin CLI was not found; install Devin Desktop or set LEAVE_DEVIN_BIN")?;
    let acp_command = args.acp_command.unwrap_or_else(|| {
        serde_json::json!({"command": devin_binary.to_string_lossy(), "args": ["acp"]}).to_string()
    });
    let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), args.port);
    let access = if args.away {
        let away = away::enable(args.port).await?;
        println!("Leave away access: {}", away.url);
        local_server::HostAccess::Tailnet {
            owner_login: away.owner_login,
            url: away.url,
        }
    } else {
        local_server::HostAccess::Local
    };
    let chrome_binary = args
        .chrome_binary
        .filter(|path| path.is_file())
        .or_else(discover_chrome_binary);
    if args.grant_preview && chrome_binary.is_none() {
        bail!("preview was granted but Chromium was not found; set LEAVE_CHROME_BIN");
    }
    local_server::serve_local(
        store.clone(),
        workspace,
        listen,
        args.web_dir.unwrap_or_else(discover_web_directory),
        acp_command,
        devin_binary,
        local_server::LocalServeConfig {
            access,
            terminal_granted: args.grant_terminal,
            preview_granted: args.grant_preview,
            chrome_binary,
        },
    )
    .await
}

async fn connect(
    store: &EventStore,
    args: ConnectArgs,
    data_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let devin = discover_devin_binary()
        .context("Devin CLI was not found; install Devin Desktop or set LEAVE_DEVIN_BIN")?;
    let auth = devin_auth_check(&devin).await;
    if !auth.ok {
        println!("Opening the official Devin login flow…");
        let status = Command::new(&devin)
            .args(["auth", "login"])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await
            .context("could not start Devin authentication")?;
        if !status.success() || !devin_auth_check(&devin).await.ok {
            bail!("Devin authentication did not complete; run `devin auth login` and try again");
        }
    }
    let root = WorkspaceRoot::register(&args.path).await?;
    let existing = store
        .list_workspaces()
        .await?
        .into_iter()
        .find(|workspace| workspace.canonical_path == root.as_path());
    let workspace = if let Some(mut workspace) = existing {
        if args.expose_global_customization && !workspace.expose_global_customization {
            workspace.expose_global_customization = true;
            store.upsert_workspace(&workspace).await?;
        }
        workspace
    } else {
        let workspace = WorkspaceRecord {
            id: Uuid::now_v7(),
            name: args.name.unwrap_or_else(|| {
                root.as_path().file_name().map_or_else(
                    || "workspace".into(),
                    |value| value.to_string_lossy().into_owned(),
                )
            }),
            canonical_path: root.as_path().to_path_buf(),
            expose_history: false,
            expose_project_customization: true,
            expose_global_customization: args.expose_global_customization,
        };
        store.upsert_workspace(&workspace).await?;
        workspace
    };
    println!("Connected Devin to workspace {}.", workspace.name);
    if args.background {
        let chrome_binary = args.grant_preview.then(discover_chrome_binary).flatten();
        if args.grant_preview && chrome_binary.is_none() {
            bail!("preview was granted but Chromium was not found; set LEAVE_CHROME_BIN");
        }
        let detail = service::install(&service::ServiceInstall {
            workspace: workspace.id,
            web_dir: args.web_dir.unwrap_or_else(discover_web_directory),
            data_dir: data_dir.to_path_buf(),
            devin_binary: devin,
            chrome_binary,
            port: args.port,
            away: args.away,
            terminal: args.grant_terminal,
            preview: args.grant_preview,
        })
        .await?;
        println!("{detail}");
        if args.away {
            println!("Run `leave away status` to see the private phone URL.");
        }
        return Ok(());
    }
    serve(
        store,
        ServeArgs {
            workspace: workspace.id,
            remote: false,
            away: args.away,
            port: args.port,
            web_dir: args.web_dir,
            acp_command: None,
            devin_binary: Some(devin),
            grant_terminal: args.grant_terminal,
            grant_preview: args.grant_preview,
            chrome_binary: None,
        },
    )
    .await
}

async fn away_command(args: AwayArgs, json: bool) -> anyhow::Result<()> {
    match args.command {
        AwayCommand::Status => print_value(&away::status().await?, json),
        AwayCommand::Disable => {
            let detail = away::disable().await?;
            print_value(
                &serde_json::json!({"disabled": true, "detail": detail}),
                json,
            )
        }
    }
}

async fn service_command(
    args: ServiceArgs,
    json: bool,
    data_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let detail = match args.command {
        ServiceCommand::Install {
            workspace,
            away,
            grant_terminal,
            grant_preview,
            port,
            web_dir,
        } => {
            let devin_binary = discover_devin_binary()
                .context("Devin CLI was not found; install Devin Desktop or set LEAVE_DEVIN_BIN")?;
            let chrome_binary = grant_preview.then(discover_chrome_binary).flatten();
            if grant_preview && chrome_binary.is_none() {
                bail!("preview was granted but Chromium was not found; set LEAVE_CHROME_BIN");
            }
            service::install(&service::ServiceInstall {
                workspace,
                web_dir: web_dir.unwrap_or_else(discover_web_directory),
                data_dir: data_dir.to_path_buf(),
                devin_binary,
                chrome_binary,
                port,
                away,
                terminal: grant_terminal,
                preview: grant_preview,
            })
            .await?
        }
        ServiceCommand::Status => service::status().await?,
        ServiceCommand::Uninstall => service::uninstall().await?,
    };
    print_value(&serde_json::json!({"detail": detail}), json)
}

async fn print_status(store: &EventStore, paths: &AppPaths, json: bool) -> anyhow::Result<()> {
    let report = status_report(store, paths).await?;
    print_value(&report, json)
}

async fn doctor(store: &EventStore, paths: &AppPaths, json: bool) -> anyhow::Result<()> {
    let report = status_report(store, paths).await?;
    if json {
        print_value(&report, true)?;
    } else {
        print_doctor(&report);
    }
    if !report.devin.ok {
        bail!("Leave cannot find the official Devin command on this computer")
    }
    if !report.devin_auth.ok {
        bail!("Devin is installed but signed out on this computer")
    }
    Ok(())
}

/// Print the checks as a short list with the next step for anything failing.
fn print_doctor(report: &StatusReport) {
    println!("Leave {}", report.version);
    println!();
    print_check(
        "Devin command",
        &report.devin,
        "Open Leave Setup and choose Install Devin, or follow https://docs.devin.ai/cli",
    );
    print_check(
        "Devin sign-in",
        &report.devin_auth,
        "Run `devin auth login`, or choose Sign in to Devin in Leave Setup",
    );
    println!();
    println!("Workspaces registered: {}", report.workspace_count);
    println!("Data folder: {}", report.data_directory.display());
    if !report.remote_crypto_gate.allows_remote_release() {
        println!(
            "Public internet relay: off. Private phone access uses your own Tailscale network."
        );
    }
}

fn print_check(label: &str, check: &Check, next_step: &str) {
    let state = if check.ok { "ok" } else { "needs attention" };
    println!("[{state}] {label}");
    if !check.detail.trim().is_empty() {
        println!(
            "        {}",
            check.detail.trim().replace('\n', "\n        ")
        );
    }
    if !check.ok {
        println!("        Next: {next_step}");
    }
}

async fn status_report(store: &EventStore, paths: &AppPaths) -> anyhow::Result<StatusReport> {
    let (devin, devin_auth) = if let Some(binary) = discover_devin_binary() {
        (
            command_check(&binary, &["--version"]).await,
            devin_auth_check(&binary).await,
        )
    } else {
        let missing = Check {
            ok: false,
            detail: "Devin CLI was not found on PATH or in the Devin Desktop bundle".into(),
        };
        (
            missing,
            Check {
                ok: false,
                detail: "Authentication cannot be checked until Devin CLI is available".into(),
            },
        )
    };
    Ok(StatusReport {
        version: env!("CARGO_PKG_VERSION"),
        data_directory: paths.data_dir.clone(),
        devin,
        devin_auth,
        remote_crypto_gate: CryptoReleaseStatus::current(),
        workspace_count: store.list_workspaces().await?.len(),
    })
}

fn preview_command(args: &PreviewArgs) -> anyhow::Result<()> {
    match args.command {
        PreviewCommand::Install => {
            if let Some(binary) = discover_chrome_binary() {
                println!(
                    "Chromium is available at {}. Leave will use an ephemeral profile when --grant-preview is set.",
                    binary.display()
                );
                Ok(())
            } else {
                bail!(
                    "Chromium was not found. Automatic Chrome for Testing download remains disabled because Google's official manifest does not include signed checksums; install Chromium or set LEAVE_CHROME_BIN"
                )
            }
        }
    }
}

async fn handoff(args: HandoffArgs) -> anyhow::Result<()> {
    let devin = discover_devin_binary().context("Devin CLI was not found")?;
    let status = Command::new(devin)
        .arg("--resume")
        .arg(&args.session_id)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to open the official Devin CLI session")?;
    if !status.success() {
        bail!("Devin exited with {status}")
    }
    Ok(())
}

async fn command_check(command: &std::path::Path, arguments: &[&str]) -> Check {
    match Command::new(command).args(arguments).output().await {
        Ok(output) if output.status.success() => Check {
            ok: true,
            detail: command_output(&output.stdout, &output.stderr),
        },
        Ok(output) => Check {
            ok: false,
            detail: command_output(&output.stdout, &output.stderr),
        },
        Err(error) => Check {
            ok: false,
            detail: error.to_string(),
        },
    }
}

async fn devin_auth_check(command: &std::path::Path) -> Check {
    let mut check = command_check(command, &["auth", "status"]).await;
    if check.ok && check.detail.to_ascii_lowercase().contains("not logged in") {
        check.ok = false;
    }
    check
}

fn command_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if !stdout.is_empty() {
        return stdout;
    }
    String::from_utf8_lossy(stderr).trim().to_owned()
}

fn discover_devin_binary() -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os("LEAVE_DEVIN_BIN") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Some(path);
        }
    }

    let executable_name = if cfg!(windows) { "devin.exe" } else { "devin" };
    if let Some(paths) = std::env::var_os("PATH")
        && let Some(path) = std::env::split_paths(&paths)
            .map(|directory| directory.join(executable_name))
            .find(|candidate| candidate.is_file())
    {
        return Some(path);
    }

    #[cfg(target_os = "linux")]
    {
        let desktop_bundle = PathBuf::from(
            "/usr/share/devin-desktop/resources/app/extensions/windsurf/devin/bin/devin",
        );
        if desktop_bundle.is_file() {
            return Some(desktop_bundle);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let relative = "Contents/Resources/app/extensions/windsurf/devin/bin/devin";
        let mut applications = vec![PathBuf::from("/Applications/Devin.app")];
        if let Some(home) = dirs::home_dir() {
            applications.push(home.join("Applications/Devin.app"));
        }
        for application in applications {
            let candidate = application.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    #[cfg(windows)]
    for root in ["LOCALAPPDATA", "ProgramFiles", "ProgramW6432"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
    {
        for relative in [
            "Programs/Devin/resources/app/extensions/windsurf/devin/bin/devin.exe",
            "Devin/resources/app/extensions/windsurf/devin/bin/devin.exe",
            "devin-desktop/resources/app/extensions/windsurf/devin/bin/devin.exe",
        ] {
            let candidate = root.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // The official installer writes into the user's home before the shell that
    // started Leave has picked up the new PATH entry.
    if let Some(home) = dirs::home_dir() {
        for relative in DEVIN_HOME_INSTALL_PATHS {
            let candidate = home.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Locations the official installer uses inside the user's home directory.
#[cfg(windows)]
const DEVIN_HOME_INSTALL_PATHS: &[&str] = &[
    ".devin\\bin\\devin.exe",
    ".local\\bin\\devin.exe",
    "AppData\\Local\\devin\\bin\\devin.exe",
];

/// Locations the official installer uses inside the user's home directory.
#[cfg(not(windows))]
const DEVIN_HOME_INSTALL_PATHS: &[&str] = &[
    ".devin/bin/devin",
    ".local/bin/devin",
    ".local/share/devin/bin/devin",
];

fn discover_chrome_binary() -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os("LEAVE_CHROME_BIN") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Some(path);
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["chrome.exe", "chromium.exe"]
    } else {
        &["google-chrome", "chromium", "chromium-browser"]
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            if let Some(binary) = names
                .iter()
                .map(|name| directory.join(name))
                .find(|candidate| candidate.is_file())
            {
                return Some(binary);
            }
        }
    }
    #[cfg(target_os = "linux")]
    for candidate in ["/usr/bin/chromium", "/usr/bin/chromium-browser"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    #[cfg(target_os = "macos")]
    for candidate in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    #[cfg(windows)]
    for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
    {
        for relative in [
            "Google/Chrome/Application/chrome.exe",
            "Chromium/Application/chromium.exe",
        ] {
            let candidate = root.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn discover_web_directory() -> PathBuf {
    if let Ok(executable) = std::env::current_exe()
        && let Some(prefix) = executable.parent().and_then(std::path::Path::parent)
    {
        let installed = prefix.join("share").join("leave").join("web");
        if installed.join("index.html").is_file() {
            return installed;
        }
    }
    if let Some(data) = dirs::data_local_dir() {
        let installed = data.join("leave").join("web");
        if installed.join("index.html").is_file() {
            return installed;
        }
    }
    PathBuf::from("apps/web/dist")
}

fn print_value(value: &impl Serialize, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        let value = serde_json::to_value(value)?;
        match value {
            serde_json::Value::Array(items) if items.is_empty() => {
                println!("No workspaces registered.");
            }
            _ => println!("{}", serde_json::to_string_pretty(&value)?),
        }
    }
    Ok(())
}

struct AppPaths {
    data_dir: PathBuf,
    database: PathBuf,
}

impl AppPaths {
    fn discover(override_directory: Option<PathBuf>) -> anyhow::Result<Self> {
        let data_dir = match override_directory {
            Some(path) => path,
            None => dirs::data_local_dir()
                .context("operating system did not provide a local data directory")?
                .join("leave"),
        };
        let data_dir =
            std::path::absolute(data_dir).context("could not resolve the Leave data directory")?;
        let database = data_dir.join("host.db");
        Ok(Self { data_dir, database })
    }
}
