use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, Implementation, InitializeRequest, NewSessionRequest,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use anyhow::{Context, anyhow, bail};
use leave_core::{CommandClaim, EventRecord, EventStore, SessionRecord, WorkspaceRecord};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};
use uuid::Uuid;

const PERMISSION_TIMEOUT: Duration = Duration::from_mins(5);

/// Browser-safe representation of one durably appended host event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEvent {
    pub sequence: i64,
    pub event_id: Uuid,
    pub workspace_id: Uuid,
    pub session_id: Option<String>,
    pub kind: String,
    pub occurred_at_unix_ms: i64,
    pub payload: Value,
}

impl LocalEvent {
    pub fn from_record(record: EventRecord) -> Self {
        let payload = serde_json::from_slice(&record.payload).unwrap_or_else(
            |_| json!({ "unparseablePayload": true, "size": record.payload.len() }),
        );
        Self {
            sequence: record.sequence,
            event_id: record.event_id,
            workspace_id: record.workspace_id,
            session_id: record.session_id,
            kind: record.kind,
            occurred_at_unix_ms: record.occurred_at.unix_timestamp() * 1_000
                + i64::from(record.occurred_at.millisecond()),
            payload,
        }
    }
}

/// Current health of the supervised ACP worker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub state: AgentState,
    pub detail: String,
    pub agent_info: Option<Value>,
    pub capabilities: Option<Value>,
}

/// Coarse connection state exposed to the local UI.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Starting,
    Ready,
    Error,
    Stopped,
}

impl AgentStatus {
    fn starting(command: &str) -> Self {
        Self {
            state: AgentState::Starting,
            detail: format!("Starting ACP agent with `{command}`"),
            agent_info: None,
            capabilities: None,
        }
    }
}

#[derive(Debug)]
struct PendingPermission {
    session_id: String,
    option_ids: Vec<String>,
    decision_tx: oneshot::Sender<Option<String>>,
}

#[derive(Debug)]
enum WorkerCommand {
    CreateSession {
        title: String,
        response: oneshot::Sender<anyhow::Result<SessionRecord>>,
    },
    ResumeSession {
        session_id: String,
        response: oneshot::Sender<anyhow::Result<()>>,
    },
    Prompt {
        session_id: String,
        text: String,
        command_id: Uuid,
        response: oneshot::Sender<anyhow::Result<PromptAccepted>>,
    },
    Cancel {
        session_id: String,
        response: oneshot::Sender<anyhow::Result<()>>,
    },
}

/// Result returned after a prompt has been durably accepted for execution.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptAccepted {
    pub command_id: Uuid,
    pub duplicate: bool,
}

/// Handle used by the loopback API to control one supervised ACP worker.
#[derive(Clone)]
pub struct AcpHandle {
    command_tx: mpsc::Sender<WorkerCommand>,
    status: Arc<RwLock<AgentStatus>>,
    event_tx: broadcast::Sender<LocalEvent>,
    pending_permissions: Arc<Mutex<HashMap<Uuid, PendingPermission>>>,
}

impl AcpHandle {
    /// Start an ACP worker without blocking the HTTP server on agent availability.
    pub fn start(store: EventStore, workspace: WorkspaceRecord, command: String) -> Self {
        let (command_tx, command_rx) = mpsc::channel(64);
        let (event_tx, _) = broadcast::channel(512);
        let status = Arc::new(RwLock::new(AgentStatus::starting(&command)));
        let pending_permissions = Arc::new(Mutex::new(HashMap::new()));

        let worker_status = Arc::clone(&status);
        let worker_event_tx = event_tx.clone();
        let worker_permissions = Arc::clone(&pending_permissions);
        tokio::spawn(async move {
            if let Err(error) = store.mark_workspace_sessions_offline(workspace.id).await {
                tracing::error!(%error, "could not mark persisted sessions for ACP resume");
            }
            let result = run_worker(
                store.clone(),
                workspace.clone(),
                command_rx,
                worker_event_tx.clone(),
                Arc::clone(&worker_status),
                worker_permissions,
                command,
            )
            .await;

            let next_status = match result {
                Ok(()) => AgentStatus {
                    state: AgentState::Stopped,
                    detail: "ACP worker stopped".into(),
                    agent_info: None,
                    capabilities: None,
                },
                Err(error) => {
                    tracing::error!(error = %error, "ACP worker stopped with an error");
                    let payload = json!({ "message": error.to_string() });
                    if let Err(store_error) = publish_event(
                        &store,
                        workspace.id,
                        None,
                        "host_error",
                        payload,
                        &worker_event_tx,
                    )
                    .await
                    {
                        tracing::error!(error = %store_error, "could not persist ACP failure");
                    }
                    AgentStatus {
                        state: AgentState::Error,
                        detail: error.to_string(),
                        agent_info: None,
                        capabilities: None,
                    }
                }
            };
            *worker_status.write().await = next_status;
        });

        Self {
            command_tx,
            status,
            event_tx,
            pending_permissions,
        }
    }

