//! Route registration and the bearer tokens that authorize attachment.
//!
//! A route is the relay's whole view of a workspace: an opaque identifier and
//! the set of endpoints allowed to attach to it. The relay authorizes
//! attachment and nothing else. It cannot read a frame, and it never learns
//! which workspace, repository, or person a route belongs to.
//!
//! Tokens are the bootstrap credential used before hosted accounts exist. They
//! are compared in constant time and stored only as a hash, so a dump of relay
//! state does not yield a working token.

use leave_crypto::{StateKey, subtle_eq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::broadcast;
use uuid::Uuid;

/// A frame together with the endpoint that published it.
///
/// The relay fans a frame out to every other endpoint on the route, never
/// back to its sender: an endpoint cannot decrypt its own MLS message, and
/// echoing it would waste the connection's frame budget.
pub type RoutedFrame = Arc<(u64, Vec<u8>)>;

/// Hands out a unique identifier per attached endpoint.
static NEXT_ENDPOINT: AtomicU64 = AtomicU64::new(1);

/// Claim an identifier for one attached endpoint.
pub fn next_endpoint_id() -> u64 {
    NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed)
}

/// How many frames a route buffers for a slow endpoint before it is dropped.
const CHANNEL_CAPACITY: usize = 256;

/// One workspace channel the relay is willing to route.
pub struct Route {
    /// Hash of the token that authorizes attachment.
    token_hash: [u8; 32],
    /// Fanout channel shared by every attached endpoint.
    sender: broadcast::Sender<RoutedFrame>,
}

impl Route {
    /// Subscribe an endpoint to this route's fanout.
    pub fn subscribe(&self) -> broadcast::Receiver<RoutedFrame> {
        self.sender.subscribe()
    }

    /// The sender used to publish a frame to every other endpoint.
    pub fn sender(&self) -> broadcast::Sender<RoutedFrame> {
        self.sender.clone()
    }
}

/// Every route this relay currently serves.
#[derive(Default)]
pub struct RouteTable {
    routes: HashMap<String, Route>,
}

/// What the relay returns when a host registers a route.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRoute {
    /// Opaque route identifier, carried in every envelope.
    pub route_id: String,
    /// Bearer token an endpoint presents to attach. Shown once.
    pub token: String,
}

impl RouteTable {
    /// Register a new route and return its identifier and single-use token.
    pub fn register(&mut self) -> RegisteredRoute {
        let route_id = Uuid::now_v7().to_string();
        let token = new_token();
        let (sender, _receiver) = broadcast::channel(CHANNEL_CAPACITY);
        self.routes.insert(
            route_id.clone(),
            Route {
                token_hash: hash_token(&token),
                sender,
            },
        );
        RegisteredRoute { route_id, token }
    }

    /// Look up a route, checking the presented token in constant time.
    pub fn authorize(&self, route_id: &str, token: &str) -> Option<&Route> {
        let route = self.routes.get(route_id)?;
        subtle_eq(&route.token_hash, &hash_token(token)).then_some(route)
    }

    /// Forget a route and disconnect everything attached to it.
    pub fn revoke(&mut self, route_id: &str) -> bool {
        self.routes.remove(route_id).is_some()
    }

    /// How many routes the relay is serving.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.routes.len()
    }
}

/// Draw a fresh bearer token.
fn new_token() -> String {
    // A state key is 32 bytes of operating-system randomness, which is exactly
    // what a bearer token needs.
    let key = StateKey::generate();
    hex(key.expose())
}

/// Hash a token before it is stored or compared.
fn hash_token(token: &str) -> [u8; 32] {
    *blake3::hash(token.as_bytes()).as_bytes()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        use core::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_accepts_only_its_own_token() {
        let mut table = RouteTable::default();
        let first = table.register();
        let second = table.register();

        assert!(table.authorize(&first.route_id, &first.token).is_some());
        assert!(
            table.authorize(&first.route_id, &second.token).is_none(),
            "another route's token must not attach here"
        );
        assert!(table.authorize(&first.route_id, "").is_none());
        assert!(table.authorize("unknown-route", &first.token).is_none());
    }

    #[test]
    fn tokens_are_unique_and_long_enough_to_resist_guessing() {
        let mut table = RouteTable::default();
        let first = table.register();
        let second = table.register();
        assert_ne!(first.token, second.token);
        assert_ne!(first.route_id, second.route_id);
        assert_eq!(first.token.len(), 64, "32 bytes of randomness, hex encoded");
    }

    #[test]
    fn a_revoked_route_stops_authorizing() {
        let mut table = RouteTable::default();
        let route = table.register();
        assert!(table.revoke(&route.route_id));
        assert!(table.authorize(&route.route_id, &route.token).is_none());
        assert!(!table.revoke(&route.route_id));
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn the_table_never_stores_a_usable_token() {
        let mut table = RouteTable::default();
        let route = table.register();
        let Some(stored) = table.routes.get(&route.route_id) else {
            unreachable!("the route was just registered");
        };
        assert!(
            !route
                .token
                .as_bytes()
                .windows(32)
                .any(|window| window == stored.token_hash),
            "the raw token must not be recoverable from relay state"
        );
    }
}
