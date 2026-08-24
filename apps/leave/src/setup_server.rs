//! Loopback-only first-run setup for people who do not use a terminal.

use anyhow::{Context, bail};
use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use leave_core::WorkspaceRoot;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, process::Command, sync::Mutex};
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

const HOST_PORT: u16 = 8788;
const SETUP_TOKEN_HEADER: &str = "x-leave-setup-token";

#[derive(Clone)]
struct SetupState {
    token: String,
    data_dir: PathBuf,
    web_dir: PathBuf,
    setup_port: u16,
    host_port: u16,
    child: Arc<Mutex<Option<tokio::process::Child>>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupStatus {
    version: &'static str,
    platform: PlatformView,
    devin: ToolView,
    tailscale: ToolView,
    browser: ToolView,
    folder_picker_available: bool,
    workspace_example: String,
    host_port: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformView {
    id: &'static str,
    label: &'static str,
    service_label: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolView {
    installed: bool,
    ready: bool,
    label: String,
    detail: String,
    path: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderSelection {
    path: Option<String>,
    detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
struct LaunchRequest {
    workspace_path: String,
    #[serde(default = "default_host_port")]
    port: u16,
    #[serde(default)]
    away: bool,
    #[serde(default)]
    background: bool,
    #[serde(default)]
    terminal: bool,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    global_customization: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchResult {
    local_url: String,
    away_url: Option<String>,
    workspace_path: String,
    background: bool,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    status: u16,
    message: String,
}

struct SetupError {
    status: StatusCode,
    error: anyhow::Error,
}

impl SetupError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: anyhow::anyhow!(error.to_string()),
        }
    }

    fn internal(error: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.into(),
        }
    }
}

impl IntoResponse for SetupError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: ErrorDetail {
                    status: self.status.as_u16(),
                    message: self.error.to_string(),
                },
            }),
        )
            .into_response()
    }
}

/// Serve the setup application and optionally open it in the system browser.
pub async fn serve(
    data_dir: PathBuf,
    web_dir: PathBuf,
    port: u16,
    host_port: u16,
    open_browser: bool,
) -> anyhow::Result<()> {
    if port == host_port || port == 0 || host_port == 0 {
        bail!("setup and workspace host ports must be different");
    }
    let index = web_dir.join("index.html");
    if !index.is_file() {
        bail!(
            "PWA build not found at {}; run `pnpm --filter @leave/web build` first",
            index.display()
        );
    }
    let token = Uuid::now_v7().to_string();
    let state = SetupState {
        token: token.clone(),
        data_dir,
        web_dir: web_dir.clone(),
        setup_port: port,
        host_port,
        child: Arc::new(Mutex::new(None)),
    };
    let api = Router::new()
        .route("/status", get(status))
        .route("/auth/login", post(auth_login))
        .route("/workspace/select", post(select_workspace))
        .route("/launch", post(launch))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_setup,
        ));
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(index));
    let app = Router::new()
        .nest("/api/v1/setup", api)
        .fallback_service(static_files)
        .layer(middleware::from_fn(security_headers));
    let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = TcpListener::bind(listen).await?;
    let url = format!("http://127.0.0.1:{port}/setup#token={token}");
    println!("Leave setup: {url}");
    if open_browser {
        let browser_url = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(180)).await;
            if let Err(error) = open_url(&browser_url).await {
                tracing::warn!(%error, "could not open the setup browser automatically");
            }
        });
    }
    let shutdown_state = state.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            if let Some(mut child) = shutdown_state.child.lock().await.take()
                && let Err(error) = child.kill().await
            {
                tracing::warn!(%error, "could not stop the foreground workspace host");
            }
        })
        .await?;
    Ok(())
}

async fn status(State(state): State<SetupState>) -> Result<Json<SetupStatus>, SetupError> {
    setup_status(state.host_port)
        .await
        .map(Json)
        .map_err(SetupError::internal)
}

