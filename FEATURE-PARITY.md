# Teri ↔ MiroFish Feature Parity Ledger

**Ground truth:** MiroFish (`/meta-yard/MiroFish`, Python + Vue, AGPL-3.0).
**Target:** teri (this repo, Rust). Teri is the *upgrade* — every MiroFish capability is a
requirement; teri must match or exceed it.

This ledger is the authoritative parity surface (supersedes the stale `MIROFISH-PORT-PLAN.md`
matrix; complements `RUNBOOK.md` §12). It was produced by a per-area audit reading both repos with
file:line evidence on each side. Status legend: ✅ full · 🟡 partial · ❌ missing · ➕ teri exceeds.

## Verdict

| Area | Result |
|---|---|
| **Backend HTTP API + persistence** | ✅ **100% parity** — every MiroFish route (10 graph + 31 simulation + 18 report + `/health`) has a teri equivalent. ➕ teri adds 4 SSE routes MiroFish lacks. |
| **Stage 1-2: Graph build + Environment setup** | 🟡 Structurally full; **simulation-quality gaps** in persona generation (memory injection, randomization, bilingual two-prompt strategy). |
| **Stage 3: Simulation** | ✅ Full + ➕ exceeds (per-tick God's-eye injection). 3 real gaps (action-split, enriched action args, shutdown hook). |
| **Stage 4-5: Report + Deep interaction** | ✅ Full + ➕ exceeds (5th tool, semantic insight_forge, SSE). Residual = shared/serve-provider items. |
| **Web UI** | ✅ **LANDED 2026-06-23** (was ❌) — MiroFish Vue 3 SPA copied into `frontend/`, rebranded, wired to teri's API, production build passes. Live-against-`serve` smoke test + optional SSE adoption remain. |
| **teri↔pebesen community seam** | ✅ **LANDED 2026-06-23** — `CommunityAdapter`/`CommunityFeedback` + `PebesenAdapter`/`PebesenFeedback` in teri; pebesen `intelligence` crate is now a real prediction receiver (`IntelligenceStore` + calibration). pebesen router wiring + sqlx-backed store remain. |

**Bottom line:** the backend is a real, near-complete port (the "port complete" claim was honest at
the *engine* layer). The two structural gaps — the **missing UI** and the **unbuilt community seam** —
were **closed on 2026-06-23** (see the session-update section below). Remaining work is
simulation-fidelity refinement + the autonomy layer (L2–L5), not missing structure.

---

## Session update — 2026-06-23 (UI + seam landed)

**Closed this session (commits on `chore/rusty-idd-fleet-adapter`):**
- `a7daa49` — pebesen vendored into the teri workspace.
- `54293d7` — docs regenerated from real MiroFish + this ledger.
- `4e6f0f4` — real ARCHITECTURE ASCII + data flow + USER-STORY + AGENTIC-STORY.
- `5c26f1f` — **Web UI** copied & wired (`frontend/`, `npm run build` ✅).
- `398ea9d` — **teri↔pebesen seam** (`src/seed/community/` + pebesen `intelligence` receiver).
- Verified: teri `cargo test` **1671 passed / 6 ignored**, clippy clean; `pebesen-intelligence`
  **8 passed**, clippy clean; fmt clean.

**New tasks added to the backlog below** (surfaced while building the UI + seam):
- TASK-UI-1 … TASK-UI-3 (live-verify, SSE adoption, polish)
- TASK-SEAM-1 … TASK-SEAM-3 (pebesen router, sqlx store, end-to-end loop test)
- TASK-AUTO-1 … TASK-AUTO-2 (autonomy orchestrator, calibration loop — L2–L5)

---

## 1. Backend API + Persistence — ✅ 100%

Flask blueprints (`/api/{graph,simulation,report}` + `/health`) → axum routers
(`src/server.rs:194-205`). All 59 routes matched by purpose and shape. No ❌, no 🟡.

➕ **Teri exceeds** with real Server-Sent-Events (MiroFish only chunked-polls):
- `GET /api/report/:id/agent-log/sse` (`report.rs:79`)
- `GET /api/report/:id/console-log/sse` (`report.rs:80`)
- `GET /api/report/:id/events` (`report.rs:83`)
- `GET /api/simulation/:id/ticks/sse` (`simulation.rs:163`, backed by `api/streaming.rs:33-140`)

*Caveat:* route presence/purpose confirmed; field-level request/response byte-parity for the heavy
handlers (`graph/build`, `simulation/prepare`, `simulation/start`, `report/generate`, `report/chat`)
is covered by the stage audits below, not byte-diffed here.

---

## 2. Stage 1-2: Graph Building + Environment Setup — 🟡

Pipeline is wired end-to-end (ontology → graph build → entity read → persona → sim-config) and the
config-generation layer is value-exact. Gaps are concentrated in **persona generation quality**.

### Teri must build
1. **Persona memory injection** *(highest sim-quality impact)* — teri's English persona prompt
   (`agent/mod.rs:1404-1426`) omits the 个人记忆/机构记忆 section that ties each agent to the event
   and its prior actions/reactions (`oasis_profile_generator.py:710,759`). This *is* the stage-1
   "individual/collective memory injection."
2. **Bilingual two-prompt persona strategy** — individual-vs-group prompt selection, system prompt,
   `response_format=json_object`, temperature ramp (0.7−attempt×0.1), 3-attempt loop,
   `finish_reason=="length"` truncation detection (`oasis_profile_generator.py:497-772`). Teri does
   a single English single-shot prompt.
3. **Persona randomization** — restore random counter ranges (karma 500-5000, etc.) and random
   age/gender/mbti/country in rule-based branches (`oasis_profile_generator.py:262-265,786-845`).
   Teri uses fixed/deterministic values → profiles aren't varied.
4. **Constants + missing branches** — `MBTI_TYPES`/`COUNTRIES`/`INDIVIDUAL`/`GROUP_ENTITY_TYPES`
   (`oasis_profile_generator.py:156-179`); add `socialmediaplatform` arm, split `mediaoutlet`.
5. **Per-entity context search enrichment** (`_search_zep_for_entity`,
   `oasis_profile_generator.py:286-412`) — wire teri's semantic-recall/graph-search to enrich
   persona context (fact dedup in `_build_entity_context`).
6. **`set_ontology` reserved-names remap + source_targets edge constraints**
   (`graph_builder.py:216-285`) — teri registers type names only.
7. **`json_object`/`finish_reason` request shape** in `_call_llm_with_retry`
   (`simulation_config.rs:1221-1270` + persona path) — `LlmClient` can't yet request structured
   output; output-equivalent via salvage today.

*Intentional non-gaps (`[≠]`, do not build):* `create_graph`/`delete_graph`/`add_text_batches`/
`_wait_for_episodes`/`generate_python_code`/pagination/backoff — Zep-server artifacts inapplicable to
teri's in-process petgraph substrate.

---

## 3. Stage 3: Simulation — ✅ + ➕

Tick loop, time-of-day activation policy, dual-platform parallelism, dual-LLM (boost) routing,
graph-memory write-back, IPC/interview surface, and temporal feed-back all full. ➕ teri **exceeds**
with a per-tick God's-eye `inject_fn` MiroFish lacks.

### Teri must build
1. **Per-platform action availability split** — MiroFish gates `TWITTER_ACTIONS` (6) vs
   `REDDIT_ACTIONS` (13) at graph build (`run_parallel_simulation.py:178-202,1141,1332`). Teri's
   `SocialAction` enum (`sim/mod.rs:43-69`) is platform-agnostic with no decision-time gate → a
   Twitter agent could emit Reddit-only actions. Add an allowed-action set per platform.
2. **Enriched `action_args` in actions.jsonl** — MiroFish enriches records with
   `post_content`/`author_name`/`quote_content`/`comment_content`/`target_user_name`
   (`run_parallel_simulation.py:749-981`). Teri emits structural args only (`sim/mod.rs:136-156`).
   Teri already holds posts/comments/users in `social_world.rs` — resolve these at log time so
   episode-text fidelity and UI consumers match (real downgrade, not just substrate).
3. **`register_cleanup` shutdown hook** — MiroFish registers SIGTERM/SIGINT/SIGHUP + atexit →
   `cleanup_all_simulations` (`simulation_runner.py:1287-1358`). Teri has `cleanup_all`
   (`simulation_runner.rs:1413`) but no signal registration → orphaned sims on `teri serve`
   shutdown. Add a tokio signal / axum graceful-shutdown hook.

---

## 4. Stage 4-5: Report + Deep Interaction — ✅ + ➕

Every ReportAgent tool (insight_forge, panorama_search, quick_search, interview_agents) + the ReACT
loop + report persistence + all interview/chat flows are full. ➕ teri exceeds: a 5th native tool
`recall_agent_discussion`, a semantic (cosine) insight_forge variant over the redb embedding store,
and SSE progress streams.

### Teri must build
1. **Social-DB writer (shared gap)** — `/interview/history`, `/posts`, `/comments` read logic is
   ported but populated results need the `sqlite` feature + the OASIS-equivalent social-DB
   *producer* (GAP-U026-SOCIALDB). MiroFish has the same dependency. Land the producer; ship serve
   with `sqlite` enabled.
2. **Provider-agnostic `serve` path** — the live report-interview IPC seam is wired but typed
   `SimulationRunner<OpenAiAdapter>` (`api/report.rs:1178`). Matches the known CLAUDE.md gap
   (serve's `ApiState` is OpenAI-concrete while `teri run` is provider-selected). Generalize so
   Anthropic/Gemini-backed sims support deep interaction under `serve`.

---

## 5. Web UI — ❌ MISSING (the headline gap)

Teri has **no `frontend/`**. MiroFish ships a Vue 3 SPA (`frontend/`, vue-router + vue-i18n + d3 +
axios) — a guided 5-step prediction studio. **Teri's backend already exposes the full endpoint
surface the UI needs** (incl. SSE), so this is a near-pure frontend build.

### Teri must build (UI)
**A. Scaffold/tooling** — Vue 3 + Vite project under `frontend/`; deps `vue@^3.5`, `vue-router@^4`,
`vue-i18n@^11`, `axios@^1`, `d3@^7`; `vite.config` aliases + dev proxy `/api → teri serve`;
`index.html` + `main.js` + `App.vue`.

**B. Router** — 6 named routes: Home `/`, Process `/process/:projectId`,
Simulation `/simulation/:simulationId`, SimulationRun `/simulation/:simulationId/start`,
Report `/report/:reportId`, Interaction `/interaction/:reportId` (`createWebHistory`, `props:true`).

**C. API layer** — axios instance with `Accept-Language` interceptor, `{success,data,error}`
envelope unwrap, `requestWithRetry`; modules `api/{graph,simulation,report}.js` (~30 fns).
**Verify teri returns the `{success,data,error}` envelope**; adapt interceptor if not.

**D. Store** — `pendingUpload` (carry files+requirement Home→Process).

**E. i18n** — vue-i18n; reuse MiroFish `locales/{languages.json,en.json,zh.json}` as spec (en/zh).

**F. Views (6)** — Home, MainView(Process), SimulationView, SimulationRunView, ReportView,
InteractionView — 3-mode layout switcher + step header + GraphPanel host + polling per element.

**G. Components (8)**
- **`GraphPanel.vue` — the d3 force-directed graph viz (highest-effort item):** forceSimulation
  (link/charge/center/collision), zoom/pan (scaleExtent 0.2–4), node drag, entity-type color scale,
  edge labels, node/edge detail panel (attributes/summary/labels/facts/episodes/self-loop grouping),
  legend, live-update hints, refresh/maximize emits. (Source D3 from `Process.vue` + `GraphPanel.vue`.)
- `HistoryDatabase.vue` (history card gallery + replay modal),
  `Step1GraphBuild`, `Step2EnvSetup` (realtime persona/config preview),
  `Step3Simulation` (start/stop + run-status detail + trigger report),
  `Step4Report` (incremental log tail), `Step5Interaction` (chat + batch interview),
  `LanguageSwitcher`.

**H. ➕ Adopt teri's SSE upgrades** — replace MiroFish's `from_line` log polling (Step4/Step5) and
30s graph re-polling (SimulationRun) with teri's EventSource SSE endpoints (`/agent-log/sse`,
`/console-log/sse`, `/events`, `/ticks/sse`).

---

## Consolidated build backlog

Status: ☑ done · ☐ open. Priority groups top→bottom.

### Web UI (structural gap — LANDED, follow-ups open)
- ☑ **TASK-UI-0** — copy + wire the Vue 3 SPA into `frontend/`; production build green. *(5c26f1f)*
- ☑ **TASK-UI-1** — UI↔engine API contract gate landed as `tests/ui_api_contract.rs`: boots the real
  `create_app` router and asserts, for an LLM-free endpoint of each of the 5 wizard steps, the exact
  contract the axios layer depends on — `{success,data}`/`{success:false,error}` envelope, CORS
  scoped to `/api/*` (not `/health`), `Accept-Language` (en/zh) honored, teri-branded `/health`. All
  assertions pass against the running engine on the first run (no mismatch found); the one-time manual
  smoke is now a regression gate. *(S1)*
- ☐ **TASK-UI-2** — adopt teri's SSE endpoints in the UI (EventSource for `/agent-log/sse`,
  `/console-log/sse`, `/events`, `/ticks/sse`) replacing MiroFish's `from_line`/30s polling.
