//! Serving the workspace API over the encrypted relay channel.
//!
//! A remote device sends the same request it would send to the loopback API,
//! sealed in an MLS frame. This module decrypts it, decides whether the device
//! that actually sent it is allowed to make that request, dispatches it into
//! the same router the local listener serves, and seals the answer.
//!
//! The order matters. Authorization happens after decryption and against the
//! identity the MLS layer authenticated, never against a field inside the
//! request. A device cannot claim a role by asking for one.

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use leave_crypto::{OpenedMessage, WorkspaceSession};
use leave_protocol::{
    Capability, Role, authorize,
    tunnel::{
        MAX_TUNNEL_BODY_BYTES, TUNNEL_VERSION, TunnelRequest, TunnelResponse, action_for, decode,
        encode,
    },
};
use leave_transport::RelayClient;
use std::collections::{BTreeMap, BTreeSet};
use tower::ServiceExt as _;

/// What one paired device is allowed to do.
#[derive(Debug, Clone)]
pub struct DeviceGrant {
    /// Collaboration role.
    pub role: Role,
    /// Sensitive capabilities granted on top of the role.
    pub capabilities: BTreeSet<Capability>,
}

impl DeviceGrant {
    /// The grant a newly paired phone receives.
    ///
    /// An operator can drive sessions and answer ordinary approvals, and holds
    /// no sensitive capability until the owner adds one.
    #[must_use]
    pub fn newly_paired() -> Self {
        Self {
            role: Role::Operator,
            capabilities: BTreeSet::new(),
        }
    }
}

/// Roles and capabilities for every device paired with this host.
#[derive(Debug, Clone, Default)]
pub struct DeviceRegistry {
    grants: BTreeMap<String, DeviceGrant>,
}

impl DeviceRegistry {
    /// Record what a device may do.
    pub fn set(&mut self, device_id: &str, grant: DeviceGrant) {
        self.grants.insert(device_id.to_owned(), grant);
    }

    /// Forget a device, which then loses remote access entirely.
    pub fn remove(&mut self, device_id: &str) -> bool {
        self.grants.remove(device_id).is_some()
    }

    /// What this device may do, if anything.
    #[must_use]
    pub fn get(&self, device_id: &str) -> Option<&DeviceGrant> {
        self.grants.get(device_id)
    }
}

/// Decide whether a device may make one request, and say why not.
///
/// A device that decrypted successfully but is not in the registry is refused:
/// membership of the MLS group is not by itself permission to act.
fn permitted(registry: &DeviceRegistry, device_id: &str, method: &str, path: &str) -> bool {
    let Some(grant) = registry.get(device_id) else {
        return false;
    };
    authorize(grant.role, &grant.capabilities, action_for(method, path))
}

/// Turn one decrypted request into the host's answer.
///
/// Errors are reported to the device as a status, never as a dropped
/// connection, so a phone can tell "not allowed" from "host is gone".
async fn handle(
    router: Router,
    registry: &DeviceRegistry,
    opened: &OpenedMessage,
) -> TunnelResponse {
    let request: TunnelRequest = match decode(&opened.plaintext) {
        Ok(request) => request,
        Err(_) => return refusal("unknown", StatusCode::BAD_REQUEST, "malformed request"),
    };
    if let Err(error) = request.validate() {
        return refusal(
            &request.exchange_id,
            StatusCode::BAD_REQUEST,
            &error.to_string(),
        );
    }
    if !permitted(
        registry,
        &opened.sender_device_id,
        &request.method,
        &request.path,
    ) {
        tracing::warn!(
            device = %opened.sender_device_id,
            method = %request.method,
            path = %request.path,
            "refused a remote request that the device's role does not allow"
        );
        return refusal(
            &request.exchange_id,
            StatusCode::FORBIDDEN,
            "this device is not allowed to do that",
        );
    }

    match dispatch(router, &request).await {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, "could not serve a tunnelled request");
            refusal(
                &request.exchange_id,
                StatusCode::INTERNAL_SERVER_ERROR,
                "the host could not complete that request",
            )
        }
    }
}

