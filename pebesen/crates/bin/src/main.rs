#![deny(clippy::all)]

//! `pebesen` server entry point.
//!
//! Today this serves the **intelligence receiver** — the pebesen-side half of the
//! teri ↔ pebesen prediction loop (teri's `PebesenFeedback` pushes predictions
//! here; operators report actioned outcomes; the calibration metric feeds back to
//! teri). It needs no database (the receiver's store is in-memory).
//!
//! The DB-backed platform routes (`pebesen-api`: spaces/streams/topics/messages/…)
//! mount here too once that crate exposes a `Router` and a `DATABASE_URL` is
//! configured; until then this binary stands up the live prediction loop on its own.

use std::net::SocketAddr;

use pebesen_intelligence::{IntelligenceStore, http::router};

#[tokio::main]
async fn main() {
    let addr: SocketAddr = std::env::var("PEBESEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:6000".to_string())
        .parse()
        .expect("PEBESEN_ADDR must be a valid socket address (e.g. 0.0.0.0:6000)");

    let store = IntelligenceStore::new();
    let app = router(store);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));

    println!("pebesen intelligence receiver listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .expect("pebesen server failed");
}
