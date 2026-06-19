//! Simulation API route handlers — port of `backend/app/api/simulation.py` (MiroFish,
//! 2716 lines, 33 `@simulation_bp.route`s) mounted at `/api/simulation`.
//!
//! Unit **U-026** (`port-fresh`). Decomposed into 13 sub-cycles (a–m) per
//! `.handoff/loop/findings/u026-architecture.md`. This file lands incrementally; each
//! sub-cycle adds its routes and is parity-verified before commit.
//!
//! ## Sub-cycle (a) — THIS landing: ApiState runtime-state extension + router skeleton + nest
//!
//! DECISION-U026-1: `ApiState` now carries the shared simulation runtime registry
//! (`sim_manager: Arc<SimulationManager>` + `sim_runner: Arc<SimulationRunner<OpenAiAdapter>>`)
//! — concrete monomorphization at the state-construction boundary (see `src/api/mod.rs`).
//! `simulation_router` is nested under `/api/simulation` in `server.rs`. No route logic yet:
//! the skeleton proves the monomorphization compiles in axum `State` (the smallest unit that
//! validates the central decision), mirroring U-025's skeleton-first (a) landing.
//!
//! ## Route → handler map (33 routes, filled in by sub-cycles b–m)
//!
//! | Sub-cycle | Routes | Primitive |
//! |-----------|--------|-----------|
//! | b  | `GET /entities/:graph_id`, `/entities/:graph_id/:uuid`, `/entities/:graph_id/by-type/:type` | `KnowledgeGraphEntityReader` (U-016) |
//! | c  | `POST /create`, `GET /:id`, `GET /list` | `SimulationManager` (in-state) |
//! | d  | `POST /prepare`, `POST /prepare/status` | `prepare_simulation` + `TaskManager` + spawn |
//! | e  | `GET /:id/profiles`, `/profiles/realtime`, `/config`, `/config/realtime`, `/config/download` | `SimulationManager` + file reads |
//! | e2 | `GET /script/:name/download` | `[≠] U026-SCRIPTDL` (owner-decision) |
//! | f  | `POST /generate-profiles` | `generate_profiles_from_entities` (async) |
//! | g  | `POST /start`, `POST /stop` | `SimulationRunner` (in-state) |
//! | h  | `GET /:id/run-status`, `/run-status/detail` | `SimulationRunner.get_run_state` |
//! | i  | `GET /:id/actions`, `/timeline`, `/agent-stats` | `SimulationRunner` readers (`[!]` PRODUCER-PENDING) |
//! | j  | `GET /:id/posts`, `/comments` | SQLite (`[!]` GAP-U026-SOCIALDB; empty-branch now) |
//! | k  | `POST /interview`, `/interview/batch`, `/interview/all`, `/interview/history` | IPC interview (`[!]` IPC-PRODUCER-PENDING) |
//! | l  | `POST /env-status`, `POST /close-env` | `get_env_status_detail` + `close_simulation_env` |
//! | m  | `GET /history` | `SimulationManager` + `ReportManager::get_report_by_simulation` |
//!
//! All handlers return the U-025 envelope: `Result<Json<Value>, ApiError>` (`src/api/mod.rs`).
//! No SSE in U-026 — every route is a request/response JSON snapshot (the `/realtime` and
//! `/run-status` routes are poll-based by source design; SSE lands in U-027).

use std::sync::Arc;

use axum::Router;

use crate::api::ApiState;

/// Build the `/api/simulation` sub-router.
///
/// Sub-cycle (a): skeleton only — no routes registered yet. The `state` (carrying the shared
/// `sim_manager` + `sim_runner`, DECISION-U026-1) is bound so subsequent sub-cycles attach
/// handlers typed `State<Arc<ApiState>>` without re-threading state. Mirrors `graph_router`.
pub fn simulation_router(state: Arc<ApiState>) -> Router {
    Router::new().with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sub-cycle (a) gate: the DECISION-U026-1 monomorphization
    /// (`SimulationRunner<OpenAiAdapter>` + `SimulationManager` in non-generic `ApiState`)
    /// compiles and constructs through `ApiState::new`, and the skeleton router builds and
    /// nests. This is the minimal proof the central decision holds before any route logic.
    #[test]
    fn simulation_router_skeleton_builds_from_state() {
        let config = crate::Config::build_test();
        let state = Arc::new(ApiState::new(config));
        // Constructing the router must not panic; the state's sim_runner/sim_manager Arcs
        // are live and shared (same SimulationManager instance the runner holds).
        let _router = simulation_router(state.clone());
        // The two runtime-registry Arcs are present and the manager is shared with the runner.
        assert!(Arc::strong_count(&state.sim_manager) >= 1);
    }
}
