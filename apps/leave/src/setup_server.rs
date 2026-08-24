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
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::TcpListener,
    process::Command,
    sync::{Mutex, mpsc},
};
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

const HOST_PORT: u16 = 8788;
const SETUP_TOKEN_HEADER: &str = "x-leave-setup-token";
/// Cognition's documented CLI quickstart.
const DEVIN_DOCS_URL: &str = "https://docs.devin.ai/cli";
/// The installer command published in Cognition's quickstart.
const DEVIN_INSTALL_COMMAND: &str = "curl -fsSL https://cli.devin.ai/install.sh | bash";
const TAILSCALE_DOWNLOAD_URL: &str = "https://tailscale.com/download";
/// Tailscale's documented Linux installer, which needs administrator rights.
const TAILSCALE_INSTALL_COMMAND: &str = "curl -fsSL https://tailscale.com/install.sh | sh";
/// How long the wizard waits for Tailscale to print a sign-in link.
const TAILSCALE_LINK_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct SetupState {
    token: String,
    data_dir: PathBuf,
    web_dir: PathBuf,
    setup_port: u16,
    host_port: u16,
    child: Arc<Mutex<Option<tokio::process::Child>>>,
    tailscale_login: Arc<Mutex<Option<tokio::process::Child>>>,
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
    /// Account this computer is signed in as, when the tool reports one.
    account: Option<String>,
    /// What Leave itself can do about this requirement.
    action: Option<ToolAction>,
    /// Command the person can run instead, when Leave cannot do it for them.
    manual_command: Option<String>,
}

/// A guided step Leave can run on the person's behalf from the wizard.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolAction {
    /// Endpoint the wizard calls.
    id: &'static str,
    /// Button label.
    label: String,
    /// Exact command Leave will run, shown before anything happens.
    command: String,
    /// One sentence describing the consequence.
    detail: String,
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
    /// Tailnet account allowed to open the away URL.
    away_owner: Option<String>,
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
    /// Raw tool output, shown only when someone opens the details.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

struct SetupError {
    status: StatusCode,
    error: anyhow::Error,
    detail: Option<String>,
}