    pub async fn status(&self) -> AgentStatus {
        self.status.read().await.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LocalEvent> {
        self.event_tx.subscribe()
    }

    pub async fn create_session(&self, title: String) -> anyhow::Result<SessionRecord> {
        let (response, receiver) = oneshot::channel();
        self.command_tx
            .send(WorkerCommand::CreateSession { title, response })
            .await
            .context("ACP worker is unavailable")?;
        receiver.await.context("ACP worker stopped")?
    }

    pub async fn resume_session(&self, session_id: String) -> anyhow::Result<()> {
        let (response, receiver) = oneshot::channel();
        self.command_tx
            .send(WorkerCommand::ResumeSession {
                session_id,
                response,
            })
            .await
            .context("ACP worker is unavailable")?;
        receiver.await.context("ACP worker stopped")?
    }

    pub async fn prompt(
        &self,
        session_id: String,
        text: String,
        command_id: Uuid,
    ) -> anyhow::Result<PromptAccepted> {
        let (response, receiver) = oneshot::channel();
        self.command_tx
            .send(WorkerCommand::Prompt {
                session_id,
                text,
                command_id,
                response,
            })
            .await
            .context("ACP worker is unavailable")?;
        receiver.await.context("ACP worker stopped")?
    }

    pub async fn cancel(&self, session_id: String) -> anyhow::Result<()> {
        let (response, receiver) = oneshot::channel();
        self.command_tx
            .send(WorkerCommand::Cancel {
                session_id,
                response,
            })
            .await
            .context("ACP worker is unavailable")?;
        receiver.await.context("ACP worker stopped")?
    }

    pub async fn decide_permission(
        &self,
        request_id: Uuid,
        option_id: Option<String>,
    ) -> anyhow::Result<()> {
        let mut permissions = self.pending_permissions.lock().await;
        let permission = permissions
            .get(&request_id)
            .context("permission request is no longer pending")?;
        if let Some(option_id) = option_id.as_ref()
            && !permission.option_ids.contains(option_id)
        {
            bail!("permission option is not valid for this request");
        }
        let permission = permissions
            .remove(&request_id)
            .context("permission request is no longer pending")?;
        drop(permissions);
        permission
            .decision_tx
            .send(option_id)
            .map_err(|_| anyhow!("permission request has already expired"))
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "ACP request and notification handlers stay together so the connection lifecycle is auditable"
)]
async fn run_worker(
    store: EventStore,
    workspace: WorkspaceRecord,
    command_rx: mpsc::Receiver<WorkerCommand>,
    event_tx: broadcast::Sender<LocalEvent>,
    status: Arc<RwLock<AgentStatus>>,
    pending_permissions: Arc<Mutex<HashMap<Uuid, PendingPermission>>>,
    command: String,
) -> anyhow::Result<()> {
    let agent =
        AcpAgent::from_str(&command).with_context(|| format!("invalid ACP command `{command}`"))?;
    let notification_store = store.clone();
    let notification_workspace = workspace.clone();
    let notification_event_tx = event_tx.clone();
    let permission_store = store.clone();
    let permission_workspace = workspace.clone();
    let permission_event_tx = event_tx.clone();
    let handler_permissions = Arc::clone(&pending_permissions);

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                let session_id = notification.session_id.to_string();
                match serde_json::to_value(&notification) {
                    Ok(payload) => {
                        if let Err(error) = publish_event(
                            &notification_store,
                            notification_workspace.id,
                            Some(&session_id),
                            "session_update",
                            payload,
                            &notification_event_tx,
                        )
                        .await
                        {
                            tracing::error!(error = %error, "could not persist ACP session update");
                        }
                    }
                    Err(error) => {
                        tracing::error!(error = %error, "could not encode ACP session update");
                    }
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, connection| {
                let request_id = Uuid::now_v7();
                let session_id = request.session_id.to_string();
                let option_ids = request
                    .options
                    .iter()
                    .map(|option| option.option_id.to_string())
                    .collect::<Vec<_>>();
                let payload = match serde_json::to_value(&request) {
                    Ok(request) => json!({ "requestId": request_id, "request": request }),
                    Err(error) => {
                        tracing::error!(error = %error, "could not encode permission request");
                        return responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ));
                    }
                };
                if let Err(error) = publish_event(
                    &permission_store,
                    permission_workspace.id,
                    Some(&session_id),
                    "permission_requested",
                    payload,
                    &permission_event_tx,
                )
                .await
                {
                    tracing::error!(error = %error, "permission denied because its audit event could not be stored");
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }

                let (decision_tx, decision_rx) = oneshot::channel();
                handler_permissions.lock().await.insert(
                    request_id,
                    PendingPermission {
                        session_id: session_id.clone(),
                        option_ids,
                        decision_tx,
                    },
                );

                let task_permissions = Arc::clone(&handler_permissions);
                let task_store = permission_store.clone();
                let task_event_tx = permission_event_tx.clone();
                let task_workspace_id = permission_workspace.id;
                connection.spawn(async move {
                    let selected = tokio::time::timeout(PERMISSION_TIMEOUT, decision_rx)
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .flatten();
                    task_permissions.lock().await.remove(&request_id);
                    let outcome = selected.as_ref().map_or(
                        RequestPermissionOutcome::Cancelled,
                        |option_id| {
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option_id.clone(),
                            ))
                        },
                    );
                    responder.respond(RequestPermissionResponse::new(outcome))?;
                    let payload = json!({
                        "requestId": request_id,
                        "optionId": selected,
                        "status": if selected.is_some() { "selected" } else { "cancelled" }
                    });
                    if let Err(error) = publish_event(
                        &task_store,
                        task_workspace_id,
                        Some(&session_id),
                        "permission_resolved",
                        payload,
                        &task_event_tx,
                    )
                    .await
                    {
                        tracing::error!(error = %error, "could not persist permission result");
                    }
                    Ok(())
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let initialize = connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1).client_info(
                        Implementation::new("leave", env!("CARGO_PKG_VERSION"))
                            .title("Leave CLI"),
                    ),
                )
                .block_task()
                .await?;

            let agent_info = serde_json::to_value(&initialize.agent_info).ok();
            let capabilities = serde_json::to_value(&initialize.agent_capabilities).ok();
            *status.write().await = AgentStatus {
                state: AgentState::Ready,
                detail: "Connected to Devin over ACP v1".into(),
                agent_info,
                capabilities,
            };
            if let Err(error) = publish_event(
                &store,
                workspace.id,
                None,
                "acp_ready",
                serde_json::to_value(&initialize).unwrap_or_else(|_| json!({})),
                &event_tx,
            )
            .await
            {
                tracing::error!(error = %error, "could not persist ACP readiness event");
            }

            command_loop(
                connection,
                command_rx,
                store,
                workspace,
                event_tx,
                pending_permissions,
            )
            .await;
            Ok(())
        })
        .await
        .context("failed to run ACP agent")?;
    Ok(())
}

