# U-025 graph HTTP routes (port-fresh) — Architecture, Shared Route Seam & Sub-Cycle Decomposition

Architect decision for porting MiroFish's `graph_bp` blueprint
(`backend/app/api/graph.py`, 622 lines, 10 routes mounted at `/api/graph`) to teri, AND for the
**shared axum route-layer landing** that U-026 (`/api/simulation`) and U-027 (`/api/report`) inherit.
This is the FIRST of the three route units; the seam decisions here are project-wide.

Grounded in the standing architecture (`target-architecture.md`): **DECISION-9** (`graph_id:str` is the
Zep server-graph selector → `[≠]` inexpressible; the bound `&KnowledgeGraph` IS teri's selector),
**DECISION-11** (caller-constructs llm/graph handles; service structs hold NO llm/graph), **DECISION-14**
(`LlmClient` is NOT dyn-compatible — generic `complete_json<T>`/`chat_json<T>` methods — so use a concrete
generic `<L: LlmClient + Send + Sync + 'static>`, never `Arc<dyn LlmClient>`). All citations are
`file:line` in both repos.

---

## 0. Source/destination contract — verified primitives

| Need (X) | teri primitive (Y) | Signature verified | Notes |
|---|---|---|---|
| `ProjectManager` get/list/delete/create/save/reset state | `models::project::ProjectManager` (`src/models/project.rs:332`) | `from_config(&Config)` (`:350`); `get_project(&str)->Result<Option<Project>>` (`:470`); `list_projects(usize)` (`:497`); `delete_project(&str)->Result<bool>` (`:527`); `create_project(&str)` (`:399`); `save_project(&mut Project)` (`:451`); `save_file_to_project(&str,&[u8],&str)` (`:554`); `save_extracted_text`/`get_extracted_text` (`:591`/`:605`) | stateless, fs-backed; built per-request from config |
| `TaskManager` get/list + build progress | `task::TaskManager` (`src/task.rs:195`) | `global()->&'static` singleton (`:206`); `get_task(&str)->Option<Task>` (`:259`); `list_tasks(Option<&str>)->Vec<Value>` (`:382`) | process-global singleton — NOT held in ApiState |
| `OntologyGenerator.generate` | `services::ontology::OntologyGenerator<L>` (`src/services/ontology.rs:297`) | `new(L)` (`:306`); `async generate(&[String],&str,Option<&str>)->Result<Value>` (`:324`) | generic over `L: LlmClient`; constructed per-request |
| `GraphBuilderService.build_graph_async` | `services::graph_builder::build_graph_async<L>` (`src/services/graph_builder.rs:84`) | `(llm:L, text:String, ontology:Value, graph_name:String, chunk_size:usize, chunk_overlap:usize, _batch_size:usize) -> String` (task_id) | does NOT take project_id; embeds graph in task result; `[≠]` no Zep `graph_id` (U-015) |
| `Config` (ZEP_API_KEY, chunk defaults, allowed_extensions, upload_folder) | `config::Config` | `zep_api_key:Option<String>` (`src/config.rs:27`), `allowed_extensions:Vec<String>` (`:38`), `default_chunk_size`/`default_chunk_overlap` (`:40`/`:42`), `upload_folder` (`:34`) | carried in ApiState |
| LLM adapter from config | `llm::OpenAiAdapter::new(&LlmConfig)` (`src/llm.rs:313`) | also `AnthropicAdapter` (`:575`), `GeminiAdapter` (`:849`) | **no provider-factory exists yet** — see DECISION-U025-1 |
| i18n messages | `i18n::t(&str)` (`src/i18n/mod.rs:186`), `i18n::t_args(&str,&[(&str,&dyn Display)])` (`:209`) | all 17 `api.*` keys this unit needs are PRESENT in `zh.json`/`en.json` (verified) | locale set by `accept_language_middleware` |

---

## 1. DECISION-U025-1 — LLM-in-axum-state (recurring; U-026/U-027 inherit this)

**The question** (brief Q1): axum handlers receive `State<Arc<ApiState>>`, which must be `Send+Sync` and
**cannot be generic** (no type params on axum state). `LlmClient` is a trait with generic methods, so
`Arc<dyn LlmClient>` is **impossible** (`complete_json<T>`/`chat_json<T>` are not dyn-compatible — this is
the exact wall hit at `graph_memory.rs:851` and resolved project-wide by DECISION-14).

**Decision: handlers CONSTRUCT the concrete adapter per-request from `state.config`. `ApiState` does NOT
carry an LLM client.** This is the only choice consistent with DECISION-11 (caller-constructs) AND
DECISION-14 (no `dyn LlmClient`) AND axum's no-generic-state constraint — and it is **byte-faithful to
MiroFish**, which constructs `OntologyGenerator()` (graph.py:217) and `GraphBuilderService(api_key=…)`
(graph.py:390, 581, 609) fresh inside each request handler, never holding them on the Flask app.

Concrete shape (a single private helper in `src/api/graph.rs`, reused by U-026/U-027):

```text
fn build_llm(config: &Config) -> OpenAiAdapter { OpenAiAdapter::new(&config.llm) }
```

- The concrete type is `OpenAiAdapter` (the OpenAI-format adapter; teri's shimmy/Ollama/OpenAI backends all
  speak this format — `config.rs:190`). Per-request construction is cheap: `OpenAiAdapter::new` builds a
  `reqwest::Client` (`llm.rs:314`), no network, no handshake. This mirrors MiroFish's per-request service
  construction exactly — **not a downgrade**.
- **Provider selection is OUT OF SCOPE for U-025** and is NOT a downgrade: MiroFish's `graph.py` itself
  hard-constructs one backend per route (Ollama via `GraphBuilderService`); there is no provider switch in
  the source routes. teri already has a single `LlmConfig` (`config.rs:65`, no provider enum). A future
  multi-provider factory is an additive superset, recorded as a non-blocking note below, NOT a `[≠]` here.
- **`build_graph_async`/`OntologyGenerator::new` take `L` by value** and require `Clone+Send+Sync+'static`
  for the spawn (graph_builder.rs:94). `OpenAiAdapter` is `Send+Sync`; it is **not currently `Clone`** —
  see sub-cycle (c) flag `- [!] U025-CLONE`: derive `Clone` on `OpenAiAdapter` (its fields — `String`s +
  `reqwest::Client` which is `Clone`/`Arc`-internally — are all `Clone`; additive, zero behavior change).

**Inheritance for U-026/U-027:** the same `build_llm(config)` helper and the same "construct-per-request,
never in state" rule apply. U-026/U-027 import it; they do NOT add an LLM field to `ApiState`.

---

## 2. DECISION-U025-2 — `ApiState` extension (shared; the seam all three units share)

axum state is `Arc<ApiState>` shared `&`-read by every handler, so everything it carries must be `Send+Sync`
and is accessed by shared reference. Decision on what ApiState CARRIES vs what handlers CONSTRUCT:

| Item | In `ApiState`? | Rationale |
|---|---|---|
| `config: Config` | **YES** (already present, `src/api/mod.rs:145`) | read by every handler; `Config` is `Clone+Send+Sync` |
| `ProjectManager` | **construct per-handler** via `ProjectManager::from_config(&state.config)` | it is stateless (fs-backed, holds only a `PathBuf`); cheap; matches MiroFish's `ProjectManager.<classmethod>` (no instance state). Could equally be cached in state, but per-handler keeps ApiState minimal and mirrors the stateless-classmethod source. |
| `TaskManager` | **NO — use `TaskManager::global()`** | it is a **process-global `OnceLock` singleton** (`task.rs:199,206`); putting it in ApiState would create a SECOND registry and break the cross-request visibility contract MiroFish guarantees via its `__new__` singleton. Handlers call `TaskManager::global()` directly. |
| LLM adapter | **NO — construct per-request** (DECISION-U025-1) | dyn-incompatible; caller-constructs idiom |
| graph-by-id store | **N/A — does not exist** | see DECISION-U025-4 gap |

**`ApiState` change for U-025: NONE required.** `ApiState { config }` already carries everything the graph
routes need at the state level (config); ProjectManager/TaskManager/LLM are all per-handler or global.
This is the minimal idiomatic landing. (If a later unit needs genuinely shared mutable runtime state — e.g.
U-026's `SimulationRunner` registry — it is added to ApiState THEN, behind its own `Arc<…>`; U-025 does not
pre-add it.)

> Decision recorded: ApiState stays `{ config }` for U-025. The `Arc<ApiState>` is `Send+Sync` because
> `Config` is. No `dyn`, no generics on state. **U-026/U-027 extend ApiState additively only when they need
> shared runtime state; they never add an LLM handle.**

---

## 3. DECISION-U025-3 — Router factory shape + error→HTTP mapping (shared seam)

### 3a. Router factory (the `graph_router` seam `server.rs` already documents)

`server.rs:184-191` documents the exact seam: `.nest("/graph", graph_router(state.clone()))` under
`/api`, with CORS scoped to `/api/*`. Decision:

```text
// src/api/graph.rs
pub fn graph_router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/project/list",            get(list_projects))
        .route("/project/:project_id",     get(get_project).delete(delete_project))
        .route("/project/:project_id/reset", post(reset_project))
        .route("/ontology/generate",       post(generate_ontology))
        .route("/build",                   post(build_graph))
        .route("/task/:task_id",           get(get_task))
        .route("/tasks",                   get(list_tasks))
        .route("/data/:graph_id",          get(get_graph_data))
        .route("/delete/:graph_id",        delete(delete_graph))
        .with_state(state)
}
```

- **Route-order caveat (contractual):** Flask's `<project_id>` converter would match the literal segment
  `list`; MiroFish relies on Flask registering `/project/list` and `/project/<project_id>` as distinct rules
  with the static rule winning. In axum, `/project/list` and `/project/:project_id` on the **same** router
  are an overlapping-route panic in some versions. **Mitigation (sub-cycle b):** register the static
  `/project/list` route and verify axum 0.7's router resolves static-before-capture (it does — static
  segments rank above captures); if a conflict surfaces, split into a dedicated `list` route on a nested
  `/project` sub-router. This is a known axum gotcha — flagged so the porter tests it, not discovers it.

