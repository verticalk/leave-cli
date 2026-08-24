//! End-to-end checks of the encrypted transport.
//!
//! These run a real relay on a loopback port, attach two endpoints to one
//! route, and exchange real MLS frames between them. The point is to prove
//! two things together: that workspace traffic survives the round trip, and
//! that the relay carrying it cannot read or forge any of it.

use axum::http::HeaderValue;
use leave_crypto::{DeviceIdentity, WorkspaceSession};
use leave_relay::{RelayConfig, router};
use leave_transport::RelayClient;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};

/// Secret the test relay accepts for route registration.
const REGISTRATION_SECRET: &str = "test-registration-secret";

struct TestRelay {
    address: SocketAddr,
    server: JoinHandle<()>,
}

impl TestRelay {
    async fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let config = RelayConfig {
            demo_mode: true,
            public_origin: HeaderValue::from_static("http://localhost:5173"),
            registration_secret: Some(Arc::from(REGISTRATION_SECRET)),
            postgres: None,
            redis: None,
        };
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router(config)).await;
        });
        Ok(Self { address, server })
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    async fn register_route(&self, secret: &str) -> anyhow::Result<reqwest::Response> {
        Ok(reqwest::Client::new()
            .post(format!("{}/api/v1/routes", self.url()))
            .header("x-leave-registration-secret", secret)
            .send()
            .await?)
    }

    /// Register a route and return its identifier and token.
    async fn route(&self) -> anyhow::Result<(String, String)> {
        let body: serde_json::Value = self
            .register_route(REGISTRATION_SECRET)
            .await?
            .json()
            .await?;
        let route_id = body["routeId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("relay did not return a route id"))?;
        let token = body["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("relay did not return a token"))?;
        Ok((route_id.to_owned(), token.to_owned()))
    }
}

impl Drop for TestRelay {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// A host and a phone that already share one MLS group.
fn paired_sessions() -> anyhow::Result<(WorkspaceSession, WorkspaceSession)> {
    let mut host = WorkspaceSession::create(DeviceIdentity::generate("host")?, "workspace-1")?;
    let phone = DeviceIdentity::generate("phone")?;
    let invitation = host.add_device(&phone.publish_key_package()?)?;
    let phone = WorkspaceSession::join(phone, &invitation.welcome)?;
    Ok((host, phone))
}

#[tokio::test]
async fn a_phone_and_a_host_exchange_work_through_the_relay() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (route_id, token) = relay.route().await?;
    let (mut host_session, mut phone_session) = paired_sessions()?;

    let mut host = RelayClient::attach(&relay.url(), &route_id, &token).await?;
    let mut phone = RelayClient::attach(&relay.url(), &route_id, &token).await?;

    // The host reports a session event; the phone reads it.
    host.send(&host_session.seal(b"devin finished the refactor")?)
        .await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), phone.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    let opened = phone_session.open(&frame)?;
    assert_eq!(opened.plaintext, b"devin finished the refactor");
    assert_eq!(opened.sender_device_id, "host");

    // The phone approves; the host reads the approval and knows who sent it.
    phone.send(&phone_session.seal(b"approve once")?).await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), host.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    let opened = host_session.open(&frame)?;
    assert_eq!(opened.plaintext, b"approve once");
    assert_eq!(opened.sender_device_id, "phone");
    Ok(())
}

#[tokio::test]
async fn the_relay_only_ever_carries_ciphertext() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (route_id, token) = relay.route().await?;
    let (mut host_session, _phone_session) = paired_sessions()?;

    let mut host = RelayClient::attach(&relay.url(), &route_id, &token).await?;
    // A third endpoint stands in for the relay operator reading the wire.
    let mut observer = RelayClient::attach(&relay.url(), &route_id, &token).await?;

    let secret = b"AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI";
    host.send(&host_session.seal(secret)?).await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), observer.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;

    assert!(
        !frame.windows(secret.len()).any(|window| window == secret),
        "the relay must never see workspace plaintext"
    );
    // Holding the route token is not enough to read the group.
    let mut outsider =
        WorkspaceSession::create(DeviceIdentity::generate("outsider")?, "workspace-1")?;
    assert!(
        outsider.open(&frame).is_err(),
        "a device outside the group must not read relayed work"
    );
    Ok(())
}

#[tokio::test]
async fn a_wrong_token_cannot_attach() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (route_id, _token) = relay.route().await?;
    assert!(
        RelayClient::attach(&relay.url(), &route_id, "not-the-token")
            .await
            .is_err(),
        "the relay must refuse an endpoint without the route token"
    );
    Ok(())
}

