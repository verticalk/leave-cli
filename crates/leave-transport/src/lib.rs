//! The host's outbound connection to a blind relay.
//!
//! Split out of the host binary so the transport can be exercised against a
//! real relay in an integration test.
//!
//! The host never listens on a public interface. When away access runs through
//! the hosted relay instead of a tailnet, the host dials out to the relay and
//! keeps that connection open. Everything it sends is already an MLS frame, so
//! the relay carries ciphertext and routing metadata only.

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use leave_protocol::{MAX_CIPHERTEXT_BYTES, PROTOCOL_VERSION, wire::RelayEnvelope};
use prost::Message as _;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message, frame::coding::CloseCode},
    },
};
use uuid::Uuid;

/// Header carrying the token that authorizes attaching to a route.
const ROUTE_TOKEN_HEADER: &str = "x-leave-route-token";

/// An attached relay route.
pub struct RelayClient {
    route_id: String,
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl RelayClient {
    /// Attach to a route with the token issued when it was registered.
    ///
    /// # Errors
    ///
    /// Returns an error when the relay refuses the token or the connection
    /// cannot be established.
    pub async fn attach(relay_url: &str, route_id: &str, token: &str) -> Result<Self> {
        let endpoint = format!(
            "{}/api/v1/ws/{route_id}",
            relay_url.trim_end_matches('/').replace("http", "ws")
        );
        let mut request = endpoint
            .as_str()
            .into_client_request()
            .context("the relay address is not a valid WebSocket URL")?;
        request.headers_mut().insert(
            ROUTE_TOKEN_HEADER,
            token
                .parse()
                .context("the route token contains characters a header cannot carry")?,
        );
        let (socket, response) = connect_async(request)
            .await
            .context("could not attach to the relay route")?;
        tracing::info!(status = ?response.status(), route_id, "attached to a relay route");
        Ok(Self {
            route_id: route_id.to_owned(),
            socket,
        })
    }

    /// Publish one encrypted frame to the other endpoints on this route.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame is too large for the relay or the
    /// connection has dropped.
    pub async fn send(&mut self, ciphertext: &[u8]) -> Result<()> {
        if ciphertext.is_empty() {
            bail!("refusing to relay an empty frame");
        }
        if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            bail!("frame exceeds the relay's size limit");
        }
        let envelope = RelayEnvelope {
            protocol_version: PROTOCOL_VERSION,
            route_id: self.route_id.clone(),
            message_id: Uuid::now_v7().to_string(),
            ciphertext: ciphertext.to_vec(),
        };
        self.socket
            .send(Message::Binary(envelope.encode_to_vec().into()))
            .await
            .context("could not publish to the relay")
    }

    /// Wait for the next encrypted frame from another endpoint.
    ///
    /// Returns `Ok(None)` when the route closes. Frames addressed to another
    /// route are dropped rather than returned.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection fails or the relay sends something
    /// that is not a binary envelope.
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        while let Some(message) = self.socket.next().await {
            match message.context("the relay connection failed")? {
                Message::Binary(frame) => {
                    let envelope = RelayEnvelope::decode(frame)
                        .context("the relay sent a frame Leave could not parse")?;
                    if envelope.route_id != self.route_id {
                        continue;
                    }
                    return Ok(Some(envelope.ciphertext));
                }
                Message::Close(frame) => {
                    if let Some(frame) = frame
                        && frame.code != CloseCode::Normal
                    {
                        bail!("the relay closed the route: {}", frame.reason);
                    }
                    return Ok(None);
                }
                // A relay that sends text is not speaking Leave's protocol.
                Message::Text(_) => bail!("the relay sent an unexpected text frame"),
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
        Ok(None)
    }

    /// Close the route cleanly.
    ///
    /// # Errors
    ///
    /// Returns an error when the close frame cannot be delivered.
    pub async fn close(mut self) -> Result<()> {
        self.socket
            .close(None)
            .await
            .context("could not close the relay route")
    }
}
