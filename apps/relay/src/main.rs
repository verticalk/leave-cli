//! Blind metadata and ciphertext relay for Leave endpoints.

use anyhow::{Context, bail};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header::ORIGIN},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use leave_crypto::CryptoReleaseStatus;
use leave_protocol::{PROTOCOL_VERSION, validate_envelope, wire::RelayEnvelope};
use prost::Message as ProstMessage;
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, broadcast};
use tower_http::{
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

const CHANNEL_CAPACITY: usize = 256;
const MAX_FRAMES_PER_SECOND: u32 = 100;

#[derive(Clone)]
struct AppState {
    demo_mode: bool,
    public_origin: HeaderValue,
    routes: Arc<Mutex<HashMap<String, broadcast::Sender<Vec<u8>>>>>,
    postgres: Option<PgPool>,
    redis: Option<redis::Client>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    protocol_version: u32,
    mode: &'static str,
    postgres: &'static str,
    redis: &'static str,
    crypto_gate: CryptoReleaseStatus,
}

#[derive(Serialize)]
struct ApiError {
    code: &'static str,
    message: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("leave=info,tower_http=info")),
        )
        .json()
        .init();

    let demo_mode = env_bool("LEAVE_DEMO_MODE");
    if !demo_mode {
        leave_crypto::require_remote_release().context(
            "internet-routed relay startup refused; use LEAVE_DEMO_MODE=true only on a local development machine",
        )?;
    }
    let bind: SocketAddr = env::var("LEAVE_RELAY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()
        .context("LEAVE_RELAY_BIND must be a socket address")?;
    if demo_mode && !bind.ip().is_loopback() {
        bail!("demo mode may only bind a loopback address")
    }
    let public_origin = HeaderValue::from_str(
        &env::var("LEAVE_PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into()),
    )?;
    let postgres = connect_postgres().await?;
    let redis = connect_redis().await?;
    let state = AppState {
        demo_mode,
        public_origin: public_origin.clone(),
        routes: Arc::new(Mutex::new(HashMap::new())),
        postgres,
        redis,
    };
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/api/v1/status", get(health))
        .route("/api/v1/hosts", get(protected_not_ready))
        .route("/api/v1/workspaces", get(protected_not_ready))
        .route("/api/v1/devices", get(protected_not_ready))
        .route("/api/v1/organizations", get(protected_not_ready))
        .route("/api/v1/ws/{route_id}", get(websocket))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(CorsLayer::new().allow_origin(public_origin))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, demo_mode, "Leave relay listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn connect_postgres() -> anyhow::Result<Option<PgPool>> {
    let Ok(url) = env::var("LEAVE_POSTGRES_URL") else {
        return Ok(None);
    };
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await?;
    for statement in include_str!("../../../migrations/relay.sql").split(";\n") {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement).execute(&pool).await?;
        }
    }
    Ok(Some(pool))
}

async fn connect_redis() -> anyhow::Result<Option<redis::Client>> {
    let Ok(url) = env::var("LEAVE_REDIS_URL") else {
        return Ok(None);
    };
    let client = redis::Client::open(url)?;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let _: String = redis::cmd("PING").query_async(&mut connection).await?;
    Ok(Some(client))
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let postgres = if let Some(pool) = &state.postgres {
        if sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pool)
            .await
            .is_ok()
        {
            "ready"
        } else {
            "error"
        }
    } else {
        "not_configured"
    };
    let redis = if let Some(client) = &state.redis {
        match client.get_multiplexed_async_connection().await {
            Ok(mut connection) => {
                let result: redis::RedisResult<String> =
                    redis::cmd("PING").query_async(&mut connection).await;
                if result.is_ok() { "ready" } else { "error" }
            }
            Err(_) => "error",
        }
    } else {
        "not_configured"
    };
    Json(HealthResponse {
        status: if state.demo_mode {
            "development"
        } else {
            "ready"
        },
        version: env!("CARGO_PKG_VERSION"),
        protocol_version: PROTOCOL_VERSION,
        mode: if state.demo_mode {
            "loopback_demo"
        } else {
            "production"
        },
        postgres,
        redis,
        crypto_gate: CryptoReleaseStatus::current(),
    })
}

async fn protected_not_ready() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiError {
            code: "enrollment_disabled",
            message: "Passkey enrollment and MLS provisioning are blocked by the crypto release gate.",
        }),
    )
}

async fn openapi() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "openapi": "3.1.0",
        "info": { "title": "Leave Relay API", "version": env!("CARGO_PKG_VERSION") },
        "paths": {
            "/healthz": { "get": { "summary": "Relay readiness" } },
            "/api/v1/status": { "get": { "summary": "Public protocol status" } },
            "/api/v1/hosts": { "get": { "summary": "List authorized hosts", "security": [{"passkeySession": []}] } },
            "/api/v1/workspaces": { "get": { "summary": "List routed workspaces", "security": [{"passkeySession": []}] } },
            "/api/v1/devices": { "get": { "summary": "List trusted devices", "security": [{"passkeySession": []}] } },
            "/api/v1/organizations": { "get": { "summary": "List organizations", "security": [{"passkeySession": []}] } }
        },
        "components": { "securitySchemes": { "passkeySession": { "type": "apiKey", "in": "cookie", "name": "leave_session" } } }
    }))
}

async fn websocket(
    State(state): State<AppState>,
    Path(route_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !state.demo_mode {
        return protected_not_ready().await.into_response();
    }
    if headers.get(ORIGIN) != Some(&state.public_origin) {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiError {
                code: "origin_denied",
                message: "WebSocket origin was not allowed.",
            }),
        )
            .into_response();
    }
    if route_id.is_empty() || route_id.len() > 128 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    ws.max_message_size(leave_protocol::MAX_CIPHERTEXT_BYTES + 1024)
        .max_frame_size(leave_protocol::MAX_CIPHERTEXT_BYTES + 1024)
        .on_upgrade(move |socket| route_socket(state, route_id, socket))
}

async fn route_socket(state: AppState, route_id: String, socket: WebSocket) {
    let sender = {
        let mut routes = state.routes.lock().await;
        routes
            .entry(route_id.clone())
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
            .clone()
    };
    let mut receiver = sender.subscribe();
    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
        loop {
            tokio::select! {
                frame = receiver.recv() => match frame {
                    Ok(frame) => if sink.send(Message::Binary(Bytes::from(frame))).await.is_err() { break; },
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = heartbeat.tick() => if sink.send(Message::Ping(Bytes::new())).await.is_err() { break; },
            }
        }
    });

    let mut window_started = Instant::now();
    let mut frames = 0_u32;
    while let Some(message) = stream.next().await {
        let Ok(message) = message else { break };
        match message {
            Message::Binary(frame) => {
                if window_started.elapsed() >= Duration::from_secs(1) {
                    window_started = Instant::now();
                    frames = 0;
                }
                frames = frames.saturating_add(1);
                if frames > MAX_FRAMES_PER_SECOND {
                    break;
                }
                let Ok(envelope) = RelayEnvelope::decode(frame.clone()) else {
                    break;
                };
                if envelope.route_id != route_id || validate_envelope(&envelope).is_err() {
                    break;
                }
                if sender.send(frame.to_vec()).is_err() {
                    break;
                }
            }
            Message::Close(_) | Message::Text(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }
    writer.abort();
}

fn env_bool(name: &str) -> bool {
    env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"))
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