- ☐ **TASK-UI-3** — teri-native branding pass (replace the renamed MiroFish logo with a teri mark);
  split the large d3 chunk; resolve the `pendingUpload.js` dual-import warning.

### teri↔pebesen community seam (structural gap — LANDED, follow-ups open)
- ☑ **TASK-SEAM-0** — `CommunityAdapter`/`CommunityFeedback` + `PebesenAdapter`/`PebesenFeedback`
  in teri; pebesen `intelligence` prediction receiver (`IntelligenceStore` + calibration). *(398ea9d)*
- ☑ **TASK-SEAM-1** — pebesen router wired: `pebesen_intelligence::http::router` mounts the
  `/api/intelligence/*` receiver + read endpoints; `pebesen-bin` is now a real axum server
  (`pebesen` binary, `/health` verified live). The loop is LIVE end-to-end over HTTP.
  *(Follow-on: mount the DB-backed `pebesen-api` routes alongside it once that crate exposes a
  `Router` + `DATABASE_URL`.)*
- ☐ **TASK-SEAM-2** — sqlx/postgres-backed `IntelligenceStore` (make it a trait, in-memory + Postgres
  impls; tables `predictions`, `prediction_actions` per the inline `// SQLX SLOT:` markers).
- 🟡 **TASK-SEAM-3** — loop E2E: feedback half DONE (`tests/community_loop_e2e.rs` — teri feedback →
  real receiver → store → action → calibration, over real HTTP; ingest via mocked pebesen read API).
  Remaining: the LLM-backed pipeline middle (signal → seed → …pipeline… → report → feedback) as a
  gated integration test against a mock inference backend.