/// Call the workspace router in-process and collect its answer.
async fn dispatch(router: Router, request: &TunnelRequest) -> Result<TunnelResponse> {
    let mut builder = Request::builder()
        .method(request.method.as_str())
        .uri(request.path.as_str());
    for (name, value) in &request.headers {
        // Hop-by-hop and transport headers belong to the loopback listener,
        // not to a tunnelled request.
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "connection" | "upgrade" | "transfer-encoding" | "content-length"
        ) {
            continue;
        }
        builder = builder.header(name, value);
    }
    let http_request = builder
        .body(Body::from(request.body.clone()))
        .context("could not build the tunnelled request")?;

    let response = router
        .oneshot(http_request)
        .await
        .context("the workspace router failed")?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    let body = to_bytes(response.into_body(), MAX_TUNNEL_BODY_BYTES)
        .await
        .context("the response body was too large to carry")?;
    Ok(TunnelResponse {
        version: TUNNEL_VERSION,
        exchange_id: request.exchange_id.clone(),
        status,
        headers,
        body: body.to_vec(),
    })
}

/// The device a request asks to revoke, if that is what it is asking.
fn revocation_target(opened: &OpenedMessage) -> Option<String> {
    let request: TunnelRequest = decode(&opened.plaintext).ok()?;
    if request.method != "DELETE" {
        return None;
    }
    let device = request.path.strip_prefix("/api/v1/local/devices/")?;
    (!device.is_empty() && !device.contains('/')).then(|| device.to_owned())
}

/// Remove a device from the workspace and rotate the group past it.
///
/// Only an owner may do this, and only to a device that is not itself. The
/// group commit is what actually ends access: after it, the removed phone
/// cannot read anything new even if it still holds the route token.
fn revoke(
    session: &mut WorkspaceSession,
    registry: &mut DeviceRegistry,
    opened: &OpenedMessage,
    target: &str,
) -> TunnelResponse {
    let exchange_id = decode::<TunnelRequest>(&opened.plaintext)
        .map(|request| request.exchange_id)
        .unwrap_or_default();
    let allowed = registry.get(&opened.sender_device_id).is_some_and(|grant| {
        authorize(
            grant.role,
            &grant.capabilities,
            leave_protocol::Action::ManageMembers,
        )
    });
    if !allowed {
        return refusal(
            &exchange_id,
            StatusCode::FORBIDDEN,
            "only a workspace owner may revoke a device",
        );
    }
    if target == opened.sender_device_id {
        return refusal(
            &exchange_id,
            StatusCode::BAD_REQUEST,
            "a device cannot revoke itself",
        );
    }
    match session.remove_device(target) {
        Ok(_commit) => {
            registry.remove(target);
            tracing::info!(device = %target, "revoked a device and rotated the group");
            TunnelResponse {
                version: TUNNEL_VERSION,
                exchange_id,
                status: StatusCode::NO_CONTENT.as_u16(),
                headers: BTreeMap::new(),
                body: Vec::new(),
            }
        }
        Err(error) => {
            tracing::warn!(%error, device = %target, "could not revoke a device");
            refusal(
                &exchange_id,
                StatusCode::NOT_FOUND,
                "that device is not a member of this workspace",
            )
        }
    }
}

/// The group member that holds no grant yet, which is the one just admitted.
fn newest_device(session: &WorkspaceSession, registry: &DeviceRegistry) -> Option<String> {
    session
        .member_device_ids()
        .ok()?
        .into_iter()
        .find(|device| device != session.device_id() && registry.get(device).is_none())
}

fn refusal(exchange_id: &str, status: StatusCode, message: &str) -> TunnelResponse {
    TunnelResponse {
        version: TUNNEL_VERSION,
        exchange_id: exchange_id.to_owned(),
        status: status.as_u16(),
        headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
        body: format!("{{\"error\":{{\"message\":{message:?}}}}}").into_bytes(),
    }
}

/// Everything one served relay route needs.
pub struct RouteService {
    /// The attached relay connection.
    pub client: RelayClient,
    /// This host's view of the workspace group.
    pub session: WorkspaceSession,
    /// What each paired device may do.
    pub registry: DeviceRegistry,
    /// The workspace API, identical to the one the loopback listener serves.
    pub router: Router,
    /// Secret from the pairing code, while an invitation is open.
    pub pairing: Option<leave_crypto::PairingSecret>,
}

