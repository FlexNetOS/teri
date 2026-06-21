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

> **SUPERSEDED by §4a-REFINED below.** A closer read of the Python `build_task` background thread
> (graph.py:378-509) shows option (ii) DROPS three real project-state observables
> (`project.graph_id`, `project.status=GRAPH_COMPLETED/FAILED`, `project.error`) that the frontend reads
> via `GET /api/graph/project/<id>`. Option (ii) was a reuse-by-narrowing the no-downgrade directive
> forbids. The refined decision (§4a-REFINED) is **(A) additively extend** `build_graph_async` with an
> optional project-completion hook. Read §4a-REFINED, not the option-(ii) call above.

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

---

## §4a-REFINED — (d) project-completion: ADDITIVELY EXTEND, do not narrow

**This section REPLACES §4a's option-(ii) call.** A closer read of the Python `build_task` background
thread (graph.py:378-509) shows option (ii) silently drops real project-state observables. The refined
decision is **(A) additively extend `build_graph_async`/`build_graph_worker` with an optional
project-completion hook**. Every claim is grounded `file:line` in both repos.

### R.1 — The finding option (ii) missed (why narrowing is a downgrade)

Python `build_task` performs **three** `ProjectManager.save_project(project)` mutations from inside the
background thread that teri's worker does NOT:

| # | When | Mutation | graph.py:line |
|---|---|---|---|
| 1 | after `create_graph` (mid-build, ~10%) | `project.graph_id = graph_id; save` | 413-415 |
| 2 | on success (terminal) | `project.status = GRAPH_COMPLETED; save` | 472-474 |
| 3 | on failure (terminal) | `project.status = FAILED; project.error = str(e); save` | 500-502 |

A frontend polling `GET /api/graph/project/<id>` during/after a build therefore observes the project
transition `…→GRAPH_BUILDING→GRAPH_COMPLETED` (or `FAILED` + a populated `error`), and `project.graph_id`
populated. The project's `to_dict()` serialises `status`, `graph_id`, and `error` (project.rs:170,172,180;
`to_dict` already parity-verified in U-011). These are **contractual, observable** outputs of route (1)
`GET /project/<id>` (graph.py:36), which renders them.

**Option (ii) leaves the project stuck at `GRAPH_BUILDING` forever**, with `graph_id` either null or
`=task_id` set at spawn, and never records the `FAILED`+`error` path. The COMPLETED/FAILED/error
transitions are LOST. That is precisely the reuse-by-narrowing the no-downgrade directive forbids
("complete the feature, never silently narrow"). It does **not** clear the `[≠]` bar (the transitions are
genuinely expressible in teri — `Project.status`/`graph_id`/`error` fields all exist, project.rs:157/170/180
— and they are observable via route 1). So this MUST be ported, not `[≠]`'d.

### R.2 — Decision: (A) additively EXTEND (chosen). (B) rejected.

**(A) chosen.** `build_graph_async`/`build_graph_worker` gain an OPTIONAL project-completion hook. When
present, the worker applies the three mutations faithfully at the terminal transition. When absent (every
existing caller), behavior is byte-identical to U-015's verified shape (zero blast radius). This preserves
every project-state observable while keeping `build_graph_worker_inner` (the project-agnostic pipeline that
tests drive directly) untouched.

**(B) rejected.** Keeping option (ii) would require showing the dropped transitions are non-observable.
They are not: route 1 (`GET /project/<id>`) serves `project.to_dict()` including `status`/`graph_id`/`error`,
and the frontend renders the CREATED→…→COMPLETED/FAILED progression. A `[≠]` here would be a disguised
feature-skip and the parity gate would FAIL it. (B) is therefore not a legal `[≠]`.

### R.3 — The exact additive signature (zero blast radius)

The existing test `test_build_graph_async_returns_task_id_immediately` (graph_builder.rs:398) calls
`build_graph_async(mock_llm, text, ontology, name, 500, 50, 3)` — **7 positional args**. Rust has no default
parameters, so an 8th positional param WOULD break that call. To keep blast radius truly zero, introduce a
small owned struct and a NON-breaking second entry point:

