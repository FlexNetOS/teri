# U-028 / U-029 / U-030 — Platform simulation producers — Target Architecture

**Units:** U-028 `TwitterPlatform` · U-029 `RedditPlatform` · U-030 `MultiPlatformRunner`
**Class:** `map-onto-substrate` — the OASIS subprocess platform simulation reimplemented natively
onto teri's `src/sim::SimEngine` + `AgentPool` + `KnowledgeGraph` + `ActionLogger`.
**Source X (contract):**
`backend/scripts/run_twitter_simulation.py` (780L, `TwitterSimulationRunner`),
`backend/scripts/run_reddit_simulation.py` (769L, `RedditSimulationRunner`),
`backend/scripts/run_parallel_simulation.py` (1699L, dual-platform).
**Destination Y:** worktree `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri`.
**Why it matters:** these units are the *producer frontier* `GAP-U026-RUNINPUTS-BUILDER`
(`findings/u026-g-architecture.md` §0). They land `SimConfig::from_simulation_config` (the `engine`
field builder) + a profile→`AgentPool` reader (the `pool` field builder) + the actions.jsonl
producer wiring — the two load-bearing `RunInputs<OpenAiAdapter>` fields with no production builder.

---

## 0. The architectural frame — what "map-onto-substrate" means here, precisely

MiroFish runs each platform as a **separate Python subprocess** that imports OASIS
(`oasis.make(...)`, `env.step(...)`, `generate_twitter_agent_graph(...)`, `LLMAction`,
camel-ai `ModelFactory`) and drives a round loop. teri already replaced the **subprocess
lifecycle** (`subprocess.Popen` → `tokio::spawn` of `SimEngine::run`) in U-022
(`simulation_runner.rs:1519 spawn_sim_task` → `:1553 engine.run(&mut pool, &graph, &*llm)`).

What U-022 left to **this** unit is the *content* of the run: nothing currently builds the
`engine`/`pool`, and `SimEngine::run` (`sim/mod.rs:529-644`) emits only **in-memory snapshots** —
it never writes `actions.jsonl`. But the landed monitor (`spawn_monitor_task`, `:1663`) tails
`{sim_dir}/{platform}/actions.jsonl` **by byte offset** (`:1609-1617`) as its sole data source, and
the U-026 h/i SSE tail (landed) consumes the same file. So the substrate mapping has **three**
producer obligations, each onto a *distinct already-landed substrate*:

| OASIS construct (X) | teri substrate (Y) | landed? | this unit's obligation |
|---|---|---|---|
| `oasis.make` + `env.step(LLMAction)` per round | `SimEngine::run` tick loop + `Agent::prepare_action`/`commit_action` (`agent/mod.rs:333,346`) | substrate ✓, run logic ✓ | **map round→tick**; the per-round activation gate (`_get_active_agents_for_round`) onto the engine |
| `generate_twitter_agent_graph(csv)` / `generate_reddit_agent_graph(json)` | `AgentPool` of `Agent::new(Persona)` (`agent/mod.rs:312,701`) | substrate ✓ | **profile→AgentPool reader** (§3) — *no such reader exists* (`u026-g` table row 2) |
| config `time_config`→`total_rounds` | `SimConfig{max_ticks}` (`sim/mod.rs:286,312`) | substrate ✓ | **`SimConfig::from_simulation_config`** (§2) — *no such fn exists* (`u026-g` table row 1) |
| OASIS DB `trace` table → action stream | `PlatformActionLogger.log_action` → `actions.jsonl` (`action_logger.rs:115-136`) | writer ✓, **not wired to engine** | **wire the logger into the run** (§5) so the monitor/SSE see real data |

The substrate is sufficient — **no new engine primitive is needed for the happy path** (the
`inject_fn` seam + per-tick hook already exist). The work is *adapters and mappings*, not new core.

### Decision: REIMPLEMENT-onto-substrate, NOT DELEGATE. (DECISION-U028-1)
OASIS itself (the social-graph recommender, the env.step engine, `generate_*_agent_graph`'s
follow-graph construction, the SQLite `trace` DB) is **not** ported — teri's `SimEngine` +
`Agent::prepare_action` + `KnowledgeGraph` are the native re-expression. This is the
already-ratified `map-onto-substrate` class. The OASIS-framework internals are the `[≠]` boundary
(§1). We map the **observable, deterministic** surface (round math, time mapping, activation
structure, action records) and `[≠]`-flag the framework internals with rationale.

---

## 1. The parity contract vs the `[≠]` OASIS-framework boundary

The Python activation (`_get_active_agents_for_round`, twitter `:462-529`, reddit `:469-...`) is
**UNSEEDED stochastic**: `random.uniform`/`random.random`/`random.sample` with no `random.seed(...)`
anywhere in either script (verified — no seed call). So **even Python is run-to-run
non-deterministic**. This bifurcates the contract cleanly:

### 1A. Deterministically / byte verifiable (the parity-verifier DOES differential-test these)
| Behavior | Python file:line | teri landing | Verify how |
|---|---|---|---|
| `total_rounds = (total_hours*60)//minutes_per_round` (integer floor) | twitter `:550`, reddit `:539` | `SimConfig::from_simulation_config` (§2) | table of `(hours,mpr)`→rounds; already partially in `simulation_runner.rs:1100-1107` |
| `max_rounds` truncation `min(total_rounds, max_rounds)` (only when `>0`) | twitter `:553-557`, reddit `:543-546` | §2 | `(rounds,max)`→`min`; matches `simulation_runner.rs:1109-1118` |
| Round→time mapping: `simulated_minutes = round*mpr`; `simulated_hour=(min//60)%24`; `simulated_day=min//(60*24)+1` | twitter `:635-637` | §4 activation gate + §5 `log_round_start(simulated_hour)` | table of `(round,mpr)`→`(hour,day)` |
| Activation **structure**: peak/off-peak/normal multiplier *selection* (`hour∈peak_hours`→peak mult, `∈off_peak`→off mult, else 1.0) | twitter `:490-495` | §4 | given a fixed `current_hour`, assert the *selected multiplier value* (deterministic given hour) |
| `target_count = int(uniform(min,max) * multiplier)` — the **formula shape** (truncation, multiplier applied) | twitter `:497` | §4 | with a **seeded** RNG fixture (§4), assert exact count |
| active_hours **gating**: `current_hour ∉ active_hours` → agent excluded (deterministic, pre-RNG) | twitter `:507-508` | §4 | per-agent: hour-in/out → included/excluded |
| activity_level **threshold semantics**: `random.random() < activity_level` (level 0.0→never, 1.0→always) | twitter `:511` | §4 | boundary: level=0.0 excludes all; level=1.0 (pre-sample) includes all candidates |
| `random.sample(candidates, min(target_count, len(candidates)))` — the **cap** `min(target,len)` | twitter `:515-518` | §4 | with seeded RNG, assert the selected set; cap asserted regardless of seed |
| CREATE_POST for each `initial_posts[i]` (poster_agent_id, content) | twitter `:607-627` | §5 initial-action injection | actions.jsonl first N records are CreatePost with the configured content |
| Empty-round skip (`if not active_agents: continue` → no env.step, no record) | twitter `:644-645` | §4/§5 | a fully-inactive hour produces NO action records for that round |
| The 4 AVAILABLE_ACTIONS sets differ twitter vs reddit | twitter `:389-396` (6 actions), reddit `:389-402` (15 actions) | §6 platform action-set | the agent's selectable action vocabulary differs by platform |

### 1B. Inherently distributional / `[≠]` (owner-decision, with rationale — NOT silently dropped)
- `- [≠] U028-RNG-SEQUENCE` — the **exact** sequence of agents activated per round. Python is
  unseeded `random.*`; the precise multiset is non-reproducible **in Python itself**. teri uses its
  own RNG (§4). The *distribution* (expected active-count given hour/multiplier/levels) and the
  *structure* (§1A) are preserved; the exact draw is not a contract. **Bar check:** this is a
  genuine non-contractual/unobservable-sequence divergence (the source has no stable sequence to
  match), the legal `[≠]` case — NOT a feature skip.
- `- [≠] U028-OASIS-INTERNALS` — `generate_twitter_agent_graph`/`generate_reddit_agent_graph`'s
  follow-graph + recommender, `env.step`'s post-ranking (`recency/popularity/relevance` weights),
  the SQLite `trace` schema, `LLMAction`'s internal prompt assembly. teri re-expresses the
  agent-decision loop via `Agent::prepare_action` + `KnowledgeGraph`; it does NOT reproduce OASIS's
  recommender ranking byte-for-byte. **Bar check:** genuinely inexpressible-without-OASIS internals;
  the `PlatformConfig` weights (`recency_weight` etc.) ARE carried in the config artifact (U-019,
  `simulation_config.rs:385-409`) and remain available to any future ranking — they are not dropped,
  just not consumed by teri's recommender-free decision path. This is the substrate-gap `[≠]`,
  rationale-recorded.
- `- [≠] U028-INTERVIEW-DB-READ` — `IPCHandler._get_interview_result` reads OASIS's
  `twitter_simulation.db` `trace` table (twitter `:300-341`). teri's interview path already maps onto
  the in-process IPC (U-026 e/f, `RunHandle.ipc_client`); the SQLite read is `[≠]` (no OASIS DB).
  Already covered by landed IPC wiring — noted here for completeness, not new work.

