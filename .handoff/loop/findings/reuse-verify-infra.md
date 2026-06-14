# Reuse-Y Verification — Infra units (MiroFish → teri)

Date: 2026-06-14
Mode: differential reuse-Y verification, default-skeptical. Reuse is never trusted; a
divergence RECLASSIFIES to extend-Y with the exact missing behavior.
Worktree: /home/drdave/Desktop/meta/.worktrees/mirofish-port/teri

---

## U-004 — logging (reuse-Y candidate → teri tracing setup)

### Source contract (MiroFish `logger.py`)
- `LOG_DIR = backend/logs/` (logger.py:27); dir created `os.makedirs(exist_ok=True)` (logger.py:42). [S-026]
- `_ensure_utf8_stdout()` reconfigures stdout/stderr to UTF-8 on win32 only (logger.py:13-23). [S-027]
- `setup_logger(name, level=DEBUG)` (logger.py:30): **two handlers** —
  1. `RotatingFileHandler` → `{YYYY-MM-DD}.log`, maxBytes=10MB, backupCount=5, level=DEBUG,
     detailed formatter `[time] LEVEL [name.func:line] msg` (logger.py:66-75). [S-028]
  2. `StreamHandler(stdout)`, level=INFO, simple formatter `[time] LEVEL: msg` (logger.py:80-82).
  - init-once guard: `if logger.handlers: return logger` (logger.py:52); `propagate=False`. [S-028]
- `get_logger(name)` (logger.py:91): returns existing or calls setup_logger. [S-029]
- module-level `logger = setup_logger()` (logger.py:108). [S-030]
- shortcuts `debug/info/warning/error/critical` delegate to module logger (logger.py:112-125). [S-031..S-035]

### teri (DEST) behavior
- `src/logging.rs::init_logging(level)` — `tracing_subscriber::fmt().with_env_filter(...)
  .with_target(true).with_level(true).init()` (logging.rs:4-12).
- `src/main.rs:73` and `:102` — `tracing_subscriber::fmt().with_env_filter(&config.logging.level).init()`.
- tracing's `fmt()` default writer = **stdout/stderr only**. No file appender, no rotation.
- init-once: tracing `.init()` sets the **global** subscriber once; a 2nd `.init()` panics (NOT a
  silent no-op). teri calls it at exactly one site per subprocess entry (run / serve), so single-init
  holds in practice, but there is no MiroFish-style idempotent guard.

### Differential: teri-covers vs gap
| Source contract behavior | teri | Verdict |
|---|---|---|
| Console output with levels + structured fields | tracing fmt with_level/with_target | COVERED |
| Level filtering (env-driven) | EnvFilter (logging.rs:5-7) | COVERED + superior (RUST_LOG) |
| Init-once | global subscriber, single call-site | COVERED (different mechanism) |
| UTF-8 stdout | Rust stdout is UTF-8 natively; win32 reconfig N/A | COVERED (not applicable) |
| **Rotating FILE handler** (10MB×5, dated file, DEBUG to disk) | **ABSENT** | **GAP** |

### Evidence
- Source file logging: `logger.py:66-75` (RotatingFileHandler, 10MB, backupCount=5).
- teri console-only: `logging.rs:9`, `main.rs:73`, `main.rs:102` — no `tracing_appender`,
  no `rolling`, no `with_writer(file)`. Confirmed by grep over src/ (zero appender/rolling hits).

### VERDICT: U-004 → **intentional-divergence `- [≠]`** (console-only), low-criticality.
Rationale: teri's design (CLAUDE.md) is "no secrets/files on disk; env-driven config." Console/stdout
logging + `RUST_LOG` env filter is the idiomatic Rust equivalent of console+level handlers, and
operators capture stdout to a file/rotation at the process-manager layer (as MiroFish itself does at
simulation_runner.py:427-428, redirecting child stdout into `simulation.log`). The rotating-file
appender is therefore an environmental concern, not a behavioral capability the engine must own.
This is an ALLOWED `- [≠]` only with owner sign-off + recorded rationale (above). If owner wants
parity, the exact missing behavior is below.