```rust
/// Optional project-completion hook for `build_graph_async` (port of build_task's three
/// `ProjectManager.save_project` mutations, graph.py:413-415,472-474,500-502).
/// `manager` holds only a `PathBuf` → Clone+Send+Sync+'static, safe to move into tokio::spawn.
pub struct ProjectCompletion {
    pub manager: ProjectManager,   // ProjectManager (project.rs:332) — derive Clone (see R.6)
    pub project_id: String,
}

// EXISTING entry point — unchanged 7-arg signature. Delegates with `None`.
pub fn build_graph_async<L>(
    llm: L, text: String, ontology: Value, graph_name: String,
    chunk_size: usize, chunk_overlap: usize, _batch_size: usize,
) -> String
where L: LlmClient + Clone + Send + Sync + 'static
{
    build_graph_async_with_completion(
        llm, text, ontology, graph_name, chunk_size, chunk_overlap, _batch_size, None,
    )
}

// NEW additive entry point — carries the optional hook. The (d) /build handler calls THIS.
#[allow(clippy::too_many_arguments)]
pub fn build_graph_async_with_completion<L>(
    llm: L, text: String, ontology: Value, graph_name: String,
    chunk_size: usize, chunk_overlap: usize, _batch_size: usize,
    completion: Option<ProjectCompletion>,
) -> String
where L: LlmClient + Clone + Send + Sync + 'static
{ /* create task; spawn worker with `completion` threaded through */ }
```

**Rationale for the wrapper (not a new 8th param on the same fn):** keeping `build_graph_async` at 7 args
means the existing test (graph_builder.rs:398) and every doc-referenced call shape compile unchanged — the
blast radius is literally zero, no call-site edits. The (d) handler is the sole caller of the new
`*_with_completion` form. (Equivalent acceptable variant: make the 8th param and update the one test call —
but the wrapper keeps U-015's verified surface byte-stable, which is the safer no-downgrade choice. Porter
picks; default = wrapper.)

### R.4 — Worker hook placement (exact call sites)

The hook fires in **`build_graph_worker`** (the outer wrapper, graph_builder.rs:130-156) — NOT in
`build_graph_worker_inner` (164-308), which tests drive directly and must stay project-agnostic. The outer
wrapper already owns both terminal branches:

```rust
async fn build_graph_worker<L>(
    task_id: String, llm: L, text: String, ontology: Value,
    graph_name: String, chunk_size: usize, chunk_overlap: usize,
    completion: Option<ProjectCompletion>,   // NEW — threaded from build_graph_async_with_completion
) where L: LlmClient + Clone + Send + Sync + 'static {
    let result = build_graph_worker_inner(&task_id, llm, text, ontology,
                                          graph_name, chunk_size, chunk_overlap).await;
    match &result {
        Ok(())  => { /* SUCCESS terminal — fires AFTER inner set the task result via complete_task */
            if let Some(c) = &completion {
                // port of graph.py:472-474 (+ graph_id=task_id, see R.5)
                if let Ok(Some(mut p)) = c.manager.get_project(&c.project_id) {
                    p.status   = ProjectStatus::GraphCompleted;
                    p.graph_id = Some(task_id.clone());   // teri graph handle = task_id (R.5)
                    let _ = c.manager.save_project(&mut p);
                }
            }
        }
        Err(e) => {                                  // FAILURE terminal
            // existing: route to fail_task (unchanged)
            TaskManager::global().fail_task(&task_id, e.to_string());
            if let Some(c) = &completion {            // port of graph.py:500-502
                if let Ok(Some(mut p)) = c.manager.get_project(&c.project_id) {
                    p.status = ProjectStatus::Failed;
                    p.error  = Some(e.to_string());
                    let _ = c.manager.save_project(&mut p);
                }
            }
        }
    }
}
```