**create_app wiring change (un-stub the seam):** `create_app(state: Arc<ApiState>)` currently ignores
`_state` (`server.rs:178`). U-025 changes it to nest the graph router under `/api` with CORS scoped there,
exactly as the in-file TODO block at `server.rs:184-193` prescribes. **U-026/U-027 add `.nest("/simulation",
…)` / `.nest("/report", …)` to the SAME `api_router`** — so sub-cycle (a) builds that `api_router`
scaffold once and the later units only add a nest line.

### 3b. Error → HTTP mapping (shared; contractual JSON body)

MiroFish handlers return `jsonify({...}), <status>` with TWO body shapes (both `success:false` envelopes):

- Validation/not-found (400/404): `{"success": false, "error": "<localized>"}` (and `/build`'s
  `graph_building` case ALSO carries `"task_id"`, graph.py:329).
- Unhandled exception (500): `{"success": false, "error": str(e), "traceback": "<full traceback>"}`
  (graph.py:251-255, 524-528, 589-593, 617-621).

Success bodies are `{"success": true, "data": …}` or `{"success": true, "message": …}` (+ `"count"` on
list routes).

**Decision: per-handler return type `Result<Json<Value>, ApiError>`, with `ApiError` a local newtype
implementing `IntoResponse`.** NOT a blanket `TeriError: IntoResponse` — because the wire envelope
(`success`/`error`/`traceback`) is graph-API-specific and must NOT leak teri's `Display` strings verbatim
into the contractual shape. The newtype:

```text
struct ApiError { status: StatusCode, body: Value }   // body is the exact {success:false,...} object
impl IntoResponse for ApiError { fn into_response(self) -> Response { (self.status, Json(self.body)).into_response() } }
// constructors that build the EXACT MiroFish body:
ApiError::client(status, error_msg)            // {"success":false,"error":msg}
ApiError::client_with(status, error_msg, extra)// adds extra keys, e.g. "task_id"
ApiError::server(err)                          // {"success":false,"error":err.to_string(),"traceback":<backtrace>}
```

- **`traceback` fidelity (`[≠]` candidate, recorded honestly):** MiroFish emits Python's
  `traceback.format_exc()` — a Python stack string. teri has no Python stack. Decision: emit the key
  `"traceback"` with teri's best available context (the `TeriError` chain / `std::backtrace` when enabled),
  so the **3-key shape `{success,error,traceback}` is preserved** (the frontend may render/ignore the field).
  The *value* being a Rust string rather than a Python stack is `[≠] non-contractual` (the frontend treats
  `traceback` as opaque debug text; the contractual keys are `success`+`error`). This is recorded as
  `- [≠] U025-TRACEBACK` and CHALLENGED by the parity gate — it is NOT a feature skip (the key is present
  and populated).
- **`preserve_order`:** all response bodies are built with `serde_json::json!`/`Value` which serialize keys
  in insertion order via teri's serde config (already proven for `/health` and `to_dict` shapes); the
  parity gate diffs the JSON object, key-set + values, against the live Python responses.