**If reclassified to extend-Y (owner's call):**
- Missing behavior: rotating file sink — dated/size-rolled log file at DEBUG, console stays INFO.
- Target: `src/logging.rs::init_logging` (logging.rs:9).
- Source line: `logger.py:66-75`.
- Idiom map: add `tracing-appender` (`rolling::daily` or size-based) as a 2nd layer via
  `tracing_subscriber::registry().with(fmt_stdout).with(fmt_file)`; gate file path behind a config
  key so the "no files on disk" default is preserved (opt-in).

---

## U-048 / U-047 — JSONL action-log streaming (reuse-Y → teri SimEngine streaming)

> Ledger note: the task framed this as "U-048", but symbol-map U-048 = **Report Streaming Contract**
> (report_agent.py:2100, S-1057). The **JSONL tail-read** contract described in the task is
> symbol-map **U-047 / S-1056** (`simulation_runner.py:563`). I verified the JSONL-tail runtime
> contract (the behavior the task specifies); the id is flagged for the orchestrator, no ledger edit made.

### Source contract (MiroFish runtime — simulation_runner.py + action_logger.py)
Behavioral contract = **ordered, no-loss, catch-up streaming of simulation action events to observers,
with an explicit end-of-sim signal.**
- Producer: `PlatformActionLogger.log_action / log_round_*/ log_simulation_*` append one JSON object
  per line to `{platform}/actions.jsonl` (action_logger.py:43-116). Append-only → total order by line.
- Consumer: `_monitor_simulation` daemon thread (simulation_runner.py:482) loops while `process.poll()
  is None`, and `_read_action_log(path, position, …)` (simulation_runner.py:583) does
  `f.seek(position) … return f.tell()` (simulation_runner.py:611-688) — **tail by FILE POSITION**:
  each line is parsed exactly once, never re-read, never dropped. [no-loss + ordered]
- Catch-up: a late reader passes its stored `position`/`from_line` and receives every line appended
  since — full history catch-up (simulation_runner.py:611). [S-1056]
- End-of-sim: `event_type == "simulation_end"` flips `*_completed`, and
  `_check_all_platforms_completed` → `RunnerStatus.COMPLETED` (simulation_runner.py:622-640;
  emitted at action_logger.py:105-116). [explicit terminal signal]
- Round progress: `round_end` events update current_round/simulated_hours (simulation_runner.py:643-661).

### teri (DEST) behavior — `SimEngine` streaming
- `subscribe()` → `broadcast::Receiver<WorldSnapshot>` (sim/mod.rs:412); channel capacity **64**
  (sim/mod.rs:393).
- `subscribe_with_history()` → `(Receiver, Arc<Mutex<Vec<WorldSnapshot>>>)` (sim/mod.rs:427-431). The
  Vec is the canonical store, pushed tick-by-tick in `run()` (sim/mod.rs:495). Late subscriber drains
  the Vec to replay missed ticks, dedups by `WorldSnapshot::tick` (documented contract, sim/mod.rs:422-426).
- `run()` per tick: `snapshot_tx.send(clone)` (sim/mod.rs:489) → hooks (sim/mod.rs:491-493) →
  `snapshot_history.push` (sim/mod.rs:495). Ordered: ticks advance 1..max (sim/mod.rs:459-460);
  history index == tick (verified by test, sim/mod.rs:861-863).
- `api::streaming::TickBuffer` (streaming.rs:16): bounded FIFO, `push` drops oldest at capacity and
  RETURNS the dropped snapshot (streaming.rs:48-60) → explicit backpressure signal. `StreamAdapter`
  bridges broadcast→buffer via `as_hook()` (streaming.rs:140-145).
- Lag signal: `StreamEvent::lag_gap(n)` exists for `RecvError::Lagged(n)` so SSE clients know to
  replay history (api/mod.rs:78-88).

### Differential: teri-covers vs gap
| Source contract behavior | teri mechanism | Verdict |
|---|---|---|
| **Event ordering** | ticks 1..N monotonic; history index==tick | COVERED — test sim/mod.rs:861-874 asserts order in history AND broadcast |
| **No-loss / catch-up (late subscriber gets full history)** | `snapshot_history` Vec replay (broadcast alone is lossy past 64) | COVERED — replay Vec is unbounded, never drops; test sim/mod.rs:572-581 |
| **Backpressure (bounded channel)** | `TickBuffer` bounded, drops-oldest + returns dropped | COVERED — test streaming.rs:181-203 (drop oldest, newest retained) |
| **Explicit end-of-sim signal** | run() returns `SimulationResult`; NO in-band terminal event on the stream | **GAP** (see below) |
| Round/progress events on the stream | per-tick snapshot carries tick/events; no distinct round_end event-type | partial — folded into per-tick snapshot (acceptable) |

### The one real gap — end-of-sim terminal signal
MiroFish emits an **in-band** `simulation_end` record on the same JSONL stream the observer is
tailing (action_logger.py:105-116; consumed simulation_runner.py:622-640). A pure stream subscriber
in teri sees the broadcast channel simply **stop** (sender dropped → `RecvError::Closed`) — there is
no in-band "this was the last tick" snapshot/event. `run()` returns `SimulationResult` to its *direct
caller*, but a detached `subscribe()`/SSE consumer gets channel-closed, not a typed terminal marker.
- Source: action_logger.py:105 (`log_simulation_end`), simulation_runner.py:623 (detect).
- teri: sim/mod.rs:496 — loop ends, no terminal send; `run()` returns at sim/mod.rs:500.

### Durable-replay-after-restart (checked, NOT a gap for this contract)
MiroFish's tail is durable across a *server* restart only because the JSONL file persists on disk and
the reader re-seeks by stored position. teri's `snapshot_history` is in-memory → lost on process
restart. BUT: MiroFish's monitor thread itself does not survive a restart either (it re-tails from a
persisted offset of a persisted file); teri's equivalent persistence is U-046/redb-backed
`SimulationResult` storage, out of scope for this streaming unit. So restart-durability is a
storage-layer concern, not a streaming-contract gap. Noted, not counted against U-047.