Placement notes (port-faithful):
- The success hook fires **after** `build_graph_worker_inner` returns `Ok` — i.e. after the inner already
  called `complete_task` (graph_builder.rs:306). Python's order is the same: task COMPLETED (481-493) is set
  inside the thread, then `project.status=GRAPH_COMPLETED` is the last project save (472-474, actually just
  before the task complete in Python, but both are terminal and observed only after the thread ends — order
  between the two terminal saves is not separately observable, so "task-complete then project-complete" is
  faithful).
- The failure hook fires alongside the existing `fail_task` (graph_builder.rs:154), mirroring Python's
  `except` block which does both `project.status=FAILED;save` (500-502) and `task fail` (504-509).
- The hook **swallows** a project-reload/save error (`if let Ok(Some(...))`, `let _ =`) — Python's
  `save_project` inside the thread is best-effort (a thread exception there would only be logged); the task
  status is the authoritative progress signal. A failed project save must not panic the spawned worker.
- **Mutation #1 (mid-build `graph_id` set, graph.py:413-415) is NOT ported as a separate mid-build save** —
  see R.5: teri has no mid-build graph handle, so `graph_id` is set once at the COMPLETED terminal. This is
  observably equivalent (R.5 proves the during-build `graph_id` is not observed).

### R.5 — graph_id timing: set at COMPLETED, not at spawn (adjudicated)

Python sets `project.graph_id` **mid-build** at `create_graph` time (graph.py:413-414, ~10% progress) — the
Zep server handle, available the moment the remote graph object is created. teri has **no mid-build graph
handle**: the graph is built in-process and only exists once `build_graph_worker_inner` finishes
(graph_builder.rs:262-307); there is no Zep `graph_id` (DECISION-9 / U-015 `[≠]`). The teri graph handle IS
the `task_id` (DECISION-9 / §4b: `/data/<graph_id>` resolves `graph_id==task_id` → `task.result["graph"]`).

**Decision: set `project.graph_id = Some(task_id)` at the COMPLETED transition (R.4 success hook), NOT at
spawn.** Justification that this is observably equivalent to Python:

- During the build, Python's `project.graph_id` is the Zep handle (a server graph id). teri's would be null.
  **Is the during-build `graph_id` observable / does the frontend use it?** No: the frontend tracks build
  progress via `GET /task/<id>` (the `/build` response hands back `task_id` and the localized
  `graphBuildStarted` message pointing at `/task/<id>`, graph.py:515-521; i18n `graphBuildStarted` =
  "...Query progress via /task/{taskId}", en.json:336). `graph_id` is consumed only **after** COMPLETED, to
  address `/data/<graph_id>` and `/delete/<graph_id>`. The two handles are also semantically different
  (Zep server id vs teri task id) and not byte-comparable anyway, so a differential gate cannot compare the
  *value* mid-build — only "is it set when the consumer reads it." The consumer reads it after COMPLETED;
  teri sets it at COMPLETED. **Null-until-complete is faithful.**
- Setting `graph_id=task_id` at COMPLETED (not spawn) also keeps `graph_id` and `status` consistent: a
  project is only ever `GraphCompleted` ⇔ `graph_id` populated, which matches Python's end state and avoids a
  spurious "graph_id set but status still BUILDING/FAILED" intermediate teri would otherwise expose.
