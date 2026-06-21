# U-026 sub-cycle (g) — `/start` + `/stop` lifecycle — Target Architecture

**Sub-cycle:** U-026 (g) · source `MiroFish/backend/app/api/simulation.py` `start_simulation` (`:1451-1641`), `stop_simulation` (`:1644-1700`), `_check_simulation_prepared` (`:240-356`).
**Destination:** extend `src/api/simulation.rs` — two new handlers + one private helper. In-state primitives `state.sim_runner: Arc<SimulationRunner<OpenAiAdapter>>` + `state.sim_manager: Arc<SimulationManager>` (DECISION-U026-1, sub-cycle (a) — LANDED).
**Class:** `port-fresh`. Deps: (a) ApiState ✓, (c) create/get/list ✓ (`SimulationManager` in-state confirmed).
**Inherited seam:** `ApiError::{client,client_with,server}`; `Result<Json<Value>,ApiError>`; `State(state): State<Arc<ApiState>>`; `load_entity_reader_graph(state, graph_id) -> Result<KnowledgeGraph,ApiError>` (`simulation.rs:162`, the graph-load convention reused for `graph_for_updater`); `python_isoformat_local()` (`models/project.rs:50`, `pub(crate)`).

---

## 0. THE CENTRAL VERDICT — `RunInputs<OpenAiAdapter>` builder: **CANNOT be built now → BLOCKED → honest-degradation landing**

### Investigation (decisive, file:line both sides)
A production `/start` needs `state.sim_runner.start_simulation(.., inputs: RunInputs<OpenAiAdapter>, ..)` (`simulation_runner.rs:1054-1063`). `RunInputs<L> { engine: SimEngine, pool: AgentPool, graph: KnowledgeGraph, llm: Arc<L> }` (`:943-953`). I traced every constructor for each field:

| `RunInputs` field | Only constructor that exists | Production builder from a prepared sim's artifacts? |
|---|---|---|
| `engine: SimEngine` | `SimEngine::new(SimConfig)` (`sim/mod.rs:438`); `SimConfig::new(max_ticks, parallelism)` (`sim/mod.rs:312`) — fields `{max_ticks:u32, parallelism:usize, inject_fn}` only | **NO.** `SimConfig` is a thin tick-engine config; there is **no** `SimConfig::from_simulation_config(json)` / `SimEngine::from_config(sim_id)`. The `simulation_config.json` `time_config`→`total_rounds` mapping the runner computes (`:1071-1092`) feeds `run_state.total_rounds` (a status field) — it does **not** parametrize the `SimEngine` that actually runs. Nothing maps the prepared config (topic/scenario/round cadence) into `max_ticks`/`inject_fn`. |
| `pool: AgentPool` | `AgentPool::new()` (`agent/*.rs:697`) + manual `add_agent(Agent::new(Persona{..}))` | **NO.** Exhaustive search: every `reddit_profiles.json`/`twitter_profiles.csv` reference in `src/` (non-test) is the **manager writing** them (prepare-side, U-023, `simulation_manager.rs:1304-1382`) or the **manager reading** them as raw JSON for the `/profiles` API (`get_profiles`, `:1603-1628`). **Zero code reads a profile file and constructs `Agent`/`Persona` into an `AgentPool`.** The only pool-build path is the test helper `run_inputs(max_ticks)` (`simulation_runner.rs:2746` — 1 hard-coded agent, `MockLlm`). |
| `graph: KnowledgeGraph` | `load_entity_reader_graph` / `KnowledgeGraph::new()` | Partial — loadable via the graph-id convention, but moot while engine+pool are unbuildable. |
| `llm: Arc<OpenAiAdapter>` | `build_llm(&config)` (`api/mod.rs:246`) | YES. |

**Conclusion:** the two load-bearing fields — `engine` parametrized from the prepared config, and `pool` built from the prepared profiles — have **no production builder anywhere in teri**. They are produced by the **platform/social-sim engine** = U-028 (twitter) / U-029 (reddit) / U-030 (parallel), all `- [ ]` deferred (`loop_state.md:144`). This is the **same producer frontier** already flagged as `GAP-SOCIAL-WORLDSTATE` (`loop_state.md:369`) and `PRODUCER-PENDING` (groups i/j/k/l). U-019 (`SimulationConfigGenerator`, LANDED, `services/simulation_config.rs`) produces the config *artifact* on the prepare side — it does **not** build the runtime `SimConfig`/pool.