async fn auth_login(State(state): State<SetupState>) -> Result<Json<SetupStatus>, SetupError> {
    let devin = crate::discover_devin_binary()
        .context("Devin was not found. Install Devin Desktop or the official Devin CLI first.")
        .map_err(SetupError::bad_request)?;
    let mut command = Command::new(&devin);
    command
        .args(["auth", "login"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_mins(10), command.output())
        .await
        .map_err(|_| SetupError::bad_request(anyhow::anyhow!("Devin login timed out. Try again.")))?
        .context("could not start the official Devin login")
        .map_err(SetupError::internal)?;
    if !output.status.success() {
        return Err(SetupError::bad_request(command_detail(
            &output.stdout,
            &output.stderr,
        )));
    }
    let next = setup_status(state.host_port)
        .await
        .map_err(SetupError::internal)?;
    if !next.devin.ready {
        return Err(SetupError::bad_request(
            "Devin still reports that this computer is signed out. Finish the browser login, then try again.",
        ));
    }
    Ok(Json(next))
}

async fn select_workspace() -> Result<Json<FolderSelection>, SetupError> {
    select_folder()
        .await
        .map(Json)
        .map_err(SetupError::internal)
}

async fn launch(
    State(state): State<SetupState>,
    Json(request): Json<LaunchRequest>,
) -> Result<Json<LaunchResult>, SetupError> {
    if request.port == 0 || request.port == state.setup_port {
        return Err(SetupError::bad_request(
            "Choose a workspace host port other than the setup port.",
        ));
    }
    let root = WorkspaceRoot::register(request.workspace_path.trim())
        .await
        .context("Choose an existing workspace folder")
        .map_err(SetupError::bad_request)?;
    let current = setup_status(state.host_port)
        .await
        .map_err(SetupError::internal)?;
    if !current.devin.ready {
        return Err(SetupError::bad_request(
            "Connect Devin before starting the workspace.",
        ));
    }
    if request.away && !current.tailscale.ready {
        return Err(SetupError::bad_request(
            "Phone access needs Tailscale signed in on this computer.",
        ));
    }
    if request.preview && !current.browser.ready {
        return Err(SetupError::bad_request(
            "Browser preview needs Chromium or Chrome for Testing on this computer.",
        ));
    }
    if state.child.lock().await.is_some() {
        return Err(SetupError::bad_request(
            "A foreground workspace host is already running from this setup window.",
        ));
    }

    let mut command = workspace_command(&state, &request, root.as_path())
        .context("could not prepare the Leave workspace host")
        .map_err(SetupError::internal)?;

    if request.background {
        let output = command
            .output()
            .await
            .context("could not install the Leave background host")
            .map_err(SetupError::internal)?;
        if !output.status.success() {
            return Err(SetupError::bad_request(command_detail(
                &output.stdout,
                &output.stderr,
            )));
        }
    } else {
        let log_path = state.data_dir.join("workspace-host.log");
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("could not open {}", log_path.display()))
            .map_err(SetupError::internal)?;
        let error_log = log.try_clone().map_err(SetupError::internal)?;
        let child = command
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .spawn()
            .context("could not start the Leave workspace host")
            .map_err(SetupError::internal)?;
        *state.child.lock().await = Some(child);
    }

    wait_for_workspace(request.port, root.as_path(), &state)
        .await
        .map_err(SetupError::bad_request)?;
    let away_url = request.away.then_some(current.tailscale.url).flatten();
    Ok(Json(LaunchResult {
        local_url: format!("http://127.0.0.1:{}", request.port),
        away_url,
        workspace_path: root.as_path().to_string_lossy().into_owned(),
        background: request.background,
    }))
}

fn workspace_command(
    state: &SetupState,
    request: &LaunchRequest,
    root: &Path,
) -> anyhow::Result<Command> {
    let binary = std::env::current_exe().context("could not locate Leave")?;
    let mut command = Command::new(binary);
    command
        .arg("--data-dir")
        .arg(&state.data_dir)
        .arg("connect")
        .arg(root)
        .arg("--port")
        .arg(request.port.to_string())
        .arg("--web-dir")
        .arg(&state.web_dir)
        .stdin(Stdio::null());
    for (enabled, argument) in [
        (request.away, "--away"),
        (request.background, "--background"),
        (request.terminal, "--grant-terminal"),
        (request.preview, "--grant-preview"),
        (
            request.global_customization,
            "--expose-global-customization",
        ),
    ] {
        if enabled {
            command.arg(argument);
        }
    }
    Ok(command)
}