async fn command_loop(
    connection: ConnectionTo<Agent>,
    mut command_rx: mpsc::Receiver<WorkerCommand>,
    store: EventStore,
    workspace: WorkspaceRecord,
    event_tx: broadcast::Sender<LocalEvent>,
    pending_permissions: Arc<Mutex<HashMap<Uuid, PendingPermission>>>,
) {
    while let Some(command) = command_rx.recv().await {
        match command {
            WorkerCommand::CreateSession { title, response } => {
                let result =
                    create_session(&connection, &store, &workspace, &event_tx, title).await;
                let _ = response.send(result);
            }
            WorkerCommand::ResumeSession {
                session_id,
                response,
            } => {
                let result =
                    resume_session(&connection, &store, &workspace, &event_tx, session_id).await;
                let _ = response.send(result);
            }
            WorkerCommand::Prompt {
                session_id,
                text,
                command_id,
                response,
            } => {
                let result = accept_prompt(
                    &connection,
                    &store,
                    &workspace,
                    &event_tx,
                    session_id,
                    text,
                    command_id,
                )
                .await;
                let _ = response.send(result);
            }
            WorkerCommand::Cancel {
                session_id,
                response,
            } => {
                let result = cancel_session(
                    &connection,
                    &store,
                    &workspace,
                    &event_tx,
                    &pending_permissions,
                    session_id,
                )
                .await;
                let _ = response.send(result);
            }
        }
    }
}

