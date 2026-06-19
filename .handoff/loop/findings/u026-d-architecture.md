# U-026 sub-cycle (d) — `POST /prepare` + `POST /prepare/status` (target architecture)

Port-and-merge, target == dest == teri. Source: `MiroFish/backend/app/api/simulation.py`
`prepare_simulation` (L359-639) and `get_prepare_status` (L642-752).
Dest substrate: `src/api/simulation.rs`, `src/services/simulation_manager.rs`, `src/task.rs`,
`src/api/mod.rs`, `src/services/graph_builder.rs` (the proven spawn template).

This file records the ONE hard structural decision (Send-safe background progress) plus the full
route shape so the porter executes (d) against a recorded design, not an invented one.

---

## 1. The core decision: how to run `prepare_simulation` as a Send-safe background task that keeps reporting progress

### Constraint (verified)
`SimulationManager::prepare_simulation` (simulation_manager.rs:1180) takes
`progress_callback: Option<&mut dyn FnMut(PrepareProgress<'_>)>`. `&mut dyn FnMut` is `!Send`.
A future that holds it live across an `.await` (the pipeline awaits `generate_profiles_from_entities`
and `generate_config`) is therefore `!Send` and **cannot** be handed to `tokio::spawn` (which
requires `Future: Send`). Running with `None` would freeze `/prepare/status` progress at 0 until the
whole pipeline finishes — a **capability downgrade** of the observable contract. Not allowed.

### DECISION-U026-d-1 — adopt **option (b)**: a dedicated `prepare_simulation_async_with_completion` worker that builds a `Send` progress closure INTERNALLY, mirroring `build_graph_async_with_completion`.

**Do NOT change `prepare_simulation`'s callback bound (reject option (a)).** Rationale:

1. **The spawn template already proves (b) works with zero signature surgery.**
   `build_graph_async_with_completion` (graph_builder.rs:156) is the live, route-driven precedent:
   it owns all inputs by value (`L: LlmClient + Clone + Send + Sync + 'static`), captures locale, and
   does `tokio::spawn(crate::i18n::with_locale(locale, async move { worker(...).await }))`. It is
   called from a real route at `graph.rs:802` with `build_llm(&state.config)` → `OpenAiAdapter`.
   So the exact shape (own inputs → spawn → worker drives pipeline → complete/fail at terminal) is
   already verified green in teri with the concrete adapter.

2. **`[!] U025-CLONE` is RESOLVED.** `OpenAiAdapter` derives `Clone` (llm.rs:307); `reqwest::Client`
   is `Clone`. It is already moved into `tokio::spawn` from a live route. So `OpenAiAdapter:
   Clone + Send + Sync + 'static` holds today — no prerequisite work for (d).

3. **Why the internal closure is Send (the crux).** Inside the worker we build a
   `let mut cb = |p: PrepareProgress| { ... TaskManager::global() ... };` that captures **only owned
   `String`s** (`task_id`, the i18n stage-name strings). `TaskManager::global()` returns
   `&'static TaskManager` (OnceLock) and its methods take `&self` over a `parking_lot::Mutex`
   (`Send + Sync`). So `cb: FnMut(PrepareProgress) + Send`. We then call
   `prepare_simulation(..., Some(&mut cb))`. The `&mut dyn FnMut` reference handed in is a **local
   borrow that never crosses an `.await` of the WORKER future** — it is created and consumed entirely
   within the single `prepare_simulation(...).await` call expression. The future that is actually
   spawned (`async move { worker(task_id, llm, graph, ...).await }`) owns `String`/`OpenAiAdapter`/
   `KnowledgeGraph`/generators (all `Send`) and holds no `!Send` value across any await — so the
   **worker future is `Send`** even though `prepare_simulation`'s own future (which transiently holds
   the `&mut dyn` internally) is `!Send`. The non-Send-ness is contained below the spawn boundary; we
   never spawn `prepare_simulation`'s future, we spawn the worker's. This is exactly how (b) buys
   Send-safety without touching the inner signature.