- On the FAILED path, `graph_id` stays `None` (the force-reset already cleared it; we never set it) — matches
  Python (a failed build never reaches line 414's save in a way the final state keeps, because 500-502
  doesn't set graph_id and the create_graph save only happens if create_graph succeeded; teri's "set only at
  COMPLETED" gives the same terminal observable: FAILED ⇒ graph_id None).

So: graph_id set **once, at the COMPLETED terminal, = task_id**. The synchronous-path `graph_id` is NOT set
at spawn (the route sets only `status=GraphBuilding` + `graph_build_task_id=task_id`, see R.7 step 13).

### R.6 — `ProjectManager` is spawn-safe; derive `Clone`

`ProjectManager` (project.rs:332) holds a single field `projects_dir: PathBuf` (project.rs:334). `PathBuf`
is `Clone + Send + Sync + 'static`, so `ProjectManager` is `Send + Sync + 'static` and safe to move into
`tokio::spawn` inside the `ProjectCompletion` struct. It does **not currently derive `Clone`** (no `#[derive]`
on the struct, project.rs:331-335). **Action (sub-cycle d):** add `#[derive(Clone)]` to `ProjectManager` —
additive, trivial (the sole field is `Clone`), zero behavior change. Even Clone is not strictly required here
(the hook takes `&self`/owns a single instance moved into the spawn), but deriving it is cheap and matches
the `Clone`-everywhere posture of the spawn-bound types. The route constructs
`ProjectManager::from_config(&state.config)` (project.rs:350) and moves it into `ProjectCompletion`.

### R.7 — Blast radius confirmation (build_graph_async callers)

Searched all of `src/` + `tests/` for callers of `build_graph_async` / `build_graph_worker` /
`build_graph_worker_inner`:

| Caller | Location | Impact of the additive change |
|---|---|---|
| `test_build_graph_async_returns_task_id_immediately` | graph_builder.rs:398 (7-arg call) | **ZERO** — `build_graph_async` keeps its 7-arg signature; delegates with `None` (R.3). Test compiles + passes unchanged. |
| `test_build_graph_worker_inner_completes_with_result` | graph_builder.rs:445 | **ZERO** — calls `build_graph_worker_inner` (project-agnostic), which is untouched by the hook (hook lives in the outer `build_graph_worker`). |
| `test_*_routes_err_to_fail_task` | graph_builder.rs:526 | **ZERO** — same: drives `build_graph_worker_inner` directly. |
| doc-comment references (mod.rs:245, graph/mod.rs:511, llm.rs:305, graph_builder.rs:7,53,380) | — | **ZERO** — comments, not calls. |
| **NEW:** `/build` handler (sub-cycle d) | `src/api/graph.rs` (to be written) | the ONLY caller of the new `build_graph_async_with_completion`; passes `Some(ProjectCompletion{...})`. |

There is **no existing production caller** of `build_graph_async` — the only production caller will be the
(d) `/build` handler written in the same sub-cycle. U-015's verified surface (the 7-arg `build_graph_async`
+ `build_graph_worker_inner`) is therefore byte-stable; the hook is purely additive.

### R.8 — ZEP-guard adjudication: KEEP (expressible + faithful, NOT keyless-inexpressible)

teri **does** have a `zep_api_key: Option<String>` config field (config.rs:27, populated from
`std::env::var("ZEP_API_KEY")`, config.rs:250). Moreover `Config::validate_collect()` already treats
ZEP_API_KEY as **required** ("ZEP_API_KEY is not set", config.rs:350-352), a direct port of Python's
`validate()`. So teri is **not** keyless w.r.t. ZEP at the config layer — the guard is fully expressible and
the project's keyless-by-design posture (envctl model) applies to *injection*, not to dropping the contract.

**Decision: KEEP the guard, port it literally.** The (d) handler's step 1 is:
```rust
let mut errors = Vec::new();
if state.config.zep_api_key.as_deref().unwrap_or("").is_empty() {
    errors.push(i18n::t("api.zepApiKeyMissing"));        // en.json:331 / zh.json:331 PRESENT
}
if !errors.is_empty() {
    return Err(ApiError::client(StatusCode::INTERNAL_SERVER_ERROR,         // 500
        i18n::t_args("api.configError", &[("details", &errors.join("; "))]))); // configError, :330
}
```
This is byte-faithful to graph.py:286-295 (errors list → `t('api.configError', details=…)` → 500). It is
**NOT** an inexpressible `[≠]` (the field + the i18n keys + the validate contract all exist). The earlier
`- [≠] U025-ZEPGUARD?` flag is **RESOLVED to KEEP/PORTED** — drop the `?`-divergence; it is a normal ported
guard. (The runtime reality that envctl usually injects the key only means the guard passes in practice; the
*error path* when it is absent is still contractual and ported.)

