//! Ephemeral Chromium previews restricted to an approved loopback origin.

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{net::TcpListener, path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    process::Command,
    sync::{Mutex, broadcast, mpsc},
};
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use uuid::Uuid;

/// Metadata for one running managed preview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    /// Host-lifetime preview identifier.
    pub preview_id: Uuid,
    /// Approved loopback URL.
    pub url: String,
}

/// Browser input accepted from the preview tab.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreviewControl {
    /// Navigate within the already-approved origin.
    Navigate { url: String },
    /// Dispatch a mouse click at CSS viewport coordinates.
    Click { x: f64, y: f64 },
    /// Insert text into the focused element.
    Text { text: String },
}

#[derive(Clone)]
pub struct PreviewManager {
    enabled: bool,
    chrome: Option<PathBuf>,
    sessions: Arc<Mutex<std::collections::HashMap<Uuid, PreviewSession>>>,
}

#[derive(Clone)]
struct PreviewSession {
    approved_origin: String,
    controls: mpsc::Sender<PreviewControl>,
    frames: broadcast::Sender<String>,
}

impl PreviewManager {
    /// Construct the managed preview capability.
    #[must_use]
    pub fn new(enabled: bool, chrome: Option<PathBuf>) -> Self {
        Self {
            enabled,
            chrome,
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Whether the owner granted preview and a browser is available.
    #[must_use]
    pub fn available(&self) -> bool {
        self.enabled && self.chrome.is_some()
    }

    /// Start one ephemeral browser profile at an approved loopback URL.
    pub async fn create(&self, url: &str, width: u16, height: u16) -> anyhow::Result<PreviewView> {
        if !self.enabled {
            bail!("preview access is off; restart Leave with --grant-preview");
        }
        let chrome = self
            .chrome
            .clone()
            .context("Chrome for Testing or Chromium was not found")?;
        let approved = validate_loopback_url(url)?;
        if !(320..=2_560).contains(&width) || !(320..=2_560).contains(&height) {
            bail!("preview dimensions must be between 320 and 2560 pixels");
        }
        let preview_id = Uuid::now_v7();
        let session = launch_browser(chrome, approved.clone(), width, height).await?;
        self.sessions.lock().await.insert(preview_id, session);
        Ok(PreviewView {
            preview_id,
            url: approved.to_string(),
        })
    }

    /// Subscribe to current screencast frames. Frames are never persisted.
    pub async fn subscribe(&self, preview_id: Uuid) -> anyhow::Result<broadcast::Receiver<String>> {
        Ok(self
            .sessions
            .lock()
            .await
            .get(&preview_id)
            .context("preview was not found or has expired")?
            .frames
            .subscribe())
    }

    /// Send validated browser input.
    pub async fn control(&self, preview_id: Uuid, control: PreviewControl) -> anyhow::Result<()> {
        let session = self
            .sessions
            .lock()
            .await
            .get(&preview_id)
            .cloned()
            .context("preview was not found or has expired")?;
        if let PreviewControl::Navigate { url } = &control {
            let target = validate_loopback_url(url)?;
            if origin(&target) != session.approved_origin {
                bail!("preview navigation must stay on its approved loopback origin");
            }
        }
        session
            .controls
            .send(control)
            .await
            .context("preview browser stopped")
    }
}

#[allow(clippy::too_many_lines)]
async fn launch_browser(
    chrome: PathBuf,
    approved: Url,
    width: u16,
    height: u16,
) -> anyhow::Result<PreviewSession> {
    let debugging_port = reserve_loopback_port()?;
    let profile = tempfile::tempdir().context("could not create an ephemeral browser profile")?;
    let mut child = Command::new(chrome)
        .arg("--headless=new")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-default-apps")
        .arg("--disable-extensions")
        .arg("--disable-sync")
        .arg("--metrics-recording-only")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg(format!("--remote-debugging-port={debugging_port}"))
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .arg(format!("--window-size={width},{height}"))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("could not start Chromium")?;

    let endpoint = wait_for_page(debugging_port).await.inspect_err(|_error| {
        let _ = child.start_kill();
    })?;
    let (mut socket, _) = tokio_tungstenite::connect_async(&endpoint)
        .await
        .context("could not connect to Chromium DevTools")?;
    let approved_origin = origin(&approved);
    socket
        .send(Message::Text(
            json!({"id": 1, "method": "Page.enable"}).to_string().into(),
        ))
        .await?;
    socket
        .send(Message::Text(
            json!({"id": 2, "method": "Runtime.enable"})
                .to_string()
                .into(),
        ))
        .await?;
    socket
        .send(Message::Text(
            json!({
                "id": 3,
                "method": "Fetch.enable",
                "params": {"patterns": [{"urlPattern": "*", "requestStage": "Request"}]}
            })
            .to_string()
            .into(),
        ))
        .await?;
    socket
        .send(Message::Text(
            json!({"id": 4, "method": "Page.navigate", "params": {"url": approved.as_str()}})
                .to_string()
                .into(),
        ))
        .await?;

    let (controls, mut control_rx) = mpsc::channel::<PreviewControl>(64);
    let (frames, _) = broadcast::channel::<String>(8);
    let task_frames = frames.clone();
    let task_origin = approved_origin.clone();
    tokio::spawn(async move {
        let _profile = profile;
        let mut next_id = 10_u64;
        let mut capture = tokio::time::interval(Duration::from_millis(550));
        capture.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = capture.tick() => {
                    next_id += 1;
                    let message = json!({
                        "id": next_id,
                        "method": "Page.captureScreenshot",
                        "params": {"format": "jpeg", "quality": 72, "fromSurface": true}
                    });
                    if socket.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                }
                control = control_rx.recv() => {
                    let Some(control) = control else { break };
                    for message in control_messages(control, &mut next_id) {
                        if socket.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                    }
                }
                incoming = socket.next() => {
                    let Some(incoming) = incoming else { break };
                    let Ok(Message::Text(text)) = incoming else { continue };
                    let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                    if let Some(data) = value.pointer("/result/data").and_then(Value::as_str) {
                        let _ = task_frames.send(data.to_owned());
                    }
                    if value.get("method").and_then(Value::as_str) == Some("Fetch.requestPaused") {
                        let request_id = value.pointer("/params/requestId").and_then(Value::as_str);
                        let request_url = value.pointer("/params/request/url").and_then(Value::as_str);
                        if let (Some(request_id), Some(request_url)) = (request_id, request_url) {
                            next_id += 1;
                            let allowed = Url::parse(request_url)
                                .is_ok_and(|url| origin(&url) == task_origin);
                            let message = if allowed {
                                json!({"id": next_id, "method": "Fetch.continueRequest", "params": {"requestId": request_id}})
                            } else {
                                json!({"id": next_id, "method": "Fetch.failRequest", "params": {"requestId": request_id, "errorReason": "BlockedByClient"}})
                            };
                            if socket.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                        }
                    }
                }
            }
        }
        let _ = child.kill().await;
    });
    Ok(PreviewSession {
        approved_origin,
        controls,
        frames,
    })
}