impl SetupError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: anyhow::anyhow!(error.to_string()),
            detail: None,
        }
    }

    fn internal(error: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.into(),
            detail: None,
        }
    }

    /// A sentence the person can act on, keeping the tool output for details.
    fn advice(message: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            status: StatusCode::BAD_REQUEST,
            error: anyhow::anyhow!(message.into()),
            detail: (!detail.trim().is_empty()).then_some(detail),
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
                    detail: self.detail,
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
        tailscale_login: Arc::new(Mutex::new(None)),
    };
    let api = Router::new()
        .route("/status", get(status))
        .route("/install/devin", post(install_devin))
        .route("/auth/login", post(auth_login))
        .route("/tailscale/connect", post(tailscale_connect))
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
            if let Some(mut child) = shutdown_state.tailscale_login.lock().await.take() {
                let _ = child.kill().await;
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
        return Err(SetupError::advice(
            "Devin's sign-in did not finish. Complete it in the browser window Devin opened, then choose Check again.",
            command_detail(&output.stdout, &output.stderr),
        ));
    }
    let next = setup_status(state.host_port)
        .await
        .map_err(SetupError::internal)?;
    if !next.devin.ready {
        return Err(SetupError::bad_request(
            "Devin still reports that this computer is signed out. Finish the browser sign-in, then choose Check again.",
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
            "Phone access needs Tailscale connected on this computer. Go back to Check this computer and choose Connect Tailscale.",
        ));
    }
    if request.preview && !current.browser.ready {
        return Err(SetupError::bad_request(
            "Browser preview needs Chromium or Chrome for Testing on this computer. Turn the option off or install Chromium, then try again.",
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
    let away_owner = request.away.then_some(current.tailscale.account).flatten();
    Ok(Json(LaunchResult {
        local_url: format!("http://127.0.0.1:{}", request.port),
        away_url,
        away_owner,
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
                || "Optional. Chromium lets Devin show you a running app.".into(),
                |_| "Chromium is ready for isolated local previews.".into(),
            ),
            path: browser_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            url: browser_path
                .is_none()
                .then(|| "https://www.chromium.org/getting-involved/download-chromium/".into()),
            account: None,
            action: None,
            manual_command: None,
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
            detail: "Leave needs Cognition's official Devin CLI on this computer.".into(),
            path: None,
            url: Some(DEVIN_DOCS_URL.into()),
            account: None,
            action: devin_install_action(),
            manual_command: unix_only(DEVIN_INSTALL_COMMAND),
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
    let account = ready.then(|| account_from_status(&detail)).flatten();
    ToolView {
        installed: true,
        ready,
        label: "Devin".into(),
        detail: if ready {
            account.as_ref().map_or_else(
                || "Devin is installed and signed in on this computer.".into(),
                |account| format!("Signed in to Devin as {account}."),
            )
        } else {
            "Devin is installed but signed out. Leave can open Devin's official sign-in for you."
                .into()
        },
        path: Some(path.to_string_lossy().into_owned()),
        url: None,
        account,
        action: (!ready).then(|| ToolAction {
            id: "connectDevin",
            label: "Sign in to Devin".into(),
            command: "devin auth login".into(),
            detail: "Opens Cognition's official sign-in. Leave never reads Devin's credentials."
                .into(),
        }),
        manual_command: (!ready).then(|| "devin auth login".into()),
    }
}

/// The guided installer Leave can run, where the official command needs no administrator.
fn devin_install_action() -> Option<ToolAction> {
    if cfg!(windows) {
        return None;
    }
    Some(ToolAction {
        id: "installDevin",
        label: "Install Devin".into(),
        command: DEVIN_INSTALL_COMMAND.into(),
        detail: "Runs Cognition's published installer for your user account only.".into(),
    })
}

/// Return the value only on platforms where Leave offers the shell installer.
fn unix_only(command: &str) -> Option<String> {
    (!cfg!(windows)).then(|| command.to_owned())
}

/// Read an account name out of `devin auth status` output without parsing its layout.
fn account_from_status(detail: &str) -> Option<String> {
    detail
        .split_whitespace()
        .find(|word| {
            let trimmed = word.trim_matches(|character: char| !character.is_ascii_graphic());
            trimmed.contains('@') && trimmed.contains('.') && trimmed.len() > 4
        })
        .map(|word| {
            word.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '@' && character != '.'
            })
            .to_owned()
        })
}

async fn install_devin(State(state): State<SetupState>) -> Result<Json<SetupStatus>, SetupError> {
    if cfg!(windows) {
        return Err(SetupError::bad_request(
            "Install Devin with Cognition's PowerShell quickstart, then choose Check again.",
        ));
    }
    if crate::discover_devin_binary().is_some() {
        return status(State(state)).await;
    }
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(DEVIN_INSTALL_COMMAND)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_mins(10), command.output())
        .await
        .map_err(|_| {
            SetupError::bad_request(
                "The Devin installer did not finish in ten minutes. Check this computer's internet connection and try again.",
            )
        })?
        .context("could not start the official Devin installer")
        .map_err(SetupError::internal)?;
    if !output.status.success() {
        return Err(SetupError::advice(
            "Cognition's installer did not finish. Install Devin from the quickstart, then choose Check again.",
            command_detail(&output.stdout, &output.stderr),
        ));
    }
    let next = setup_status(state.host_port)
        .await
        .map_err(SetupError::internal)?;
    if !next.devin.installed {
        return Err(SetupError::advice(
            "The installer finished but Leave still cannot find the Devin command. Open a new terminal and run devin --version.",
            command_detail(&output.stdout, &output.stderr),
        ));
    }
    Ok(Json(next))
}

async fn tailscale_status() -> ToolView {
    let Some(mut command) = crate::away::tailscale_command() else {
        return ToolView {
            installed: false,
            ready: false,
            label: "Phone access".into(),
            detail: "Optional. Tailscale gives your phone a private address for this computer."
                .into(),
            path: None,
            url: Some(TAILSCALE_DOWNLOAD_URL.into()),
            account: None,
            action: None,
            manual_command: cfg!(target_os = "linux").then(|| TAILSCALE_INSTALL_COMMAND.into()),
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
            account: None,
            action: None,
            manual_command: Some("tailscale status".into()),
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
    let account = value.as_ref().and_then(tailnet_login);
    ToolView {
        installed: true,
        ready,
        label: "Phone access".into(),
        detail: if ready {
            account.as_ref().map_or_else(
                || "Tailscale is connected and ready for private phone access.".into(),
                |account| {
                    format!(
                        "Tailscale is connected as {account}. Only that account may open Leave."
                    )
                },
            )
        } else {
            "Tailscale is installed but signed out. Leave can start the sign-in for you.".into()
        },
        path: None,
        url: dns_name,
        account,
        action: (!ready).then(|| ToolAction {
            id: "connectTailscale",
            label: "Connect Tailscale".into(),
            command: "tailscale up".into(),
            detail: "Starts Tailscale's own sign-in and shows you the link to finish it.".into(),
        }),
        manual_command: (!ready).then(|| "tailscale up".into()),
    }
}

/// The tailnet login this computer is signed in as, when Tailscale reports one.
fn tailnet_login(status: &Value) -> Option<String> {
    if let Some(login) = status
        .pointer("/Self/UserProfile/LoginName")
        .and_then(Value::as_str)
    {
        return Some(login.to_ascii_lowercase());
    }
    let user_id = status.pointer("/Self/UserID").and_then(Value::as_u64)?;
    status
        .get("User")
        .and_then(|users| users.get(user_id.to_string()))
        .and_then(|user| user.get("LoginName"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TailscaleConnection {
    /// Tailscale finished signing in without any further step.
    connected: bool,
    /// Tailscale's own sign-in page, when it needs a browser.
    login_url: Option<String>,
    /// One sentence for the wizard to show.
    detail: String,
}

async fn tailscale_connect(
    State(state): State<SetupState>,
) -> Result<Json<TailscaleConnection>, SetupError> {
    let mut command = crate::away::tailscale_command()
        .context("Install Tailscale on this computer first, then choose Check again.")
        .map_err(SetupError::bad_request)?;
    if state.tailscale_login.lock().await.is_some() {
        return Err(SetupError::bad_request(
            "A Tailscale sign-in is already waiting. Finish it in your browser, then choose Check again.",
        ));
    }
    let mut child = command
        .arg("up")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start Tailscale")
        .map_err(SetupError::internal)?;
    let mut lines = merged_lines(&mut child);
    let mut transcript = String::new();
    let deadline = tokio::time::Instant::now() + TAILSCALE_LINK_TIMEOUT;
    loop {
        tokio::select! {
            line = tokio::time::timeout_at(deadline, lines.recv()) => match line {
                Ok(Some(line)) => {
                    if let Some(url) = login_url(&line) {
                        *state.tailscale_login.lock().await = Some(child);
                        let opened = open_url(&url).await.is_ok();
                        return Ok(Json(TailscaleConnection {
                            connected: false,
                            login_url: Some(url),
                            detail: if opened {
                                "Tailscale's sign-in page is open in your browser. Finish it, then choose Check again.".into()
                            } else {
                                "Open this Tailscale sign-in link, then choose Check again.".into()
                            },
                        }));
                    }
                    transcript.push_str(&line);
                    transcript.push('\n');
                }
                Ok(None) => break,
                Err(_) => {
                    *state.tailscale_login.lock().await = Some(child);
                    return Ok(Json(TailscaleConnection {
                        connected: false,
                        login_url: None,
                        detail: "Tailscale is still connecting. Choose Check again in a moment."
                            .into(),
                    }));
                }
            },
        }
    }
    let status = child
        .wait()
        .await
        .context("could not read Tailscale's result")
        .map_err(SetupError::internal)?;
    if !status.success() {
        return Err(SetupError::advice(
            tailscale_advice(&transcript),
            transcript,
        ));
    }
    Ok(Json(TailscaleConnection {
        connected: true,
        login_url: None,
        detail: "Tailscale is connected on this computer.".into(),
    }))
}

/// Turn a Tailscale failure into one sentence a person can act on.
fn tailscale_advice(transcript: &str) -> String {
    let lowered = transcript.to_ascii_lowercase();
    if lowered.contains("permission denied")
        || lowered.contains("access denied")
        || lowered.contains("operator")
        || lowered.contains("must be run as root")
    {
        "Tailscale needs administrator rights on this computer. Open the Tailscale app and sign in, or run tailscale up from an administrator terminal.".into()
    } else if lowered.contains("not running") || lowered.contains("connect: no such file") {
        "Tailscale is installed but its background service is not running. Start Tailscale, then choose Check again.".into()
    } else {
        "Tailscale could not finish signing in. Open the Tailscale app, sign in there, then choose Check again.".into()
    }
}

/// Extract a Tailscale sign-in URL from one line of its output.
fn login_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        .take_while(|character| !character.is_whitespace())
        .collect();
    let trimmed = url.trim_end_matches(['.', ',', ')']).to_owned();
    trimmed.contains("tailscale.com").then_some(trimmed)
}

/// Stream a child's stdout and stderr as lines on one channel.
fn merged_lines(child: &mut tokio::process::Child) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel(32);
    for stream in [
        child.stdout.take().map(StdioStream::Out),
        child.stderr.take().map(StdioStream::Err),
    ]
    .into_iter()
    .flatten()
    {
        let sender = sender.clone();
        tokio::spawn(async move {
            let mut lines = match stream {
                StdioStream::Out(stdout) => BufReader::new(Box::new(stdout) as BoxedRead).lines(),
                StdioStream::Err(stderr) => BufReader::new(Box::new(stderr) as BoxedRead).lines(),
            };
            while let Ok(Some(line)) = lines.next_line().await {
                if sender.send(line).await.is_err() {
                    break;
                }
            }
        });
    }
    receiver
}

type BoxedRead = Box<dyn tokio::io::AsyncRead + Send + Unpin>;

enum StdioStream {
    Out(tokio::process::ChildStdout),
    Err(tokio::process::ChildStderr),
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
            detail: None,
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

    #[test]
    fn reads_a_tailscale_sign_in_link() {
        assert_eq!(
            login_url("To authenticate, visit: https://login.tailscale.com/a/1234abcd"),
            Some("https://login.tailscale.com/a/1234abcd".into())
        );
        assert_eq!(
            login_url("Success. Visit https://example.com/other."),
            None,
            "only Tailscale's own sign-in link is offered"
        );
        assert_eq!(login_url("Backend state: Running"), None);
    }

    #[test]
    fn tailscale_advice_names_the_next_step() {
        assert!(
            tailscale_advice("Access denied: tailscaled requires elevated permissions")
                .contains("administrator")
        );
        assert!(
            tailscale_advice("failed to connect to local tailscaled; it is not running")
                .contains("background service")
        );
        assert!(tailscale_advice("").contains("Tailscale app"));
    }

    #[test]
    fn reads_an_account_out_of_devin_status() {
        assert_eq!(
            account_from_status("Logged in as person@example.com (team)"),
            Some("person@example.com".into())
        );
        assert_eq!(account_from_status("Logged in"), None);
    }

    #[test]
    fn offers_the_shell_installer_only_where_it_runs() {
        let action = devin_install_action();
        if cfg!(windows) {
            assert!(action.is_none());
        } else {
            let action = action.unwrap_or_else(|| unreachable!("unix always offers an installer"));
            assert_eq!(action.command, DEVIN_INSTALL_COMMAND);
            assert_eq!(action.id, "installDevin");
        }
    }

    #[test]
    fn reads_the_tailnet_login_from_a_user_map() {
        let status = serde_json::json!({
            "Self": {"UserID": 7},
            "User": {"7": {"LoginName": "Owner@Example.com"}}
        });
        assert_eq!(tailnet_login(&status), Some("owner@example.com".into()));
        assert_eq!(tailnet_login(&serde_json::json!({"Self": {}})), None);
    }
}