### Decision: `/start` ports the FULL boundary, then honest-errors at the unbuilt runtime — `[!] GAP-U026-RUNINPUTS-BUILDER`
Per standing law (no downgrade; honest `[!]` over silent stub), `/start` ports **everything portable** and emits a structured honest error exactly at the `RunInputs`-construction point. **No stub success, no fabricated `run_state`, no `todo!()`.**

**Ported in full (everything up to the runtime boundary):**
1. Body parse `or {}`; all validation: `simulation_id` (400 `requireSimulationId`), `max_rounds` (`≤0`→400 `maxRoundsPositive`, non-int→400 `maxRoundsInvalid`), `platform∉{twitter,reddit,parallel}`→400 `invalidPlatform`.
2. `state.sim_manager.get_simulation(id)` → `None`→404 `simulationNotFound{id}`.
3. The full `state.status != READY` state-machine branch: `_check_simulation_prepared` (§3, incl. its auto-upgrade write); the `RUNNING`+live-`running`-run sub-branch (force→stop, else 400 `simRunningForceHint`); the `force`→`cleanup_simulation_logs`+`force_restarted=true` branch (§4); else not-prepared→400 `simNotReady{status}`; on prepared: set status `READY` + save.
4. `enable_graph_memory_update` graph_id resolution: `state.graph_id` → else `ProjectManager::from_config(&config).get_project(state.project_id)?.graph_id` → none→400 `graphIdRequiredForMemory` (§2).

**Honest-error boundary (the gap):** at the point Python calls `SimulationRunner.start_simulation(...)`, teri must build `RunInputs<OpenAiAdapter>`. Since no production builder exists, the handler returns a **500** (`Exception`→500 in Python, the faithful catch-all class) with a precise message, e.g. `ApiError::server("simulation runtime not available: no RunInputs builder — blocked on U-028/U-029/U-030 platform producers (GAP-U026-RUNINPUTS-BUILDER)")`. This is reached **only after** the entire validation + state-machine ran and a valid, prepared request crossed every gate. **Do NOT** call `state.sim_runner.start_simulation` with a `MockLlm`/test-helper `RunInputs` — that is a fabricated success and a downgrade.

> Porter note: do **not** introduce a `build_run_inputs` stub that returns a hand-rolled 1-agent pool. The honest 500 IS the deliverable; the real builder lands with U-028/029/030 (which will provide `SimConfig::from_simulation_config` + a profile→`AgentPool` reader), at which point this one boundary call is replaced and the `[!]` clears. Wire the boundary so that swap is a single localized edit (build `RunInputs` in one helper that currently returns the gap-error).

### What the parity-verifier CAN prove now (and cannot)
**CAN (port + verify against the "valid request reaches the start boundary, then honest-errors on the unbuilt runtime" contract):**
- Every 400/404 validation + state-machine path, byte-for-byte against Python (same i18n strings, same status codes, same branch order). These are the bulk of `/start`'s observable contract.
- `_check_simulation_prepared`'s full info-dict + the `preparing→ready` auto-upgrade side-effect on `state.json` (observable file write — golden-diff the post-call file).
- `cleanup_simulation_logs`'s file/dir deletions (observable filesystem effect) when `force=true`.
- The graph_id-resolution 400 (`graphIdRequiredForMemory`).
- The terminal: a fully-valid prepared request yields the structured 500 with the gap message (asserted shape, NOT a 200 success).

**CANNOT (deferred to U-028/029/030):** the 200-success path — `run_state.to_dict()` + `max_rounds_applied`/`graph_memory_update_enabled`/`force_restarted`/`graph_id`, the RUNNING-state persist, the actual spawn. Differential parity against Python's 200 is **impossible** until the producer lands the runtime. Flagged, not hand-waved.

### `/start` is portable TODAY (the boundary is honest) → port it now, gap-flagged. Do NOT defer the whole route.

---

## 1. `/stop` — **fully portable now, NO gap.** Port complete.
`stop_simulation` (Python `:1644-1700`) maps entirely onto landed primitives — confirmed:
- `state.sim_runner.stop_simulation(id)` (`simulation_runner.rs:1234`, async) — full stop semantics LANDED (Stopping→grace-timeout→abort→Stopped, monitor abort, updater stop).
- `state.sim_manager.get_simulation(id)` (`:1490`) + status→`Paused` + save.
No `RunInputs`, no producer dep. `/stop` ports + parity-verifies completely. **This is the clean half of (g).**