**Inheritance:** `ApiError` + `build_llm` live in `src/api/graph.rs` initially; if U-026/U-027 need them,
sub-cycle (a) places `ApiError` in `src/api/mod.rs` (next to `ApiState`) so all three route modules share
ONE error type. **Decision: put `ApiError` in `src/api/mod.rs` from the start** (it is inherently shared
infra), `build_llm` too. This is the explicit shared-seam deliverable of sub-cycle (a).

---

## 4. DECISION-U025-4 — `/build` delegation + the graph_id persistence gap

### 4a. `/build` route → `build_graph_async` (delegate, with the project-state wrapper ported)

MiroFish's `/build` (graph.py:260-529) is the big one: it does (1) ZEP_API_KEY config check → 500; (2)
parse JSON; (3) project lookup → 404; (4) status-machine guards (CREATED→400 ontologyNotGenerated;
GRAPH_BUILDING && !force→400 with task_id; force-reset for BUILDING/FAILED/COMPLETED); (5) resolve
graph_name/chunk_size/chunk_overlap; (6) fetch extracted_text → 400; (7) fetch ontology → 400; (8)
create task, set project GRAPH_BUILDING + save; (9) **spawn `build_task`** (the inline background thread)
which runs the Zep pipeline + updates project.graph_id/status on completion; (10) return
`{success:true, data:{project_id, task_id, message}}`.

**Decision: the route DELEGATES the build pipeline to `build_graph_async` (U-015, verified), and the route
handler PORTS the surrounding project-state wrapper (steps 1-8, 10) directly.** The inline `build_task`
closure's pipeline body (steps inside the thread) is **already U-015's `build_graph_worker_inner`** —
do NOT re-implement it. BUT note the divergence the porter must handle and record:

- MiroFish's `build_task` updates `project.graph_id` + `project.status=GRAPH_COMPLETED/FAILED` from
  INSIDE the background thread (graph.py:413-415, 473-474, 500-502). teri's `build_graph_async` knows
  nothing about projects (it has no project_id arg). So the project-status-on-completion update is a
  behavior that lives in the Python `build_task` but has **no home** in teri's `build_graph_async`.
