# U-030 PARALLEL DUAL-SINK — Architecture / Design Doc

Status: DESIGN (no production code). Porter implements per the cycle split in §7.
Author: rust-port-architect. Class: `extend-Y` (extends the landed U-028 additive producer seam).

Source of truth (READ): `MiroFish/backend/scripts/run_parallel_simulation.py`
- `run_twitter_simulation` L1101-1290, `run_reddit_simulation` L1293-1490 (structurally identical
  — confirmed: round-0 `log_round_start(0,0)` → per `initial_posts` `CREATE_POST log_action(round_num=0,…)`
  → `log_round_end(0, initial_action_count)`; main loop `log_round_start(round_num+1, simulated_hour)`
  ALWAYS, `if not active_agents: log_round_end(+1,0); continue`; `log_simulation_end(total_rounds, total_actions)`).
- `main()` L1492+: one `SimulationLogManager` → `twitter_logger`/`reddit_logger`;
  `asyncio.gather(run_twitter_simulation(...twitter_logger...), run_reddit_simulation(...reddit_logger...))`.

teri sources cited:
- `src/sim/mod.rs`: `RunProducer` L545-551 (`logger: Arc<PlatformActionLogger>`, `config: Value`),
  `RunProducer::minutes_per_round` L553-566, `log_err` L568, `SimEngine` fields L572-593
  (`producer: Option<RunProducer>`), `with_producer` L659-661, `run()` producer wiring L760-933,
  `SocialAction::oasis_action_type/args` L107-154 (`CreatePost{content}` → `{"content":content}`).
- `src/sim/action_logger.rs`: `PlatformActionLogger` L82-202 (`new(platform,&Path)` writes
  `{base}/{platform}/actions.jsonl`, stamps `"platform"` in start/end; `log_*` all `&self`).
- `src/agent/mod.rs`: `Platform{Twitter,Reddit}` L16-19 (`#[serde lowercase]`, Copy),
  `SocialProfile{user_id:u64, platform:Platform, persona:String,…}` L34-55, `Persona.social:Option<SocialProfile>` L102.
- `src/api/simulation.rs`: `build_run_inputs` L2094-2160 (single-platform producer attach L2131-2137),
  `parallel` honest-500 L2301-2307.
- `src/services/simulation_runner.rs`: `monitor_simulation` L1689-1768 (tails BOTH twitter+reddit logs
  unconditionally), `apply_log_record` `simulation_end` → per-platform `*_completed` L1882-1912,
  `check_all_platforms_completed` (S-615) L2014-2042 (`os.path.exists`-gated).

---

## 0. Key landed facts that make this a clean extension (de-risking the blocker)

The perceived "two-loops-vs-one-loop" wall is **already substantially mitigated by what landed in
cycles 49-51**, and the monitor side needs **ZERO changes**:

- The monitor (`monitor_simulation`) **already tails both** `{id}/twitter/actions.jsonl` AND
  `{id}/reddit/actions.jsonl` every poll, unconditionally, guarded only by `*.exists()`
  (`simulation_runner.rs:1693-1717`). The dual-gate `check_all_platforms_completed` (S-615) is
  `os.path.exists`-driven and already correct: a platform is "enabled" iff its `actions.jsonl`
  exists, and the run completes only when every enabled platform's `*_completed` is set
  (`simulation_runner.rs:2025-2042`). The `simulation_end` handler already sets `twitter_completed`
  / `reddit_completed` per the `platform` arg derived from the file path (L1882-1912).
  **⇒ Once both streams are written and each hits `simulation_end`, the dual-gate fires with no
  monitor edit.** This is the central de-risk: U-030 is purely a *producer-write-side* change.
- The single shared `completion_rx` (one `SimEngine::run`'s `SimCompletion`) drives the monitor's
  loop-exit + final-pass. teri runs ONE unified loop, so there is exactly one completion signal —
  it fires after BOTH platforms' `simulation_end` records are written (they are written in the same
  `run()`), and the final tail pass reads both logs. No second engine, no join needed.