### VERDICT: U-047 (JSONL streaming) → **extend-Y**.
Three of four contract behaviors are reuse-confirmed against teri's broadcast+history+TickBuffer
(ordering, no-loss/catch-up, backpressure — all test-proven). The streaming mechanism maps cleanly.
One behavior is missing:
- **Missing behavior:** in-band end-of-sim terminal signal delivered to *stream subscribers*
  (so a detached SSE/broadcast consumer gets a typed "simulation_end" marker, not a bare
  channel-close).
- **Target:** `SimEngine::run()` end-of-loop, broadcast a terminal `WorldSnapshot`/event before
  return — `src/sim/mod.rs:496` (after the tick loop, before sim/mod.rs:500); and a corresponding
  `StreamEvent::sim_end(...)` next to `lag_gap` in `src/api/mod.rs:82`.
- **Source line:** `action_logger.py:105` (`log_simulation_end`) + `simulation_runner.py:623`
  (`event_type == "simulation_end"` detection → COMPLETED).
- Severity: low-medium. Round-progress events are acceptably folded into per-tick snapshots; only the
  terminal marker is a genuine observable-behavior gap for stream consumers.

---

## Test evidence (GREEN, this worktree)
- `cargo test --lib sim::` → 36 passed (incl. test_subscribe_with_history_returns_shared_arc:572,
  test_sim_engine_run_basic_with_broadcast:810 — ordering+history+broadcast).
- `cargo test --lib api::streaming` → 9 passed (TickBuffer backpressure/drain/peek, StreamAdapter).
- No test exercises an end-of-sim terminal marker on the stream (confirms the gap is unproven → real).

## Symbol coverage (NOT marking shared symbol-map — per instructions)
- U-004 S-026..S-035: behavior-covered as `- [≠]` (console-only divergence) EXCEPT rotating-file
  capability (S-028's file-handler half = the gap).
- U-047 S-1056: ordering/no-loss/catch-up/backpressure confirmed; end-of-sim terminal signal = gap.