/// Serve the workspace over one attached relay route until it closes.
///
/// # Errors
///
/// Returns an error when the relay connection fails. A refused or malformed
/// request is answered, not fatal.
pub async fn serve_route(service: RouteService) -> Result<()> {
    let RouteService {
        mut client,
        mut session,
        mut registry,
        router,
        mut pairing,
    } = service;
    while let Some(frame) = client.recv().await? {
        if leave_crypto::is_pairing_frame(&frame) {
            // A pairing code admits exactly one device, then closes.
            let Some(secret) = pairing.as_ref() else {
                tracing::warn!("ignored a pairing request while no code was showing");
                continue;
            };
            match leave_crypto::accept_pairing(&mut session, &frame, secret) {
                Ok(invitation) => {
                    client
                        .send(&leave_crypto::pairing_welcome(&invitation))
                        .await?;
                    if let Some(device) = newest_device(&session, &registry) {
                        tracing::info!(device = %device, "paired a new device");
                        registry.set(&device, DeviceGrant::newly_paired());
                    }
                    pairing = None;
                }
                Err(error) => {
                    tracing::warn!(%error, "refused a pairing request");
                }
            }
            continue;
        }
        let opened = match session.open(&frame) {
            Ok(opened) => opened,
            Err(error) => {
                // A frame that will not open is not necessarily an attack: it
                // may be one this device already processed, or one for another
                // member. Dropping it is correct and must not end the route.
                tracing::debug!(%error, "ignored a frame this host could not open");
                continue;
            }
        };
        if let Some(target) = revocation_target(&opened) {
            let response = revoke(&mut session, &mut registry, &opened, &target);
            let payload = encode(&response).context("could not encode the answer")?;
            let sealed = session
                .seal(&payload)
                .context("could not encrypt the answer")?;
            client.send(&sealed).await?;
            continue;
        }
        let response = handle(router.clone(), &registry, &opened).await;
        let payload = encode(&response).context("could not encode the answer")?;
        let sealed = session
            .seal(&payload)
            .context("could not encrypt the answer")?;
        client.send(&sealed).await?;
    }
    Ok(())
}

/// Register a route, start a workspace group, and show a pairing code.
///
/// # Errors
///
/// Returns an error when the relay refuses registration or the route cannot
/// be attached.
pub async fn open_route(
    relay: &crate::local_server::RelayAccess,
    workspace_id: &str,
    router: Router,
) -> Result<RouteService> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Registered {
        route_id: String,
        token: String,
    }

    let registered: Registered = reqwest::Client::new()
        .post(format!("{}/api/v1/routes", relay.url.trim_end_matches('/')))
        .header("x-leave-registration-secret", &relay.registration_secret)
        .send()
        .await
        .context("could not reach the relay")?
        .error_for_status()
        .context("the relay refused to register a route")?
        .json()
        .await
        .context("the relay returned an unexpected registration answer")?;

    let identity = leave_crypto::DeviceIdentity::generate(&format!("host-{workspace_id}"))
        .context("could not create this host's device identity")?;
    let session = WorkspaceSession::create(identity, workspace_id)
        .context("could not start the workspace group")?;
    let client = RelayClient::attach(&relay.url, &registered.route_id, &registered.token)
        .await
        .context("could not attach to the relay route")?;

    let pairing = leave_crypto::PairingSecret::generate();
    println!(
        "Pair a phone with this code: {}",
        pairing_payload(
            &relay.url,
            &registered.route_id,
            &registered.token,
            &pairing
        )
    );

    Ok(RouteService {
        client,
        session,
        registry: DeviceRegistry::default(),
        router,
        pairing: Some(pairing),
    })
}