#[tokio::test]
async fn a_route_cannot_be_registered_without_the_secret() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let response = relay.register_route("wrong-secret").await?;
    assert_eq!(response.status(), 401);
    Ok(())
}

#[tokio::test]
async fn one_route_token_does_not_open_another_route() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (first_route, _first_token) = relay.route().await?;
    let (_second_route, second_token) = relay.route().await?;
    assert!(
        RelayClient::attach(&relay.url(), &first_route, &second_token)
            .await
            .is_err(),
        "each route must accept only its own token"
    );
    Ok(())
}

#[tokio::test]
async fn a_frame_addressed_to_another_route_is_dropped() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (route_id, token) = relay.route().await?;
    let (mut host_session, mut phone_session) = paired_sessions()?;

    let mut host = RelayClient::attach(&relay.url(), &route_id, &token).await?;
    let mut phone = RelayClient::attach(&relay.url(), &route_id, &token).await?;

    // The relay closes a connection that publishes under the wrong route, so
    // the honest endpoint still receives nothing from it.
    host.send(&host_session.seal(b"legitimate")?).await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), phone.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    assert_eq!(phone_session.open(&frame)?.plaintext, b"legitimate");
    Ok(())
}

#[tokio::test]
async fn oversized_and_empty_frames_are_refused_before_they_reach_the_relay() -> anyhow::Result<()>
{
    let relay = TestRelay::start().await?;
    let (route_id, token) = relay.route().await?;
    let mut host = RelayClient::attach(&relay.url(), &route_id, &token).await?;

    assert!(host.send(b"").await.is_err());
    assert!(
        host.send(&vec![0_u8; leave_protocol::MAX_CIPHERTEXT_BYTES + 1])
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn a_new_phone_pairs_over_the_relay_and_then_reads_work() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (route_id, token) = relay.route().await?;

    // The host has a workspace and shows a pairing code.
    let mut host_session =
        WorkspaceSession::create(DeviceIdentity::generate("host")?, "workspace-1")?;
    let secret = leave_crypto::PairingSecret::generate();
    let mut host = RelayClient::attach(&relay.url(), &route_id, &token).await?;

    // The phone scanned the code, so it holds the route token and the secret.
    let phone_identity = DeviceIdentity::generate("phone")?;
    let mut phone = RelayClient::attach(&relay.url(), &route_id, &token).await?;
    phone
        .send(&leave_crypto::pairing_request(
            &phone_identity.publish_key_package()?,
            &secret,
        ))
        .await?;

    // The host admits it and answers with the welcome.
    let request = tokio::time::timeout(Duration::from_secs(5), host.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    assert!(leave_crypto::is_pairing_frame(&request));
    let invitation = leave_crypto::accept_pairing(&mut host_session, &request, &secret)?;
    host.send(&leave_crypto::pairing_welcome(&invitation))
        .await?;

    let welcome = tokio::time::timeout(Duration::from_secs(5), phone.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    let mut phone_session = WorkspaceSession::join(
        phone_identity,
        &leave_crypto::read_pairing_welcome(&welcome)?,
    )?;

    // From here the phone is an ordinary member of the workspace.
    host.send(&host_session.seal(b"devin needs an approval")?)
        .await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), phone.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    let opened = phone_session.open(&frame)?;
    assert_eq!(opened.plaintext, b"devin needs an approval");
    assert_eq!(opened.sender_device_id, "host");
    Ok(())
}

#[tokio::test]
async fn the_route_token_alone_does_not_pair_a_device() -> anyhow::Result<()> {
    let relay = TestRelay::start().await?;
    let (route_id, token) = relay.route().await?;
    let mut host_session =
        WorkspaceSession::create(DeviceIdentity::generate("host")?, "workspace-1")?;
    let secret = leave_crypto::PairingSecret::generate();
    let mut host = RelayClient::attach(&relay.url(), &route_id, &token).await?;

    // An attacker who obtained the route token, but never saw the QR code.
    let attacker = DeviceIdentity::generate("attacker")?;
    let mut intruder = RelayClient::attach(&relay.url(), &route_id, &token).await?;
    intruder
        .send(&leave_crypto::pairing_request(
            &attacker.publish_key_package()?,
            &leave_crypto::PairingSecret::generate(),
        ))
        .await?;

    let request = tokio::time::timeout(Duration::from_secs(5), host.recv())
        .await?
        .transpose()
        .ok_or_else(|| anyhow::anyhow!("the route closed before delivering"))??;
    assert!(
        leave_crypto::accept_pairing(&mut host_session, &request, &secret).is_err(),
        "attaching to the relay must not be enough to join the workspace"
    );
    assert_eq!(host_session.member_device_ids()?, ["host"]);
    Ok(())
}