**What the parity-verifier asserts for U-028/029/030:** every row of §1A (with a **seeded** RNG
fixture so the stochastic-but-structured parts become reproducible for the test — see §4), the
actions.jsonl record shape (§5), and the config→engine + profile→pool builders (§2/§3) round-tripped
against real prepared-sim artifacts. It does NOT assert the §1B items; those are challenged at the
gate against the `[≠]` bar and pass as recorded divergences.

---

## 2. `SimConfig::from_simulation_config` — the deterministic config→runtime mapping (CYCLE 1)

**Lands:** `impl SimConfig { pub fn from_simulation_config(config: &serde_json::Value, max_rounds: Option<i64>, parallelism: usize) -> Self }` in `src/sim/mod.rs` (alongside `SimConfig::new` at `:312`).

This is the cleanest, purest, **fully-deterministic** first deliverable. It reads the U-019
`simulation_config.json` artifact (shape = `SimulationParameters::to_dict()`,
`simulation_config.rs:551-596` — 13 keys incl. `time_config`) and computes `max_ticks`.

### Exact field mapping (Python file:line on the LEFT, already-proven teri math on the RIGHT)
| `SimConfig` field | Source | Python file:line | Formula |
|---|---|---|---|
| `max_ticks` ← `total_rounds` | `time_config.total_simulation_hours` (default 72) × 60 ÷ `time_config.minutes_per_round` (default 30), floored | twitter `:546-550`, reddit `:538-539` | `((hours*60)/mpr) as i64` then `if let Some(mr)=max_rounds && mr>0 { min(total, mr) }` — **identical to `simulation_runner.rs:1095-1118`** (reuse that exact code) |
| `parallelism` | not from config — caller-supplied (the run's LLM concurrency; OASIS used `semaphore=30`, twitter `:596`) | — | param; default to existing `SimConfig::default().parallelism` (8) or 30 to mirror OASIS semaphore — **owner pick, default 8** (teri convention) |
| `inject_fn` | none at config level (activation is §4, applied per-tick) | — | `None` here; §4 may install it |

**Critical reuse, NOT re-derivation:** `simulation_runner.rs:1095-1118` ALREADY computes exactly
this `total_rounds` (with the same default-72/default-30 chain, the same `as i64` floor matching
Python `//`, the same `min(total, mr)` truncation, the same `mr > 0` guard, and the same zero-divisor
guard). `SimConfig::from_simulation_config` should **call into / mirror** that block so there is ONE
truncation implementation. After it lands, `start_simulation`'s inline `total_rounds` (a status field
at `:1123`) and the `SimEngine`'s `max_ticks` derive from the **same** function — closing the
`u026-g` row-1 gap ("the config→total_rounds mapping does not parametrize the engine").