```rust
// POST /stop  (Python :1644-1700)
async fn stop_simulation(State(state): State<Arc<ApiState>>, body: Option<Json<Value>>)
    -> Result<Json<Value>, ApiError>
{
    let data = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));      // "or {}"
    let id = data.get("simulation_id").and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::client(StatusCode::BAD_REQUEST, t("api.requireSimulationId")))?;
    // run_state = runner.stop_simulation(id)  — ValueError→400, Exception→500 (map via TeriError class)
    let run_state = state.sim_runner.stop_simulation(id).await
        .map_err(map_runner_err_400_else_500)?;                        // see §5 error-class mapping
    // state = manager.get_simulation(id); if state: status=PAUSED + save
    if let Some(mut s) = state.sim_manager.get_simulation(id)? {
        s.status = SimulationStatus::Paused;
        state.sim_manager.save_simulation_state(&mut s)?;              // ← needs pub(crate), §6
    }
    Ok(Json(json!({ "success": true, "data": run_state.to_dict() })))
}
```

---

## 2. `graph_for_updater` — load + wrap as `Arc<Mutex<KnowledgeGraph>>` (only when memory enabled)
The runner's `graph_for_updater: Option<Arc<tokio::sync::Mutex<KnowledgeGraph>>>` (`simulation_runner.rs:1062`) is consumed only inside the runtime spawn — i.e. **past the gap boundary**. Therefore for the present landing it is **not exercised** (we honest-error before `start_simulation`). **But the graph_id RESOLUTION (the 400 gate) IS ported now** (it runs before the boundary). Specify the future wiring so the boundary-swap is mechanical:
- When `enable_graph_memory_update` and `graph_id` resolved: `let g = load_entity_reader_graph(&state, graph_id).await?;` (reuses `simulation.rs:162` — same ZEP-guard + task-resolution convention as sub-cycle (b)/(f)) then `Some(Arc::new(tokio::sync::Mutex::new(g)))`; else `None`.
- Record this as a **commented seam** in the gap-boundary helper (so U-028/029/030 wires it without re-deriving). It is NOT live-tested in (g) because the boundary errors first — note `- [!] GRAPH-UPDATER-WIRING-PENDING` (sub-family of RUNINPUTS-BUILDER).

---

## 3. `_check_simulation_prepared` — port as a private fn in `simulation.rs` (full closure incl. auto-upgrade write)
**Lands:** `fn check_simulation_prepared(config: &Config, simulation_id: &str) -> (bool, Value)` — private in `src/api/simulation.rs` (shared with sub-cycle (d) `/prepare`, which is why the full CJK info-dict is ported faithfully even though `/start` ignores it). Signature takes `&Config` (for `oasis_simulation_data_dir`); returns the bool + the info `serde_json::Value` (the Python info-dict).