- `RunProducer` is additive/opt-in; `with_producer` is the only attach point; every existing
  single-platform caller and the 4 in-crate `run()` tests (L2147, 2212, 2253, 2318) construct
  `RunProducer { logger, config }` directly.

**Blast radius of the producer-shape change:** `RunProducer` literal construction sites =
`build_run_inputs` (1) + 4 unit tests in `sim/mod.rs`. The `with_producer` signature itself is
NOT changing (see §1). So the field-shape change touches ≤5 construction sites, all in-repo,
all mechanical. Low risk.

---

## 1. RunProducer generalization shape (work item §1)

**DECISION-U030-1 — keep the field a single `logger`, generalize its routing capability via a new
internal type; preserve `RunProducer`'s public field layout so the single-platform case is
byte-identical and no caller changes its construction call shape beyond the logger value.**

Two shapes were considered:

| Option | Verdict |
|---|---|
| **(A)** `RunProducer.logger: PlatformLoggerSet` (a small struct that is EITHER one logger or a platform→logger map) | **CHOSEN.** Single-platform path unchanged at the call SITE pattern; routing is internal. |
| (B) Add a second optional field `reddit_logger: Option<Arc<…>>` | Rejected — asymmetric, leaks "twitter is primary," and `run()` would branch on field presence everywhere. |
| (C) `Vec<(Platform, Arc<…>)>` directly on `RunProducer` | Rejected as the public field — but it IS the internal repr of the chosen struct. A `Vec` (not `HashMap`) because there are exactly 2 platforms; linear scan over ≤2 entries is trivial and keeps insertion order deterministic (matters for nothing observable, but is simplest). |

### New internal type (in `src/sim/mod.rs`, next to `RunProducer`)

```rust
/// The set of per-platform action loggers a RunProducer fans records out to.
///
/// Single-platform: exactly one entry (twitter-only OR reddit-only). The unified loop routes
/// every record to it — byte-identical to the pre-U030 single-`logger` field.
/// Parallel: two entries (twitter + reddit). Boundary records (simulation_start/round_start/
/// round_end/simulation_end) fan out to ALL; `log_action` routes to the action's platform.
pub struct PlatformLoggerSet {
    /// Invariant: 1 entry (single-platform) or 2 (parallel); never empty. Insertion order is
    /// twitter-before-reddit for the parallel constructor (observably irrelevant — each writes
    /// its own file — but deterministic).
    loggers: Vec<(Platform, Arc<action_logger::PlatformActionLogger>)>,
}

impl PlatformLoggerSet {
    /// Single-platform set (the U-028 case). `platform` is the producer's platform.
    pub fn single(platform: Platform, logger: Arc<action_logger::PlatformActionLogger>) -> Self {
        Self { loggers: vec![(platform, logger)] }
    }
    /// Parallel dual set.
    pub fn parallel(
        twitter: Arc<action_logger::PlatformActionLogger>,
        reddit: Arc<action_logger::PlatformActionLogger>,
    ) -> Self {
        Self { loggers: vec![(Platform::Twitter, twitter), (Platform::Reddit, reddit)] }
    }
    /// All loggers, for boundary-record fan-out.
    fn iter(&self) -> impl Iterator<Item = &(Platform, Arc<action_logger::PlatformActionLogger>)> {
        self.loggers.iter()
    }
    /// The logger for a given platform, if installed.
    fn get(&self, platform: Platform) -> Option<&Arc<action_logger::PlatformActionLogger>> {
        self.loggers.iter().find(|(p, _)| *p == platform).map(|(_, l)| l)
    }
}
```

### RunProducer field change

```rust
pub struct RunProducer {
    pub loggers: PlatformLoggerSet,   // was: pub logger: Arc<PlatformActionLogger>
    pub config: serde_json::Value,
}
```

`RunProducer::minutes_per_round` is unchanged (reads `config` only).

### `with_producer` signature — UNCHANGED

