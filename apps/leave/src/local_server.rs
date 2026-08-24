use crate::{
    acp::{AcpHandle, AgentStatus, LocalEvent, PromptAccepted},
    customization::{self, CustomizationMutation, DevinCommandOutput},
    git::{self, GitBranch, GitStatus},
    preview::{PreviewControl, PreviewManager, PreviewView},
    terminal::{TerminalManager, TerminalView},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use futures_util::{SinkExt, StreamExt};
use leave_core::{
    DirectoryEntry, EventStore, FileSnapshot, GuardedFileSystem, GuardedFsError, SessionRecord,
    WorkspaceRecord, WorkspaceRoot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{net::SocketAddr, path::PathBuf};
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

#[derive(Clone)]
struct LocalServerState {
    store: EventStore,
    workspace: WorkspaceRecord,
    acp: AcpHandle,
    files: GuardedFileSystem,
    devin_binary: PathBuf,
    access: HostAccess,
    terminal: TerminalManager,
    preview: PreviewManager,
}

/// Access and capability configuration selected by the host owner.
#[derive(Debug, Clone)]
pub struct LocalServeConfig {
    /// Localhost-only or owner-restricted tailnet access.
    pub access: HostAccess,
    /// Explicit raw PTY grant for this host run.
    pub terminal_granted: bool,
    /// Explicit managed browser grant for this host run.
    pub preview_granted: bool,
    /// Managed or system Chromium binary selected by the host.
    pub chrome_binary: Option<PathBuf>,
}

/// Network boundary enforced by the local HTTP server.
#[derive(Debug, Clone)]
pub enum HostAccess {
    /// Requests are only expected from localhost.
    Local,
    /// Requests may arrive through Tailscale Serve for this exact owner.
    Tailnet {
        /// Normalized Tailscale account login.
        owner_login: String,
        /// Tailnet-only HTTPS URL.
        url: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalStatus {
    status: &'static str,
    version: &'static str,
    mode: &'static str,
    host: HostView,
    workspace: WorkspaceRecord,
    agent: AgentStatus,
    remote_available: bool,
    away_url: Option<String>,
    capabilities: CapabilityView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
struct CapabilityView {
    files: bool,
    git: bool,
    project_customization: bool,
    global_customization: bool,
    terminal: bool,
    preview: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HostView {
    name: &'static str,
    platform: &'static str,
    architecture: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionBody {
    #[serde(default)]
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptBody {
    command_id: Uuid,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionBody {
    #[serde(default)]
    option_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "default_event_limit")]
    limit: u32,
}

fn default_event_limit() -> u32 {
    500
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventPage {
    events: Vec<LocalEvent>,
    next_cursor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryQuery {
    #[serde(default)]
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileBody {
    path: String,
    base_hash: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitDiffQuery {
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitPathsBody {
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitCommitBody {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitBranchBody {
    name: String,
    #[serde(default)]
    create: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomizationQuery {
    category: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTerminalBody {
    #[serde(default = "default_terminal_rows")]
    rows: u16,
    #[serde(default = "default_terminal_cols")]
    cols: u16,
}

const fn default_terminal_rows() -> u16 {
    30
}

const fn default_terminal_cols() -> u16 {
    100
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePreviewBody {
    url: String,
    #[serde(default = "default_preview_width")]
    width: u16,
    #[serde(default = "default_preview_height")]
    height: u16,
}

const fn default_preview_width() -> u16 {
    390
}

const fn default_preview_height() -> u16 {
    844
}

/// Serve the production PWA and local control API from one loopback origin.
pub async fn serve_local(
    store: EventStore,
    workspace: WorkspaceRecord,
    listen: SocketAddr,
    web_dir: PathBuf,
    acp_command: String,
    devin_binary: PathBuf,
    config: LocalServeConfig,
) -> anyhow::Result<()> {
    if !listen.ip().is_loopback() {
        anyhow::bail!("the local alpha may only listen on a loopback address");
    }
    let index = web_dir.join("index.html");
    if !index.is_file() {
        anyhow::bail!(
            "PWA build not found at {}; run `pnpm --filter @leave/web build` first",
            index.display()
        );
    }

    let root = WorkspaceRoot::register(&workspace.canonical_path).await?;
    let acp = AcpHandle::start(store.clone(), workspace.clone(), acp_command);
    let state = LocalServerState {
        store,
        files: GuardedFileSystem::new(root),
        terminal: TerminalManager::new(workspace.canonical_path.clone(), config.terminal_granted),
        preview: PreviewManager::new(config.preview_granted, config.chrome_binary),
        workspace,
        acp,
        devin_binary,
        access: config.access,
    };
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(index));
    let app = Router::new()
        .route("/api/v1/local/status", get(status))
        .route("/api/v1/local/openapi.json", get(local_openapi))
        .route("/api/v1/local/workspaces", get(workspaces))
        .route("/api/v1/local/sessions", get(sessions).post(create_session))
        .route("/api/v1/local/sessions/{session_id}", get(session))
        .route(
            "/api/v1/local/sessions/{session_id}/events",
            get(session_events),
        )
        .route(
            "/api/v1/local/sessions/{session_id}/resume",
            post(resume_session),
        )
        .route("/api/v1/local/sessions/{session_id}/prompts", post(prompt))
        .route("/api/v1/local/sessions/{session_id}/cancel", post(cancel))
        .route(
            "/api/v1/local/permissions/{request_id}",
            post(decide_permission),
        )
        .route("/api/v1/local/events", get(workspace_events))
        .route("/api/v1/local/ws", get(websocket))
        .route("/api/v1/local/files", get(list_files))
        .route("/api/v1/local/file", get(read_file).put(write_file))
        .route("/api/v1/local/git/status", get(git_status))
        .route("/api/v1/local/git/diff", get(git_diff))
        .route(
            "/api/v1/local/git/branches",
            get(git_branches).post(git_switch),
        )
        .route("/api/v1/local/git/worktrees", get(git_worktrees))
        .route("/api/v1/local/git/stage", post(git_stage))
        .route("/api/v1/local/git/unstage", post(git_unstage))
        .route("/api/v1/local/git/commit", post(git_commit))
        .route("/api/v1/local/git/push", post(git_push))
        .route(
            "/api/v1/local/customization",
            get(customization_read).post(customization_mutate),
        )
        .route("/api/v1/local/terminals", post(create_terminal))
        .route(
            "/api/v1/local/terminals/{terminal_id}/ws",
            get(terminal_websocket),
        )
        .route("/api/v1/local/previews", post(create_preview))
        .route(
            "/api/v1/local/previews/{preview_id}/control",
            put(preview_control),
        )
        .route(
            "/api/v1/local/previews/{preview_id}/ws",
            get(preview_websocket),
        )
        .fallback_service(static_files)
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_access,
        ))
        .with_state(state);

    let listener = TcpListener::bind(listen).await?;
    tracing::info!(url = %format!("http://{listen}"), "Leave local workspace is ready");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "could not install Ctrl+C handler");
            }
        })
        .await?;
    Ok(())
}

async fn status(State(state): State<LocalServerState>) -> Json<LocalStatus> {
    let (mode, remote_available, away_url) = match &state.access {
        HostAccess::Local => ("local", false, None),
        HostAccess::Tailnet { url, .. } => ("tailnet", true, Some(url.clone())),
    };
    Json(LocalStatus {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        mode,
        host: HostView {
            name: "This computer",
            platform: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        workspace: state.workspace.clone(),
        agent: state.acp.status().await,
        remote_available,
        away_url,
        capabilities: CapabilityView {
            files: true,
            git: true,
            project_customization: state.workspace.expose_project_customization,
            global_customization: state.workspace.expose_global_customization,
            terminal: state.terminal.enabled(),
            preview: state.preview.available(),
        },
    })
}

async fn local_openapi() -> Json<serde_json::Value> {
    Json(json!({
        "openapi": "3.1.0",
        "info": {"title": "Leave Host API", "version": env!("CARGO_PKG_VERSION")},
        "paths": {
            "/api/v1/local/status": {"get": {"summary": "Host, agent, access, and capability state"}},
            "/api/v1/local/sessions": {"get": {"summary": "List sessions"}, "post": {"summary": "Create an ACP session"}},
            "/api/v1/local/sessions/{session_id}/prompts": {"post": {"summary": "Send one deduplicated prompt"}},
            "/api/v1/local/permissions/{request_id}": {"post": {"summary": "Resolve an expiring ACP permission"}},
            "/api/v1/local/files": {"get": {"summary": "List one guarded directory"}},
            "/api/v1/local/file": {"get": {"summary": "Read guarded UTF-8 text"}, "put": {"summary": "Hash-checked atomic file write"}},
            "/api/v1/local/git/status": {"get": {"summary": "Structured Git status"}},
            "/api/v1/local/git/diff": {"get": {"summary": "Bounded Git diff"}},
            "/api/v1/local/customization": {"get": {"summary": "List/show documented Devin customization"}, "post": {"summary": "Apply a confirmed plugin or MCP mutation"}},
            "/api/v1/local/terminals": {"post": {"summary": "Create an explicitly granted PTY"}},
            "/api/v1/local/previews": {"post": {"summary": "Create an explicitly granted loopback preview"}}
        }
    }))
}

async fn workspaces(State(state): State<LocalServerState>) -> Json<Vec<WorkspaceRecord>> {
    Json(vec![state.workspace])
}

async fn sessions(
    State(state): State<LocalServerState>,
) -> Result<Json<Vec<SessionRecord>>, ApiError> {
    Ok(Json(state.store.list_sessions(state.workspace.id).await?))
}

async fn session(
    State(state): State<LocalServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionRecord>, ApiError> {
    Ok(Json(require_session(&state, &session_id).await?))
}

async fn create_session(
    State(state): State<LocalServerState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<(StatusCode, Json<SessionRecord>), ApiError> {
    require_ready(&state).await?;
    let session = state
        .acp
        .create_session(body.title)
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn resume_session(
    State(state): State<LocalServerState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_ready(&state).await?;
    require_session(&state, &session_id).await?;
    state
        .acp
        .resume_session(session_id)
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn prompt(
    State(state): State<LocalServerState>,
    Path(session_id): Path<String>,
    Json(body): Json<PromptBody>,
) -> Result<(StatusCode, Json<PromptAccepted>), ApiError> {
    require_ready(&state).await?;
    require_session(&state, &session_id).await?;
    let text = body.text.trim();
    if text.is_empty() {
        return Err(ApiError::bad_request("prompt cannot be empty"));
    }
    if text.chars().count() > 100_000 {
        return Err(ApiError::bad_request("prompt is too large"));
    }
    let accepted = state
        .acp
        .prompt(session_id, text.to_owned(), body.command_id)
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok((StatusCode::ACCEPTED, Json(accepted)))
}

async fn cancel(
    State(state): State<LocalServerState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_ready(&state).await?;
    require_session(&state, &session_id).await?;
    state
        .acp
        .cancel(session_id)
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn decide_permission(
    State(state): State<LocalServerState>,
    Path(request_id): Path<Uuid>,
    Json(body): Json<PermissionBody>,
) -> Result<StatusCode, ApiError> {
    state
        .acp
        .decide_permission(request_id, body.option_id)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn session_events(
    State(state): State<LocalServerState>,
    Path(session_id): Path<String>,
    Query(query): Query<EventQuery>,
) -> Result<Json<EventPage>, ApiError> {
    require_session(&state, &session_id).await?;
    let records = state
        .store
        .session_events_after(
            state.workspace.id,
            &session_id,
            query.after.max(0),
            query.limit,
        )
        .await?;
    Ok(Json(event_page(records)))
}

async fn workspace_events(
    State(state): State<LocalServerState>,
    Query(query): Query<EventQuery>,
) -> Result<Json<EventPage>, ApiError> {
    let records = state
        .store
        .events_after(state.workspace.id, query.after.max(0), query.limit)
        .await?;
    Ok(Json(event_page(records)))
}

async fn list_files(
    State(state): State<LocalServerState>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<DirectoryEntry>>, ApiError> {
    state
        .files
        .list_directory(query.path)
        .await
        .map(Json)
        .map_err(ApiError::from_guarded_fs)
}

async fn read_file(
    State(state): State<LocalServerState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<FileSnapshot>, ApiError> {
    state
        .files
        .read_text(query.path)
        .await
        .map(Json)
        .map_err(ApiError::from_guarded_fs)
}

async fn write_file(
    State(state): State<LocalServerState>,
    Json(body): Json<WriteFileBody>,
) -> Result<Json<FileSnapshot>, ApiError> {
    if body.content.len() > 2 * 1024 * 1024 {
        return Err(ApiError::payload_too_large(
            "file exceeds the 2 MiB direct-editing limit",
        ));
    }
    state
        .files
        .write_text(body.path, &body.base_hash, &body.content)
        .await
        .map(Json)
        .map_err(ApiError::from_guarded_fs)
}

async fn git_status(State(state): State<LocalServerState>) -> Result<Json<GitStatus>, ApiError> {
    git::status(&state.workspace.canonical_path)
        .await
        .map(Json)
        .map_err(ApiError::git)
}

async fn git_diff(
    State(state): State<LocalServerState>,
    Query(query): Query<GitDiffQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let diff = git::diff(
        &state.workspace.canonical_path,
        query.path.as_deref(),
        query.staged,
    )
    .await
    .map_err(ApiError::git)?;
    Ok(Json(json!({ "diff": diff })))
}

async fn git_branches(
    State(state): State<LocalServerState>,
) -> Result<Json<Vec<GitBranch>>, ApiError> {
    git::branches(&state.workspace.canonical_path)
        .await
        .map(Json)
        .map_err(ApiError::git)
}

async fn git_switch(
    State(state): State<LocalServerState>,
    Json(body): Json<GitBranchBody>,
) -> Result<StatusCode, ApiError> {
    git::switch_branch(&state.workspace.canonical_path, &body.name, body.create)
        .await
        .map_err(ApiError::git)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn git_worktrees(
    State(state): State<LocalServerState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let worktrees = git::worktrees(&state.workspace.canonical_path)
        .await
        .map_err(ApiError::git)?;
    Ok(Json(json!({ "worktrees": worktrees })))
}

async fn git_stage(
    State(state): State<LocalServerState>,
    Json(body): Json<GitPathsBody>,
) -> Result<StatusCode, ApiError> {
    git::stage(&state.workspace.canonical_path, &body.paths)
        .await
        .map_err(ApiError::git)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn git_unstage(
    State(state): State<LocalServerState>,
    Json(body): Json<GitPathsBody>,
) -> Result<StatusCode, ApiError> {
    git::unstage(&state.workspace.canonical_path, &body.paths)
        .await
        .map_err(ApiError::git)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn git_commit(
    State(state): State<LocalServerState>,
    Json(body): Json<GitCommitBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let output = git::commit(&state.workspace.canonical_path, &body.message)
        .await
        .map_err(ApiError::git)?;
    Ok(Json(json!({ "output": output })))
}

async fn git_push(
    State(state): State<LocalServerState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let output = git::push(&state.workspace.canonical_path)
        .await
        .map_err(ApiError::git)?;
    Ok(Json(json!({ "output": output })))
}

async fn customization_read(
    State(state): State<LocalServerState>,
    Query(query): Query<CustomizationQuery>,
) -> Result<Json<DevinCommandOutput>, ApiError> {
    if !state.workspace.expose_project_customization {
        return Err(ApiError::forbidden(
            "project customization is not granted for this workspace",
        ));
    }
    let result = if let Some(name) = query.name {
        customization::show(
            &state.devin_binary,
            &state.workspace.canonical_path,
            &query.category,
            &name,
            state.workspace.expose_global_customization,
        )
        .await
    } else {
        customization::list(
            &state.devin_binary,
            &state.workspace.canonical_path,
            &query.category,
            state.workspace.expose_global_customization,
        )
        .await
    };
    result.map(Json).map_err(ApiError::customization)
}

async fn customization_mutate(
    State(state): State<LocalServerState>,
    Json(body): Json<CustomizationMutation>,
) -> Result<Json<DevinCommandOutput>, ApiError> {
    if !state.workspace.expose_project_customization {
        return Err(ApiError::forbidden(
            "project customization is not granted for this workspace",
        ));
    }
    customization::mutate(
        &state.devin_binary,
        &state.workspace.canonical_path,
        &body,
        state.workspace.expose_global_customization,
    )
    .await
    .map(Json)
    .map_err(ApiError::customization)
}

async fn create_terminal(
    State(state): State<LocalServerState>,
    Json(body): Json<CreateTerminalBody>,
) -> Result<(StatusCode, Json<TerminalView>), ApiError> {
    let terminal = state
        .terminal
        .create(body.rows, body.cols)
        .await
        .map_err(ApiError::forbidden_error)?;
    Ok((StatusCode::CREATED, Json(terminal)))
}

async fn terminal_websocket(
    State(state): State<LocalServerState>,
    Path(terminal_id): Path<Uuid>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_same_origin(&headers)?;
    let mut output = state
        .terminal
        .subscribe(terminal_id)
        .await
        .map_err(ApiError::not_found_error)?;
    let terminal = state.terminal.clone();
    Ok(upgrade
        .on_upgrade(move |socket| async move {
            let (mut sender, mut receiver) = socket.split();
            loop {
                tokio::select! {
                    incoming = receiver.next() => match incoming {
                        Some(Ok(Message::Binary(bytes))) => {
                            if terminal.write(terminal_id, bytes.to_vec()).await.is_err() { break; }
                        }
                        Some(Ok(Message::Text(text))) => {
                            let value = serde_json::from_str::<serde_json::Value>(&text);
                            let resize = value.ok().and_then(|value| {
                                (value.get("type").and_then(serde_json::Value::as_str) == Some("resize"))
                                    .then(|| (
                                        value.get("rows").and_then(serde_json::Value::as_u64),
                                        value.get("cols").and_then(serde_json::Value::as_u64),
                                    ))
                            });
                            if let Some((Some(rows), Some(cols))) = resize {
                                let (Ok(rows), Ok(cols)) = (u16::try_from(rows), u16::try_from(cols)) else { continue };
                                let _ = terminal.resize(terminal_id, rows, cols).await;
                            }
                        }
                        Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                        Some(Ok(_)) => {}
                    },
                    bytes = output.recv() => match bytes {
                        Ok(bytes) => if sender.send(Message::Binary(bytes.into())).await.is_err() { break; },
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = sender.send(Message::Close(None)).await;
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        })
        .into_response())
}

async fn create_preview(
    State(state): State<LocalServerState>,
    Json(body): Json<CreatePreviewBody>,
) -> Result<(StatusCode, Json<PreviewView>), ApiError> {
    let preview = state
        .preview
        .create(&body.url, body.width, body.height)
        .await
        .map_err(ApiError::forbidden_error)?;
    Ok((StatusCode::CREATED, Json(preview)))
}

async fn preview_control(
    State(state): State<LocalServerState>,
    Path(preview_id): Path<Uuid>,
    Json(body): Json<PreviewControl>,
) -> Result<StatusCode, ApiError> {
    state
        .preview
        .control(preview_id, body)
        .await
        .map_err(ApiError::bad_request_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn preview_websocket(
    State(state): State<LocalServerState>,
    Path(preview_id): Path<Uuid>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_same_origin(&headers)?;
    let mut frames = state
        .preview
        .subscribe(preview_id)
        .await
        .map_err(ApiError::not_found_error)?;
    let previews = state.preview.clone();
    Ok(upgrade
        .on_upgrade(move |socket| async move {
            let (mut sender, mut receiver) = socket.split();
            loop {
                tokio::select! {
                    incoming = receiver.next() => match incoming {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(control) = serde_json::from_str::<PreviewControl>(&text) {
                                let _ = previews.control(preview_id, control).await;
                            }
                        }
                        Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                        Some(Ok(_)) => {}
                    },
                    frame = frames.recv() => match frame {
                        Ok(data) => {
                            let message = json!({"type": "frame", "mediaType": "image/jpeg", "data": data}).to_string();
                            if sender.send(Message::Text(message.into())).await.is_err() { break; }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        })
        .into_response())
}

async fn websocket(
    State(state): State<LocalServerState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_same_origin(&headers)?;
    Ok(upgrade
        .on_upgrade(move |socket| async move {
            let (mut sender, mut receiver) = socket.split();
            let mut events = state.acp.subscribe();
            loop {
                tokio::select! {
                    incoming = receiver.next() => {
                        match incoming {
                            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                            Some(Ok(_)) => {}
                        }
                    }
                    event = events.recv() => {
                        let message = match event {
                            Ok(event) => serde_json::to_string(&json!({ "type": "event", "event": event })),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                serde_json::to_string(&json!({ "type": "replay_required", "skipped": skipped }))
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        };
                        let Ok(message) = message else { break };
                        if sender.send(Message::Text(message.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .into_response())
}

async fn require_session(
    state: &LocalServerState,
    session_id: &str,
) -> Result<SessionRecord, ApiError> {
    state
        .store
        .get_session(state.workspace.id, session_id)
        .await?
        .ok_or_else(|| ApiError::not_found("session was not found in this workspace"))
}

async fn require_ready(state: &LocalServerState) -> Result<(), ApiError> {
    let status = state.acp.status().await;
    if status.state == crate::acp::AgentState::Ready {
        Ok(())
    } else {
        Err(ApiError::unavailable(status.detail))
    }
}

fn event_page(records: Vec<leave_core::EventRecord>) -> EventPage {
    let events = records
        .into_iter()
        .map(LocalEvent::from_record)
        .collect::<Vec<_>>();
    let next_cursor = events.last().map_or(0, |event| event.sequence);
    EventPage {
        events,
        next_cursor,
    }
}

fn require_same_origin(headers: &HeaderMap) -> Result<(), ApiError> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("WebSocket Host header is missing"))?;
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("WebSocket Origin header is missing"))?;
    let allowed_http = format!("http://{host}");
    let allowed_https = format!("https://{host}");
    if origin == allowed_http || origin == allowed_https {
        Ok(())
    } else {
        Err(ApiError::forbidden("cross-origin WebSocket denied"))
    }
}

async fn authorize_access(
    State(state): State<LocalServerState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let login = request
        .headers()
        .get("tailscale-user-login")
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase);
    if !access_allowed(&state.access, host, login.as_deref()) {
        return ApiError::forbidden("this Leave host only accepts its owner's Tailscale identity")
            .into_response();
    }
    next.run(request).await
}

fn access_allowed(access: &HostAccess, host: &str, login: Option<&str>) -> bool {
    match access {
        HostAccess::Local => true,
        HostAccess::Tailnet { owner_login, .. } => {
            is_local_host(host) || login.is_some_and(|login| login == owner_login)
        }
    }
}

fn is_local_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.starts_with("localhost:")
        || host == "127.0.0.1"
        || host.starts_with("127.0.0.1:")
        || host == "[::1]"
        || host.starts_with("[::1]:")
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self' ws: wss:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: message.into(),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn from_guarded_fs(error: GuardedFsError) -> Self {
        match error {
            GuardedFsError::Conflict { .. } => Self::conflict(error.to_string()),
            GuardedFsError::InvalidPath
            | GuardedFsError::OutsideWorkspace
            | GuardedFsError::SymlinkWriteDenied => Self::forbidden(error.to_string()),
            GuardedFsError::NotUtf8 => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: error.to_string(),
            },
            GuardedFsError::TooLarge => Self::payload_too_large(error.to_string()),
            GuardedFsError::NotDirectory => Self::bad_request(error.to_string()),
            GuardedFsError::Io(ref source) if source.kind() == std::io::ErrorKind::NotFound => {
                Self::not_found("file or directory was not found")
            }
            GuardedFsError::Io(_) => Self::unavailable("the host could not access that path"),
        }
    }

    fn git(error: anyhow::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::conflict(message)
    }

    fn customization(error: anyhow::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::bad_request(message)
    }

    fn forbidden_error(error: anyhow::Error) -> Self {
        let message = error.to_string();
        drop(error);
        if message.contains(" is off") {
            Self::forbidden(message)
        } else {
            Self::unavailable(message)
        }
    }

    fn not_found_error(error: anyhow::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::not_found(message)
    }

    fn bad_request_error(error: anyhow::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::bad_request(message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(%error, "local API request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "the local host could not complete this request".into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "status": self.status.as_u16(),
                    "message": self.message
                }
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailnet_access_requires_the_exact_owner_identity() {
        let access = HostAccess::Tailnet {
            owner_login: "owner@example.com".into(),
            url: "https://host.example.ts.net".into(),
        };
        assert!(access_allowed(&access, "127.0.0.1:8788", None));
        assert!(access_allowed(
            &access,
            "host.example.ts.net",
            Some("owner@example.com")
        ));
        assert!(!access_allowed(
            &access,
            "host.example.ts.net",
            Some("shared-user@example.com")
        ));
        assert!(!access_allowed(&access, "host.example.ts.net", None));
    }
}