> **Note on `minutes_per_round` default skew:** the runner uses default **30** (`simulation_runner.rs:1099`,
> matching the *scripts*' `time_config.get("minutes_per_round", 30)` twitter `:547`). But the U-019
> dataclass default is **60** (`simulation_config.rs:212 default_minutes_per_round`). This is NOT a
> conflict: the scripts' `.get(k, 30)` default only fires when the key is ABSENT, and U-019 always
> writes the key (default 60). So a real artifact carries `minutes_per_round` explicitly; the `30`
> fallback is dead unless the key is missing. `from_simulation_config` must use the **same fallback
> as the scripts (30)** for byte-parity on a key-absent artifact — flag `- [≠] U028-MPR-DEFAULT-30`
> ONLY if a divergence is observed (it should not be, since the key is always present). Match `:1099`.

### Parity contract (CYCLE 1): a pure table test. `(hours, mpr, max_rounds) → max_ticks`, differential
against the Python `(total_hours*60)//minutes_per_round` + `min(...,max_rounds)`. **No RNG, no I/O,
no LLM — clean deterministic PASS.** This is the lowest-risk, highest-certainty cycle.

---

## 3. Profile→`AgentPool` reader — `twitter_profiles.csv` / `reddit_profiles.json` → `AgentPool` (CYCLE 2)

**Lands:** a new reader, recommended location `src/services/oasis_profile_export.rs` (it already owns
the *writer* `save_twitter_csv`/`save_reddit_json` — colocating the inverse read keeps the
format-contract in one module) OR a sibling `src/services/profile_loader.rs`. Signature:
```rust
pub fn load_agent_pool(sim_dir: &Path, platform: &str) -> Result<AgentPool>
//   platform "twitter" → read {sim_dir}/twitter_profiles.csv  (Python twitter:419-421)
//   platform "reddit"  → read {sim_dir}/reddit_profiles.json   (Python reddit:428)
//   platform "parallel" → read BOTH and union (U-030; see §7)
```
This is the `u026-g` row-2 gap ("zero code reads a profile file and constructs `Agent`/`Persona`").

### The CSV contract (what the reader parses — INVERSE of the landed writer)
The writer `save_twitter_csv` (`oasis_profile_export.rs:227-284`) emits header
`["user_id","name","username","user_char","description"]` then per-row
`[idx, name, username, user_char, description]` (`:253,267-273`). The reader inverts:

| CSV column | → teri `Persona`/`SocialProfile` field | Notes |
|---|---|---|
| `user_id` (0-based row idx) | `SocialProfile.user_id: u64` | the OASIS numeric id; parse to u64 |
| `name` | `Persona.name` | the display name |
| `username` | `SocialProfile.user_name` | OASIS handle (writer's "username" no-underscore key) |
| `user_char` | `SocialProfile.persona` (the LLM-system-prompt personality) | writer composed it as `"{bio} {persona}"`; on read it is the personality blob |
| `description` | `SocialProfile.bio` | writer set `description = bio` (`:265`) |

### The JSON contract (reddit — INVERSE of `save_reddit_json` / `to_reddit_format`)
`reddit_profiles.json` is a JSON **array** of objects with keys (always-present)
`user_id, username, name, bio, persona, karma, created_at` and conditional
`age, gender, mbti, country, profession, interested_topics` (`agent/mod.rs:115-160`
`to_reddit_format`; `oasis_profile_export.rs:88+` `save_reddit_json` forces OASIS defaults). The
reader maps each object → `SocialProfile` (the inverse of `to_reddit_format`).

### Field-mapping completeness check (NO silent drop)
- `Persona{name, background, traits, role, social}` (`agent/mod.rs:94-103`). The profile carries
  `name`→`Persona.name`; `bio`/`persona`/`user_name`/`user_id`/demographics→`Persona.social` (a
  `SocialProfile`, `agent/mod.rs:33-76`). **`background`/`traits`/`role` have NO profile source** →
  fill with derived defaults: `role` = `source_entity_type` if present else `"agent"`; `background` =
  the bio; `traits` = `[]`. **`- [≠] U028-PERSONA-CORE-FROM-PROFILE`** — `background`/`traits`/`role`
  are teri-`Persona` core fields with no OASIS-profile counterpart (OASIS profiles are bio/persona
  only). **Bar check:** this is filling a teri-superset field from the available data, not dropping
  a source behavior (the OASIS profile has no `traits`/`role` to begin with) — the legal
  "dest-superset" `[≠]`. The *consumed* fields (the LLM-system-prompt `persona`, the `user_name`,
  `user_id`) ARE all read; nothing OASIS produces is dropped.
- The **CSV is lossy by design**: `save_twitter_csv` collapsed `bio+persona`→`user_char` and dropped
  karma/follower/demographics (twitter CSV has only 5 columns). A twitter round-trip therefore CANNOT
  recover those — but that matches OASIS, which only feeds the 5-column CSV to
  `generate_twitter_agent_graph`. So the reader recovers exactly what OASIS would. **Reddit JSON is
  lossless** for the always-present keys. Flag `- [≠] U028-CSV-LOSSY` (the writer's lossy collapse is
  the OASIS contract, not a teri downgrade — challenge-passes the bar).

### Parity contract (CYCLE 2): write a known profile set via the landed writer, read it back via
`load_agent_pool`, assert the `AgentPool` has N agents with the expected `name`/`user_name`/`persona`
per row (round-trip golden). For reddit, assert the conditional-key recovery. Deterministic, no LLM,
no RNG — clean PASS. Independent of CYCLE 1.

---

## 4. The time-based activation policy (`_get_active_agents_for_round`) → `SimEngine` (CYCLE 3a)

**Source:** twitter `:462-529` (reddit `:469-...` identical structure). **The decision: a seedable
activation policy applied as a per-tick gate, NOT a new engine core.**

### Mapping onto the substrate
`SimEngine::run` (`sim/mod.rs:555-630`) currently prepares an action for **every** agent each tick
(`:595-602` iterates `pool.agents`). The Python loop instead activates a **subset** per round
(`active_agents`) and only those `env.step` (twitter `:640-654`). Two viable substrate maps:

- **Option A (chosen): an `ActivationPolicy` consulted inside the run, gating which agents
  `prepare_action`.** Add an optional policy to `SimConfig` (additive, like `inject_fn`):
  `SimConfig.activation: Option<Arc<dyn ActivationPolicy>>` where
  `fn active_agents(&self, tick: u32, agents: &[Agent], rng: &mut dyn RngCore) -> Vec<usize>`.
  When `None`, every agent acts (preserves all existing callers exactly — same additive-seam
  discipline as `with_shutdown`/`inject_fn`, `sim/mod.rs:471,340`). When `Some`, only returned
  indices `prepare_action` that tick; the rest are skipped (mirroring `if not active_agents:
  continue`, twitter `:644`). This keeps the engine generic and the OASIS-specific time policy in a
  `TimeActivationPolicy` struct (in `sim/` or `services/`).
- Option B (rejected): bake the policy into `inject_fn`. Rejected — `inject_fn` mutates `WorldState`
  *after* actions commit (`:617-619`); activation must gate *before* `prepare_action`. Wrong phase.

### The policy's deterministic structure (ported exactly, §1A rows)
`TimeActivationPolicy { time_config: TimeSimulationConfig, agent_configs: Vec<AgentActivityConfig> }`
(both already landed types, `simulation_config.rs:261,130`). Per tick → derive
`simulated_hour=(tick*mpr//60)%24` (twitter `:636`), then:
1. multiplier select: `hour∈peak_hours`→`peak_activity_multiplier`; `∈off_peak_hours`→`off_peak`;
   else `1.0` (twitter `:490-495`).
2. `target_count = (uniform(agents_per_hour_min, agents_per_hour_max) * multiplier) as i64` (`:497`).
3. candidates: for each `agent_config`, skip if `hour ∉ active_hours` (`:507-508`); else include iff
   `rng.gen::<f64>() < activity_level` (`:511`).
4. select: `sample(candidates, min(target_count, candidates.len()))` (`:515-518`).
5. map selected agent_ids → pool indices.

### RNG decision — **introduce a seedable RNG for testability** (DECISION-U028-2)
Python is unseeded (`random.*`). teri introduces `rand` with a **`StdRng` seeded per-run**, stored on
the policy / threaded through `active_agents(rng)`. Two modes:
- **Production:** seed from entropy (`StdRng::from_entropy()`) — matches Python's unseeded
  non-determinism (`[≠] U028-RNG-SEQUENCE`, §1B).
- **Test:** seed from a fixed value (`StdRng::seed_from_u64(K)`) so §1A rows
  (`target_count` formula, `activity_level` threshold, `sample` cap) become **reproducible** and the
  parity-verifier can assert exact activation sets. This is the *only* way the stochastic-but-
  structured behavior is differential-testable — without it, only the gating (active_hours,
  multiplier-select) is verifiable. The seed is a teri addition (`[≠]`-neutral: Python had none; a
  seedable RNG is a strict testability superset, not an observable production divergence).

**Dependency:** add `rand` to `Cargo.toml` (the canonical Rust RNG crate — no equivalent already in
tree for this; `uuid` uses its own). Record in the dep table: `python random` → `rand` crate.

### Parity contract (CYCLE 3a): with `StdRng::seed_from_u64(K)`, assert (i) multiplier selection per
hour (deterministic), (ii) active_hours gating (deterministic), (iii) `target_count`/`sample` cap and
the selected set (reproducible under fixed seed), (iv) activity_level boundaries (0.0/1.0). The
unseeded production path is `[≠]`-flagged, not tested for sequence.

---

## 5. `RunInputs` builder + actions.jsonl producer wiring (CYCLE 3b — the gap closure)

This is where `/start`'s `GAP-U026-RUNINPUTS-BUILDER` (`u026-g` §0) clears via **one localized swap**.

### 5A. The `build_run_inputs` helper (the single localized edit in `/start`)
`u026-g` §5 specified the GAP BOUNDARY as a single helper currently returning the honest-500. Land:
```rust
// in src/api/simulation.rs (or a runner-side builder) — replaces the §0 honest-500
fn build_run_inputs(state: &ApiState, sim_id: &str, platform: &str, max_rounds: Option<i64>)
    -> Result<(RunInputs<OpenAiAdapter>, Option<Arc<Mutex<KnowledgeGraph>>>), ApiError>
```
Assembling each `RunInputs` field (`simulation_runner.rs:958-968`) — every field now has a builder:
- `engine: SimEngine::new(SimConfig::from_simulation_config(&cfg, max_rounds, parallelism))` (§2),
  then `engine.register_snapshot_hook(...)` is NOT the action sink (§5B) — the logger is.
- `pool: load_agent_pool(&sim_dir, platform)?` (§3).
- `graph: load_entity_reader_graph(&state, graph_id).await?` when memory enabled, else
  `KnowledgeGraph::new()` (the read-only graph the engine reads; `u026-g` §2 already specified the
  `Arc<Mutex<_>>` wrap for `graph_for_updater`).
- `llm: Arc::new(build_llm(&state.config)?)` (`api/mod.rs:246`, already YES in `u026-g` table).

Then `/start` calls `state.sim_runner.start_simulation(id, platform, max_rounds, enable_graph,
graph_id, inputs, graph_for_updater).await` (the real call, `simulation_runner.rs:1069`), sets
`sim.status=RUNNING` + save, assembles the 200 response (`run_state.to_dict()` +
`max_rounds_applied`/`graph_memory_update_enabled`/`force_restarted`/`graph_id`). **This is the exact
one-line swap `u026-g` §5 line 200-208 promised** ("ONE localized swap").

### 5B. actions.jsonl producer wiring — THE load-bearing connection (the real risk of this unit)
**Problem (verified):** `run_sim_body` → `engine.run(&mut pool, &graph, &*llm)`
(`simulation_runner.rs:1553`) emits only in-memory `WorldSnapshot`s (`sim/mod.rs:621-629`). It
**never** calls `PlatformActionLogger.log_action` (`action_logger.rs:115`). But the landed monitor
(`spawn_monitor_task:1663`) AND the U-026 h/i SSE tail read **only** `{sim_dir}/{platform}/actions.jsonl`
(`:1609-1617`). **Nothing writes that file** → the monitor tails an empty file → never detects
`simulation_end` → run never marked COMPLETED. The producer MUST emit records.

**Decision (DECISION-U028-3): wire `PlatformActionLogger` into the run via a snapshot hook OR a
direct per-commit emit.** Two options:
- **Option A (chosen): emit inside the run loop at the commit phase.** After
  `agent.commit_action(&action)` (`sim/mod.rs:607-613`), translate the committed `Action::Social(_)`
  into the 8-key `log_action` record (`round=tick`, `agent_id`, `agent_name=persona.name`,
  `action_type`, `action_args`, `result`, `success=true`) and append to the platform logger. Plus
  `log_round_start(round, simulated_hour)` / `log_round_end(round, actions_count)` /
  `log_simulation_start(config)` / `log_simulation_end(total_rounds, total_actions)` at the loop
  boundaries (`action_logger.rs:140,152,167,192`). This requires the run to **hold a
  `PlatformActionLogger`** — pass it into `RunInputs`/`spawn_sim_task`, or have `run_sim_body`
  construct it from `{sim_dir}/{platform}` (`PlatformActionLogger::new(platform, sim_dir)`,
  `action_logger.rs:94`). **Recommended:** construct it in `run_sim_body`/`build_run_inputs` from the
  sim_dir+platform (the logger already derives `{base}/{platform}/actions.jsonl`).
- **Option B: a snapshot hook (`register_snapshot_hook`, `sim/mod.rs:477`) that flushes the tick's
  `WorldSnapshot.events` to the logger.** Cleaner separation (engine stays logger-agnostic) but a
  `WorldSnapshot` event (`Event{agent_id:Uuid, action, timestamp}`, `sim/mod.rs:127-132`) lacks the
  OASIS `agent_id:i64`/`agent_name`/`round` the record needs — the hook would have to re-derive them.
  **Trade-off:** Option A has the agent name + integer id in scope at commit; Option B does not.
  **Choose A** but keep the engine change minimal: thread an `Option<&PlatformActionLogger>` (or an
  `Arc`) into `run`/`run_sim_body` — additive, `None`-default preserves all existing callers (same
  discipline as `with_shutdown`).

**The action_type/args translation** (the `Action::Social(SocialAction)` → OASIS string + args):
`SocialAction` (`sim/mod.rs:40-65`) already carries the full OASIS taxonomy. Map each variant to its
OASIS `action_type` string (`CreatePost`→`"create_post"`, `Like{Post}`→`"like_post"`,
`Like{Comment}`→`"like_comment"`, … — the inverse of the taxonomy comment `sim/mod.rs:20-35`) and its
`action_args` (e.g. `CreatePost{content}`→`{"content": content}`). This map is **deterministic and
golden-testable**.

### Parity contract (CYCLE 3b): run a tiny seeded sim (seeded activation §4, a `MockLlm` that returns
fixed actions) end-to-end; assert (i) `{sim_dir}/{platform}/actions.jsonl` exists and contains the
expected `simulation_start`, per-round `round_start`/actions/`round_end`, `simulation_end` records in
order (golden-diff the JSONL); (ii) the landed monitor, fed this file, transitions the run to
COMPLETED; (iii) `/start` returns the **200** path (no longer the honest-500) — closing
`GAP-U026-RUNINPUTS-BUILDER`. This is the integration cycle and the highest-risk one (it touches the
engine signature + the runner spawn + the API handler), hence last.

---

## 6. Platform action-sets (twitter vs reddit AVAILABLE_ACTIONS) — folded into §3/§4

twitter `AVAILABLE_ACTIONS` = 6: CREATE_POST, LIKE_POST, REPOST, FOLLOW, DO_NOTHING, QUOTE_POST
(twitter `:389-396`). reddit = 15: LIKE_POST, DISLIKE_POST, CREATE_POST, CREATE_COMMENT,
LIKE_COMMENT, DISLIKE_COMMENT, SEARCH_POSTS, SEARCH_USER, TREND, REFRESH, DO_NOTHING, FOLLOW, MUTE
(reddit `:389-402`). teri's `SocialAction` enum (`sim/mod.rs:40-65`) is the **union** and already
flags `REFRESH` as `[≠]` (filtered, never an agent activity) and includes `TREND`. The
platform-specific *vocabulary restriction* (which actions the agent may select for this platform) is
a `platform: &str` parameter on the activation/decision path — `U-028` (twitter) restricts to the 6,
`U-029` (reddit) to the 15-minus-REFRESH. This is a small per-platform constant slice, no new types.
`- [≠] U028-REFRESH` already recorded in the enum (`sim/mod.rs:34`). **No new flag.**

---

## 7. U-030 MultiPlatformRunner (parallel) — composition, not new logic

`run_parallel_simulation.py` (1699L) runs twitter + reddit **together**. In teri this is **U-030 =
the `platform="parallel"` branch already wired through every landed layer**: `start_simulation`
already sets BOTH `twitter_running` + `reddit_running` for the non-twitter/non-reddit case
(`simulation_runner.rs:1164-1172`); the monitor already dual-platform-gates `simulation_end` (S-615,
`:1199-1205`). So U-030 lands as: `load_agent_pool(.., "parallel")` reads BOTH profile files and
unions the pools (§3); `build_run_inputs` emits to BOTH `twitter/actions.jsonl` +
`reddit/actions.jsonl` (the per-agent platform determines the sink). **No new engine, no new runner
method** — it is the parallel *composition* of U-028 + U-029 over the already-dual-platform
lifecycle. Risk: the dual-sink action routing (which agent → which platform's actions.jsonl). Verify
against `run_parallel_simulation.py`'s dual-`env.step` loop.

---

## 8. `TeriError::Timeout` variant — resolves `[≠]U026-k-TIMEOUT504` + `[≠]U026-l-TIMEOUT` (CYCLE 1, trivial)

**Confirmed addition.** `error.rs:4-55` has NO `Timeout` variant; `api/simulation.rs:2675-2679`
records the `[≠]` that an IPC `TimeoutError` (Python→HTTP **504**) currently folds into
`TeriError::Sim`→400 via `map_runner_err`. The fix:
```rust
// src/error.rs — add to TeriError enum:
#[error("Timeout: {0}")]
Timeout(String),
```
**Call sites to update:**
1. `src/api/simulation.rs` interview path (`:2793-2804`, `interview_agent`) — on
   `tokio::time::timeout(...)` elapsed, return `TeriError::Timeout(...)`; the API error-mapper maps
   `TeriError::Timeout` → **504 GATEWAY_TIMEOUT** with i18n key `interviewTimeout`.
2. `src/api/simulation.rs` batch path (`:2867-2887`, `interview_agents_batch`) → `batchInterviewTimeout`,
   504. (The `globalInterviewTimeout` is the outer wrap.)
3. `src/api/simulation.rs` close-env path (`:3015-3023`, `[!] U026-l-TIMEOUT`) — `send_command`'s
   elapsed timeout currently folds into `TeriError::Sim`; surface `TeriError::Timeout` → 504.
4. The shared `map_runner_err` (the error-class mapper) gains a `TeriError::Timeout(_) → 504` arm
   **before** the `Sim→400, else→500` arms (`u026-g` §6 error-class mapping). Order matters: Timeout
   must be matched before the catch-all.

**Blast radius:** one enum variant + ~4 call-site arms + 1 mapper arm. `kb_callers` on `map_runner_err`
before editing (low risk — it is the localized API error mapper). The i18n keys
(`interviewTimeout`/`batchInterviewTimeout`/`globalInterviewTimeout`) are already referenced in the
`[≠]` comments — confirm present in `en.json`/`zh.json` (they were authored for the 504 path). This
clears two `[≠]`s into faithful 504s — a no-downgrade *upgrade*.

---

## 9. Proposed cycle decomposition (3-cycle budget, dependency- and risk-ordered)

Each cycle is independently portable and parity-verifiable in one loop cycle, deterministic-and-pure
first. Deps are explicit.

| Cycle | Lands | Deps | Parity contract (differential-testable) | Risk |
|---|---|---|---|---|
| **1** | (a) `SimConfig::from_simulation_config` (§2) — reuse the `simulation_runner.rs:1095-1118` truncation. (b) `TeriError::Timeout` variant + 4 call-site + mapper arms (§8) → 504 paths. | none (both pure; §2 reuses landed math, §8 is enum+mapping) | (a) `(hours,mpr,max_rounds)→max_ticks` table vs Python `//`+`min`; (b) interview/batch/close-env timeout → **504** (was 400/500), 3 `[≠]`→faithful. | **Low** — pure functions, no I/O/LLM/RNG. Clean PASS. Unblocks the `engine` field + clears 2 `[≠]`. |
| **2** | `load_agent_pool` profile→`AgentPool` reader (§3) — CSV (twitter) + JSON (reddit) inverse of the landed writer; the `Persona`/`SocialProfile` field map. | none (independent of C1; reads the landed writer's output) | write-via-landed-writer → read-back → assert N agents + `name`/`user_name`/`persona` per row; reddit conditional-key recovery. Golden round-trip, no LLM/RNG. | **Low-Med** — deterministic I/O; the CSV-lossy + persona-core `[≠]`s are recorded, challenge-pass the bar. Unblocks the `pool` field. |
| **3** | (a) `TimeActivationPolicy` + `SimConfig.activation` seam + seedable `rand` RNG (§4). (b) `build_run_inputs` (the §0 honest-500 → real builder swap) + actions.jsonl producer wiring into the run (§5) + U-030 parallel dual-sink (§7). | **C1 (engine config) + C2 (pool)** — both `RunInputs` fields must exist first. | (a) seeded: multiplier-select + active_hours gating + `target_count`/`sample` cap + activity_level boundaries vs Python `:490-518`; (b) end-to-end seeded+MockLlm sim → `actions.jsonl` golden (start/round/end records) → monitor→COMPLETED → `/start` returns **200** (clears `GAP-U026-RUNINPUTS-BUILDER`). | **High** — touches engine signature (additive `activation`+logger seams), runner spawn, API handler. The integration + gap-closure cycle, correctly last. |

**If the budget is tight:** Cycle 3 MAY split into 3a (activation policy, unit-testable in isolation
against seeded fixtures) and 3b (the RunInputs/logger/200 integration), since 3a is pure and 3b is
the wiring. The architect recommends 3a/3b as **one cycle if the porter is confident**, else split —
but 3b cannot land before 3a (it needs the activation to produce non-empty rounds).

---

## 10. Consolidated flags (no silent drop)
- `- [≠] U028-RNG-SEQUENCE` — exact per-round activation multiset. Python is unseeded; no stable
  sequence exists to match. Structure + distribution preserved (§1A); a seedable RNG makes tests
  reproducible. **Legal `[≠]`** (non-contractual sequence).
- `- [≠] U028-OASIS-INTERNALS` — OASIS recommender ranking / follow-graph / `env.step` post-ordering /
  SQLite `trace` schema. Re-expressed via `SimEngine`+`Agent::prepare_action`+`KnowledgeGraph`;
  `PlatformConfig` weights carried in the artifact but not consumed by teri's recommender-free path.
  **Legal `[≠]`** (substrate-gap, rationale recorded). NOT a feature skip — no observable export dropped.
- `- [≠] U028-PERSONA-CORE-FROM-PROFILE` — `Persona.background/traits/role` have no OASIS-profile
  source; filled from bio/entity_type/defaults. **Legal `[≠]`** (dest-superset field; OASIS has no
  such field to drop).
- `- [≠] U028-CSV-LOSSY` — twitter CSV's 5-column bio+persona collapse is the OASIS contract (what
  `generate_twitter_agent_graph` consumes), not a teri downgrade. Reddit JSON is lossless. **Legal `[≠]`.**
- `- [≠] U028-INTERVIEW-DB-READ` — OASIS `trace`-table interview read; already mapped onto in-process
  IPC (U-026 e/f). No new work.
- `- [≠] U028-REFRESH` — already recorded in `SocialAction` (`sim/mod.rs:34`). No new flag.
- **Resolved by this unit (no longer gaps):** `[!] GAP-U026-RUNINPUTS-BUILDER` (cleared in C3b),
  `[!] GRAPH-UPDATER-WIRING-PENDING` (the §5A `graph_for_updater` wrap goes live in C3b),
  `[≠] U026-k-TIMEOUT504` + `[≠] U026-l-TIMEOUT` (cleared in C1 → faithful 504s).
- **New dependency:** `python random` → `rand` crate (seedable `StdRng`). Record in the dep table.

**No disguised feature-skips.** Every `[≠]` above is either a non-contractual stochastic sequence, a
genuine OASIS-internal inexpressibility, or a dest-superset field — each challenge-passes the
`references/parity-ledger.md` `[≠]` bar. The actions.jsonl producer, the profile reader, the config
mapper, and the 504 paths are all PORTED, not skipped.