`pub fn with_producer(&mut self, producer: RunProducer)` stays. Callers still pass a `RunProducer`;
only the literal's `logger:` becomes `loggers:`. This is the additive-seam discipline: the engine's
public attach API is stable; only the value's internal shape grows.

### Caller migration (mechanical, ≤5 sites)

- `build_run_inputs` single-platform (L2131-2137): becomes
  `RunProducer { loggers: PlatformLoggerSet::single(platform_enum, logger), config: config.clone() }`
  where `platform_enum` is parsed from the `&str` platform ("twitter"→`Platform::Twitter`,
  "reddit"→`Platform::Reddit`; "parallel" never reaches the single builder — it takes the new
  parallel path in §5). **Output byte-identical** to today: one logger, every record routes to it.
- The 4 `sim/mod.rs` tests (L2147/2212/2253/2318): `RunProducer { logger, config }` →
  `RunProducer { loggers: PlatformLoggerSet::single(Platform::Twitter, logger), config }`.
  (They all test the twitter single-platform stream; pick the platform their fixture logger uses.)

**Byte-identity proof for single-platform:** with a one-entry set, every fan-out in §2 iterates
exactly one logger and every `log_action` route in §3 resolves to that same logger (or the
invariant in §3 holds). The record bytes, order, and counts are produced by the same
`PlatformActionLogger` methods in the same order as today. ∎

---

## 2. Boundary-record fan-out + per-platform accumulators (work item §2)

Boundary records (`simulation_start`, `round_start`, `round_end`, `simulation_end`) go to **ALL**
loggers — but `round_end` and `simulation_end` carry **per-platform** counts.

**DECISION-U030-2 — per-platform accumulators keyed by `Platform`, materialized as a tiny
fixed-size helper (not a `HashMap`), seeded from the producer's logger set so single-platform keeps
exactly one accumulator.**

In `run()`, replace the two scalar accumulators (`total_actions`, `round_action_count`) with
per-platform maps scoped to the producer's installed platforms:

```rust
// Built once, before the loop, from the producer's logger set. One entry per installed platform.
let mut total_actions: PerPlatform<i64> = PerPlatform::zeroed(&producer.loggers); // platforms present
// per tick:
let mut round_counts: PerPlatform<i64> = PerPlatform::zeroed(&producer.loggers);
```