Port the full `:240-356` closure exactly:
1. `sim_dir = config.oasis_simulation_data_dir().join(simulation_id)`; missing → `(false, {"reason":"模拟目录不存在"})`.
2. Required files `["state.json","simulation_config.json","reddit_profiles.json","twitter_profiles.csv"]`; partition into `existing_files`/`missing_files`; any missing → `(false, {"reason":"缺少必要文件","missing_files":[..],"existing_files":[..]})`.
3. Read+parse `state.json`; on read/parse failure → `(false, {"reason": format!("读取状态文件失败: {e}")})` (the catch-all, Python `:355-356`).
4. `status = state_data["status"]` (default `""`), `config_generated = state_data["config_generated"]` (default false).
5. `prepared_statuses = ["ready","preparing","running","completed","stopped","failed"]`. If `status ∈ prepared_statuses && config_generated`:
   - `profiles_count` = len of `reddit_profiles.json` if it's a JSON array, else 0 (Python `:317-321`).
   - **AUTO-UPGRADE SIDE EFFECT (observable — MUST port, `:323-334`):** if `status == "preparing"`: set `state_data["status"]="ready"`, `state_data["updated_at"]=python_isoformat_local()` (reuse `models/project.rs:50` — matches Python `datetime.now().isoformat()`, local naive ISO), rewrite `state.json` with `serde_json::to_string_pretty` (Python `json.dump(ensure_ascii=False, indent=2)` → 2-space, non-ASCII preserved; teri's `to_string_pretty` is 2-space + serde_json keeps non-ASCII by default). On write error: log warn, continue (Python `:333-334` catches). Set local `status="ready"`.
   - Return `(true, { "status":status, "entities_count":state_data["entities_count"]||0, "profiles_count":profiles_count, "entity_types":state_data["entity_types"]||[], "config_generated":config_generated, "created_at":state_data["created_at"], "updated_at":state_data["updated_at"], "existing_files":existing_files })`.
6. Else → `(false, {"reason": format!("状态不在已准备列表中或config_generated为false: status={status}, config_generated={config_generated}"), "status":status, "config_generated":config_generated})`.

> **Parity note:** the auto-upgrade write is the one observable side effect in (g) that is NOT a producer-blocked path — golden-test it: seed `state.json` with `status="preparing", config_generated=true` + all 4 required files, call `/start`, assert the on-disk `state.json` now reads `status:"ready"` with a fresh `updated_at`. The info-dict's CJK `reason` strings are internal to `/start` (not surfaced in its response) but ARE the `/prepare` (d) contract — port verbatim.

---

## 4. `cleanup_simulation_logs` — **port now** (scoped, deterministic file deletion; reachable only via `force=true`)
Read MiroFish `simulation_runner.py:1103-1181`. It is a pure filesystem op — **port it now** as a method on the teri runner (it belongs to the runner, mirroring source): `pub async fn cleanup_simulation_logs(&self, simulation_id: &str) -> CleanupResult` (or return a `serde_json::Value`/struct `{success:bool, cleaned_files:Vec<String>, errors:Option<Vec<String>>}` to match the Python dict the handler inspects via `.get("success")`).

**Deletes (exactly, `:1136-1147`):**
- Files in `{sim_dir}`: `run_state.json`, `simulation.log`, `stdout.log`, `stderr.log`, `twitter_simulation.db`, `reddit_simulation.db`, `env_status.json`.
- In dirs `["twitter","reddit"]`: `{dir}/actions.jsonl`.
- Per-file: skip if absent; on remove error push to `errors`, else push name to `cleaned_files`.
- **In-memory cleanup (`:1171-1173`):** Python `del cls._run_states[id]`. teri analog: drop the run handle / cached state if registered — `self.runs.lock().await.remove(simulation_id)` (drops the `RunHandle`, the teri fold of `_run_states`). Do this so a subsequent `get_run_state` re-reads fresh from disk (matching Python). **Caveat:** if a live run exists and `force` reached here, the RUNNING sub-branch already called `stop_simulation` (which itself `runs.remove`s) — so this remove is typically a no-op; still port it (idempotent).
- `sim_dir` missing → `{success:true, message:"模拟目录不存在，无需清理"}` (`:1129-1130`).
- Return `success = errors.is_empty()`.

Handler use (Python `:1566-1571`): `if force { let r = runner.cleanup_simulation_logs(id).await; if !r.success { warn!(...) }; force_restarted = true; }` — cleanup failure is **non-fatal** (warn, continue), matching source. **Does NOT** delete config/profiles (verified — none in the list). New flag `- [!]` NOT needed; this is fully portable.

> The DB files it deletes (`*_simulation.db`) won't exist until U-028/029/030 — but `os.remove`-skip-if-absent is the faithful behavior (Python skips absent files too), so cleanup is parity-correct **today**.

---

## 5. `/start` handler shape (decision-dense skeleton — porter executes verbatim)
```rust
// POST /start  (Python :1451-1641)
async fn start_simulation(State(state): State<Arc<ApiState>>, body: Option<Json<Value>>)
    -> Result<Json<Value>, ApiError>
{
    let data = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    // simulation_id required → 400 requireSimulationId
    let id = data.get("simulation_id").and_then(Value::as_str).filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::client(StatusCode::BAD_REQUEST, t("api.requireSimulationId")))?;
    let platform = data.get("platform").and_then(Value::as_str).unwrap_or("parallel");
    // max_rounds: optional. Python int() coercion + ≤0 / non-int branches:
    //   present & numeric int & ≤0 → 400 maxRoundsPositive
    //   present & not int-coercible → 400 maxRoundsInvalid
    //   (Python int("5")==5 coerces numeric strings; mirror with as_i64 / parse, see note)
    let max_rounds: Option<i64> = match data.get("max_rounds") {
        None | Some(Value::Null) => None,
        Some(v) => Some(coerce_max_rounds(v)?),   // Err(maxRoundsInvalid) / Err(maxRoundsPositive)
    };
    let enable_graph_memory_update = data.get("enable_graph_memory_update").and_then(Value::as_bool).unwrap_or(false);
    let force = data.get("force").and_then(Value::as_bool).unwrap_or(false);
    if !matches!(platform, "twitter"|"reddit"|"parallel") {
        return Err(ApiError::client(StatusCode::BAD_REQUEST,
            t_args("api.invalidPlatform", &[("platform", platform)])));
    }
    // manager.get_simulation(id) → None → 404
    let mut sim = state.sim_manager.get_simulation(id)?
        .ok_or_else(|| ApiError::client(StatusCode::NOT_FOUND,
            t_args("api.simulationNotFound", &[("id", id)])))?;

    let mut force_restarted = false;
    if sim.status != SimulationStatus::Ready {
        let (is_prepared, _info) = check_simulation_prepared(&state.config, id);   // §3
        if is_prepared {
            if sim.status == SimulationStatus::Running {
                if let Some(rs) = state.sim_runner.get_run_state(id).await? {
                    if rs.runner_status == RunnerStatus::Running {          // .value=="running"
                        if force {
                            if let Err(e) = state.sim_runner.stop_simulation(id).await {
                                tracing::warn!("停止模拟时出现警告: {e}");    // warn, continue
                            }
                        } else {
                            return Err(ApiError::client(StatusCode::BAD_REQUEST,
                                t("api.simRunningForceHint")));
                        }
                    }
                }
            }
            if force {
                let r = state.sim_runner.cleanup_simulation_logs(id).await;  // §4
                if !r.success { tracing::warn!("清理日志时出现警告: {:?}", r.errors); }
                force_restarted = true;
            }
            sim.status = SimulationStatus::Ready;
            state.sim_manager.save_simulation_state(&mut sim)?;             // §6 pub(crate)
        } else {
            return Err(ApiError::client(StatusCode::BAD_REQUEST,
                t_args("api.simNotReady", &[("status", sim.status.as_str())])));
        }
    }

    // graph_id resolution (only when memory enabled) — 400 graphIdRequiredForMemory
    let graph_id: Option<String> = if enable_graph_memory_update {
        let gid = sim.graph_id.clone()
            .or_else(|| ProjectManager::from_config(&state.config)
                .get_project(&sim.project_id).ok().flatten().and_then(|p| p.graph_id));
        let gid = gid.filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::client(StatusCode::BAD_REQUEST, t("api.graphIdRequiredForMemory")))?;
        Some(gid)
    } else { None };

    // === GAP BOUNDARY (GAP-U026-RUNINPUTS-BUILDER) =========================
    // Python: run_state = SimulationRunner.start_simulation(id, platform, max_rounds,
    //         enable_graph_memory_update, graph_id) ; state.status=RUNNING+save ;
    //         response = run_state.to_dict() + max_rounds_applied? + graph_memory_update_enabled
    //                    + force_restarted + graph_id?
    // teri: no production RunInputs<OpenAiAdapter> builder exists (engine+pool from prepared
    //       config/profiles = U-028/029/030). Emit honest 500, do NOT fabricate a run_state.
    Err(ApiError::server(format!(
        "simulation runtime not available: no RunInputs builder for '{id}' \
         (blocked on U-028/U-029/U-030 platform producers — GAP-U026-RUNINPUTS-BUILDER)"
    )))
    // When the producer lands: build RunInputs (engine from simulation_config.json via
    //   SimConfig::from_simulation_config; pool from {platform}_profiles via a profile→AgentPool
    //   reader; graph via load_entity_reader_graph when memory enabled, wrapped Arc<Mutex<_>>;
    //   llm = build_llm(&state.config)), call state.sim_runner.start_simulation(..),
    //   set sim.status=RUNNING + save, assemble the 200 response. ONE localized swap.
}
```
**`coerce_max_rounds`:** Python `int(max_rounds)` accepts int and numeric string (`int("5")`), raises `ValueError`/`TypeError` on garbage. Port: if `Value::Number` integer → use it; if `Value::String` → `parse::<i64>()` (Err→`maxRoundsInvalid`); if number is float/other → Python `int(5.9)` truncates but `int()` of a bool/None differs — match the observed inputs (frontend sends int or numeric string); a non-coercible value → `maxRoundsInvalid`; a coerced `≤0` → `maxRoundsPositive`. Keep the two distinct error keys.

---

## 6. Small wiring items (`- [!]`, low blast radius — measure, don't treat as walls)
- **`save_simulation_state` is private** (`simulation_manager.rs:922`, no `pub`). Both `/start` and `/stop` call it (Python `manager._save_simulation_state`). **Make it `pub(crate)`** — blast radius is one keyword; it's already the crate-internal save path the manager uses. `- [!] SAVE-STATE-VISIBILITY` (trivial).
- **`SimulationStatus::as_str()`** — confirm a `&str` accessor returning `"ready"`/`"running"`/etc. exists for the `simNotReady{status}` interpolation (the enum has string forms at `:633`). If only `Display`/serde exists, use the existing `to_dict`/serde string form; do NOT invent a new mapping (reuse `:633`).
- **Error-class mapping (`map_runner_err_400_else_500`)**: Python `/start` + `/stop` map `ValueError→400, Exception→500`. teri's runner errors are `TeriError::Sim(String)` (the `ValueError` analog, e.g. "模拟不存在", "模拟未在运行", "模拟已在运行中"). Map `TeriError::Sim(_) → 400`, everything else → 500. This preserves the source's two-tier error contract. Apply in `/stop` (live now) and keep ready for the post-gap `/start` 200-path.
- **i18n: all 8 keys CONFIRMED present** in both `en.json` + `zh.json` (`requireSimulationId :343`, `simulationNotFound :344`, `maxRoundsPositive :357`, `maxRoundsInvalid :358`, `invalidPlatform :359`, `simRunningForceHint :360`, `simNotReady :361`, `graphIdRequiredForMemory :362`). **No new i18n work.**

---

## 7. Sub-cycle split recommendation — **SPLIT (g) into (g1)=/stop, (g2)=/start**
The two routes have asymmetric portability:
- **`/stop` is 100% portable + fully parity-verifiable now** (no gap). Clean PASS.
- **`/start` ports the full boundary + state-machine + helpers but terminates at an honest 500** on the producer gap; its 200-success path is producer-blocked.

Bundling them risks the parity-verifier treating the whole (g) as "partial" and stalling. **Recommend:**

| Cycle | Lands | Parity contract | Flags |
|---|---|---|---|
| **(g1)** | `/stop` handler + `save_simulation_state` `pub(crate)` + `map_runner_err_400_else_500` | Full differential parity vs Python `/stop` (200 success + 400 + missing-id) — **clean PASS** | — |
| **(g2)** | `/start` handler + `check_simulation_prepared` (full, incl auto-upgrade write) + `cleanup_simulation_logs` (runner method) + `coerce_max_rounds` + graph_id resolution | Parity vs Python for every 400/404/state-machine path + the auto-upgrade file write + cleanup file deletions + the terminal honest-500 on a valid prepared request | `- [!] GAP-U026-RUNINPUTS-BUILDER` (200-path → U-028/029/030); `- [!] GRAPH-UPDATER-WIRING-PENDING`; `- [!] SAVE-STATE-VISIBILITY` (cleared in g1) |

If the loop prefers one cycle, the porter MAY land both together — but the parity-verifier MUST be told (g2) is verified against the **"valid request → honest 500 at the runtime boundary"** contract, not the 200 success. **Recommended: do (g1) first** (clean, unblocks `save_simulation_state`), then (g2) the same or next cycle.

---

## 8. Consolidated flags (no silent drop)
- `- [!] GAP-U026-RUNINPUTS-BUILDER` — `/start`'s 200-success path needs a production `RunInputs<OpenAiAdapter>` (engine from `simulation_config.json`, pool from `{platform}_profiles.*`). **No builder exists** (`SimConfig` is a thin tick-config; no profile→`AgentPool` reader). Blocked on **U-028/U-029/U-030** (platform producers, `- [ ]`). **Ported now:** full validation + state-machine + helpers + honest 500 at the boundary. **Deferred:** the spawn + 200 response. Same producer frontier as `GAP-SOCIAL-WORLDSTATE` (`loop_state.md:369`) / `PRODUCER-PENDING` (groups i/j/k/l). One localized swap clears it.
- `- [!] GRAPH-UPDATER-WIRING-PENDING` — `graph_for_updater` load+wrap (`Arc<Mutex<KnowledgeGraph>>` via `load_entity_reader_graph`) is specified + commented at the boundary but not live until the producer lands (sub-family of the above). The graph_id 400-gate IS ported now.
- `- [!] SAVE-STATE-VISIBILITY` — `save_simulation_state` → `pub(crate)` (1-keyword; cleared in g1).
- **No `[≠]` in (g).** Nothing here is intentionally divergent: `/stop` is a faithful full port; `/start`'s gap is genuine inexpressibility-until-producer (a `[!]`, not a `[≠]`), and the honest 500 is the no-downgrade-honest landing, not a capability cut.