async fn wait_for_workspace(port: u16, root: &Path, state: &SetupState) -> anyhow::Result<()> {
    let endpoint = format!("http://127.0.0.1:{port}/api/v1/local/status");
    for _ in 0..100 {
        if let Ok(response) = reqwest::get(&endpoint).await
            && let Ok(value) = response.json::<Value>().await
            && value
                .pointer("/workspace/canonical_path")
                .and_then(Value::as_str)
                .is_some_and(|path| Path::new(path) == root)
        {
            return Ok(());
        }
        let mut child = state.child.lock().await;
        let foreground_exited = if let Some(process) = child.as_mut() {
            process.try_wait()?.is_some()
        } else {
            false
        };
        if foreground_exited {
            *child = None;
            bail!("Leave could not start the workspace host. Check workspace-host.log.");
        }
        drop(child);
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    bail!("Leave did not become ready in time. Check the service status or workspace-host.log.")
}

async fn setup_status(host_port: u16) -> anyhow::Result<SetupStatus> {
    let devin = devin_status().await;
    let tailscale = tailscale_status().await;
    let browser_path = crate::discover_chrome_binary();
    Ok(SetupStatus {
        version: env!("CARGO_PKG_VERSION"),
        platform: platform(),
        devin,
        tailscale,
        browser: ToolView {
            installed: browser_path.is_some(),
            ready: browser_path.is_some(),
            label: "Browser preview".into(),
            detail: browser_path.as_ref().map_or_else(
                || "Optional. Install Chromium to use managed previews.".into(),
                |_| "Chromium is ready for isolated local previews.".into(),
            ),
            path: browser_path.map(|path| path.to_string_lossy().into_owned()),
            url: None,
        },
        folder_picker_available: folder_picker_available(),
        workspace_example: workspace_example(),
        host_port,
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "could not install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let terminate = async {
            match signal(SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(error) => tracing::error!(%error, "could not install terminate handler"),
            }
        };
        tokio::select! { () = ctrl_c => {}, () = terminate => {} }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}

async fn devin_status() -> ToolView {
    let Some(path) = crate::discover_devin_binary() else {
        return ToolView {
            installed: false,
            ready: false,
            label: "Devin".into(),
            detail: "Install Devin Desktop or the official Devin CLI, then check again.".into(),
            path: None,
            url: Some("https://docs.devin.ai/cli".into()),
        };
    };
    let output = Command::new(&path).args(["auth", "status"]).output().await;
    let (ready, detail) = match output {
        Ok(output) => {
            let detail = command_detail(&output.stdout, &output.stderr);
            let ready =
                output.status.success() && !detail.to_ascii_lowercase().contains("not logged in");
            (ready, detail)
        }
        Err(error) => (false, error.to_string()),
    };
    ToolView {
        installed: true,
        ready,
        label: "Devin".into(),
        detail,
        path: Some(path.to_string_lossy().into_owned()),
        url: None,
    }
}

async fn tailscale_status() -> ToolView {
    let Some(mut command) = crate::away::tailscale_command() else {
        return ToolView {
            installed: false,
            ready: false,
            label: "Phone access".into(),
            detail: "Optional. Install Tailscale on this computer and your phone.".into(),
            path: None,
            url: Some("https://tailscale.com/download".into()),
        };
    };
    let output = command.args(["status", "--json"]).output().await;
    let Ok(output) = output else {
        return ToolView {
            installed: true,
            ready: false,
            label: "Phone access".into(),
            detail: "Tailscale is installed but Leave could not read its connection state.".into(),
            path: None,
            url: None,
        };
    };
    let value = serde_json::from_slice::<Value>(&output.stdout).ok();
    let ready = output.status.success()
        && value
            .as_ref()
            .and_then(|value| value.get("BackendState"))
            .and_then(Value::as_str)
            == Some("Running");
    let dns_name = value
        .as_ref()
        .and_then(|value| value.pointer("/Self/DNSName"))
        .and_then(Value::as_str)
        .map(|value| value.trim_end_matches('.'))
        .filter(|value| !value.is_empty())
        .map(|value| format!("https://{value}"));
    ToolView {
        installed: true,
        ready,
        label: "Phone access".into(),
        detail: if ready {
            "Tailscale is connected and ready for private phone access.".into()
        } else {
            "Open Tailscale and sign in before enabling phone access.".into()
        },
        path: None,
        url: dns_name,
    }
}

async fn authorize_setup(
    State(state): State<SetupState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(SETUP_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    if supplied != Some(state.token.as_str()) {
        return SetupError {
            status: StatusCode::FORBIDDEN,
            error: anyhow::anyhow!("This setup link has expired. Open Leave Setup again."),
        }
        .into_response();
    }
    next.run(request).await
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

async fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open").arg(url).status().await?;
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status().await?;
    #[cfg(windows)]
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()
        .await?;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("opening a browser is not supported on this platform");
    if !status.success() {
        bail!("the operating system could not open the setup URL");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn folder_picker_available() -> bool {
    command_on_path("zenity") || command_on_path("kdialog")
}

#[cfg(any(target_os = "macos", windows))]
const fn folder_picker_available() -> bool {
    true
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
const fn folder_picker_available() -> bool {
    false
}

async fn select_folder() -> anyhow::Result<FolderSelection> {
    #[cfg(target_os = "linux")]
    {
        let command = if command_on_path("zenity") {
            Some((
                "zenity",
                vec![
                    "--file-selection",
                    "--directory",
                    "--title=Choose a Leave workspace",
                ],
            ))
        } else if command_on_path("kdialog") {
            Some((
                "kdialog",
                vec![
                    "--getexistingdirectory",
                    ".",
                    "--title",
                    "Choose a Leave workspace",
                ],
            ))
        } else {
            None
        };
        let Some((program, arguments)) = command else {
            return Ok(FolderSelection {
                path: None,
                detail:
                    "No desktop folder picker was found. Paste the workspace folder path instead."
                        .into(),
            });
        };
        return picker_output(Command::new(program).args(arguments)).await;
    }
    #[cfg(target_os = "macos")]
    {
        return picker_output(Command::new("osascript").args([
            "-e",
            "POSIX path of (choose folder with prompt \"Choose a Leave workspace\")",
        ]))
        .await;
    }
    #[cfg(windows)]
    {
        const SCRIPT: &str = "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; $dialog.Description = 'Choose a Leave workspace'; if ($dialog.ShowDialog() -eq 'OK') { Write-Output $dialog.SelectedPath }";
        return picker_output(Command::new("powershell.exe").args([
            "-NoProfile",
            "-STA",
            "-Command",
            SCRIPT,
        ]))
        .await;
    }
    #[allow(unreachable_code)]
    Ok(FolderSelection {
        path: None,
        detail: "Paste the workspace folder path.".into(),
    })
}

async fn picker_output(command: &mut Command) -> anyhow::Result<FolderSelection> {
    let output = command.output().await?;
    if !output.status.success() {
        return Ok(FolderSelection {
            path: None,
            detail: "Folder selection was cancelled.".into(),
        });
    }
    let path = String::from_utf8(output.stdout)?.trim().to_owned();
    Ok(FolderSelection {
        path: (!path.is_empty()).then_some(path),
        detail: "Folder selected.".into(),
    })
}

fn command_on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .any(|candidate| candidate.is_file())
    })
}

const fn platform() -> PlatformView {
    #[cfg(target_os = "linux")]
    return PlatformView {
        id: "linux",
        label: "Linux",
        service_label: "systemd user service",
    };
    #[cfg(target_os = "macos")]
    return PlatformView {
        id: "macos",
        label: "macOS",
        service_label: "LaunchAgent",
    };
    #[cfg(windows)]
    return PlatformView {
        id: "windows",
        label: "Windows",
        service_label: "Scheduled Task",
    };
    #[allow(unreachable_code)]
    PlatformView {
        id: "unknown",
        label: "This computer",
        service_label: "background service",
    }
}

fn workspace_example() -> String {
    #[cfg(windows)]
    return r"C:\Users\you\Projects\my-app".into();
    #[cfg(target_os = "macos")]
    return "/Users/you/Projects/my-app".into();
    #[cfg(not(any(target_os = "macos", windows)))]
    "/home/you/Projects/my-app".into()
}

const fn default_host_port() -> u16 {
    HOST_PORT
}

fn command_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if stdout.is_empty() {
        String::from_utf8_lossy(stderr).trim().to_owned()
    } else {
        stdout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_has_a_service_label() {
        let platform = platform();
        assert!(!platform.id.is_empty());
        assert!(!platform.service_label.is_empty());
    }

    #[test]
    fn setup_and_host_use_different_default_ports() {
        assert_ne!(8790, default_host_port());
    }
}