4. **Why NOT option (a) (change the bound to `+ Send`).** It is technically smaller but:
   - It would force the raw-pointer trick at simulation_manager.rs:1321 to be re-audited: the
     `&raw mut progress_callback` → `unsafe { &mut *cb_ptr }` aliasing argument is currently written
     for a `!Send` `&mut dyn FnMut`. Adding `+ Send` to the trait object changes the type the raw
     pointer points at (`Option<&mut (dyn FnMut(..) + Send)>`) and would need the SAFETY comment and
     all 7 in-crate test call-sites rewritten. That's a contract change to a U-023-verified,
     byte-parity-PASSed surface for **no behavioral gain** — pure risk.
   - It pushes a Send obligation onto every future caller of `prepare_simulation` (incl. the 7 tests),
     coupling an inner pipeline method to the spawn concern. (b) keeps that concern at the route/worker
     layer where it belongs (idiomatic: the spawn-adapter owns the Send story, the pipeline stays
     pure). **`prepare_simulation`'s signature is UNCHANGED — blast radius on it = 0.**

   (Option (c) "other construction" — e.g. `tokio::task::spawn_local` + a `LocalSet`, or a channel +
   a sync progress drainer — is strictly more machinery than (b) for the same result, and `spawn_local`
   would fight axum's multi-threaded runtime. Rejected.)

### Worker shape (mirror graph_builder.rs:156-197 exactly)

```rust
// src/services/simulation_manager.rs (additive; new pub fn next to prepare_simulation)
// OR src/api/simulation.rs as a route-local helper — PLACE IT IN simulation_manager.rs so it sits
// with prepare_simulation and is unit-testable without an HTTP harness (matches build_graph_async
// living in graph_builder.rs, not the route).

#[allow(clippy::too_many_arguments)]
pub fn spawn_prepare_simulation(
    manager: Arc<SimulationManager>,        // moved in; owns state.json cache
    simulation_id: String,
    simulation_requirement: String,
    document_text: String,
    defined_entity_types: Option<Vec<String>>,
    use_llm_for_profiles: bool,
    parallel_profile_count: usize,
    llm: crate::llm::OpenAiAdapter,         // Clone+Send+Sync+'static (verified)
    graph: crate::graph::KnowledgeGraph,    // owned; resolved from graph_id before spawn
    config_generator: crate::services::simulation_config::SimulationConfigGenerator<crate::llm::OpenAiAdapter>,
) -> String {
    // 1. create task (mirrors Python create_task L491-498) — done in the ROUTE, not here,
    //    because the route must (a) put task_id in the immediate response AND (b) set
    //    state.status=PREPARING + save BEFORE returning. So the route creates the task and
    //    passes task_id in. (graph_builder creates inside; here the route owns ordering — see §2.)
    //    -> Therefore: take `task_id: String` as a parameter; route created it.
    let task_id_worker = task_id.clone();
    let locale = crate::i18n::get_locale();              // capture before spawn (L504-505)
    tokio::spawn(crate::i18n::with_locale(locale, async move {
        prepare_worker(task_id_worker, manager, simulation_id, simulation_requirement,
                       document_text, defined_entity_types, use_llm_for_profiles,
                       parallel_profile_count, llm, graph, config_generator).await;
    }));
    task_id
}
```
(Final signature: add `task_id: String` as the first param; route creates the task so it can also do
the PREPARING-save and embed `task_id`/`expected_entities_count` in the immediate response. This is
the ONE deviation from graph_builder — justified by Python L490-502 ordering: create_task →
status=PREPARING+save → response, all before the thread reads anything.)

```rust
async fn prepare_worker( /* same params incl. task_id */ ) {
    use crate::task::{TaskManager, TaskStatus};
    let tm = TaskManager::global();

    // L511-516: PROCESSING / progress=0 / "startPreparingEnv"
    tm.update_task(&task_id, Some(TaskStatus::Processing), Some(0),
                   Some(crate::i18n::t("progress.startPreparingEnv")), None, None, None);

    // Build the Send progress closure (the crux — captures only owned String task_id).
    let task_id_cb = task_id.clone();
    let persona_generator = crate::agent::PersonaGenerator::new();
    let mut cb = move |p: PrepareProgress<'_>| {
        stage_overall_update(&task_id_cb, p);          // see §1.x mapping below
    };

    let result = manager.prepare_simulation(
        &simulation_id, &simulation_requirement, &document_text,
        defined_entity_types.as_deref(), use_llm_for_profiles, parallel_profile_count,
        &llm, &graph, &persona_generator, &config_generator,
        Some(&mut cb),                                  // &mut dyn FnMut, never crosses worker await
    ).await;

    match result {
        Ok(result_state) => {
            // L593-597: complete_task(result=result_state.to_simple_dict())
            tm.complete_task(&task_id, result_state.to_simple_dict());
        }
        Err(e) => {
            // L599-608: fail_task + reload state → status=FAILED, error=e, save (best-effort)
            tracing::error!("准备模拟失败: {e}");
            tm.fail_task(&task_id, e.to_string());
            if let Ok(Some(mut st)) = manager.get_simulation(&simulation_id) {
                st.status = SimulationStatus::Failed;
                st.error = Some(e.to_string());
                let _ = manager.save_simulation_state(&mut st);   // best-effort, never panic worker
            }
        }
    }
}
```

> NOTE on the inner FAILED-save double-write: `prepare_simulation` ITSELF already sets status=Failed +
> save on its internal try/except (simulation_manager.rs:1271-1286, and the zero-entities path
> L1261-1266 returns `Ok` with status=Failed). The Python route ALSO re-loads and sets FAILED on its
> own `except` (L604-608). Both layers writing FAILED is faithful to Python (Python has the same
> double-write: `prepare_simulation` sets FAILED internally on L450-457 AND `run_prepare` re-sets it
> on L604-608). Keep the worker's outer FAILED-save to match — it is idempotent on disk.

### 1.x — stage → overall-% mapping + progress_detail (MUST match Python L522-581 exactly)

The progress closure maps each `PrepareProgress{stage, progress, current, total, message}` (where
`progress` is the *stage-local* 0-100 the pipeline already emits) to an **overall** percentage and a
`progress_detail` dict, then calls `update_task(progress=overall, message=detailed, progress_detail)`.

**stage_weights** (Python L524-529; iteration order is the dict-literal order, which fixes
`stage_index`):

| order | stage                  | (start, end) overall band |
|-------|------------------------|---------------------------|
| 1     | `reading`              | (0, 20)                   |
| 2     | `generating_profiles`  | (20, 70)                  |
| 3     | `generating_config`    | (70, 90)                  |
| 4     | `copying_scripts`      | (90, 100)                 |

`overall = start + (end - start) * stage_progress / 100`, **integer truncation** (Python `int(...)`).
Use `start + ((end - start) * progress) / 100` in i64 (floor for non-negatives = Python `int`).
Unknown stage → `(0, 100)` and `stage_index = 1` (Python `.get(stage,(0,100))` and the
`index()+1 if stage in ... else 1`).

> `total_stages = 4` (len of stage_weights), even though teri's `prepare_simulation` only EMITS
> stages 1-3 (`reading`/`generating_profiles`/`generating_config`) — confirmed: it never fires
> `copying_scripts` (simulation_manager.rs has no such callback; the source's stage-4 "copying_scripts"
> is a Python-side artifact that teri folded away). The MAPPING TABLE still carries all 4 so
> `total_stages` = 4 and the band math for the 3 emitted stages is byte-identical to Python. This is
> NOT a downgrade: teri emits the same overall %s for the same stage_progress values; stage 4 simply
> never fires in either teri's pipeline reality. `[~] U026-d-STAGE4`: the band `(90,100)` is dead in
> teri (no emitter) but kept in the table for index/total fidelity. Document, don't drop.

**stage_names** — i18n keys (Python L535-540):
`reading→progress.readingGraphEntities`, `generating_profiles→progress.generatingProfiles`,
`generating_config→progress.generatingSimConfig`, `copying_scripts→progress.preparingScripts`.
Resolve via `crate::i18n::t(key)`; unknown stage → the raw stage string (Python `.get(stage, stage)`).

**progress_detail dict** (Python L556-565) — `HashMap<String, Value>`, keys (order is irrelevant for a
JSON object but list them for parity review):
```
current_stage        = stage (str)
current_stage_name   = stage_names.get(stage, stage)
stage_index          = i64 (1-based, per table)
total_stages         = 4
stage_progress       = p.progress (stage-local 0-100)
current_item         = p.current.unwrap_or(0)            // Python detail["current"], default 0
total_items          = p.total.unwrap_or(0)              // Python detail["total"], default 0
item_description      = p.message                         // Python `message`
```
(Python also maintains a `stage_details[stage]` accumulator L546-552, but the only observable output
is `progress_detail_data` above; the accumulator is internal bookkeeping — teri can compute the dict
directly from the current event, no accumulator needed. The `item_name` kwarg Python stores is folded
into `message` already per PrepareProgress doc L69-71, and `progress_detail` uses `item_description =
message`, so nothing is lost.)

**detailed_message** (Python L568-574) — the `update_task` message string:
```
if total_items > 0:
    "[{stage_index}/{total_stages}] {stage_name}: {current_item}/{total_items} - {message}"
else:
    "[{stage_index}/{total_stages}] {stage_name}: {message}"
```
where `total_items = p.total.unwrap_or(0)`, `current_item = p.current.unwrap_or(0)`,
`stage_name = t(stage_names key)`, `message = p.message`.

Then: `tm.update_task(&task_id, None, Some(overall), Some(detailed_message), None, None,
Some(progress_detail))` (status untouched → stays PROCESSING; mirrors Python L576-581 which passes
only progress/message/progress_detail).

> NOTE the FIRST `update_task` (L511-516) sets status=PROCESSING explicitly; subsequent per-stage
> updates leave status unset. complete_task/fail_task flip to COMPLETED/FAILED. Faithful.

---

## 2. `POST /prepare` route — full flow (Python L405-639)

Handler signature (axum, mirrors existing routes in simulation.rs):
```rust
async fn prepare_simulation_route(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<Value>,            // request.get_json() or {} — tolerate missing/empty
) -> Result<Json<Value>, ApiError>
```
Use `Json<Value>` (not a typed struct) because the body has 5 optional fields with Python-default
semantics and the response is a hand-built envelope — matches how other simulation.rs routes handle
loose JSON. `request.get_json() or {}` → if extraction fails, treat as `{}` (an empty object); axum's
`Json<Value>` rejects a malformed/absent body, so either (a) use `Option<Json<Value>>` and default to
`json!({})`, or (b) a custom extractor. **Use `Option<Json<Value>>` → `body.map(|j|j.0).unwrap_or(json!({}))`.**

Steps:
1. **`simulation_id = body["simulation_id"]`** (str). Missing/empty/non-str → **400**
   `{success:false, error: t("api.requireSimulationId")}`. (Python L408-413.)
2. **`state = sim_manager.get_simulation(simulation_id)`** (DECISION-U026-1: use `state.sim_manager`,
   NOT a fresh manager — Python uses `SimulationManager()` but teri's shared Arc is the U-026 decision).
   `Ok(None)` → **404** `{success:false, error: t_args("api.simulationNotFound",[("id",sim_id)])}`.
   `Err` → 500. (Python L415-422.)
3. **`force_regenerate = body["force_regenerate"]` (bool, default false).**
4. **already_prepared short-circuit (only if NOT force):** `check_simulation_prepared(&state.config,
   sim_id)` → `(is_prepared, prepare_info)`. If `is_prepared` → **200**
   ```json
   {"success":true,"data":{
     "simulation_id": sim_id, "status":"ready",
     "message": t("api.alreadyPrepared"), "already_prepared": true,
     "prepare_info": prepare_info }}
   ```
   (Python L429-444. `check_simulation_prepared` is the verified pub(crate) port at
   simulation.rs:1537 — reuse, do not reimplement. NOTE its OBSERVABLE side effect: it auto-upgrades a
   `preparing` state.json to `ready` on disk — that is faithful and already tested.)
5. **`project = ProjectManager::from_config(&state.config).get_project(state.project_id)`**
   `Ok(None)` → **404** `{... error: t_args("api.projectNotFound",[("id",project_id)])}`. (L449-454.)
   (ProjectManager is instance-based in teri — `ProjectManager::from_config(&config)`, project.rs:373;
   `get_project` at project.rs:493, `get_extracted_text` at project.rs:628.)
6. **`simulation_requirement = project.simulation_requirement.unwrap_or_default()`**; if empty →
   **400** `{... error: t("api.projectMissingRequirement")}`. (L457-462.)
7. **`document_text = pm.get_extracted_text(project_id).ok().flatten().unwrap_or_default()`** (L465).
8. **Parse options:** `entity_types: Option<Vec<String>> = body["entity_types"]` (array of str, else
   None); `use_llm_for_profiles: bool = body["use_llm_for_profiles"]` default **true** (L468);
   `parallel_profile_count: usize = body["parallel_profile_count"]` default **5** (L469).
9. **SYNCHRONOUS entity-count preview (BEFORE spawn) — best-effort (Python L471-488):**
   - Resolve graph via the EXISTING `load_entity_reader_graph(&state, &state.graph_id)` (simulation.rs:201,
     which applies the ZEP guard `[≠] U026-ZEPKEY` and graph_id→task→result["graph"]→KnowledgeGraph).
   - `let reader = KnowledgeGraphEntityReader::new(&graph); let preview =
     reader.filter_defined_entities(entity_types.as_deref(), /*enrich_with_edges=*/ false);`
     (enrich=false → Python L480, "不获取边信息" fast path).
   - `state.entities_count = preview.filtered_count; state.entity_types = preview.entity_types
     .iter().cloned().collect();`
   - **Wrap the whole block in best-effort:** on ANY `Err` (ZEP-guard 500, task-not-found, deserialize
     failure) → `tracing::warn!("同步获取实体数量失败...: {e}")` and CONTINUE (Python `except…warn,
     continue` L486-488). Do NOT propagate. The graph resolved here is then **reused** as the owned
     `graph` moved into the worker (resolve once). If resolution FAILS, the preview is skipped AND the
     worker still needs a graph — so re-resolve inside the worker is NOT how Python does it (Python's
     thread calls `manager.prepare_simulation` which itself reads Zep). **teri choice:** resolve the
     graph ONCE here; on success move it into the worker; on FAILURE we cannot run the pipeline (teri's
     `prepare_simulation` REQUIRES `&KnowledgeGraph` — it is a producer input, unlike Python where the
     manager reaches into Zep internally). See `[!] U026-d-GRAPHREQ` below.
10. **`task_id = TaskManager::global().create_task("simulation_prepare", Some(metadata))`** where
    `metadata = {"simulation_id": sim_id, "project_id": project_id}` (L491-498).
11. **`state.status = Preparing; sim_manager.save_simulation_state(&mut state)`** (L500-502) — saves the
    PREPARING status **and** the just-set `entities_count`/`entity_types`.
12. **Spawn:** `spawn_prepare_simulation(task_id.clone(), state.sim_manager.clone(), sim_id, requirement,
    document_text, entity_types, use_llm_for_profiles, parallel_profile_count, build_llm(&state.config),
    graph, SimulationConfigGenerator::new(build_llm(&state.config), &state.config.llm.model_name,
    &state.config.llm.base_url))`. (L504-612.) `PersonaGenerator::new()` is built inside the worker.
13. **Immediate 200 response (L614-625):**
    ```json
    {"success":true,"data":{
      "simulation_id": sim_id, "task_id": task_id, "status":"preparing",
      "message": t("api.prepareStarted"), "already_prepared": false,
      "expected_entities_count": state.entities_count,   // 0 if preview failed
      "entity_types": state.entity_types }}              // [] if preview failed
    ```
14. **Error envelopes:** ValueError-equivalent (Python L627-631) → 404 with `error:str(e)` — in teri,
    a `TeriError::Sim` / not-found from steps maps to its 404. Outer catch (L633-639) → 500
    `{success:false, error:str(e), traceback: ...}` → teri uses `ApiError::server`
    (`[≠] U025-TRACEBACK`: teri's 500 omits the live `traceback` string, same precedent as graph routes).

---

## 3. `POST /prepare/status` route — full branch tree (Python L672-752)

```rust
async fn prepare_status_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError>
```
`body or {}`. `task_id = body["task_id"]` (Option<&str>), `simulation_id = body["simulation_id"]`
(Option<&str>). Branch tree (evaluate in THIS order — Python's control flow):

**B1. If `simulation_id` present** → `check_simulation_prepared(&state.config, sim_id)`. If
`is_prepared` → **200** (L681-692):
```json
{"success":true,"data":{
  "simulation_id": sim_id, "status":"ready", "progress":100,
  "message": t("api.alreadyPrepared"), "already_prepared": true,
  "prepare_info": prepare_info }}
```
(If not prepared, fall through — do NOT return yet.)

**B2. If `task_id` is absent** (L695-711):
  - **B2a.** `simulation_id` present (and not prepared, per B1) → **200** (L698-707):
    ```json
    {"success":true,"data":{
      "simulation_id": sim_id, "status":"not_started", "progress":0,
      "message": t("api.notStartedPrepare"), "already_prepared": false }}
    ```
  - **B2b.** neither present → **400** `{success:false, error: t("api.requireTaskOrSimId")}` (L708-711).

**B3. `task_id` present** → `task = TaskManager::global().get_task(task_id)`:
  - **B3a. `task == None`** (L716-737):
    - if `simulation_id` present AND `check_simulation_prepared` → `is_prepared` → **200** (L719-732):
      ```json
      {"success":true,"data":{
        "simulation_id": sim_id, "task_id": task_id, "status":"ready", "progress":100,
        "message": t("api.taskCompletedPrepared"), "already_prepared": true,
        "prepare_info": prepare_info }}
      ```
      (NOTE: this calls `check_simulation_prepared` a SECOND time — Python L719 does too; it is the
      not-prepared-at-B1 + task-gone case where the sim finished between the two checks. Faithful:
      call it again, don't cache the B1 result. The `task_id` field IS present in this response,
      unlike B1.)
    - else → **404** `{success:false, error: t_args("api.taskNotFound",[("id",task_id)])}` (L734-737).
  - **B3b. `task == Some(t)`** (L739-745): `let mut d = t.to_dict(); d["already_prepared"] = false;`
    → **200** `{"success":true,"data": d}`. (`Task::to_dict()` is the verified port at task.rs:164 —
    emits task_id/task_type/status/created_at/updated_at/progress/message/progress_detail/result/
    error/metadata. The route injects `already_prepared:false` into that object.)

**Outer catch** (L747-752) → 500 `{success:false, error:str(e)}` → `ApiError::server`.

> Ordering subtlety to preserve: B1 runs FIRST and unconditionally (if simulation_id present), so a
> finished sim returns `ready/already_prepared:true` even when a task_id is also supplied. Only when
> B1 says not-prepared do we look at task_id. The two-`check_simulation_prepared`-calls in the
> task-gone path (B1 then B3a) is intentional and Python-faithful.

---

## 4. Reuse map (no reimplementation)

| Need | Reuse (verified dest symbol) | Source |
|------|------------------------------|--------|
| already-prepared check + auto-upgrade side effect | `check_simulation_prepared` (pub(crate)) | simulation.rs:1537 |
| graph_id → KnowledgeGraph (+ ZEP guard `[≠] U026-ZEPKEY`) | `load_entity_reader_graph` | simulation.rs:201 |
| entity preview filter | `KnowledgeGraphEntityReader::new` + `filter_defined_entities` | entity_reader.rs |
| the whole 3-stage pipeline + PrepareProgress emission | `SimulationManager::prepare_simulation` (U-023, UNCHANGED) | simulation_manager.rs:1180 |
| task lifecycle | `TaskManager::global()` create/update/complete/fail + `Task::to_dict` | task.rs |
| result payload | `SimulationState::to_simple_dict` (9 keys, verified order) | simulation_manager.rs:346 |
| spawn template | `build_graph_async_with_completion` (mirror, don't import) | graph_builder.rs:156 |
| LLM client | `build_llm(&config)` → `OpenAiAdapter` (Clone+Send+Sync+'static, verified) | mod.rs:247 |
| project access | `ProjectManager::from_config` + get_project/get_extracted_text | project.rs:373/493/628 |
| locale capture across spawn | `i18n::get_locale()` + `i18n::with_locale` | i18n/mod.rs:174/143 |

---

## 5. Risk flags for the no-downgrade gate

- **`[!] U026-d-GRAPHREQ` (PRODUCER GAP — surface to gate, decide before porting):** teri's
  `prepare_simulation` takes `graph: &KnowledgeGraph` as a REQUIRED input (it cannot reach into Zep
  the way Python's manager does — teri has no live Zep client; `[≠] U026-ZEPKEY` keeps the guard but
  the graph comes from a prior `build_graph` task's stored result via `load_entity_reader_graph`).
  Consequence: **the route can only run prepare end-to-end if `state.graph_id` resolves to a completed
  graph_build task in `TaskManager`** (graph stored in `result["graph"]`). In Python the entity read
  is best-effort + the thread retries; in teri the graph is a hard input to the worker. **Decision:**
  if graph resolution at step 9 FAILS, the route still creates the task + spawns, but the worker's
  `prepare_simulation` will produce zero entities → FAILED terminal (matches Python's
  zero-entities→FAILED path L1261-1266). i.e. the failure is observable via `/prepare/status` as a
  failed task, NOT a route 500. This is faithful (Python also degrades to a failed prepare when Zep is
  empty). **This is the SAME producer dependency the (b)/(f) entity routes already accept** — not new
  risk, but flag it: **a green end-to-end (d) test REQUIRES a seeded graph_build task** (the parity
  fixture must first run build_graph or inject a task with `result["graph"]`). Without that, (d)'s
  happy path is producer-gated on U-015/graph_build, exactly as (b)/(f) are.

- **Does prepare run end-to-end TODAY?** YES for the machinery — `prepare_simulation` is U-023
  tested-green, `OpenAiAdapter` Clone is resolved, the spawn template is live. The route + worker are
  fully implementable now with NO new producer. The ONLY gate is the **test fixture** must seed a
  graph (per `[!] U026-d-GRAPHREQ`) for a happy-path differential; the not-found/400/already-prepared/
  status branches need no graph and are runnable immediately.

- **`[~] U026-d-STAGE4`:** the `copying_scripts` (90-100) band is dead in teri (no emitter in
  `prepare_simulation`), kept in the mapping table for `total_stages=4`/`stage_index` fidelity. Not a
  downgrade — teri's emitted overall %s for stages 1-3 are byte-identical to Python.

- **`[≠]` inherited, NOT new:** `U026-ZEPKEY` (ZEP guard kept in graph resolution), `U025-TRACEBACK`
  (500 omits live traceback string). Both are pre-approved precedents from (b)/U-025; reused, not
  introduced.

- **No `prepare_simulation` signature change** → blast radius on the U-023 surface and its 7 in-crate
  test callers = **0**. The raw-pointer trick at simulation_manager.rs:1321 is UNTOUCHED (we reject
  option (a) precisely to keep it untouched).

---

## 6. Sub-cycle (d) decomposition is itself a single loop cycle

(d) is already one sub-cycle of U-026. Implement in this order within the cycle, each independently
compile-checkable:
1. `prepare_worker` + `spawn_prepare_simulation` + the stage→overall closure (in simulation_manager.rs,
   unit-testable via TaskManager without HTTP).
2. `prepare_simulation_route` (the 14-step flow + entity preview reuse).
3. `prepare_status_route` (the B1→B3 branch tree).
4. Register both on the router (mirror existing `.route("/prepare", post(...))` /
   `.route("/prepare/status", post(...))` next to the other simulation routes).
5. Differential parity: branches needing no graph first (400/404/already-prepared/status tree), then
   the happy path WITH a seeded graph_build task (`[!] U026-d-GRAPHREQ`).
