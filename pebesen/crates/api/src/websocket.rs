// WebSocket support temporarily simplified - see TODO.md for full implementation
use axum::{
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use dashmap::DashMap;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub user_id: Uuid,
    pub subscribed_spaces: HashSet<Uuid>,
    pub redis_subscriptions: HashSet<String>,
}

/// Connection manager for WebSocket connections
/// Maps user_id to their subscribed space IDs
#[derive(Clone)]
pub struct ConnectionManager {
    pub connections: Arc<DashMap<Uuid, HashSet<Uuid>>>,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
        }
    }

    pub fn subscribe_to_space(&self, user_id: Uuid, space_id: Uuid) {
        self.connections
            .entry(user_id)
            .or_default()
            .insert(space_id);
    }

    pub fn unsubscribe_from_space(&self, user_id: Uuid, space_id: Uuid) {
        self.connections.entry(user_id).and_modify(|spaces| {
            spaces.remove(&space_id);
        });
    }

    pub fn remove_connection(&self, user_id: Uuid) {
        self.connections.remove(&user_id);
    }
}

pub async fn websocket_handler(
    State(pool): State<PgPool>,
    State(manager): State<ConnectionManager>,
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, pool, manager, query))
}

async fn handle_socket(
    mut socket: WebSocket,
    _pool: PgPool,
    _manager: ConnectionManager,
    query: WsQuery,
) {
    // Require auth (JWT in ?token= query param)
    let token = query.token;
    let _user_id = if let Some(t) = token {
        let jwt_secret =
            std::env::var("JWT_SECRET").unwrap_or_else(|_| "default_secret".to_string());
        match jsonwebtoken::decode::<pebesen_core::AuthClaims>(
            &t,
            &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_ref()),
            &jsonwebtoken::Validation::default(),
        ) {
            Ok(data) => data.claims.user_id,
            Err(_) => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
        }
    } else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    // Simple echo loop for now - full implementation in TODO.md
    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(Message::Text(text)) => {
                let _ = socket.send(Message::Text(text)).await;
            }
            Ok(Message::Close(_)) => break,
            _ => {}
        }
    }
}