### R.9 — task-name `[≠]` adjudication

Python names the task `f"构建图谱: {graph_name}"` (graph.py:366) — a localized, graph-name-interpolated label.
teri's `build_graph_async` creates the task as `create_task("graph_build", metadata)` (graph_builder.rs:102),
a stable machine token, with `graph_name` carried in the task **metadata** (graph_builder.rs:98). The task's
`task_type`/name shape is **U-015's already-verified contract** (the `test_*` at graph_builder.rs:414 asserts
`task.task_type == "graph_build"`). **Decision: accept `- [≠] U025-TASKNAME`** — the human-readable
build-title-as-task-name is not reproduced; `graph_name` is preserved in metadata instead. This is a legal
`[≠]`: the task name is non-contractual debug/display text (the frontend keys off `task_id` + `task_type` +
progress/result, not the free-text name), and changing it would re-open U-015's verified surface. Do **not**
pass the localized name through (that would churn U-015 for a non-observable). Gate challenges; default
`[≠]` accepted (graph_name is preserved in metadata, so no data is lost — only the display label differs).

### R.10 — Full ordered (d) handler step list (graph.py:283-372, 515-522)

The synchronous `/build` handler, in order. Note **build_graph_async creates the task FIRST and returns
task_id**, so Python's "create_task" (graph.py:365-366) is **SUBSUMED** by the call — the handler sets the
project fields AFTER getting task_id (minor reorder vs Python, flagged below).

1. **ZEP guard** (R.8): `zep_api_key` empty → push `zepApiKeyMissing` → 500 `configError`. (graph.py:286-295)
2. **Parse JSON body** (axum `Json<Value>` or tolerant `Option`): `data = body or {}`. (graph.py:298)
3. **`project_id` required**: missing/empty → 400 `requireProjectId`. (graph.py:299-306)
4. **Project lookup**: `manager.get_project(&project_id)?` → `None` → 404 `projectNotFound(id)`. (graph.py:309-314)
5. **Read `force`**: `data.force` default `false`. (graph.py:317)
6. **Status machine — CREATED**: `status==Created` → 400 `ontologyNotGenerated`. (graph.py:319-323)
7. **Status machine — BUILDING && !force**: → 400 `{success:false, error:graphBuilding, task_id:
   project.graph_build_task_id}` (the extra `task_id` key — `ApiError::client_with`). (graph.py:325-330)
8. **Force-reset**: if `force && status ∈ {GraphBuilding, Failed, GraphCompleted}` →
   `status=OntologyGenerated; graph_id=None; graph_build_task_id=None; error=None`. (graph.py:333-337)
9. **Resolve graph_name**: `data.graph_name` else `project.name` (non-empty) else `"MiroFish Graph"`.
   (graph.py:340) — keep the literal default string `"MiroFish Graph"` (do NOT localize/rename).
10. **Resolve chunk_size / chunk_overlap**: `data.chunk_size` else `project.chunk_size` (if non-zero) else
    `config.default_chunk_size`; same for overlap with `config.default_chunk_overlap`. (graph.py:341-342)
11. **Update project chunk config**: `project.chunk_size = chunk_size; project.chunk_overlap = chunk_overlap`
    (in-memory; saved at step 13). (graph.py:345-346)
12. **Fetch extracted text**: `manager.get_extracted_text(&project_id)?` → empty/None → 400 `textNotFound`.
    (graph.py:349-354)
13. **Fetch ontology**: `project.ontology` → None/empty → 400 `ontologyNotFound`. (graph.py:357-362)
14. **Spawn (SUBSUMES create_task)**: call
    `build_graph_async_with_completion(build_llm(&state.config), text, ontology, graph_name, chunk_size as
    usize, chunk_overlap as usize, /*batch*/3, Some(ProjectCompletion{ manager:
    ProjectManager::from_config(&state.config), project_id }))` → returns `task_id`. This **creates the task
    internally** (graph_builder.rs:102) — there is NO separate `TaskManager::create_task` call in the
    handler. (subsumes graph.py:364-366; pipeline = graph.py:378-509 via the worker+hook)
