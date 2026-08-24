//! Blind metadata and ciphertext relay for Leave endpoints.
//!
//! The relay routes opaque frames between endpoints that hold a route token.
//! It cannot decrypt a frame or authorize a workspace action.

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
    routing::{delete, get, post},
};
use futures_util::{SinkExt, StreamExt};
use leave_crypto::CryptoReleaseStatus;
use leave_protocol::{PROTOCOL_VERSION, validate_envelope, wire::RelayEnvelope};
use prost::Message as ProstMessage;
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
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

mod routes;

use routes::{RegisteredRoute, RouteTable, RoutedFrame, next_endpoint_id};

const MAX_FRAMES_PER_SECOND: u32 = 100;

#[derive(Clone)]
struct AppState {
    demo_mode: bool,
    public_origin: HeaderValue,
    routes: Arc<Mutex<RouteTable>>,
    /// Secret a host presents to register a route, until hosted accounts exist.
    registration_secret: Option<Arc<str>>,
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

/// Configuration for one relay process.
pub struct RelayConfig {
    /// Allow loopback-only development operation without the release gate.
    pub demo_mode: bool,
    /// Origin a browser endpoint must present.
    pub public_origin: HeaderValue,
    /// Secret a host presents to register a route.
    pub registration_secret: Option<Arc<str>>,
    /// Optional metadata database.
    pub postgres: Option<PgPool>,
    /// Optional presence and fanout cache.
    pub redis: Option<redis::Client>,
}

/// Build the relay's HTTP and WebSocket router.
pub fn router(config: RelayConfig) -> Router {
    let public_origin = config.public_origin.clone();
    let state = AppState {
        demo_mode: config.demo_mode,
        public_origin: config.public_origin,
        routes: Arc::new(Mutex::new(RouteTable::default())),
        registration_secret: config.registration_secret,
        postgres: config.postgres,
        redis: config.redis,
    };
    Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/api/v1/status", get(health))
        .route("/api/v1/hosts", get(protected_not_ready))
        .route("/api/v1/workspaces", get(protected_not_ready))
        .route("/api/v1/devices", get(protected_not_ready))
        .route("/api/v1/organizations", get(protected_not_ready))
        .route("/api/v1/routes", post(register_route))
        .route("/api/v1/routes/{route_id}", delete(revoke_route))
        .route("/api/v1/ws/{route_id}", get(websocket))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(CorsLayer::new().allow_origin(public_origin))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
}

/// Read the process environment into a relay configuration.
///
/// # Errors
///
/// Returns an error when the release gate blocks internet-routed operation,
/// when the bind address is unusable, or when a configured database is
/// unreachable.
pub async fn config_from_environment() -> anyhow::Result<(RelayConfig, SocketAddr)> {
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
    let registration_secret = env::var("LEAVE_RELAY_REGISTRATION_SECRET")
        .ok()
        .filter(|secret| !secret.is_empty())
        .map(Arc::from);
    if registration_secret.is_none() {
        tracing::warn!("LEAVE_RELAY_REGISTRATION_SECRET is unset; route registration is refused");
    }
    Ok((
        RelayConfig {
            demo_mode,
            public_origin,
            registration_secret,
            postgres: connect_postgres().await?,
            redis: connect_redis().await?,
        },
        bind,
    ))
}

/// Serve the relay until the process is asked to stop.
///
/// # Errors
///
/// Returns an error when the listener cannot bind or the server fails.
pub async fn serve(config: RelayConfig, bind: SocketAddr) -> anyhow::Result<()> {
    let demo_mode = config.demo_mode;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, demo_mode, "Leave relay listening");
    axum::serve(listener, router(config))
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

/// Header carrying the bearer token that authorizes attaching to a route.
const ROUTE_TOKEN_HEADER: &str = "x-leave-route-token";
/// Header carrying the secret that authorizes registering a new route.
const REGISTRATION_HEADER: &str = "x-leave-registration-secret";
/// Subprotocol a browser uses to pass its token, which cannot set headers.
const TOKEN_SUBPROTOCOL_PREFIX: &str = "leave-token.";

async fn register_route(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RegisteredRoute>, (StatusCode, Json<ApiError>)> {
    let Some(expected) = state.registration_secret.as_deref() else {
        return Err(unauthorized("route registration is not enabled here"));
    };
    let presented = headers
        .get(REGISTRATION_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !leave_crypto::subtle_eq(expected.as_bytes(), presented.as_bytes()) {
        return Err(unauthorized("the registration secret was not accepted"));
    }
    let route = state.routes.lock().await.register();
    tracing::info!(route_id = %route.route_id, "registered a route");
    Ok(Json(route))
}

async fn revoke_route(
    State(state): State<AppState>,
    Path(route_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let token = presented_token(&headers);
    {
        let routes = state.routes.lock().await;
        if routes.authorize(&route_id, &token).is_none() {
            return Err(unauthorized("this token cannot revoke that route"));
        }
    }
    state.routes.lock().await.revoke(&route_id);
    tracing::info!(route_id = %route_id, "revoked a route");
    Ok(StatusCode::NO_CONTENT)
}

async fn websocket(
    State(state): State<AppState>,
    Path(route_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if route_id.is_empty() || route_id.len() > 128 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // A browser cannot set a request header on a WebSocket, so the token may
    // also arrive as a subprotocol. Either way it is checked before upgrade.
    let token = presented_token(&headers);
    let (sender, receiver) = {
        let routes = state.routes.lock().await;
        let Some(route) = routes.authorize(&route_id, &token) else {
            return unauthorized("this token cannot attach to that route").into_response();
        };
        (route.sender(), route.subscribe())
    };
    // Browser endpoints are additionally held to the relay's own origin.
    if headers.contains_key(ORIGIN) && headers.get(ORIGIN) != Some(&state.public_origin) {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiError {
                code: "origin_denied",
                message: "WebSocket origin was not allowed.",
            }),
        )
            .into_response();
    }
    ws.max_message_size(leave_protocol::MAX_CIPHERTEXT_BYTES + 1024)
        .max_frame_size(leave_protocol::MAX_CIPHERTEXT_BYTES + 1024)
        .on_upgrade(move |socket| route_socket(route_id, sender, receiver, socket))
}

/// Read the route token from either the header or the WebSocket subprotocol.
fn presented_token(headers: &HeaderMap) -> String {
    if let Some(token) = headers
        .get(ROUTE_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
    {
        return token.to_owned();
    }
    headers
        .get(http::header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .map(str::trim)
                .find_map(|entry| entry.strip_prefix(TOKEN_SUBPROTOCOL_PREFIX))
        })
        .unwrap_or_default()
        .to_owned()
}

fn unauthorized(message: &'static str) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiError {
            code: "route_unauthorized",
            message,
        }),
    )
}

async fn route_socket(
    route_id: String,
    sender: broadcast::Sender<RoutedFrame>,
    mut receiver: broadcast::Receiver<RoutedFrame>,
    socket: WebSocket,
) {
    let endpoint_id = next_endpoint_id();
    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
        loop {
            tokio::select! {
                frame = receiver.recv() => match frame {
                    // Never echo a frame back to the endpoint that sent it.
                    Ok(frame) if frame.0 == endpoint_id => {}
                    Ok(frame) => if sink.send(Message::Binary(Bytes::from(frame.1.clone()))).await.is_err() { break; },
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
                // A send fails only when nothing else is attached, which is
                // normal while one endpoint is offline.
                let _ = sender.send(RoutedFrame::new((endpoint_id, frame.to_vec())));
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