/// The single string a pairing QR code carries.
///
/// It holds everything a phone needs and nothing it does not: where the relay
/// is, which route to attach to, the token for that route, and the one-time
/// secret that authorizes joining the group.
#[must_use]
pub fn pairing_payload(
    relay_url: &str,
    route_id: &str,
    token: &str,
    secret: &leave_crypto::PairingSecret,
) -> String {
    let secret = secret
        .expose()
        .iter()
        .fold(String::new(), |mut text, byte| {
            use core::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        });
    format!("leave://pair?relay={relay_url}&route={route_id}&token={token}&secret={secret}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(role: Role, capabilities: &[Capability]) -> DeviceRegistry {
        let mut registry = DeviceRegistry::default();
        registry.set(
            "phone",
            DeviceGrant {
                role,
                capabilities: capabilities.iter().copied().collect(),
            },
        );
        registry
    }

    #[test]
    fn an_unpaired_device_may_do_nothing() {
        let registry = registry_with(Role::Owner, &[]);
        assert!(!permitted(
            &registry,
            "a-device-nobody-paired",
            "GET",
            "/api/v1/local/status"
        ));
    }

    #[test]
    fn a_viewer_may_read_but_not_write() {
        let registry = registry_with(Role::Viewer, &[]);
        assert!(permitted(&registry, "phone", "GET", "/api/v1/local/status"));
        assert!(!permitted(&registry, "phone", "PUT", "/api/v1/local/file"));
        assert!(!permitted(
            &registry,
            "phone",
            "POST",
            "/api/v1/local/sessions/abc/prompts"
        ));
    }

    #[test]
    fn a_newly_paired_phone_can_drive_devin_but_not_edit_files() {
        let mut registry = DeviceRegistry::default();
        registry.set("phone", DeviceGrant::newly_paired());
        assert!(permitted(
            &registry,
            "phone",
            "POST",
            "/api/v1/local/sessions/abc/prompts"
        ));
        assert!(permitted(&registry, "phone", "GET", "/api/v1/local/files"));
        assert!(!permitted(&registry, "phone", "PUT", "/api/v1/local/file"));
        assert!(!permitted(
            &registry,
            "phone",
            "POST",
            "/api/v1/local/git/commit"
        ));
    }

    #[test]
    fn a_terminal_needs_the_grant_even_for_an_owner() {
        let owner = registry_with(Role::Owner, &[]);
        assert!(!permitted(
            &owner,
            "phone",
            "POST",
            "/api/v1/local/terminals"
        ));
        let granted = registry_with(Role::Owner, &[Capability::RawPty]);
        assert!(permitted(
            &granted,
            "phone",
            "POST",
            "/api/v1/local/terminals"
        ));
    }

    #[test]
    fn a_maintainer_may_edit_and_commit() {
        let registry = registry_with(Role::Maintainer, &[]);
        assert!(permitted(&registry, "phone", "PUT", "/api/v1/local/file"));
        assert!(permitted(
            &registry,
            "phone",
            "POST",
            "/api/v1/local/git/commit"
        ));
    }

    #[test]
    fn removing_a_device_ends_its_remote_access() {
        let mut registry = registry_with(Role::Owner, &[Capability::RawPty]);
        assert!(permitted(&registry, "phone", "GET", "/api/v1/local/status"));
        assert!(registry.remove("phone"));
        assert!(!permitted(
            &registry,
            "phone",
            "GET",
            "/api/v1/local/status"
        ));
    }
}

#[cfg(test)]
mod end_to_end {
    use super::*;
    use axum::{Json, routing::get};
    use leave_crypto::DeviceIdentity;
    use leave_relay::{RelayConfig, router as relay_router};
    use std::{sync::Arc, time::Duration};
    use tokio::net::TcpListener;

    /// Stands in for the workspace API, with one readable and one writable
    /// route, so the test exercises the tunnel rather than Devin itself.
    fn workspace_router() -> Router {
        Router::new()
            .route(
                "/api/v1/local/status",
                get(|| async { Json(serde_json::json!({"status": "ok"})) }),
            )
            .route(
                "/api/v1/local/file",
                axum::routing::put(|body: String| async move {
                    Json(serde_json::json!({"written": body.len()}))
                }),
            )
    }

    struct Harness {
        phone: RelayClient,
        phone_session: WorkspaceSession,
    }

    /// Start a relay, pair a phone, and serve the workspace over the route.
    async fn harness(registry: DeviceRegistry) -> Result<Harness> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let config = RelayConfig {
            demo_mode: true,
            public_origin: axum::http::HeaderValue::from_static("http://localhost:5173"),
            registration_secret: Some(Arc::from("secret")),
            postgres: None,
            redis: None,
        };
        tokio::spawn(async move {
            let _ = axum::serve(listener, relay_router(config)).await;
        });
        let url = format!("http://{address}");

        let body: serde_json::Value = reqwest::Client::new()
            .post(format!("{url}/api/v1/routes"))
            .header("x-leave-registration-secret", "secret")
            .send()
            .await?
            .json()
            .await?;
        let route_id = body["routeId"].as_str().unwrap_or_default().to_owned();
        let token = body["token"].as_str().unwrap_or_default().to_owned();

        let mut host_session =
            WorkspaceSession::create(DeviceIdentity::generate("host")?, "workspace-1")?;
        let phone_identity = DeviceIdentity::generate("phone")?;
        let invitation = host_session.add_device(&phone_identity.publish_key_package()?)?;
        let phone_session = WorkspaceSession::join(phone_identity, &invitation.welcome)?;

