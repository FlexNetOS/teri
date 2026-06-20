# U-027 Architecture — `report.py` HTTP API → `teri::api::report`

**Unit:** U-027 · source `backend/app/api/report.py` (1020 lines, ~17 routes, blueprint prefix `/api/report`)
**Class:** `port-fresh` (the ROUTING layer is new) over a **`reuse-Y` substrate** (every producer — `ReportAgent`, `ReportManager`, `ReportLogger`, `ReportConsoleLogger`, `ReportTools` — already landed + parity-verified in U-024). U-027 writes ZERO new producer logic; it wires HTTP handlers onto U-024's verified symbols. Deps: U-024 (substrate), U-017 (graph search inside `ReportTools`).
**Landing:** new module `src/api/report.rs` + `pub mod report;` in `src/api/mod.rs` + one `.nest("/report", report_router(state.clone()))` line in `src/server.rs:196` (the RESERVED slot). Mirrors `src/api/simulation.rs` exactly.
**Date:** 2026-06-19. Grounded on direct reads of report.py, server.rs, api/mod.rs, report/mod.rs, report/sink.rs, report/manager.rs, services/zep_tools.rs, services/simulation_manager.rs, api/simulation.rs.

> Note on the ledger: the parity-ledger U-027 row lists `GET /<report_id>/status`, `GET /graph-search`, `GET /graph-statistics` (GET) and `/sections/<n>`. The **actual source** (read in full) has NO `/<report_id>/status` route, the tools routes are **POST** `/tools/search` + `/tools/statistics`, and the single-section route is `GET /<report_id>/section/<int:section_index>`. **Source is authoritative** — this design ports the 17 routes that exist in report.py. The ledger summary was a guess; flag `[!] U027-LEDGER-DRIFT` for the cartographer.

---

## 1. Route → handler → teri-symbol map (all 17 source routes)

All handlers are `async fn(...) -> Result<Json<Value>, ApiError>` using the U-025 `ApiError::{client,client_with,server}` envelope (api/mod.rs:167-227) — the `{success,error[,traceback]}` shape is inherited, `[≠] U025-TRACEBACK` carries (Rust backtrace string in the `traceback` value; contract preserved). Success bodies are hand-built `serde_json::json!({"success": true, "data": …})` exactly like simulation.rs. State is `State(Arc<ApiState>)`.