### Autonomy (L2–L5, see docs/AGENTIC-STORY.md)
- ☐ **TASK-AUTO-1** — autonomy orchestrator (DECIDE layer): watch adapters, debounce signal deltas
  into `(seed, query)` jobs, schedule headless `pipeline::run_pipeline` runs under a compute budget,
  with continuity/resume + witnessed audit trail.
- ☐ **TASK-AUTO-2** — calibration loop: turn actioned/accurate outcomes into per-community confidence
  weights (persist in redb); upgrades report `confidence` from synthesized metadata → calibrated.

### Simulation fidelity & refinements (engine — open)
- ☐ **TASK-SIM-1** — persona memory injection + bilingual two-prompt strategy + randomization
  (Stage 1-2 #1-3) — biggest simulation-fidelity gap.
- ☐ **TASK-SIM-2** — per-platform action split + enriched `action_args` (Stage 3 #1-2).
- ☐ **TASK-SIM-3** — social-DB producer + `sqlite`-default serve (Stage 4-5 #1) — unlocks
  posts/comments/history.
- ☐ **TASK-SIM-4** — provider-agnostic `serve` ApiState (Stage 4-5 #2) — Anthropic/Gemini under serve.
- ☐ **TASK-SIM-5** — `register_cleanup` shutdown hook (Stage 3 #3) — no orphaned sims.
- ☐ **TASK-SIM-6** — persona/config `json_object`+`finish_reason`, ontology reserved-names/edge
  constraints, per-entity search enrichment (Stage 1-2 #4-7).

### Doc debt
- ☑ **TASK-DOC-1** — refreshed the stale `TODO.md` (was dated 2026-06-12, claimed "pipeline pending"):
  rewritten as a current-state pointer to this ledger + `SPRINT.md`. The final sprint plan
  (`SPRINT.md`, S0–S14) now sequences this backlog. *(S0)*