- **Resolution (`- [!] U025-BUILD-PROJSTATE`, sub-cycle d):** the `/build` handler must either (i) pass a
  completion hook / project_id into a thin teri-side wrapper that updates project state when the task
  completes, OR (ii) record that project.graph_id is set to the teri task_id (since teri has no Zep
  graph_id — see 4b) and project.status transitions are driven by polling the task. **Architect call:
  option (ii)** — set `project.graph_id = Some(task_id)` at spawn (the task_id IS teri's graph handle,
  consistent with DECISION-9/U-015 where the embedded graph replaces the Zep graph_id), and the
  GRAPH_COMPLETED/FAILED transition is reflected by the task status (the frontend already polls
  `/task/<id>`). The synchronous part (set GRAPH_BUILDING + graph_build_task_id + save, graph.py:370-372)
  IS ported verbatim. This preserves every observable: the response shape, the project status progression
  (BUILDING at request time, terminal state visible via task poll), and force-rebuild reset. Flagged for
  the parity gate to confirm no project-state observable is dropped.

### 4b. The graph_id-persistence gap — `get_graph_data` / `delete_graph` (`- [!] U025-GRAPHSTORE`)

**Finding (do NOT hand-wave):** MiroFish's `/data/<graph_id>` and `/delete/<graph_id>`
(graph.py:569-622) call `GraphBuilderService(api_key).get_graph_data(graph_id)` /
`.delete_graph(graph_id)`, which fetch/delete a graph **persisted on the Zep server by `graph_id`**
(`graph_builder.py:426-505` `fetch_all_nodes/fetch_all_edges(client, graph_id)`; `:503-505`
`client.graph.delete(graph_id=…)`). The return is a rich `{graph_id, nodes[], edges[], node_count,
edge_count}` with per-node `{uuid,name,labels,summary,attributes,created_at}` and per-edge temporal
fields (`fact,fact_type,source/target_node_uuid+name,valid_at,invalid_at,expired_at,episodes,…`).

**teri has NO graph-by-id store.** Verified: the only `graph_id` occurrences are the `Project.graph_id`
field, report/sim metadata strings, and `GraphMemoryUpdater.graph_label` — none is a retrievable graph
registry. `build_graph_async` **embeds the serialized graph in the task result and then drops it**
(`graph_builder.rs:289-306`); nothing keys a `KnowledgeGraph` by id for later fetch/delete.

So `/data/<graph_id>` and `/delete/<graph_id>` **cannot be byte-faithfully ported as-is** — there is no
backing store to read/delete from. This is a genuine **`- [!]` blocked**, NOT a `[≠]` (the routes produce
observable output a frontend consumes — it is portable, just missing its substrate). **Resolution path
(decided, so the porter is not blocked):**