| # | Source route (report.py) | Method | Handler | teri symbol(s) reused | Notes / gaps |
|---|---|---|---|---|---|
| 1 | `/generate` (L25) | POST | `generate_report_route` | `ReportManager::get_report_by_simulation` (mgr:830), `ReportAgent::new_react` (mod:677), `generate_report` (mod:1635), `load_entity_reader_graph` (sim:205), `build_llm` (mod:247), `TaskManager::create_task` | **async background — Decision (i) below.** force_regenerate short-circuit reuses `get_report_by_simulation` + `ReportStatus::Completed`. Needs `ProjectManager`+`SimulationManager` for graph_id/requirement resolution (same as sim prepare route). |
| 2 | `/generate/status` (L203) | POST | `generate_status_route` | `ReportManager::get_report_by_simulation`, `TaskManager::global().get_task` + `Task::to_dict` | simulation_id short-circuit (already-completed) then task lookup. |
| 3 | `/<report_id>` (L277) | GET | `get_report_route` | `ReportManager::get_report` (mgr:744) + `Report::to_dict` (mod:547) | 404 on None. |
| 4 | `/by-simulation/<sim_id>` (L319) | GET | `get_report_by_sim_route` | `ReportManager::get_report_by_simulation` (mgr:830) + `Report::to_dict` | 404 includes `"has_report": false`; 200 includes `"has_report": true` (`client_with`/manual body). |
| 5 | `/list` (L358) | GET | `list_reports_route` | `ReportManager::list_reports(Some(sim)/None, limit)` (mgr:868) + `Report::to_dict` | query params `simulation_id` (Option), `limit` (default 50). `Query<HashMap>` extractor. |
| 6 | `/<report_id>/download` (L398) | GET | `download_report_route` | `ReportManager::get_report` + `get_report_markdown_path` (mgr:86, **private** → see GAP-A), `report.markdown_content` | returns `text/markdown` attachment, `download_name={report_id}.md`. **NOT** a `Json` response → handler returns `Response` (own `Result<Response,ApiError>`). |
| 7 | `/<report_id>` DELETE (L444) | DELETE | `delete_report_route` | `ReportManager::delete_report` (mgr:916) | bool → 404 if false. Same path as #3, different method (axum `.delete()` on same route). |
| 8 | `/chat` (L472) | POST | `chat_route` | `ReportAgent::new_react` + `ReportAgent::chat` (mod:2124), `ReportTools::new` (zep:512), `build_llm`, `load_entity_reader_graph`, `ReportManager`, `ChatResponse::to_dict` (mod:606) | needs graph (ReportTools borrow-facade), llm, manager, sim/project resolution. `chat_history` parsed to `Vec<ChatMessage>`. |
| 9 | `/<report_id>/progress` (L569) | GET | `get_progress_route` | `ReportManager::get_progress` (mgr:464) | Option<Map> → 404 on None. |
| 10 | `/<report_id>/sections` (L610) | GET | `get_sections_route` | `ReportManager::get_generated_sections` (mgr:483) + `get_report` (for is_complete) | builds `{report_id, sections, total_sections, is_complete}`. |
| 11 | `/<report_id>/section/<int:section_index>` (L661) | GET | `get_single_section_route` | `ReportManager::get_section_path` (mgr:101, **private** → GAP-A) OR a new `get_single_section` (GAP-B) | reads `section_{NN:02}.md` from disk; 404 if absent. Path capture is `usize` (`Path<(String, usize)>`). |
| 12 | `/check/<sim_id>` (L707) | GET | `check_report_status_route` | `ReportManager::get_report_by_simulation` + `ReportStatus::value`/`Completed` | builds `{simulation_id, has_report, report_status, report_id, interview_unlocked}`. |
| 13 | `/<report_id>/agent-log` (L758) | GET | `get_agent_log_route` | `ReportManager::get_agent_log(id, from_line)` (mgr:222) | query `from_line` (default 0). Returns the `{logs,total_lines,from_line,has_more}` map verbatim. |
| 14 | `/<report_id>/agent-log/stream` (L817) | GET | `get_agent_log_stream_route` | `ReportManager::get_agent_log_stream` (mgr:278) | **NOT SSE in source** — Flask returns a one-shot JSON `{logs,count}`. See Decision (ii) note. |
| 15 | `/<report_id>/console-log` (L853) | GET | `get_console_log_route` | `ReportManager::get_console_log(id, from_line)` (mgr:146) | query `from_line`. Returns `{logs,total_lines,from_line,has_more}`. |
| 16 | `/<report_id>/console-log/stream` (L899) | GET | `get_console_log_stream_route` | `ReportManager::get_console_log_stream` (mgr:200) | **NOT SSE in source** — one-shot JSON `{logs,count}`. See Decision (ii). |
| 17 | `/tools/search` (L935) | POST | `tools_search_route` | `ReportTools::search_graph` (zep:929) + `GraphSearchResult::to_dict` | needs graph (from body `graph_id`) + llm + `ReportTools::new`. |
| 18 | `/tools/statistics` (L983) | POST | `tools_statistics_route` | `ReportTools::get_graph_statistics` (zep:684) | needs graph + `ReportTools::new`. |