async fn create_session(
    connection: &ConnectionTo<Agent>,
    store: &EventStore,
    workspace: &WorkspaceRecord,
    event_tx: &broadcast::Sender<LocalEvent>,
    title: String,
) -> anyhow::Result<SessionRecord> {
    let response = connection
        .send_request(NewSessionRequest::new(&workspace.canonical_path))
        .block_task()
        .await
        .context("Devin rejected session creation")?;
    let now = now_unix_millis();
    let session = SessionRecord {
        session_id: response.session_id.to_string(),
        workspace_id: workspace.id,
        title: normalize_title(&title),
        state: "idle".into(),
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    store.upsert_session(&session).await?;
    publish_event(
        store,
        workspace.id,
        Some(&session.session_id),
        "session_created",
        json!({ "session": session, "acp": response }),
        event_tx,
    )
    .await?;
    Ok(session)
}

async fn resume_session(
    connection: &ConnectionTo<Agent>,
    store: &EventStore,
    workspace: &WorkspaceRecord,
    event_tx: &broadcast::Sender<LocalEvent>,
    session_id: String,
) -> anyhow::Result<()> {
    use agent_client_protocol::schema::v1::ResumeSessionRequest;

    connection
        .send_request(ResumeSessionRequest::new(
            session_id.clone(),
            &workspace.canonical_path,
        ))
        .block_task()
        .await
        .context("Devin could not resume this session")?;
    store.update_session_state(&session_id, "idle").await?;
    publish_event(
        store,
        workspace.id,
        Some(&session_id),
        "session_resumed",
        json!({ "sessionId": session_id }),
        event_tx,
    )
    .await?;
    Ok(())
}

async fn accept_prompt(
    connection: &ConnectionTo<Agent>,
    store: &EventStore,
    workspace: &WorkspaceRecord,
    event_tx: &broadcast::Sender<LocalEvent>,
    session_id: String,
    text: String,
    command_id: Uuid,
) -> anyhow::Result<PromptAccepted> {
    if text.trim().is_empty() {
        bail!("prompt cannot be empty");
    }
    let session = store
        .get_session(workspace.id, &session_id)
        .await?
        .context("session is not registered in this workspace")?;
    if session.state == "working" {
        bail!("this session already has an active Devin turn");
    }
    if store.claim_command(command_id).await? == CommandClaim::Duplicate {
        return Ok(PromptAccepted {
            command_id,
            duplicate: true,
        });
    }
    if !store.update_session_state(&session_id, "working").await? {
        bail!("session disappeared before the prompt was accepted");
    }
    publish_event(
        store,
        workspace.id,
        Some(&session_id),
        "user_prompt",
        json!({ "commandId": command_id, "text": text }),
        event_tx,
    )
    .await?;

    let prompt_connection = connection.clone();
    let prompt_store = store.clone();
    let prompt_event_tx = event_tx.clone();
    let workspace_id = workspace.id;
    let task_session_id = session_id.clone();
    connection.spawn(async move {
        let result = prompt_connection
            .send_request(PromptRequest::new(
                task_session_id.clone(),
                vec![ContentBlock::Text(TextContent::new(text))],
            ))
            .block_task()
            .await;
        let (kind, payload) = match result {
            Ok(response) => (
                "prompt_completed",
                serde_json::to_value(&response).unwrap_or_else(|_| json!({})),
            ),
            Err(error) => ("prompt_failed", json!({ "message": error.to_string() })),
        };
        if let Err(error) = prompt_store
            .update_session_state(&task_session_id, "idle")
            .await
        {
            tracing::error!(error = %error, "could not update session state");
        }
        if let Err(error) = publish_event(
            &prompt_store,
            workspace_id,
            Some(&task_session_id),
            kind,
            payload,
            &prompt_event_tx,
        )
        .await
        {
            tracing::error!(error = %error, "could not persist prompt completion");
        }
        Ok(())
    })?;

    Ok(PromptAccepted {
        command_id,
        duplicate: false,
    })
}

async fn cancel_session(
    connection: &ConnectionTo<Agent>,
    store: &EventStore,
    workspace: &WorkspaceRecord,
    event_tx: &broadcast::Sender<LocalEvent>,
    pending_permissions: &Arc<Mutex<HashMap<Uuid, PendingPermission>>>,
    session_id: String,
) -> anyhow::Result<()> {
    connection
        .send_notification(CancelNotification::new(session_id.clone()))
        .context("could not send ACP cancellation")?;

    let request_ids = pending_permissions
        .lock()
        .await
        .iter()
        .filter_map(|(request_id, pending)| {
            (pending.session_id == session_id).then_some(*request_id)
        })
        .collect::<Vec<_>>();
    for request_id in request_ids {
        if let Some(pending) = pending_permissions.lock().await.remove(&request_id) {
            let _ = pending.decision_tx.send(None);
        }
    }
    store.update_session_state(&session_id, "idle").await?;
    publish_event(
        store,
        workspace.id,
        Some(&session_id),
        "session_cancelled",
        json!({ "sessionId": session_id }),
        event_tx,
    )
    .await?;
    Ok(())
}

pub async fn publish_event(
    store: &EventStore,
    workspace_id: Uuid,
    session_id: Option<&str>,
    kind: &str,
    payload: Value,
    event_tx: &broadcast::Sender<LocalEvent>,
) -> anyhow::Result<LocalEvent> {
    let bytes = serde_json::to_vec(&payload)?;
    let record = store
        .append_event(workspace_id, session_id, kind, &bytes)
        .await?;
    let event = LocalEvent::from_record(record);
    let _ = event_tx.send(event.clone());
    Ok(event)
}

fn normalize_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "New Devin session".into()
    } else {
        title.chars().take(120).collect()
    }
}

fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}