        let host = RelayClient::attach(&url, &route_id, &token).await?;
        let phone = RelayClient::attach(&url, &route_id, &token).await?;
        tokio::spawn(async move {
            let _ = serve_route(RouteService {
                client: host,
                session: host_session,
                registry,
                router: workspace_router(),
                pairing: None,
            })
            .await;
        });
        Ok(Harness {
            phone,
            phone_session,
        })
    }

    impl Harness {
        /// Make one request the way a remote device would, and read the answer.
        async fn request(
            &mut self,
            method: &str,
            path: &str,
            body: &[u8],
        ) -> Result<TunnelResponse> {
            let request = TunnelRequest {
                version: TUNNEL_VERSION,
                exchange_id: "exchange-1".into(),
                method: method.into(),
                path: path.into(),
                headers: BTreeMap::new(),
                body: body.to_vec(),
            };
            let sealed = self.phone_session.seal(&encode(&request)?)?;
            self.phone.send(&sealed).await?;
            let frame = tokio::time::timeout(Duration::from_secs(5), self.phone.recv())
                .await?
                .transpose()
                .context("the route closed before answering")??;
            let opened = self.phone_session.open(&frame)?;
            Ok(decode(&opened.plaintext)?)
        }
    }

    fn registry(role: Role, capabilities: &[Capability]) -> DeviceRegistry {
        let mut registry = DeviceRegistry::default();
        registry.set(
            "phone",
            DeviceGrant {
                role,
                capabilities: capabilities.iter().copied().collect(),
            },
        );
        registry
    }

    #[tokio::test]
    async fn a_phone_reads_the_workspace_through_the_relay() -> Result<()> {
        let mut harness = harness(registry(Role::Operator, &[])).await?;
        let response = harness.request("GET", "/api/v1/local/status", b"").await?;
        assert_eq!(response.status, 200);
        assert!(String::from_utf8_lossy(&response.body).contains("\"ok\""));
        Ok(())
    }

    #[tokio::test]
    async fn a_role_that_may_not_write_is_refused_over_the_relay() -> Result<()> {
        let mut harness = harness(registry(Role::Operator, &[])).await?;
        let response = harness
            .request("PUT", "/api/v1/local/file", b"contents")
            .await?;
        assert_eq!(
            response.status, 403,
            "an operator must not be able to write files remotely"
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_maintainer_may_write_over_the_relay() -> Result<()> {
        let mut harness = harness(registry(Role::Maintainer, &[])).await?;
        let response = harness
            .request("PUT", "/api/v1/local/file", b"contents")
            .await?;
        assert_eq!(response.status, 200);
        assert!(String::from_utf8_lossy(&response.body).contains("\"written\":8"));
        Ok(())
    }

    #[tokio::test]
    async fn a_device_that_was_never_paired_is_refused() -> Result<()> {
        // The device decrypts, because it is in the group, but holds no grant.
        let mut harness = harness(DeviceRegistry::default()).await?;
        let response = harness.request("GET", "/api/v1/local/status", b"").await?;
        assert_eq!(response.status, 403);
        Ok(())
    }

    #[tokio::test]
    async fn only_an_owner_may_revoke_a_device() -> Result<()> {
        let mut harness = harness(registry(Role::Operator, &[])).await?;
        let response = harness
            .request("DELETE", "/api/v1/local/devices/some-other-phone", b"")
            .await?;
        assert_eq!(
            response.status, 403,
            "an operator must not be able to revoke devices"
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_device_cannot_revoke_itself() -> Result<()> {
        let mut harness = harness(registry(Role::Owner, &[])).await?;
        let response = harness
            .request("DELETE", "/api/v1/local/devices/phone", b"")
            .await?;
        assert_eq!(response.status, 400);
        Ok(())
    }

    #[tokio::test]
    async fn revoking_an_unknown_device_reports_not_found() -> Result<()> {
        let mut harness = harness(registry(Role::Owner, &[])).await?;
        let response = harness
            .request("DELETE", "/api/v1/local/devices/never-paired", b"")
            .await?;
        assert_eq!(response.status, 404);
        Ok(())
    }

    #[tokio::test]
    async fn a_remote_device_cannot_reach_past_the_workspace_api() -> Result<()> {
        let mut harness = harness(registry(Role::Owner, &[])).await?;
        for path in [
            "/index.html",
            "/api/v1/setup/status",
            "/api/v1/local/../secret",
        ] {
            let response = harness.request("GET", path, b"").await?;
            assert_eq!(response.status, 400, "{path} must be refused");
        }
        Ok(())
    }
}