(18 handlers; 17 distinct *paths* — #3 and #7 share `/<report_id>` GET/DELETE.)

### Producer/substrate gaps (symbols that may need adding)

- **GAP-A (private path helpers):** `ReportManager::get_report_markdown_path` (mgr:86) and `get_section_path` (mgr:101) are **private** (`fn`, not `pub fn`). Routes #6 and #11 need them. **Decision:** add thin `pub fn get_markdown_path(&self, id)` / `pub fn get_single_section(&self, id, idx) -> Option<(String,String)>` wrappers on `ReportManager` — a ~2-symbol additive extension to manager.rs, zero blast radius (new pub fns). Do NOT make the existing private fns pub (keep internal contract). This is the cleanest landing for route #11 which in Python inlines the `open(section_path)` read (report.py:684).
- **GAP-B (single-section read):** the Python `/section/<n>` handler does its own `os.path.exists` + `open().read()` (report.py:676-694). Port as `ReportManager::get_single_section(&self, report_id, idx) -> Option<(filename, content)>` (returns None when file absent → 404). One new pub method (~1 symbol).
- **NO OTHER GAPS.** `get_sections`/`get_progress`/`get_agent_log[_stream]`/`get_console_log[_stream]`/`list_reports`/`get_report[_by_simulation]`/`delete_report`/`save_report` all exist and are parity-verified (U-024 f). `markdown_content` is a public field on `Report`. `Report::to_dict`/`ReportOutline::to_dict`/`ChatResponse::to_dict` exist.

---

## 2. Sub-cycle decomposition (a→f; each = one loop cycle)

Ordered simplest-read-first → async/SSE/chat last, mirroring how U-026 was sequenced. Each sub-cycle is independently portable AND parity-verifiable in one cycle. Symbol counts are *handlers + helpers*; tests are extra.

### (a) Pure read routes — the easy wins (~6 handlers + router skeleton)
Routes #3 `/<report_id>`, #4 `/by-simulation/<sim_id>`, #5 `/list`, #9 `/<report_id>/progress`, #12 `/check/<sim_id>`, #7 DELETE `/<report_id>`.
All single-call `ReportManager` reads → `*.to_dict()` → envelope. No graph, no llm, no async. **Lands `report_router()` factory + the `.nest("/report", …)` wiring** (server.rs:196) so the blueprint is live from cycle 1. **Deps:** none beyond U-024. **Risk:** trivial; verify each `to_dict` key-order + 404 path against report.py. ~7 symbols.

### (b) Log read routes (~4 handlers)
Routes #13 `/agent-log`, #14 `/agent-log/stream`, #15 `/console-log`, #16 `/console-log/stream`.
Reuse `get_agent_log`/`get_agent_log_stream`/`get_console_log`/`get_console_log_stream`. `from_line` query param (default 0). **These are one-shot JSON in source — NOT SSE** (see Decision ii). ~4 symbols. **Deps:** (a) router exists.

### (c) Sections + download routes (~3 handlers + GAP-A/B pub wrappers)
Routes #6 `/download`, #10 `/sections`, #11 `/section/<int>`.
Lands the **GAP-A/B `ReportManager` pub wrappers** (`get_markdown_path`, `get_single_section`). #6 returns a `text/markdown` attachment (`Response`, not `Json`) — the one non-envelope handler; verify `Content-Disposition: attachment; filename="{id}.md"` + the temp-file fallback when the on-disk md is absent (report.py:416-433 → in Rust: if `markdown_content` non-empty but no file, serve the in-memory string as the body). ~5 symbols. **Deps:** (a).

### (d) Tools debug routes (~2 handlers)
Routes #17 `/tools/search`, #18 `/tools/statistics`.
Body-supplied `graph_id` → `load_entity_reader_graph` → `ReportTools::new(&graph, &llm)` → `search_graph`/`get_graph_statistics` → `to_dict`. First sub-cycle to exercise the **ReportTools borrow-facade** + `build_llm` in a handler. ZEP-guard inherited via `load_entity_reader_graph` (`[≠] U026-ZEPKEY`). ~2 symbols. **Deps:** (a). **Producer-gated:** needs a built graph task (graph_id resolvable) to return non-empty results, but runs end-to-end against an empty graph today (returns empty result set, not an error — same as source on a missing graph).

### (e) Chat route (~1 handler, heaviest non-async)
Route #8 `/chat`.
Full resolution chain: body → `SimulationManager::get_simulation` → `ProjectManager::get_project` → graph_id/requirement → `load_entity_reader_graph` → `ReportAgent::new_react` → `ReportTools::new` → `build_llm` → `ReportManager::new` → `agent.chat(&tools,&llm,&manager,msg,&history)` → `ChatResponse::to_dict`. Parse `chat_history` JSON array → `Vec<ChatMessage>`. ~2 symbols (handler + a `chat_history` parse helper). **Deps:** (a),(d) (reuses graph/llm wiring from d). **Producer-gated:** `chat` reads the existing report markdown + calls the LLM with tools; runs end-to-end (returns a placeholder-report answer if no report exists — `chat` handles empty `report_content`, mod:2148-2168).

### (f) Async generate + status — the keystone (~2 handlers + 1 spawn fn + 1 worker fn)
Routes #1 `/generate`, #2 `/generate/status`.
Lands **Decision (i)** below: route creates `report_id` + `TaskManager::create_task("report_generate", …)` eagerly (returns report_id+task_id immediately, report.py:109-122), then spawns the `!Send` `generate_report` future on a **dedicated OS thread + current-thread tokio runtime** (the `spawn_prepare_simulation` template). The worker calls `agent.generate_report(&tools,&llm,&manager, &mut NullSink_or_FileFanout, Some(report_id))` then `manager.save_report` + `complete_task`/`fail_task`. `/generate/status` reuses (b)-style task lookup. ~4 symbols. **Deps:** (a)-(e) (reuses every wiring). **Highest risk** — the Send-ness + thread-spawn + task lifecycle; isolate it last.

**Total: ~24 symbols across 6 sub-cycles.** (a) ships a working blueprint; (f) closes the keystone.

---

## 3. The two structural decisions

### Decision (i) — `/generate` async background pattern: **dedicated OS thread + current-thread tokio runtime** (NOT `tokio::spawn`)

**Grounded ruling.** `ReportAgent::generate_report` (mod:1635) is `async fn(&mut self, …, sink: &mut dyn ReportSink, …)` and its body wraps the sink in `RefCell<&mut dyn ReportSink>` (mod:1672) and drives `Fn` progress closures that borrow `&sink_cell` **across `.await` points** inside the section-generation loop. `RefCell` is `!Sync`; a future holding a live `&RefCell<…>` borrow across `.await` is **`!Send`**. This is the *identical* property that forced `DECISION-U026-d-1-REVISED` for `prepare_simulation` (simulation_manager.rs:1819-1834): a `!Send` future cannot be awaited inside a `tokio::spawn` worker without `+ Send`-bounding the whole call chain.

Therefore U-027 **reuses the `spawn_prepare_simulation` template verbatim** (simulation_manager.rs:1836-1883):
1. **Route** (`generate_report_route`) creates `report_id = format!("report_{}", &uuid_hex[..12])` and `task_id = TaskManager::global().create_task("report_generate", metadata)` — eagerly, so the immediate JSON response carries both (report.py:109-122 ordering preserved). Captures `locale = i18n::get_locale()` before spawning.
2. **`spawn_report_generation(...)`** (new fn in `src/api/report.rs` or `src/report/mod.rs`): `std::thread::spawn(move || { let rt = Builder::new_current_thread().enable_all().build()?; rt.block_on(with_locale(locale, async { report_generate_worker(...).await })) })`. A current-thread runtime drives the `!Send` future on one thread → **zero signature changes** to `generate_report` (blast radius 0). This is also the *more faithful* port of Python's `threading.Thread(target=run_generate, daemon=True)` (report.py:179).
3. **`report_generate_worker(...)`** (`pub(crate) async fn`, so tests drive it directly like `prepare_worker`): `update_task(PROCESSING)` → build `ReportAgent::new_react` → resolve graph via `load_entity_reader_graph` → `ReportTools::new` + `build_llm` → `agent.generate_report(&tools,&llm,&manager, sink, Some(report_id))` → `manager.save_report(&report)` → on `report.status==Completed` `complete_task({report_id,simulation_id,status})` else `fail_task`. Errors → `fail_task(e)`.

The progress callback (Python's `progress_callback(stage,progress,message)` → `task_manager.update_task`, report.py:146-151) maps to a `ReportSink` impl that calls `TaskManager::update_task` on each `event()` (a `TaskUpdateSink` — small new struct, ~1 symbol, in (f)). This is the substrate sink already anticipated by sink.rs.

**`build_graph_async_with_completion` (graph_builder.rs:156, the `tokio::spawn` Send template) does NOT apply here** — it is Send only because its worker future holds no `!Send` borrows. `generate_report` does. The OS-thread template is the correct one. Decision recorded; no alternative is viable without re-bounding a U-024-verified surface.

### Decision (ii) — agent-log/console-log "stream" routes: **one-shot JSON now (faithful), SSE-sink seam reserved**

**Grounded ruling — the source `/stream` routes are NOT SSE.** Reading report.py:817-852 and 899-934: `stream_agent_log` and `stream_console_log` are plain Flask handlers returning `jsonify({"success":true,"data":{"logs":[...],"count":N}})` — a **one-shot full-dump JSON**, no `Response(stream_with_context(...))`, no `text/event-stream`, no generator. The "stream" in the name is a misnomer (it means "get the whole stream at once"); the *incremental* polling is done by the `from_line` param on the **non-`/stream`** routes (#13, #15). The frontend tails by repeatedly polling `?from_line=N`.

**So routes #14/#16 port as ordinary JSON handlers** (`get_agent_log_stream` → `{logs,count}`), reusing `ReportManager::get_agent_log_stream`/`get_console_log_stream` (mgr:278/200) — which already exist and are parity-verified. **No axum SSE is required for byte-parity with the source.** (Confirmed: the only `text/event-stream` in teri is in llm.rs — the LLM *client* consuming OpenAI SSE + mockito mock servers; teri has **no axum SSE response helper** yet, and U-027 does not need to introduce one to match report.py.)

**Where U-024's deferred SSE sink (sink.rs §"`ChannelSink`/`SseSink` — U-027 (future)") lands:** sink.rs reserved a `ChannelSink`/`SseSink` for "HTTP SSE route", and `ReportEvent` carries `section_content` specifically so a future SSE sink can stream each section live (sink.rs:94-97). **This is a SUPERSET capability the SOURCE report.py does NOT expose** — there is no live-SSE report route in report.py (the frontend gets live progress by polling `/generate/status` + `/sections` + `/agent-log?from_line`). Therefore:
- The **`TaskUpdateSink`** (Decision i) is the only sink U-027 *needs* for parity (it fans `ReportEvent`→`TaskManager::update_task`, matching `progress_callback`).
- A live-SSE report route would be an **additive teri superset**, NOT a parity requirement. **Do NOT build it in U-027** (no source route maps to it; adding it is scope creep, not a port). The sink.rs seam stays reserved; it is exercised only if/when a teri-native SSE report endpoint is *designed* (out of U-027 scope). **This is not a downgrade** — every observable report.py behavior (one-shot stream dumps + `from_line` polling) is ported in (b); the SSE seam is a *future extension point*, correctly left dormant. Flag: `[~] U027-SSE-SEAM-DORMANT` (reserved capability, no source route — not a `[≠]`, since no source behavior is dropped).

**Net:** U-027 introduces NO axum SSE. The four log routes are JSON. This is the faithful port. If a verifier expects SSE on `/stream`, that expectation is wrong vs. the source — point them at report.py:817/899.

---

## 4. Producer/substrate gaps + `[≠]`/`[!]`/`[~]` flags — runs-today vs producer-gated

**Runs end-to-end TODAY (no producer gate):** sub-cycles (a) all reads, (b) all log reads, (c) sections+download — these read `ReportManager` state from disk; they work the moment a report exists on disk. (d) tools + (e) chat run end-to-end against an empty/missing graph (return empty/placeholder, not errors — matching source). (f) `/generate` runs end-to-end and produces a report **iff** a graph is resolvable.

**Producer-gated (degrades gracefully, does not block the port):**
- **`[!] U027-GRAPHREQ`** — `/generate`, `/chat`, `/tools/*` resolve a `KnowledgeGraph` via `load_entity_reader_graph(state, graph_id)` (sim:205), which reads a `graph_build` task result from `TaskManager`. With no built graph, `generate_report`/`chat` operate on an empty graph (report content thin, but no crash — `ReportTools` reads return empty). **Same gating as U-026's prepare route** (`[!] U026-d-GRAPHREQ`). Not a U-027 blocker; the wiring is correct, the *data* is producer-supplied. Inherits U-017 graph search (already landed).
- **`[≠] U025-TRACEBACK`** (inherited) — all 500s carry a Rust-backtrace `traceback` string, not a Python stack. Contract (`{success,error,traceback}`) preserved; value non-contractual. Legal `[≠]` (genuinely inexpressible: no Python stack in Rust).
- **`[≠] U026-ZEPKEY`** (inherited via `load_entity_reader_graph`) — the ZEP-key guard is KEPT (returns 500 `zepApiKeyMissing` when `config.zep_api_key` empty), matching source + U-025/026 precedent. Applies to `/tools/*`, `/chat`, `/generate` graph resolution.
- **`[≠] graph_id handle`** (inherited from U-024 `ReportAgent.graph_id`) — Zep server-side `graph_id` semantics are inexpressible; teri binds `&KnowledgeGraph` directly via the `ReportTools<'g>` facade. `graph_id` retained as an opaque serialization label. Legal `[≠]` (inexpressible Zep-server semantics).
- **`[~] U027-SSE-SEAM-DORMANT`** — sink.rs `SseSink`/`ChannelSink` seam reserved but NOT built (no source route maps to live SSE; see Decision ii). Reserved superset, no dropped source behavior.
- **`[!] U027-LEDGER-DRIFT`** (for cartographer) — parity-ledger U-027 row lists routes that don't exist in source (`/<id>/status`, GET `/graph-search`/`/graph-statistics`, `/sections/<n>`). Source-authoritative route list is in §1. Reconcile the ledger row to match report.py.

**GAP-A / GAP-B** (from §1): two-to-three small additive `pub fn` on `ReportManager` (`get_markdown_path`, `get_single_section`) land in sub-cycle (c). Zero blast radius (new pub methods; existing private fns unchanged). These are the only substrate additions; everything else is reuse.

**NO capability downgrade.** Every source route maps to a real handler over a verified producer. The two "stream" routes port faithfully as JSON (source IS JSON). The async-generate background pattern is preserved via the OS-thread template. No feature is dropped or stubbed.

---

## 5. Router — `report_router` factory + `.nest` wiring + axum 0.7 ordering

```rust
// src/api/report.rs
pub fn report_router(state: Arc<ApiState>) -> Router {
    Router::new()
        // ── STATIC paths FIRST (axum 0.7 ranks static > capture within a segment count) ──
        // 1-segment statics — MUST precede the 1-segment capture `/:report_id`.
        .route("/generate", post(generate_report_route))            // (f)
        .route("/generate/status", post(generate_status_route))     // (f) 2-seg static
        .route("/list", get(list_reports_route))                    // (a)
        .route("/chat", post(chat_route))                           // (e)
        // 2-segment statics whose seg-0 is static ("tools","by-simulation","check") —
        // distinct from `/:report_id/...` by full-path match (seg-0 literal != a report_id).
        .route("/tools/search", post(tools_search_route))           // (d)
        .route("/tools/statistics", post(tools_statistics_route))   // (d)
        .route("/by-simulation/:simulation_id", get(get_report_by_sim_route)) // (a)
        .route("/check/:simulation_id", get(check_report_status_route))       // (a)
        // ── CAPTURE root: `/:report_id` GET + DELETE (same path, two methods) ──
        .route("/:report_id", get(get_report_route).delete(delete_report_route)) // (a)
        // ── `/:report_id/<static-suffix>` — 2/3-segment, seg-1 capture + static tail ──
        .route("/:report_id/download", get(download_report_route))           // (c)
        .route("/:report_id/progress", get(get_progress_route))             // (a)
        .route("/:report_id/sections", get(get_sections_route))            // (c)
        // 3-segment `/:report_id/section/:idx` — :idx is an integer (Path<(String,usize)>);
        // distinct from `/sections` (2-seg) by full path. Register section/:idx; axum parses
        // the usize segment and 404s on non-integer automatically (mirrors Flask <int:>).
        .route("/:report_id/section/:section_index", get(get_single_section_route)) // (c)
        // agent-log: 2-seg `/agent-log` vs 3-seg `/agent-log/stream` — both seg-1 capture,
        // static tails differ → axum routes by full path, no order dependency. (b)
        .route("/:report_id/agent-log", get(get_agent_log_route))
        .route("/:report_id/agent-log/stream", get(get_agent_log_stream_route))
        .route("/:report_id/console-log", get(get_console_log_route))
        .route("/:report_id/console-log/stream", get(get_console_log_stream_route))
        .with_state(state)
}
```

```rust
// src/server.rs:196 — the RESERVED slot (already documented at L176/195/18)
let api_router = Router::new()
    .nest("/graph", crate::api::graph::graph_router(state.clone()))
    .nest("/simulation", crate::api::simulation::simulation_router(state.clone()))
    .nest("/report", crate::api::report::report_router(state.clone()))   // ← U-027 adds this
    .layer(cors);
```
```rust
// src/api/mod.rs:1 — add module
pub mod report;
```

**axum 0.7 ordering notes (the live traps):**
- **Static-before-capture at seg-0:** `/generate`, `/list`, `/chat`, `/generate/status`, `/tools/*` are all seg-0 statics and MUST not be shadowed by `/:report_id`. Axum 0.7 *does* rank a static segment above a capture automatically, but register statics first for clarity (same convention as simulation.rs `/list` before `/:simulation_id`, sim:118-120,129). A request to `/list` will NOT match `/:report_id` because "list" is a literal route registered; but `/tools/search` (2-seg) vs `/:report_id/download` (2-seg) are distinguished because seg-0 "tools" is a literal that never equals a real `report_*` id — full-path match resolves it. This is exactly the `[!] U026-ROUTE-ORDER-ENTITIES` pattern (sim:102-113).
- **`/by-simulation/:simulation_id` and `/check/:simulation_id`** are 2-segment with a *static* seg-0 → distinct from `/:report_id/<suffix>` (whose seg-0 is the capture). No conflict: a request `/check/sim_x` has seg-0 "check" (literal) → matches `/check/:simulation_id`, never `/:report_id/...` (which needs a non-"check" seg-0 AND a static seg-1).
- **Integer segment `/section/:section_index`:** declare the handler with `Path<(String, usize)>`; axum's `usize` deserialization 404s a non-numeric segment, faithfully reproducing Flask's `<int:section_index>` converter (report.py:661). The 2-seg `/sections` (plural) and 3-seg `/section/:idx` (singular) are different literals → no collision.
- **GET+DELETE on `/:report_id`:** use `.route("/:report_id", get(...).delete(...))` — axum method-routing on one path (source: report.py:277 GET + report.py:444 DELETE share the path).

---

## Merge-ledger row to record (no U-027 row exists yet)

```
- [ ] U-027 · port-fresh (routing) over reuse-Y substrate (U-024 producers) · `report.py /api/report` · new-module `src/api/report.rs` + nest in server.rs:196 -> teri::api::report · refs: ReportManager(mgr), ReportAgent::{new_react,generate_report,chat}, ReportTools, build_llm, load_entity_reader_graph, TaskManager, ApiError envelope · pending · ARCH: findings/u027-architecture.md — 18 handlers/17 paths; sub-cycles a→f: (a)reads+router (b)log-reads (c)sections+download[+GAP-A/B pub wrappers] (d)tools (e)chat (f)async-generate[OS-thread current-thread rt, NOT tokio::spawn — generate_report is !Send via RefCell-across-await, DECISION-U026-d template]. Decisions: (i) OS-thread spawn; (ii) /stream routes are JSON not SSE (source IS JSON — sink.rs SseSink seam stays DORMANT, superset, no source route). Flags: [!]U027-GRAPHREQ [!]U027-LEDGER-DRIFT [≠]U025-TRACEBACK/U026-ZEPKEY/graph_id-handle(inherited) [~]U027-SSE-SEAM-DORMANT.
```