`PerPlatform<T>` is a 2-slot helper (`Option<(Platform,T)>` ×2, or `Vec<(Platform,T)>` seeded from
the set's platforms) with `add(platform, delta)` and `get(platform)`. Keep it trivial.

### Fan-out at each boundary

- **`simulation_start`** (before loop, L765): `for (_, logger) in producer.loggers.iter() {
  logger.log_simulation_start(&producer.config)? }`. Each writes its own `"platform"`-stamped
  start record into its own file. **Identical to Python** — both coroutines call
  `log_simulation_start(config)` with the same `config` (`total_rounds = total_simulation_hours*2`,
  `agents_count = len(agent_configs)`). Both files get the same `total_rounds`/`agents_count` —
  matches Python (both coroutines share `config`).
- **`round_start`** (per tick, L811-819): compute `round` + `simulated_hour` ONCE (unchanged), then
  `for (_, logger) in producer.loggers.iter() { logger.log_round_start(round, simulated_hour)? }`.
  Logged for EVERY round to EVERY platform — matches Python (each coroutine logs round_start every
  round regardless of activity).
- **per-action `log_action`** — routed, NOT fanned. See §3.
- **`round_end`** (per tick, L900-907): `for (platform, logger) in producer.loggers.iter() {
  logger.log_round_end(round, round_counts.get(*platform))? }`. **Twitter's round_end gets the
  twitter action count this round; reddit's gets reddit's.** When a platform had 0 actions this
  round, it logs `round_end(round, 0)` — matches Python's `if not active_agents: log_round_end(+1,0)`
  AND its no-DB-actions branch.
- **`simulation_end`** (after loop, L920-927): `for (platform, logger) in producer.loggers.iter() {
  logger.log_simulation_end(max_ticks, total_actions.get(*platform))? }`. `total_rounds == max_ticks`
  for both (the config-derived count, unchanged from U-028's note). Each platform's `total_actions`
  is its own running sum.

Per-tick accumulation: after the commit loop, `total_actions.add(p, round_counts.get(p))` for each
installed platform (or accumulate inline in §3).

---

## 3. `log_action` routing (work item §3)

In the phase-2 commit loop (L880-900), each committed `Action::Social(sa)` currently logs to the
single `producer.logger`. Generalize:

```rust
if let (Some(producer), Action::Social(sa)) = (&self.producer, &action) {
    let social = pool.agents[idx].persona.social.as_ref();
    let agent_id = social.map(|s| s.user_id as i64).unwrap_or(0);
    // Route by the agent's platform.
    let route = social.and_then(|s| producer.loggers.get(s.platform));
    if let Some(logger) = route {
        let platform = social.unwrap().platform; // route present ⇒ social present
        let args = sa.oasis_action_args();
        logger.log_action(round, agent_id, &pool.agents[idx].persona.name,
                           sa.oasis_action_type(), Some(&args), None, true)
              .map_err(|e| log_err("log_action", e))?;
        round_counts.add(platform, 1);
    }
    // else: see invariants below — drop is correct ONLY under the stated invariant.
}
```

### Invariants (state explicitly — these are the no-silent-drop guard)

- **No social profile** (`social == None`): the agent emitted an `Action::Social` but carries no
  `SocialProfile`. This **cannot happen for a producer run**: `build_action`/`prepare_action`
  produces `Action::Social` only for agents with a social profile (the parallel/single pool is
  built by `load_agent_pool`, which sets `social` on every agent). Treat as a **structural bug if
  observed** — the existing U-028 code already `unwrap_or(0)`'d the id, silently. For U-030, keep
  the route-miss path a **no-op that does not increment the count** (an unroutable social action is
  not recorded by ANY platform — which is the honest behavior; it must not be misrouted into the
  wrong file). This preserves single-platform byte-identity because single-platform never hits it.
- **Platform with no logger installed** (single-platform producer + an agent whose
  `social.platform` ≠ the installed platform): currently impossible — `load_agent_pool(sim_dir,
  "twitter")` returns only twitter agents; "reddit" only reddit. The invariant:
  **`PlatformLoggerSet` MUST contain a logger for every platform present in the pool.** §5's
  parallel wiring guarantees this (both loggers installed; the unioned pool has only twitter+reddit
  agents). For single-platform, the pool is mono-platform and the one logger matches. The route-miss
  no-op above is the fail-safe (record dropped, never misrouted) — but it is unreachable under the
  guaranteed invariant.

**This is the single behavioral fix U-030 makes over U-028's blind `producer.logger.log_action`:**
a reddit agent's action in a parallel run now lands in `reddit/actions.jsonl`, not twitter's —
which is exactly why U-028 deferred parallel (the single-logger misroute).

---

## 4. The structural `[≠]U030-UNIFIED-LOOP` divergence (work item §4)

Python runs **two independent coroutines** under `asyncio.gather`: separate OASIS envs, separate
`get_active_agents_for_round` draws, separate `total_actions`, separate DB-fetch of actually-executed
actions. teri runs **one unified loop** over the unioned pool with **one** `ActivationPolicy` call
per tick, fanned out by platform.

### What IS byte-faithful (NOT a divergence — gate must confirm these)

1. **Per-platform record schema** — every `simulation_start`/`round_start`/`log_action`/
   `round_end`/`simulation_end` record in each file has the exact key set, types, and `"platform"`
   stamp the U-028 single-platform port already proved byte-faithful (same `PlatformActionLogger`
   methods). Verified golden-clean in cycles 49-51.
2. **Round numbering** — both files use 1-based `round = tick_idx+1`, identical across platforms
   (Python: each coroutine ranges `0..total_rounds`, logs `round_num+1`). teri's unified
   `tick_idx` drives both, so twitter and reddit share the same round sequence. (In Python the two
   coroutines have the SAME `total_rounds` formula, so they ALSO share the round sequence — faithful.)
3. **`round_start` logged every round** to every platform, even empty rounds.
4. **`round_end(round, 0)`** emitted for a platform with no actions that round (Python's
   `if not active_agents: log_round_end(+1,0); continue` AND its zero-DB-actions case).
5. **`simulation_end` `total_rounds == max_ticks`** for both platforms (config-derived count,
   matches Python even on early shutdown — inherited from U-028's note).
6. **`simulated_hour`** = `(tick*mpr/60)%24`, identical for both platforms (Python computes it the
   same way in each coroutine from the same `minutes_per_round`).

### What IS a recorded `[≠]` — TAG: `[≠]U030-UNIFIED-LOOP`

The legal `[≠]` bar: these are **inexpressible** without re-introducing two OASIS envs (which teri
deliberately does not have — there is no OASIS post-graph / DB in teri; that is the pre-existing
`[≠]U028-OASIS-INTERNALS` substrate gap). They are non-contractual *ordering/identity* differences
in WHICH agents act and HOW MANY actions occur, NOT schema/format differences:

- **(a) Activation draw is shared, not independent.** Python calls
  `get_active_agents_for_round(env, config, hour, round)` SEPARATELY per platform (each over its own
  env's agent graph). teri calls ONE `ActivationPolicy::active_agent_ids(tick)` over the unioned
  pool. Observable consequence: WHICH `user_id`s act each round is determined by one policy pass,
  not two. **This is already covered by the landed `[≠]U028-RNG-SEQUENCE`** (the activation policy +
  per-agent LLM draw ordering already diverges from Python's per-env draws). `[≠]U030-UNIFIED-LOOP`
  is the *parallel-specific* extension of that same root: the two platforms' active-sets are drawn
  together rather than independently.
- **(b) `total_actions`/per-round counts differ in VALUE** (not schema) because teri's actions come
  from the agent's own `prepare_action` result, not a re-fetch of an OASIS DB's actually-executed
  rows (`fetch_new_actions_from_db`). This is the **same** `[≠]U028-OASIS-INTERNALS` root (teri has
  no OASIS trace DB) — parallel just has two files exhibiting it. The COUNTS in `round_end`/
  `simulation_end` are therefore teri's native action counts, per platform.
- **(c) No world cross-pollination between platforms** — but there is none in Python either (the two
  envs are independent), so this is faithful, not a divergence. (Within teri's single `WorldState`,
  twitter and reddit agents share one world; Python keeps them in separate envs. Since teri's
  `WorldState` has no OASIS post-graph and actions don't cross-influence via posts — `[≠]U028-OASIS-
  INTERNALS` — this is observationally inert for the actions.jsonl records.)

**Ledger entry to record** (parity-ledger.md, U-030 row):
`[≠]U030-UNIFIED-LOOP — one unified activation/commit loop fanned to per-platform loggers vs Python's
two independent coroutines. Inexpressible without two OASIS envs (teri has none — see
[≠]U028-OASIS-INTERNALS). Schema/round-numbering/boundary-records ARE byte-faithful; only the
agent-activation draw coupling + native-vs-DB action counts diverge, both already rooted in
[≠]U028-RNG-SEQUENCE / [≠]U028-OASIS-INTERNALS. NOT a feature skip — both streams are fully emitted
and the dual-gate fires.`

This is a legal `[≠]` (genuinely inexpressible substrate gap), NOT a disguised feature-skip: both
`actions.jsonl` files ARE produced, complete, schema-faithful, and gate-detectable.

---

## 5. `build_run_inputs` parallel wiring + honest-500 → 200 (work item §5)

**DECISION-U030-3 — handle `platform=="parallel"` INSIDE `build_run_inputs` by constructing a
two-logger `RunProducer` over the unioned pool, and delete the §2301-2307 honest-500.**

Today `build_run_inputs` is documented "single-platform only" and the caller rejects parallel
before calling it (`simulation.rs:2301`). The cleanest landing keeps the rejection-site deletion
small and puts the dual-logger logic where the single-logger logic already lives.

### Changes

1. **`build_run_inputs`**: branch on `platform`:
   - `"twitter"` / `"reddit"` (unchanged): `PlatformLoggerSet::single(parse(platform), logger)`.
   - `"parallel"`:
     ```rust
     let twitter = Arc::new(PlatformActionLogger::new("twitter", &sim_dir)?);
     let reddit  = Arc::new(PlatformActionLogger::new("reddit",  &sim_dir)?);
     RunProducer { loggers: PlatformLoggerSet::parallel(twitter, reddit), config: config.clone() }
     ```
     The pool is `load_agent_pool(&sim_dir, "parallel")` (already returns the UNIONED twitter+reddit
     pool with each agent's `social.platform` set). The activation policy
     `TimeActivationPolicy::from_config(&config, None)` is installed unchanged — it gates the unioned
     pool by `user_id`, fanning to both platforms.
2. **Delete the honest-500** at `simulation.rs:2301-2307`. The caller then falls through to the
   normal `build_run_inputs(...)` → `start_simulation(...)` → 200 path for `platform=="parallel"`
   exactly as it does for twitter/reddit today. No other change to the `/start` handler.

### Monitor dual-gate fires (confirm — work item §5)

- `start_simulation` already spawns `monitor_simulation`, which tails BOTH
  `{id}/twitter/actions.jsonl` and `{id}/reddit/actions.jsonl` (`simulation_runner.rs:1693-1717`).
- The unified `run()` now writes BOTH files (both `PlatformActionLogger::new(...)` create their dirs
  + files; both receive `simulation_start` → … → `simulation_end`).
- On each platform's `simulation_end` record, `apply_log_record` sets `twitter_completed` /
  `reddit_completed` (L1882-1912). `check_all_platforms_completed` sees BOTH files exist (both
  enabled) and BOTH completed → returns `true` → `runner_status=COMPLETED` (L1907-1911, 2025-2042).
- The single `completion_rx` (one engine, one `SimCompletion`) fires once after BOTH
  `simulation_end` records are written (same `run()`); the monitor's final pass reads both logs, so
  no trailing action is lost and both `*_completed` are set before/at the COMPLETED transition.
  **No monitor edit required.** ✔

---

## 6. round-0 initial_posts (work item §6 / "work item B")

Python logs round 0 BEFORE the main loop, per platform:
`log_round_start(0,0)` → per `initial_posts` whose `poster_agent_id` maps to an agent in THAT
platform's graph: `CREATE_POST log_action(round_num=0, poster_agent_id, content, action_args={"content":content})`
→ `log_round_end(0, initial_action_count)`. Agents not in that platform's graph: `except: pass` (skip).
Python ALSO `env.step(initial_actions)` to inject posts into the world.

### Mapping to teri

In teri's UNIONED pool, an `initial_post`'s `poster_agent_id` (an OASIS `user_id`) maps to **exactly
one** agent with **one** platform. So:

- Both loggers emit `log_round_start(0, 0)` (fan-out, like every boundary record).
- For each `initial_post`: resolve `poster_agent_id` → the pool agent whose `social.user_id ==
  poster_agent_id`; route a `CREATE_POST` `log_action(0, poster_agent_id, agent_name,
  "CREATE_POST", Some({"content": content}), None, true)` to **that agent's platform logger** only;
  increment that platform's round-0 count.
  - `action_args` is `{"content": content}` — **exact match** to teri's
    `SocialAction::CreatePost{content}.oasis_action_args()` (`sim/mod.rs:134`) and to Python
    (`action_args={"content": content}`).
  - **Unresolvable `poster_agent_id`** (no pool agent with that `user_id`) → skip silently
    (Python's `except Exception: pass`). This is the round-0 analog of Python's "agent not in this
    platform's graph" skip — faithful.
- Both loggers emit `log_round_end(0, <that-platform's round-0 count>)`. A platform with no round-0
  posts emits `log_round_end(0, 0)`.

### `event_config.initial_posts` source

From the producer's `config` (the `simulation_config.json` already on `RunProducer.config`):
`config["event_config"]["initial_posts"]` — a list of `{poster_agent_id, content}`. Read with the
same defensive `.get().and_then()` chain teri already uses for `event_config.initial_posts` at
`simulation.rs:1416-1418`. `poster_agent_id` default 0, `content` default "" (Python
`post.get("poster_agent_id",0)`, `post.get("content","")`).

### `env.step(initial_actions)` world-injection → `[≠]`

teri's `WorldState` has **no OASIS post-graph**; there is nowhere to inject posts for later agents
to react to. This is a substrate gap, already the root of `[≠]U028-OASIS-INTERNALS`. **The round-0
`actions.jsonl` RECORDS are differentially portable** (we emit them faithfully); only the
side-effect of seeding the world is inexpressible.
**Ledger:** fold into `[≠]U028-OASIS-INTERNALS` (no NEW tag needed) — round-0 records ported, world
injection is the known no-OASIS-DB gap.

### Where round-0 lives in `run()`

Emit round-0 in `run()` **before** the `for tick_idx in 0..max_ticks` loop, **after**
`log_simulation_start` fan-out (§2). Sequence in `run()`:
1. `simulation_start` fan-out (all loggers).
2. **round-0 block** (NEW): `round_start(0,0)` fan-out → routed `CREATE_POST` per initial_post →
   `round_end(0, per-platform-count)` fan-out. Reads `config["event_config"]["initial_posts"]`.
   Round-0 CREATE_POSTs **also count toward `total_actions`** per platform (Python increments
   `total_actions` for each initial post — L1199, before the loop).
3. Main `0..max_ticks` loop (rounds 1..N), unchanged except §2/§3 fan-out.
4. `simulation_end` fan-out with per-platform `total_actions` (includes round-0).

**Additive-seam note:** round-0 emits only when a producer is installed AND `initial_posts` is
non-empty for a platform — but `round_start(0,0)`/`round_end(0,0)` fan out to all loggers
**whenever a producer is installed** (Python ALWAYS logs round-0 start/end even with no posts:
L1170-1209). For single-platform runs this ADDS round-0 records that U-028 did NOT emit.
**⚠ This changes single-platform output** (U-028 single-platform omitted round-0). See §6-cycle
placement: round-0 is therefore its **own cycle (C)**, applied to BOTH single and parallel, and
re-golden'd against Python for single-platform too (Python's single-platform coroutines DO emit
round-0 — so this is a parity FIX, closing a latent U-028 omission, not a regression). Record this
as a parity-ledger note: "U-030 cycle C adds round-0 emission that U-028 omitted for
single-platform; this CLOSES a gap vs Python, re-verified by golden for twitter+reddit+parallel."

---

## 7. Cycle-splitting recommendation (work item §7) — 3-cycle budget

Ordered by risk; each cycle independently parity-verifiable; the riskiest integration (§5 wiring)
is NOT rushed into the same cycle as the engine generalization.

### Cycle A — RunProducer generalization + `run()` fan-out engine (engine-only, no API change)
**Scope:** §1 (`PlatformLoggerSet`, `RunProducer.loggers`, `with_producer` unchanged), §2
(boundary fan-out + `PerPlatform` accumulators), §3 (routed `log_action` + invariants), §4 (record
the `[≠]U030-UNIFIED-LOOP` ledger entry). Migrate the 4 `sim/mod.rs` tests + `build_run_inputs`
single-platform call to `PlatformLoggerSet::single`.
**Parity-verify:** the EXISTING single-platform twitter & reddit golden tests must still pass
**byte-identically** (this is the regression gate — single-platform output unchanged). Add a
`run()`-level unit test that installs a `PlatformLoggerSet::parallel` over a tiny mixed pool and
asserts twitter actions land in the twitter logger's file and reddit in reddit's, with correct
per-platform `round_end`/`simulation_end` counts.
**Risk:** LOW-MEDIUM. Pure engine; ≤5 mechanical call-site migrations; byte-identity is the gate.
**Independently shippable:** yes — no API behavior changes (parallel still 500 at the API until B).

### Cycle B — `build_run_inputs` parallel wiring + `/start` 200 (API integration)
**Scope:** §5 (dual-logger parallel branch in `build_run_inputs`; delete the §2301-2307 honest-500).
Depends on A (needs `PlatformLoggerSet::parallel`).
**Parity-verify:** a `/start` integration test with `platform="parallel"` returns 200 (not the
honest-500); both `{id}/twitter/actions.jsonl` and `{id}/reddit/actions.jsonl` are created and each
ends with a `simulation_end` record; the monitor marks `twitter_completed && reddit_completed` and
transitions `runner_status=COMPLETED` (assert via `get_run_state` / `run_state.json`). Confirm the
dual-gate (`check_all_platforms_completed`) fires — no monitor code touched.
**Risk:** MEDIUM (the real integration: end-to-end /start → unified run → dual files → dual-gate).
This is the highest-value cycle; isolating it from A keeps the engine change provably
regression-free before wiring it live.
**Independently shippable:** yes — delivers the U-028-deferred parallel capability.

### Cycle C — round-0 initial_posts (both single + parallel)
**Scope:** §6 (round-0 block in `run()` before the main loop; reads
`config.event_config.initial_posts`; routed CREATE_POST; round-0 counts into `total_actions`).
Depends on A (uses the fan-out + routing machinery).
**Parity-verify:** golden-test round-0 emission for **twitter, reddit, AND parallel** against the
Python coroutines' round-0 output (`log_round_start(0,0)` / per-post `CREATE_POST` / `log_round_end(0,
count)`). Note in the ledger that this CLOSES a latent U-028 single-platform omission (U-028 did not
emit round-0); re-golden single-platform to confirm it now matches Python's round-0.
**Risk:** LOW-MEDIUM. Mechanically simple, but it CHANGES single-platform output (adds round-0), so
it must be a SEPARATE cycle with its own golden refresh — never folded into A (which must prove
byte-identity) or B.

**Why this order:** A proves the engine generalization is regression-free in isolation (byte-identity
gate) before B wires it to the live API; B is the single highest-risk integration, given its own
cycle and its own end-to-end gate; C is deferred last because it deliberately changes single-platform
output and needs an independent golden refresh — bundling it earlier would muddy A's byte-identity
gate or B's parallel-200 gate. If the 3-cycle budget is tight, A+B are the MVP (parallel works); C
closes the round-0 parity gap and can slip to a follow-up session without blocking parallel.

---

## Summary of decisions (for the porter)

| ID | Decision |
|---|---|
| DECISION-U030-1 | `RunProducer.loggers: PlatformLoggerSet` (internal `Vec<(Platform,Arc<Logger>)>`); `with_producer` sig UNCHANGED; single-platform via `::single`, parallel via `::parallel`. |
| DECISION-U030-2 | Per-platform `PerPlatform<i64>` accumulators (round + total), seeded from the installed platforms; boundary records fan out to all loggers, counts are per-platform. |
| DECISION-U030-3 | Route `log_action` by `agent.persona.social.platform`; invariant: the logger set contains every platform present in the pool; route-miss = no-op-no-count (never misroute). |
| `[≠]U030-UNIFIED-LOOP` | One unified loop fanned out vs two coroutines; only activation-coupling + native-vs-DB counts diverge (rooted in `[≠]U028-RNG-SEQUENCE` / `[≠]U028-OASIS-INTERNALS`). Both streams fully emitted; gate-detectable; NOT a skip. |
| round-0 / `[≠]U028-OASIS-INTERNALS` | Round-0 records ported faithfully (routed CREATE_POST + fan-out start/end); `env.step` world-injection is the known no-OASIS-DB gap. |
| Cycles | A = engine generalization (LOW-MED, byte-identity gate) → B = parallel API wiring + 200 (MED, e2e dual-gate) → C = round-0 (LOW-MED, separate golden refresh). A+B = MVP. |