- **`graph_id` in teri == the build task_id** (DECISION-9/U-015: the embedded serialized graph IS teri's
  graph handle). So `/data/<graph_id>` resolves the graph from `TaskManager::global().get_task(graph_id)`
  → `task.result["graph"]` (the `SerializableKnowledgeGraph` JSON embedded by `build_graph_worker_inner`
  at `graph_builder.rs:303`). The handler re-shapes that into the `{graph_id, nodes[], edges[],
  node_count, edge_count}` contract. The per-node/edge Zep temporal fields (`valid_at`/`invalid_at`/
  `expired_at`/`episodes`/`summary`/`labels`) are **Zep-server bitemporal artifacts with no teri
  equivalent** (consistent with U-015's wait_for_episodes `[≠]`) → those specific keys are `[≠]`
  inexpressible; the teri-present fields (uuid/name/kind→labels, from/to→source/target, count rollups)
  ARE ported. This is a **MAP-ONTO** (graph_id→task_id→embedded graph), recorded with its substrate gaps.
- **`/delete/<graph_id>`** → there is no persistent graph to delete; teri's task result is reaped by
  `cleanup_old_tasks`. Decision: `delete_graph` returns the success envelope `{success:true,message:
  api.graphDeleted}` and removes/ignores the task entry (teri has no separate graph store to purge). The
  *observable response* (success + localized message) is ported; the side effect "remove from Zep" maps
  onto "no persistent teri graph store exists" — `[≠]` inexpressible substrate, response preserved.
- **Escalation note for the owner:** if a future unit (e.g. the frontend graph-viewer) requires a
  durable, independently-addressable graph store (survives task cleanup; queryable after the build task
  ages out), that is a NEW capability not present as a teri substrate today — it would be its own unit
  (a `GraphStore` keyed by id, analogous to `ProjectManager`). It is **recorded here as the resolution
  path for U025-GRAPHSTORE**, surfaced to the orchestrator, and is the only "vendor/reimplement/FFI"-class
  decision in U-025 (decision: reimplement-as-task-result-map for now; full GraphStore deferred to its own
  unit, NOT silently dropped). The differential gate verifies the response envelope + the teri-present
  field subset; the Zep-only temporal keys are challenged as `[≠]`.

> Honest summary of 4b: **data/delete routes ARE ported** (response envelopes + teri-present graph fields,
> mapping graph_id→task_id→embedded graph), with the Zep-server bitemporal node/edge fields flagged
> `[≠] inexpressible` (same class as U-015) and a deferred `GraphStore` unit recorded as the path to full
> durability. Nothing observable-and-portable is dropped.

---

## 5. Per-route X→Y map (Python route → teri handler → landed primitive → response contract)

| # | Python route (graph.py) | teri handler | Primitive(s) | Success body | Error bodies |
|---|---|---|---|---|---|
| 1 | `GET /project/<id>` get_project (`:36`) | `get_project` | `ProjectManager::get_project` | `{success:true,data:project.to_dict()}` | 404 `{success:false,error:t(api.projectNotFound,id)}` |
| 2 | `GET /project/list` list_projects (`:55`) | `list_projects` | `list_projects(limit)` (`?limit`, default 50, `type=int`) | `{success:true,data:[…to_dict],count}` | — |
| 3 | `DELETE /project/<id>` (`:70`) | `delete_project` | `delete_project`→bool | `{success:true,message:t(api.projectDeleted,id)}` | 404 `{…error:t(api.projectDeleteFailed,id)}` |
| 4 | `POST /project/<id>/reset` (`:89`) | `reset_project` | get→mutate(status,graph_id=None,task_id=None,error=None)→save | `{success:true,message:t(api.projectReset,id),data:to_dict}` | 404 projectNotFound |
| 5 | `POST /ontology/generate` (`:122`) | `generate_ontology` | **multipart**; ProjectManager create+save_file+save_extracted_text; `OntologyGenerator::new(build_llm).generate` | `{success:true,data:{project_id,project_name,ontology,analysis_summary,files,total_text_length}}` | 400 requireSimulationRequirement / requireFileUpload / noDocProcessed; 500 envelope |
| 6 | `POST /build` (`:260`) | `build_graph` | config zep check; ProjectManager status-machine; `build_graph_async` (U-015) | `{success:true,data:{project_id,task_id,message:t(api.graphBuildStarted,taskId)}}` | 500 configError; 400 requireProjectId/ontologyNotGenerated/graphBuilding(+task_id)/textNotFound/ontologyNotFound; 404 projectNotFound; 500 envelope |
| 7 | `GET /task/<id>` (`:534`) | `get_task` | `TaskManager::global().get_task` | `{success:true,data:task.to_dict()}` | 404 t(api.taskNotFound,id) |
| 8 | `GET /tasks` (`:553`) | `list_tasks` | `TaskManager::global().list_tasks(None)` | `{success:true,data:[…to_dict],count}` | — |
| 9 | `GET /data/<graph_id>` (`:569`) | `get_graph_data` | **MAP-ONTO** graph_id→task→`result["graph"]`; zep check | `{success:true,data:{graph_id,nodes,edges,node_count,edge_count}}` (teri-present subset; Zep temporal keys `[≠]`) | 500 zepApiKeyMissing; 500 envelope |
| 10 | `DELETE /delete/<graph_id>` (`:597`) | `delete_graph` | **MAP-ONTO** no persistent store; zep check | `{success:true,message:t(api.graphDeleted,id)}` | 500 zepApiKeyMissing; 500 envelope |

**`allowed_file` helper** (graph.py:26-31): port as a private fn checking `config.allowed_extensions`
(splitext lower lstrip '.') — used by route 5.

**multipart `/ontology/generate`** (route 5): MiroFish reads `request.form.get('simulation_requirement'|
'project_name'|'additional_context')` + `request.files.getlist('files')`. teri uses axum `Multipart`
(extractor) — iterate fields, collect text fields + file parts (filename + bytes). `save_file_to_project`
already takes `&[u8]` (project.rs:554), so the file bytes flow directly. Max content length (50MB) is
enforced via axum `DefaultBodyLimit`/`RequestBodyLimitLayer` scoped to this route to mirror Flask's
`MAX_CONTENT_LENGTH` (`config.max_content_length`, config.rs:31).

**zep_api_key check (routes 6, 9, 10):** MiroFish guards on `Config.ZEP_API_KEY`. teri has
`config.zep_api_key:Option<String>` (config.rs:27). **Decision:** port the guard literally
(None→500 zepApiKeyMissing / configError) so the *error path is byte-faithful*, EVEN THOUGH teri's pipeline
is keyless (DECISION-13/U-015 noted Zep is keyless). This is contractual: the frontend may send a request
expecting the 500 when the key is unset. The guard is a pure response-shape behavior — port it; do not
optimize it away (that would be an observable divergence). Flag `- [≠] U025-ZEPGUARD?` for the gate to
decide whether keeping the guard (faithful) vs. always-pass (keyless reality) is the right contract —
**default: keep the guard (port-faithful), let the gate challenge.**

---

## 6. Sub-cycle decomposition (ordered, each independently port + parity-verifiable in one cycle)

Grouped by shared concern; the **shared seam (a) lands first** and U-026/U-027 reuse it. Each sub-cycle is
≤ ~one cycle. Deps + flags marked.

- **(a) Shared route-layer seam + un-stub `create_app`** — deps: U-002/U-003 (done).
  Deliver: `ApiError` (IntoResponse, `{success:false,error[,traceback][,extra]}` constructors) + `build_llm(config)->OpenAiAdapter` in `src/api/mod.rs` (shared); `graph_router(Arc<ApiState>)->Router` skeleton (10 routes wired to handler stubs that return 501-placeholder is NOT allowed — instead land it with route 1-2 already real, see (b), OR land an empty-but-typed router and immediately do (b) same cycle); rewrite `create_app` to build `api_router` + `.nest("/graph", graph_router(...))` + CORS scoped to `/api/*` (server.rs:184-193 TODO). **This is the seam U-026/U-027 inherit** — they add one `.nest()` line + their own `*_router`. Flags: `- [!] U025-ROUTE-ORDER` (verify `/project/list` vs `/project/:project_id` axum resolution).
- **(b) Project read/delete/reset routes (1,2,3,4)** — deps: (a), U-011 (ProjectManager, done).
  Pure ProjectManager + ApiError; no LLM, no multipart. Smallest real handlers; establishes the success/404 envelope pattern the gate locks. Verify `?limit` int-parse (route 2) + reset status-machine (route 4: ontology? →ONTOLOGY_GENERATED else CREATED; clear graph_id/task_id/error).
- **(c) Ontology generate route (5)** — deps: (a),(b), U-011, U-014 (OntologyGenerator), U-005 (i18n).
  multipart extraction (`Multipart` + `DefaultBodyLimit` 50MB), `allowed_file`, per-file save + extract + preprocess (reuse `text_processor`/`FileParser` equiv — confirm `extract_text`/`preprocess` primitives exist; if `FileParser` not yet ported that's a sub-dep), `build_llm`→`OntologyGenerator::generate`, project save. Flag: `- [!] U025-CLONE` (derive `Clone` on `OpenAiAdapter` — needed here too if generate path spawns; generate is awaited inline so Clone may not be needed until (d), but derive it in (c) to unblock (d)). Flag: `- [!] U025-FILEPARSER` (confirm file-text-extraction primitive landed; if not, it's an upstream dep).
- **(d) Build route (6)** — deps: (a),(b),(c-Clone), U-012 (TaskManager), U-015 (build_graph_async).
  Port the full project-state wrapper (config zep check, JSON parse, project lookup, status-machine guards incl. force-reset + graphBuilding+task_id 400, chunk/name resolution, text/ontology fetch, set GRAPH_BUILDING+save, set graph_id=task_id), delegate pipeline to `build_graph_async`. Flags: `- [!] U025-BUILD-PROJSTATE` (project terminal-status via task poll, §4a option ii); `- [≠] U025-ZEPGUARD?` (keep zep guard — gate challenges); `- [!] U025-CLONE` (OpenAiAdapter Clone for spawn).
- **(e) Task query routes (7,8)** — deps: (a), U-012.
  `TaskManager::global()` get/list; trivial envelopes; verify `to_dict` shape already parity-verified in U-012.
- **(f) Graph data + delete routes (9,10) — the gap routes** — deps: (a),(d), U-015.
  MAP-ONTO graph_id→task_id→`result["graph"]` reshape into `{nodes,edges,node_count,edge_count}` (teri-present field subset); delete = success envelope (no persistent store). Flags: `- [!] U025-GRAPHSTORE` (deferred durable GraphStore unit recorded; §4b); `- [≠] U025-ZEP-TEMPORAL` (Zep bitemporal node/edge fields inexpressible — same class as U-015 wait_for_episodes). **Do (f) LAST** — it depends on (d)'s graph_id=task_id convention and is the only sub-cycle carrying open `[!]`/`[≠]` the gate must adjudicate.

**Dep graph:** (a) → {(b),(e)}; (b) → (c) → (d) → (f). (e) parallel to (b)/(c)/(d). Critical path
a→b→c→d→f. Each commits independently; (a) is the shared deliverable U-026/U-027 build on.

---

## 7. No-downgrade risk flags (pre-identified for the parity gate)

| Flag | Class | What the gate must confirm |
|---|---|---|
| `U025-TRACEBACK` | `- [≠]` | 3-key `{success,error,traceback}` shape preserved; `traceback` VALUE being a Rust string (not Python stack) is non-contractual — NOT a key drop |
| `U025-ZEPGUARD?` | `- [≠]?` | keeping the `ZEP_API_KEY`-None→500 guard (port-faithful) vs keyless-always-pass — default KEEP; gate decides the contract |
| `U025-BUILD-PROJSTATE` | `- [!]` | project terminal status (COMPLETED/FAILED) observable via task poll since `build_graph_async` has no project_id; no project-state observable dropped |
| `U025-GRAPHSTORE` | `- [!]` | graph_id→task_id→embedded-graph map serves data/delete; durable GraphStore deferred to its own unit (recorded, surfaced) — routes still produce their response contract |
| `U025-ZEP-TEMPORAL` | `- [≠]` | Zep bitemporal node/edge fields (valid_at/invalid_at/expired_at/episodes/summary/labels) inexpressible (same class as U-015); teri-present fields ported |
| `U025-ROUTE-ORDER` | `- [!]` | `/project/list` resolves before `/project/:project_id` in axum 0.7 (static-before-capture); no overlap panic |
| `U025-CLONE` | `- [!]` | derive `Clone` on `OpenAiAdapter` (additive, fields all Clone) so `build_graph_async`/spawn accept it by value |
| `U025-FILEPARSER` | `- [!]` | file text-extraction + preprocess primitive landed (route 5); if absent it is an upstream dep, not a skip |

None of these is a disguised feature-skip: every route's observable response is ported; the `[≠]` items are
genuinely Zep-server-inexpressible substrate (consistent with the standing U-015 Zep `[≠]` precedent) or
non-contractual value-shape (traceback string). **When in doubt, the porter ports it and lets the gate
challenge** — a wrong `[≠]` costs one re-port cycle, not a silent drop.