fn control_messages(control: PreviewControl, next_id: &mut u64) -> Vec<Value> {
    match control {
        PreviewControl::Navigate { url } => {
            *next_id += 1;
            vec![json!({"id": *next_id, "method": "Page.navigate", "params": {"url": url}})]
        }
        PreviewControl::Click { x, y } => ["mousePressed", "mouseReleased"]
            .into_iter()
            .map(|kind| {
                *next_id += 1;
                json!({
                    "id": *next_id,
                    "method": "Input.dispatchMouseEvent",
                    "params": {"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}
                })
            })
            .collect(),
        PreviewControl::Text { text } => {
            *next_id += 1;
            vec![json!({"id": *next_id, "method": "Input.insertText", "params": {"text": text}})]
        }
    }
}

async fn wait_for_page(port: u16) -> anyhow::Result<String> {
    let endpoint = format!("http://127.0.0.1:{port}/json/list");
    for _ in 0..50 {
        if let Ok(response) = reqwest::get(&endpoint).await
            && let Ok(targets) = response.json::<Vec<Value>>().await
            && let Some(websocket) = targets.iter().find_map(|target| {
                (target.get("type").and_then(Value::as_str) == Some("page"))
                    .then(|| target.get("webSocketDebuggerUrl").and_then(Value::as_str))
                    .flatten()
            })
        {
            return Ok(websocket.to_owned());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("Chromium DevTools did not become ready")
}

fn reserve_loopback_port() -> anyhow::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn validate_loopback_url(value: &str) -> anyhow::Result<Url> {
    let url = Url::parse(value).context("preview URL is invalid")?;
    if url.scheme() != "http"
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("preview URL must be a credential-free HTTP loopback URL");
    }
    let is_loopback = match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if !is_loopback {
        bail!("preview navigation is restricted to loopback origins");
    }
    Ok(url)
}

fn origin(url: &Url) -> String {
    format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default().unwrap_or(80)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_rejects_non_loopback_and_credentials() {
        assert!(validate_loopback_url("https://example.com").is_err());
        assert!(validate_loopback_url("http://user@localhost:3000").is_err());
        assert!(validate_loopback_url("http://127.0.0.1:3000/app").is_ok());
    }
}