15. **Set project BUILDING + save**: `project.status = GraphBuilding; project.graph_build_task_id =
    Some(task_id.clone()); manager.save_project(&mut project)?`. (graph.py:370-372) — **graph_id is NOT set
    here** (R.5: set at COMPLETED by the hook).
16. **Response**: `200 {success:true, data:{project_id, task_id, message: t_args("api.graphBuildStarted",
    [("taskId", task_id)])}}`. (graph.py:515-522; i18n key en.json:336)
17. **Outer 500 envelope**: any unhandled `?`-propagated error → `ApiError::server(err)` →
    `{success:false, error:err.to_string(), traceback:<teri ctx>}` (the §3b `U025-TRACEBACK` `[≠]`).
    (graph.py:524-529)

**Reorder flag (`- [≠]`-free, behavior-equivalent):** Python saves the project (BUILDING + task_id) BEFORE
spawning the thread (graph.py:370-372 then 512-513); teri gets `task_id` from `build_graph_async_with_completion`
FIRST (step 14) THEN saves BUILDING + task_id (step 15). The reorder is **not observable**: the task and the
project-BUILDING save both become visible to subsequent polls only after the handler returns its response;
no concurrent reader can observe the intra-handler ordering. The spawned worker only touches the project at
its **terminal** transition (R.4), which is strictly after step 15's save. So no race, no observable
divergence. (If extra-defensive: the worker's success/failure hook does `get_project` fresh, so even if it
ran before step 15's save it would read the pre-BUILDING state and overwrite with the terminal state — still
correct; but it cannot, since the terminal transition is post-pipeline = long after step 15.)

### R.11 — Refined risk flags (supersede §7 rows for d)

| Flag | Class (refined) | Resolution |
|---|---|---|
| `U025-BUILD-PROJSTATE` | **PORTED** (was `- [!]`) | (A) additive hook: success → `status=GraphCompleted, graph_id=Some(task_id)`; failure → `status=Failed, error=Some(e)`; both via `ProjectCompletion` hook in `build_graph_worker` (R.4). No project-state observable dropped. |
| `U025-ZEPGUARD?` | **PORTED** (was `- [≠]?`) | KEEP guard, literal port — config field + i18n keys + validate contract all exist (R.8). Drop the `?`/divergence. |
| `U025-GRAPHID-TIMING` | **PORTED** (decision) | `graph_id=task_id` set once at COMPLETED, not spawn; null-until-complete is faithful (R.5). |
| `U025-TASKNAME` | `- [≠]` (accepted) | task name `构建图谱: {graph_name}` → stable `"graph_build"` token + graph_name in metadata; non-contractual display label, preserving U-015's verified `task_type` shape (R.9). graph_name not lost (in metadata). |
| `U025-CLONE` (carried) | `- [!]`→PORTED | derive `Clone` on `OpenAiAdapter` (spawn by value) AND on `ProjectManager` (R.6) — both additive, fields all `Clone`. |
| `U025-TRACEBACK` (carried) | `- [≠]` | unchanged from §3b — 3-key envelope preserved, value is teri context. |

**Net:** the (d) sub-cycle ports ALL project-state observables (no narrowing); the only `[≠]` items are the
task-name display label (R.9, non-contractual, graph_name preserved) and the carried traceback-value `[≠]`
(§3b). No disguised feature-skip. The differential gate verifies: (1) project transitions CREATED→BUILDING→
COMPLETED/FAILED+error via route 1 polling, (2) `graph_id` populated == task_id after COMPLETED, (3) the
`/build` response envelope + `graphBuilding`+task_id 400 + force-reset, (4) ZEP-missing → 500 configError.
