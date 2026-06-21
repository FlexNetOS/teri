# U-028/029/030 Orchestration Residual — Scoping Classification
_Cycle: post-C3b, pre-unit-flip · Date: 2026-06-20_

This document classifies every `[ ]`/`[~]` symbol in U-028 (run_twitter_simulation.py),
U-029 (run_reddit_simulation.py), and U-030 (run_parallel_simulation.py) into one of three
buckets:

- **[x]-SUBSTRATE-SATISFIED** — teri substrate already provides this behavior faithfully
- **[≠]-SUBSTRATE-GAP** — structurally inexpressible on teri's in-process substrate
- **[ ]-GENUINELY-UNPORTED** — real behavior not yet in teri, would need a port

---

## Visibility filter applied

Row-eligible: all `[ ]` and `[~]` symbols in U-028, U-029, U-030. S-876 ([x]) is skipped per
instructions.

---

## Part 1: Per-Symbol Classification Table

### U-028 — run_twitter_simulation.py

---

**S-853 · UnicodeFormatter (type)**
`run_twitter_simulation.py:53`

BUCKET: **[≠]-SUBSTRATE-GAP**

teri has no Python logging infrastructure; teri uses `tracing`. This is an OASIS logging
artifact — it exists only to decode `\\uXXXX` escape sequences in Python `logging.Formatter`
output for OASIS library loggers. teri does not run OASIS and has no equivalent multi-handler
file logging system. Folds under a new `[≠]` tag: **U028-LOGGING** (OASIS file-logger
plumbing, inexpressible in Rust tracing stack).

---

**S-854 · UnicodeFormatter.UNICODE_ESCAPE_PATTERN (field)**
`run_twitter_simulation.py:56`

BUCKET: **[≠]-SUBSTRATE-GAP** (same tag: U028-LOGGING)

The regex field is inseparable from the Python logging formatter substrate. No teri analog exists
or is needed.

---

**S-855 · UnicodeFormatter.format (method)**
`run_twitter_simulation.py:58`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-LOGGING)

The format override decodes `\\uXXXX` in OASIS log output. teri's tracing subscriber handles
Unicode natively without such preprocessing.

---

**S-856 · MaxTokensWarningFilter (type)**
`run_twitter_simulation.py:70`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-LOGGING)

Python logging filter suppressing camel-ai `max_tokens` warnings. camel-ai is the OASIS LLM
adapter layer, not used in teri. teri has no equivalent log-filter API.

---

**S-857 · MaxTokensWarningFilter.filter (method)**
`run_twitter_simulation.py:73`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-LOGGING)

Same as S-856 — OASIS/camel-ai logging artifact.

---

**S-858 · setup_oasis_logging (fn)**
`run_twitter_simulation.py:84`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-LOGGING)

Configures Python logging for OASIS-internal loggers (`social.agent`, `social.twitter`,
`social.rec`, `oasis.env`, `table`). These are OASIS library logger names that do not exist
in teri. The behavior — clean log files per simulation — is achieved in teri by
`init_logging_for_simulation` which disables the OASIS loggers at startup and uses
`SimulationLogManager` instead (U-030's `[x]` path). This function is the OASIS-side
equivalent for the single-platform scripts; it is not expressible in teri.

---

**S-859 · CommandType (type, local copy in U-028)**
`run_twitter_simulation.py:139`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

This is a local class-level enum (INTERVIEW / BATCH_INTERVIEW / CLOSE_ENV). The exact same
enum is ported as `crate::services::simulation_ipc::CommandType` at
`src/services/simulation_ipc.rs:57-64`. The string values ("interview", "batch_interview",
"close_env") match via `CommandType::as_str()`. The U-028 local copy is a duplication of
what the parallel script also had locally — both resolve to the same teri type.

Evidence: `src/services/simulation_ipc.rs:57` — `pub enum CommandType { Interview, BatchInterview, CloseEnv }` with `as_str()` at line 75 matching Python `.value` strings.

---

**S-860 · IPCHandler (type)**
`run_twitter_simulation.py:146`

BUCKET: **[≠]-SUBSTRATE-GAP** (tag: **U028-SUBPROCESS-IPC**)

`IPCHandler` is the subprocess-side IPC handler: it manages `ipc_commands/` directory polling,
`ipc_responses/` file writes, and `env_status.json` file writes — mechanisms that exist ONLY
because the Twitter simulation runs as a subprocess separated from the Flask app by an OS
process boundary. In teri, the simulation runs in-process and IPC is replaced by an mpsc+oneshot
channel pair (`SimulationIPCServer` at `src/services/simulation_ipc.rs:1069`). The observable
contract (interview command reception + response dispatch + env alive status) is FULLY satisfied
by `SimulationIPCServer`; the file-transport mechanism is structurally inexpressible.

The subprocess-IPC gap is DECISION-17 §17.4, DECISION-16, and the `[≠]`U028-SUBPROCESS-IPC
already articulated in `simulation_runner.rs:817-848`.

---

**S-861 · IPCHandler.__init__ (method)**
`run_twitter_simulation.py:149`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)

Creates `ipc_commands/` and `ipc_responses/` dirs, sets `self.simulation_dir`, `self.env`,
`self.agent_graph`. In teri: `channel(IPC_CHANNEL_BUFFER)` factory at
`src/services/simulation_ipc.rs:1212` produces the `(client, server)` pair; no dir creation
needed.

---

**S-862 · IPCHandler.update_status (method)**
`run_twitter_simulation.py:162`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)

Writes `env_status.json` with `{"status": status, "timestamp": ...}`. In teri: replaced by
`SimulationIPCServer::start()` / `stop()` flipping the shared `Arc<AtomicBool>` at
`src/services/simulation_ipc.rs:1093-1103`. Observable: `check_env_alive()` reads the bool
instead of parsing `env_status.json`.

---

**S-863 · IPCHandler.poll_command (method)**
`run_twitter_simulation.py:170`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)

Scans `ipc_commands/` dir, sorted by mtime, returns first parseable JSON file. In teri:
`SimulationIPCServer::poll_commands()` at `src/services/simulation_ipc.rs:1131` — `try_recv()`
on the mpsc receiver. FIFO ordering is preserved (mpsc is inherently FIFO, matching mtime sort).

---

**S-864 · IPCHandler.send_response (method)**
`run_twitter_simulation.py:193`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)

Writes `{cmd_id}.json` to `ipc_responses/` and removes the command file. In teri:
`SimulationIPCServer::send_response(envelope, response)` at `src/services/simulation_ipc.rs:1146`
— fires the embedded `oneshot::Sender`, which is consumed (auto-cleanup). Same observable result.

---

**S-865 · IPCHandler.handle_interview (method)**
`run_twitter_simulation.py:214`

BUCKET: **[≠]-SUBSTRATE-GAP**

Calls `self.env.step({agent: ManualAction(INTERVIEW, prompt)})` — runs an OASIS environment
step with a manual interview action. This calls into OASIS's agent graph and writes the result
to the OASIS `trace` SQLite DB. teri has no OASIS env; interview execution in teri is handled
by the route `interview_route` at `src/api/simulation.rs:2905` which sends via
`SimulationIPCClient::send_interview()` and the sim task processes it through `ipc_server`.

BUT: the interview EXECUTION contract (call the agent LLM with the interview prompt and return a
response) IS unported. This is the `env.step(ManualAction(INTERVIEW))` body — the LLM call that
produces the interview response. The current `run_sim_body` calls `engine.run()` which does not
service IPC commands (see S-877's [~] note: "wait-for-commands/IPC/env.step" are unported).

This is a boundary case between `[≠]` (OASIS env.step mechanism) and `[ ]` (interview LLM
call execution). The OASIS mechanism is `[≠]`. But the OBSERVABLE (calling an LLM with a prompt
and returning a structured response) is a `[ ]-GENUINELY-UNPORTED` behavior that the IPC server
in the sim task must actually execute. The current `run_sim_body` just calls `ipc_server.start()`
and `engine.run()` — it never services `poll_commands()`. This is the core gap.

Reclassified: **[ ]-GENUINELY-UNPORTED** — the sim task's IPC command dispatch loop (poll for
commands and handle them) is not implemented in `run_sim_body`. The in-process transport exists
(`SimulationIPCServer`) but the server is never used to actually service commands after the main
sim loop finishes. See S-877 for the containing analysis.

---

**S-866 · IPCHandler.handle_batch_interview (method)**
`run_twitter_simulation.py:248`

BUCKET: **[ ]-GENUINELY-UNPORTED** (same gap as S-865)

Same as S-865. `handle_batch_interview` calls `env.step()` with multiple `ManualAction(INTERVIEW)`
entries and collects results. The observable (batch LLM interviews returning multiple responses)
is unported for the same reason: the sim task never calls `ipc_server.poll_commands()`.

---

**S-867 · IPCHandler._get_interview_result (method)**
`run_twitter_simulation.py:300`

BUCKET: **[≠]-SUBSTRATE-GAP** (folds under **U028-OASIS-INTERNALS**)

Reads the OASIS `trace` SQLite DB for the latest `INTERVIEW` action row:
```sql
SELECT user_id, info, created_at FROM trace WHERE action='interview' AND user_id=? ORDER BY created_at DESC LIMIT 1
```
teri has no OASIS `trace` DB. Interview responses in teri would be returned directly from the
LLM call result, not from a DB read. This is a pure OASIS-internal artifact.

---

**S-868 · IPCHandler.process_commands (method)**
`run_twitter_simulation.py:343`

BUCKET: **[ ]-GENUINELY-UNPORTED**

The command dispatch loop: poll one command, dispatch to `handle_interview` /
`handle_batch_interview` / `handle_close_env`, return True/False. This is the server-side
processing loop that `run_sim_body` must call after the main simulation loop completes. The
in-process analog (`SimulationIPCServer::poll_commands()`) exists but is never called. The CLOSE
ENV command path (returning False → exit the wait loop) is also unported.

---

**S-869 · TwitterSimulationRunner (type)**
`run_twitter_simulation.py:385`

BUCKET: **[≠]-SUBSTRATE-GAP** (tag: **U028-SUBPROCESS-RUNNER**)

This class is the subprocess-side runner — it exists only because the Twitter simulation runs as
a child process. In teri's in-process model, the runner role is split between:
- `SimulationRunner::start_simulation` (`src/services/simulation_runner.rs:1069`) — lifecycle
- `RunInputs` (`src/services/simulation_runner.rs:958`) — engine + pool + graph + llm
- `run_sim_body` / `spawn_sim_task` (`src/services/simulation_runner.rs:1519+`) — execution

The class-as-subprocess-runner is structurally absent. The observable lifecycle contract is
ported.

---

**S-870 · TwitterSimulationRunner.AVAILABLE_ACTIONS (field)**
`run_twitter_simulation.py:389`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Class-level list of Twitter `ActionType` values passed to `generate_twitter_agent_graph`. teri
does not use OASIS ActionTypes at agent graph construction; the SocialAction enum at
`src/sim/mod.rs:42` is the teri equivalent and is not parameterized per-runner. The OASIS
`available_actions` list is an OASIS agent setup artifact.

---

**S-871 · TwitterSimulationRunner.__init__ (method)**
`run_twitter_simulation.py:398`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Constructor loading config from path, storing `simulation_dir`, `wait_for_commands` flag. In
teri: `SimulationRunner::new` + `start_simulation` params (the config is loaded from
`SimulationManager::get_simulation_config`). No direct analog needed.

---

**S-872 · TwitterSimulationRunner._load_config (method)**
`run_twitter_simulation.py:414`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Loads JSON config file from disk. In teri: `SimulationManager::get_simulation_config` at
`src/services/simulation_manager.rs` (already ported, U-023).

---

**S-873 · TwitterSimulationRunner._get_profile_path (method)**
`run_twitter_simulation.py:419`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Returns path to `twitter_profiles.csv`. This is the OASIS agent graph input artifact. teri uses
`load_agent_pool` with JSON profiles, not OASIS CSVs. Structurally absent.

---

**S-874 · TwitterSimulationRunner._get_db_path (method)**
`run_twitter_simulation.py:423`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)

Returns path to `twitter_simulation.db`. This is the OASIS `trace` SQLite DB path. teri has no
OASIS DB.

---

**S-875 · TwitterSimulationRunner._create_model (method)**
`run_twitter_simulation.py:427`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Creates a camel-ai OASIS model via `ModelFactory.create(ModelPlatformType.OPENAI, model_type)`,
reading `LLM_API_KEY` / `LLM_BASE_URL` / `LLM_MODEL_NAME` from env. In teri:
`build_llm`/`OpenAiAdapter` (`src/llm.rs` shimmy substrate) reads the same env vars via
`SimConfig::from_env()`. The observable (LLM client creation from env vars) is covered; the
OASIS model creation mechanism is `[≠]`.

---

**S-877 · TwitterSimulationRunner.run (method) [currently ~]**
`run_twitter_simulation.py:531`

BUCKET: **COMPOSITE** — the `[~]` mark is correct. Three sub-parts:

**(a) Producer stream:** `[x]-SUBSTRATE-SATISFIED`
`simulation_start` / `round_start(always)` / per-Social `log_action` / `round_end` / `simulation_end`.
Satisfied by `SimEngine::run` with `RunProducer` at `src/sim/mod.rs:843-1108`. Initial-posts
(round-0) also ported per C3b-ii. The `(tick*mpr/60)%24` hour formula at `src/sim/mod.rs:972`.
Differential parity is proven per the S-877 [~] annotation.

**(b) `wait_for_commands` IPC poll loop:**
After the main sim loop, if `wait_for_commands` is True (`run_twitter_simulation.py:671-695`):
```python
ipc_handler.update_status("alive")
while not _shutdown_event.is_set():
    should_continue = await ipc_handler.process_commands()
    if not should_continue: break
    await asyncio.wait_for(_shutdown_event.wait(), timeout=0.5)
```
This is **[ ]-GENUINELY-UNPORTED**. The observable contract: after simulation rounds finish,
the env stays alive and services IPC commands until a `close_env` command or SIGTERM arrives.

In teri: `run_sim_body` at `src/services/simulation_runner.rs:1542` calls `ipc_server.start()`
then `engine.run()` then `ipc_server.stop()`. After `engine.run()` returns, `ipc_server.stop()`
is called immediately — the wait-for-commands window is NOT implemented. The `SimulationIPCServer`
exists and the `SimulationIPCClient` methods exist (S-479-483), but nothing in `run_sim_body`
polls for commands after the main loop.

Note: teri's architecture means interview commands CAN be sent during the simulation (the IPC
channel is live while `run_sim_body` runs). But commands sent AFTER `engine.run()` returns are
never serviced — the `ipc_server.stop()` call happens immediately.

**(c) `env.step()` world execution:**
`[≠]-SUBSTRATE-GAP` (U028-OASIS-INTERNALS). OASIS `env.step()` mutates the simulation world
state (agents react, posts propagate, follows are recorded, etc.). teri has no OASIS world; the
`WorldState` is teri's own thin accumulator. The actual simulation execution happens in `engine.run()`
via `prepare_action` (LLM calls) + `commit_action`. This is a known structural `[≠]`.

**(d) Signal-shutdown integration:**
`[x]-SUBSTRATE-SATISFIED`. Python's `asyncio.wait_for(_shutdown_event.wait(), timeout=0.5)`
breaks the wait loop on SIGTERM/SIGINT. teri: the `AtomicBool shutdown` flag in `RunHandle`
(set by `stop_simulation` → `terminate_handle` → `shutdown.store(true)`) is checked at every
tick boundary in `SimEngine::run` (`src/sim/mod.rs:933-937`). This is the cooperative stop
analog. Additionally `task.abort()` (the SIGKILL analog) is available. The cooperative stop is
at `src/services/simulation_runner.rs:1578`.

**(e) Env cleanup (`ipc_handler.update_status("stopped")` + `env.close()`):**
`[≠]-SUBSTRATE-GAP` (U028-OASIS-INTERNALS + U028-SUBPROCESS-IPC). `env.close()` tears down the
OASIS environment and closes the SQLite DB. In teri: `ipc_server.stop()` sets the alive flag to
false (`src/services/simulation_runner.rs:1558`). No OASIS env to close.

**Summary for S-877:** Stays `[~]`. The outstanding `[ ]` is specifically **(b)**: the
post-sim IPC command service window (poll loop after `engine.run()` in `run_sim_body`).

---

**S-878 · main (fn)**
`run_twitter_simulation.py:707`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

The subprocess entrypoint: argparse, setup asyncio event loop, call `runner.run()`. In teri:
the entrypoint is `main.rs` + the API layer. `start_simulation` is the teri equivalent of the
subprocess launch. `argparse` → API route parameters. `asyncio.run(main())` → `tokio::spawn`
in `start_simulation`. No subprocess entrypoint is needed or expressible in teri.

---

**S-879 · setup_signal_handlers (fn)**
`run_twitter_simulation.py:749`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Sets `signal.SIGTERM` + `signal.SIGINT` handlers to set `_shutdown_event` (once) or force-exit
(on repeated signal). Observable contract: SIGTERM/SIGINT → graceful shutdown with resource cleanup.

In teri: `src/services/simulation_runner.rs:757-758` — `with_shutdown(flag)` + the cooperative
stop `AtomicBool`. teri's server uses `tokio::signal::ctrl_c()` (U-002, wired to `cleanup_all`
per S-626/U-049, which in turn calls `terminate_handle` with CLEANUP_GRACE). The "repeated signal
→ force exit" is covered by `task.abort()` in `terminate_handle` after the grace window. The
observable (graceful + then hard) is faithfully ported, though U-049 (the signal wiring) is
noted as `[→U-049]` in the ledger.

Cite: `src/services/simulation_runner.rs:862-884` (STOP_GRACE / CLEANUP_GRACE constants and the
terminate_handle function) + `src/services/simulation_runner.rs:1576-1600` (`terminate_handle`).

---

### U-029 — run_reddit_simulation.py

_Note: U-029 is byte-structurally nearly identical to U-028. The only material differences are:
platform = REDDIT, profile file = `reddit_profiles.json`, db = `reddit_simulation.db`, action
set = `REDDIT_ACTIONS` (13 actions vs 6), and reddit allows multiple posts per initial_posts
entry for the same agent (list dedup logic at line 603-614). The classification follows the same
logic as U-028 for matching symbols._

---

**S-880 · UnicodeFormatter · [≠]-SUBSTRATE-GAP** (U028-LOGGING mirror)
**S-881 · UnicodeFormatter.UNICODE_ESCAPE_PATTERN · [≠]-SUBSTRATE-GAP** (U028-LOGGING mirror)
**S-882 · UnicodeFormatter.format · [≠]-SUBSTRATE-GAP** (U028-LOGGING mirror)
**S-883 · MaxTokensWarningFilter · [≠]-SUBSTRATE-GAP** (U028-LOGGING mirror)
**S-884 · MaxTokensWarningFilter.filter · [≠]-SUBSTRATE-GAP** (U028-LOGGING mirror)
**S-885 · setup_oasis_logging · [≠]-SUBSTRATE-GAP** (U028-LOGGING mirror)

All six are byte-identical clones of the U-028 logging artifacts. Same analysis applies.

---

**S-886 · CommandType (U-029 local copy) · [x]-SUBSTRATE-SATISFIED**
`run_reddit_simulation.py:139`

Identical to S-859. Satisfied by `crate::services::simulation_ipc::CommandType` at
`src/services/simulation_ipc.rs:57`.

---

**S-887 · IPCHandler (U-029) · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC mirror)
**S-888 · IPCHandler.__init__ · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC mirror)
**S-889 · IPCHandler.update_status · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC mirror)
**S-890 · IPCHandler.poll_command · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC mirror)
**S-891 · IPCHandler.send_response · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC mirror)

Byte-identical to U-028 versions (same filesystem IPC transport). Same analysis.

---

**S-892 · IPCHandler.handle_interview · [ ]-GENUINELY-UNPORTED** (mirror of S-865)

reddit version: same `env.step(ManualAction(INTERVIEW))` call. Same gap.

---

**S-893 · IPCHandler.handle_batch_interview · [ ]-GENUINELY-UNPORTED** (mirror of S-866)

Same gap.

---

**S-894 · IPCHandler._get_interview_result · [≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)
Reads `reddit_simulation.db` trace table. Mirror of S-867. OASIS DB artifact.

---

**S-895 · IPCHandler.process_commands · [ ]-GENUINELY-UNPORTED** (mirror of S-868)

Same command dispatch loop gap.

---

**S-896 · RedditSimulationRunner · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER mirror)
**S-897 · RedditSimulationRunner.AVAILABLE_ACTIONS · [≠]-SUBSTRATE-GAP**
**S-898 · RedditSimulationRunner.__init__ · [≠]-SUBSTRATE-GAP**
**S-899 · RedditSimulationRunner._load_config · [≠]-SUBSTRATE-GAP**

Same subprocess-runner artifacts as U-028.

---

**S-900 · RedditSimulationRunner._get_profile_path · [≠]-SUBSTRATE-GAP**
`run_reddit_simulation.py:426`

Returns path to `reddit_profiles.json`. Mirror of S-873. OASIS artifact.

---

**S-901 · RedditSimulationRunner._get_db_path · [≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS mirror)
Returns `reddit_simulation.db`. Mirror of S-874.

---

**S-902 · RedditSimulationRunner._create_model · [≠]-SUBSTRATE-GAP** (mirror of S-875)

---

**S-903 · RedditSimulationRunner._get_active_agents_for_round (method)**
`run_reddit_simulation.py:469`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

CRITICAL QUESTION: Is S-903 byte-structurally identical to the already-[x] S-876 (Twitter)?

**Verdict: YES.** Reading `run_reddit_simulation.py:469-521` and `run_twitter_simulation.py:462-529`
side by side confirms they are structurally identical:
- Same parameters: `(self, env, current_hour, round_num) -> List`
- Same time_config key reads: `agents_per_hour_min`, `agents_per_hour_max`, `peak_hours`,
  `off_peak_hours`, `peak_activity_multiplier`, `off_peak_activity_multiplier`
- Same same `random.uniform(min, max) * multiplier` → `target_count`
- Same `active_hours`/`activity_level` per-agent gating
- Same `random.sample(candidates, min(target_count, len(candidates)))`
- Same `env.agent_graph.get_agent(agent_id)` resolution

The ONLY differences are cosmetic (missing comment lines in reddit version). The algorithm and
all config key names are byte-identical.

Therefore: S-876's port (`TimeActivationPolicy` in `src/sim/activation.rs`) covers S-903
completely. The parity proof for S-876 ("reddit≡twitter diffed" noted in the S-876 annotation)
explicitly validated this identity. S-903 is a **verify-only flip** — no additional porting work
needed, the verifier need only confirm the existing `TimeActivationPolicy` tests are valid for
the reddit case (which they are, since the policy is platform-agnostic and the config keys are
identical).

---

**S-904 · RedditSimulationRunner.run (method)**
`run_reddit_simulation.py:523`

BUCKET: **COMPOSITE** (mirror of S-877)

Same analysis as S-877. The reddit `.run` is structurally identical to the twitter `.run`:
- Same producer stream structure (proven by parity notes on S-877 citing byte-identical parallel
  coroutines)
- Same wait-for-commands IPC poll loop → **[ ]-GENUINELY-UNPORTED** (same gap as S-877b)
- Same env.step world execution → **[≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)
- Same signal-shutdown → **[x]-SUBSTRATE-SATISFIED**

One reddit-specific behavioral difference: initial_posts multi-action per agent
(`run_reddit_simulation.py:603-614` allows a list of actions per agent, vs Twitter which
overwrites duplicates). The producer side of this is PORTED: the `SimEngine::run` round-0
initial_posts path in `src/sim/mod.rs:883-923` routes each post individually by `poster_agent_id`
and there is no dedup concern since the producer records each POST separately regardless.

Stays `[~]` mirroring S-877 for the same reason.

---

**S-905 · main (fn, U-029) · [≠]-SUBSTRATE-GAP** (mirror of S-878)
**S-906 · setup_signal_handlers (fn, U-029) · [x]-SUBSTRATE-SATISFIED** (mirror of S-879)

---

### U-030 — run_parallel_simulation.py

---

**S-907 · MaxTokensWarningFilter (U-030) · [≠]-SUBSTRATE-GAP** (U028-LOGGING)
**S-908 · MaxTokensWarningFilter.filter (U-030) · [≠]-SUBSTRATE-GAP** (U028-LOGGING)

Same OASIS/camel-ai logging artifact.

---

**S-909 · disable_oasis_logging (fn)**
`run_parallel_simulation.py:120`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-LOGGING)

Suppresses OASIS library Python loggers by setting them to CRITICAL level. teri does not
use OASIS loggers. Inexpressible.

---

**S-910 · init_logging_for_simulation (fn)**
`run_parallel_simulation.py:141`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-LOGGING)

Calls `disable_oasis_logging()` and removes the old `log/` directory. The `SimulationLogManager`
in teri replaces the log management behavior; the OASIS logger suppression is a subprocess
artifact.

---

**S-911 · TWITTER_ACTIONS (const)**
`run_parallel_simulation.py:178`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

The list of OASIS `ActionType` enums passed to `generate_twitter_agent_graph`. teri's
`SocialAction` enum at `src/sim/mod.rs:42-67` represents the same action taxonomy, but as teri's
own native type — not an OASIS initialization parameter.

---

**S-912 · REDDIT_ACTIONS (const)**
`run_parallel_simulation.py:188`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER). Same as S-911.

---

**S-913 · CommandType (U-030 local copy)**
`run_parallel_simulation.py:210`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Identical to S-859 / S-886. Satisfied by `crate::services::simulation_ipc::CommandType` at
`src/services/simulation_ipc.rs:57`.

---

**S-914 · ParallelIPCHandler (type)**
`run_parallel_simulation.py:217`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC, parallel variant)

The dual-platform subprocess-side IPC handler. Holds two env+agent_graph pairs (twitter + reddit)
and dispatches interview commands to one or both. In teri: the `SimulationIPCServer` at
`src/services/simulation_ipc.rs:1069` handles a single IPC channel; the routing to
platform-specific agents is done by the sim task that receives the command and has access to the
pool (which contains both platforms' agents). The filesystem transport is structurally absent.

The observable contract (route interview commands to the appropriate platform's agents and return
responses) is covered by the existing IPC infrastructure for the portions that ARE ported; the
wait-for-commands loop gap (S-877b) affects this class's use equally.

---

**S-915 · ParallelIPCHandler.__init__ · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)
**S-916 · ParallelIPCHandler.update_status · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)
**S-917 · ParallelIPCHandler.poll_command · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)
**S-918 · ParallelIPCHandler.send_response · [≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-IPC)

Same filesystem-IPC transport artifacts.

---

**S-919 · ParallelIPCHandler._get_env_and_graph (method)**
`run_parallel_simulation.py:300`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Returns `(env, agent_graph, platform_name)` for a platform — OASIS env lookup. No OASIS envs
in teri. The platform routing exists at a higher level via `SocialProfile.platform` on pool agents.

---

**S-920 · ParallelIPCHandler._interview_single_platform (method)**
`run_parallel_simulation.py:317`

BUCKET: **[ ]-GENUINELY-UNPORTED** (partial)

Calls `agent_graph.get_agent(agent_id)` then `env.step({agent: ManualAction(INTERVIEW, prompt)})`.
The OASIS mechanism is `[≠]`. However, the observable behavior (run an LLM interview on an agent
and return a response) is the same gap as S-865. The IPC transport half is covered; the
execution half (calling the interview through the teri agent system) is genuinely unported.

---

**S-921 · ParallelIPCHandler.handle_interview (method)**
`run_parallel_simulation.py:345`

BUCKET: **[ ]-GENUINELY-UNPORTED**

Adds one capability vs U-028: the `platform` parameter allows routing to one or both platforms
(and uses `asyncio.gather` to interview both concurrently when no platform specified,
`run_parallel_simulation.py:395-414`). The concurrent dual-platform interview is the unique
behavior here. The existing teri IPC client has `send_interview(platform=Option<&str>)` at
`src/services/simulation_ipc.rs:979` that conditionally includes the platform key — but the
sim task needs to actually dispatch it. Gap: same as S-877b (IPC command service loop missing
from `run_sim_body`).

---

**S-922 · ParallelIPCHandler.handle_batch_interview (method)**
`run_parallel_simulation.py:416`

BUCKET: **[ ]-GENUINELY-UNPORTED**

Adds platform-grouping for batch interviews: splits by per-item platform key, runs twitter and
reddit batches independently. The observable (platform-aware batching with per-item platform
override) adds contract above U-028's batch. Gap: same root cause.

---

**S-923 · ParallelIPCHandler._get_interview_result (method)**
`run_parallel_simulation.py:517`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)

Reads `{platform}_simulation.db` trace table. Same OASIS DB artifact as S-867/S-894.

---

**S-924 · ParallelIPCHandler.process_commands (method)**
`run_parallel_simulation.py:560`

BUCKET: **[ ]-GENUINELY-UNPORTED** (same as S-868/S-895)

Command dispatch loop for the parallel case. Adds `platform` key passthrough to
`handle_interview` and `handle_batch_interview`. Same core gap: `run_sim_body` never calls
this.

---

**S-925 · load_config (fn)**
`run_parallel_simulation.py:604`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Reads `simulation_config.json` from path. In teri: `SimulationManager::get_simulation_config`
at `src/services/simulation_manager.rs` (U-023, already [x]).

---

**S-926 · FILTERED_ACTIONS (const)**
`run_parallel_simulation.py:611`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

`{'refresh', 'sign_up'}` — the actions excluded from `actions.jsonl` logging. In teri: these
are the actions NOT represented in `SocialAction` (`DO_NOTHING` is included per the docstring
at `src/sim/mod.rs:26-37`; `REFRESH` and `SIGN_UP` are correctly omitted per the docstring note
on `[≠]` REFRESH). The filter is embodied by the exhaustive `SocialAction` enum — any action
not in the enum is not logged. The behavioral contract (refresh/sign_up never appear in
actions.jsonl) is satisfied.

---

**S-927 · ACTION_TYPE_MAP (const)**
`run_parallel_simulation.py:614`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Maps DB action strings (e.g. `'create_post'`) to `actions.jsonl` type strings (e.g.
`'CREATE_POST'`). In teri: `SocialAction::oasis_action_type()` at `src/sim/mod.rs:109-126`
returns the exact same target strings. The source DB names are OASIS internals; the teri
producer emits the `oasis_action_type()` string directly without needing a map.

---

**S-928 · get_agent_names_from_config (fn)**
`run_parallel_simulation.py:633`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Extracts `{agent_id → entity_name}` from `agent_configs`. In teri: `load_agent_pool` builds
the `AgentPool` from `agent_configs` with `entity_name` → `Persona.name`. The `actions.jsonl`
`log_action` call at `src/sim/mod.rs:1054` uses `&pool.agents[idx].persona.name` — the name is
already on the agent, no separate map needed. The behavior (display entity_name in agent_name
field of actions.jsonl) is satisfied by the pool's persona.name.

---

**S-929 · fetch_new_actions_from_db (fn)**
`run_parallel_simulation.py:657`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)

Polls the OASIS `trace` SQLite DB by rowid for new action rows, converts them to the
`actions.jsonl` format. In teri: `SimEngine::run` produces `actions.jsonl` records directly from
the agents' `prepare_action` + `commit_action` cycle WITHOUT needing to poll a DB — the records
are produced BEFORE `env.step` would write to the DB. This function is the OASIS-DB-to-actions-log
bridge; teri's producer path is a direct bridge from the LLM output. The `[≠]U028-OASIS-INTERNALS`
divergence is: teri's `action_args` reflects the agent's intended action, not the DB-enriched
version (post_content, author_name, etc. from the DB trace). This is the existing `[≠]` tag.

Does S-929 fall under the existing `[≠]U028-OASIS-INTERNALS` tag? YES, exactly. S-929 is the
function that READS the OASIS trace DB; it is the same structural gap as `_get_interview_result`
reading the trace DB. The entire function exists only because OASIS writes to a SQLite DB and the
Python script reads it back to produce the jsonl. teri bypasses the DB entirely.

---

**S-930 · _enrich_action_context (fn)**
`run_parallel_simulation.py:749`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)

Enriches `action_args` with data from the OASIS `post`, `user`, `comment`, and `follow` tables:
- `post_content`, `post_author_name` for LIKE_POST/DISLIKE_POST/CREATE_COMMENT
- `original_content`, `original_author_name` for REPOST/QUOTE_POST
- `quote_content` from the post table for QUOTE_POST
- `target_user_name` for FOLLOW/MUTE
- `comment_content`, `comment_author_name` for LIKE_COMMENT/DISLIKE_COMMENT

All of these are OASIS relational DB lookups. teri has no OASIS `post`, `user`, `comment`, or
`follow` tables. These enrichment keys ARE the `[≠]U028-OASIS-INTERNALS` divergence.

Confirmed: S-930 and its sub-functions (S-931/S-932/S-933) ALL fold under the existing
`[≠]U028-OASIS-INTERNALS` tag.

---

**S-931 · _get_post_info · [≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)
**S-932 · _get_user_name · [≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)
**S-933 · _get_comment_info · [≠]-SUBSTRATE-GAP** (U028-OASIS-INTERNALS)

All three are OASIS SQLite DB readers. All fold under the existing `[≠]U028-OASIS-INTERNALS`
tag. No new tag needed.

---

**S-934 · create_model (fn)**
`run_parallel_simulation.py:984`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Creates camel-ai OASIS model with optional boost LLM configuration (dual-LLM: standard for
twitter, boost for reddit if configured). The dual-LLM config (`LLM_BOOST_*` env vars) is an
OASIS parallelism optimization.

In teri: a single `LlmClient` is used for all agents (configured by envctl via `agent-env.toml`).
The boost/standard dual-config is an OASIS-subprocess artifact. However, the `use_boost` flag
adds one observable: when both platforms run, reddit uses a DIFFERENT LLM config than twitter
(different API key/URL/model). This is a genuine behavioral difference between U-028 (single LLM)
and U-030 (potential dual LLM).

This is an **intentional divergence** candidate: teri currently uses a single LlmClient. The
dual-LLM parallelism optimization is an OASIS-era optimization that does not fit cleanly into
teri's architecture. Classification: **[≠]-SUBSTRATE-GAP** — requires owner decision.
Note for work-plan: flag as pending owner decision on whether dual-LLM boost config should be
implemented in teri (via `SimulationRunner::start_simulation` platform-specific LLM routing).

---

**S-935 · get_active_agents_for_round (fn, U-030)**
`run_parallel_simulation.py:1040`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Module-level free function (not a method) with identical logic to S-876 and S-903:
same parameters (`env, config, current_hour, round_num`), same time_config reads, same
algorithm. The only structural difference from S-876/S-903 is that it reads `config` directly
(passed as arg) rather than via `self.config` — because it is a free function used by both
`run_twitter_simulation` and `run_reddit_simulation` coroutines in U-030.

The `TimeActivationPolicy::active_agents(hour)` at `src/sim/activation.rs:175` is exactly
this: hour → selection. Already confirmed for S-903 (same algorithm). The `from_config`
constructor reads the same config keys. S-935 is a verify-only flip like S-903.

---

**S-936 · PlatformSimulation (type)**
`run_parallel_simulation.py:1093`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER)

Result container holding `env`, `agent_graph`, `total_actions` for one platform. In teri:
`RunHandle` (+ `SimulationRunState` for totals) covers this. The OASIS `env` field is absent
in teri. No direct analog needed.

---

**S-937 · PlatformSimulation.__init__ (method)**
`run_parallel_simulation.py:1095`

BUCKET: **[≠]-SUBSTRATE-GAP** (U028-SUBPROCESS-RUNNER). Same as S-936.

---

**S-938 · run_twitter_simulation (coroutine)**
`run_parallel_simulation.py:1101`

BUCKET: **COMPOSITE** (same structure as S-877/S-904)

This is the U-030 version of the twitter run coroutine. It is structurally more complex than the
U-028 `TwitterSimulationRunner.run`:
- It takes explicit `action_logger` and `main_logger` parameters (the U-030 unified logging)
- It reads actions from the OASIS DB after EACH `env.step()` call using `fetch_new_actions_from_db`
  to produce the `actions.jsonl` records (S-929 pattern) — this is the DB-polling path
- It includes the `_shutdown_event` check per round
- It does NOT have a `wait_for_commands` block — that is handled in `main` (S-940)

Parts breakdown:
**(a) Producer stream:** `[x]-SUBSTRATE-SATISFIED` — same analysis as S-877a
**(b) OASIS DB polling for actions:** `[≠]-SUBSTRATE-GAP` (U028-OASIS-INTERNALS) — the
`fetch_new_actions_from_db` call after each `env.step()` is the DB-read path; teri produces
records directly from `commit_action`
**(c) Signal check per round:** `[x]-SUBSTRATE-SATISFIED` — `SimEngine::run` cooperative shutdown
**(d) OASIS env setup:** `[≠]-SUBSTRATE-GAP` — `oasis.make()` + `generate_twitter_agent_graph()`

The coroutine STRUCTURE is the U-030 unit's core behavior, already ported into the unified
`SimEngine::run` with `PlatformLoggerSet::parallel` + dual `TimeActivationPolicy`. The
`run_sim_body` call in `start_simulation` is the in-process analog.

Stays `[~]` (the same IPC wait-for-commands gap from S-940/main is the remaining `[ ]` work).

---

**S-939 · run_reddit_simulation (coroutine)**
`run_parallel_simulation.py:1293`

BUCKET: **COMPOSITE** (mirror of S-938)

Byte-structurally identical to S-938 with REDDIT platform. Same classification.

Unique element: reddit initial_posts allows multiple actions per agent (the list accumulation at
`run_parallel_simulation.py:1379-1389`). This is already handled in teri's round-0 path since
each initial_post is recorded independently regardless.

---

**S-940 · main (fn, U-030)**
`run_parallel_simulation.py:1492`

BUCKET: **COMPOSITE**

The `main` function has three sections:
1. Argparse + config load + logging setup → **[≠]-SUBSTRATE-GAP** (subprocess entrypoint)
2. `asyncio.gather(run_twitter_simulation(...), run_reddit_simulation(...))` — the PARALLEL
   execution of both platform coroutines → **[x]-SUBSTRATE-SATISFIED** by `spawn_sim_task`
   spawning a single `run_sim_body` that runs the unified `SimEngine::run` with parallel
   `PlatformLoggerSet`
3. Post-sim `wait_for_commands` loop → **[ ]-GENUINELY-UNPORTED**

The `asyncio.gather` parallel execution is the U-030 defining behavior (vs U-028/029 sequential).
In teri: both platforms' agents are in the same pool; `SimEngine::run` processes all agents
each tick via `stream::buffered(parallelism)` at `src/sim/mod.rs:1013`. This IS parallel
execution — it's just in a unified loop rather than two coroutines. The `PlatformLoggerSet`
routes each agent's records to the correct platform's `actions.jsonl`.

The `wait_for_commands` loop (lines 1595-1630) is the same gap as U-028/U-029: after
`asyncio.gather` completes, it creates a `ParallelIPCHandler` and enters the 0.5s polling loop.

---

**S-941 · setup_signal_handlers (fn, U-030)**
`run_parallel_simulation.py:1653`

BUCKET: **[x]-SUBSTRATE-SATISFIED**

Structurally identical to U-028/U-029 versions. Same analysis as S-879 applies.

The U-030 version adds `loop=None` parameter and notes the `multiprocessing.resource_tracker`
cleanup in `__main__` (line 1696). The resource_tracker cleanup is a `multiprocessing` import
artifact — not relevant to teri. Signal handling is satisfied by the cooperative stop +
`terminate_handle` path.

---

## Part 2: Per-Unit Verdict

### U-028 (run_twitter_simulation.py)

**CAN THE UNIT FLIP [x] NOW? NO.**

Blocking bucket-3 work: S-865/S-866/S-868 (interview execution and command dispatch) require
that `run_sim_body` in `src/services/simulation_runner.rs:1542` service IPC commands from
`ipc_server.poll_commands()` after (and during) `engine.run()` returns.

The exact missing behavior: after `engine.run()` completes and before `ipc_server.stop()`, the
sim body must enter a wait loop polling `ipc_server.poll_commands()` and dispatching interview
commands through the teri agent pool. The `wait_for_commands` flag from the Python API side
maps to teri's `close_env_route` — which already sends `CommandType::CloseEnv` via
`SimulationIPCClient::send_close_env()`. The server-side dispatch loop is what is missing.

**Gate**: `run_sim_body` must be extended to include the post-sim IPC service loop.

Symbols blocking the flip: S-865, S-866, S-868 (and their U-029 mirrors S-892/S-893/S-895).

Symbols that are [≠] (cannot gate): S-853–858 (logging), S-860–864/S-867 (subprocess IPC
transport + OASIS DB read), S-869–875 (subprocess runner), S-878 (entrypoint).

Symbols already satisfied: S-859 (CommandType), S-877a (producer stream), S-877d (signal
shutdown), S-879 (signal handlers), S-876 ([x]).

### U-029 (run_reddit_simulation.py)

**CAN THE UNIT FLIP [x] NOW? NO, but for the SAME reason as U-028.**

U-029 is a near-perfect mirror of U-028 with the REDDIT platform substituted. The only
genuinely new behaviors vs U-028 are:
1. `_get_active_agents_for_round` (S-903) → verified **[x]** via same substrate as S-876
2. reddit initial_posts multi-action dedup → ported in round-0 path

The unit cannot flip until the IPC command service loop is implemented (same gate as U-028).
Once U-028's gap is resolved, U-029 is a **verify-only flip** — no additional porting required.
U-029's IPC, logging, and runner symbols are byte-identical to U-028's and fall under the same
[≠] tags.

### U-030 (run_parallel_simulation.py)

**CAN THE UNIT FLIP [x] NOW? NO, for the same core gap plus one additional pending decision.**

Blocking bucket-3 work:
1. **IPC command service loop** (S-920, S-921, S-922, S-924): same gap as U-028/029 — the
   post-sim wait-for-commands loop in `run_sim_body` or an equivalent mechanism.
2. **Dual-LLM boost config** (S-934): owner decision needed — does teri implement platform-
   specific LLM clients (twitter uses standard, reddit uses boost when `LLM_BOOST_*` set)?
   If owner approves divergence (`[≠]`), this gate drops.

Symbols already satisfied: S-913 (CommandType), S-925 (load_config), S-926 (FILTERED_ACTIONS),
S-927 (ACTION_TYPE_MAP), S-928 (get_agent_names_from_config), S-935 (get_active_agents_for_round),
S-938a/S-939a (parallel producer stream), S-940b (asyncio.gather → unified SimEngine), S-941
(signal handlers).

Symbols that are [≠]: S-907-912 (logging/OASIS action constants), S-914-919 (subprocess IPC
transport), S-923 (OASIS DB interview read), S-929-933 (OASIS DB polling + enrichment),
S-934 (dual-LLM, pending owner decision), S-936-937 (PlatformSimulation subprocess container).

---

## Part 3: Ordered Work-Plan for Remaining Cycles

### Cycle: IPC command service loop (GATES U-028, U-029, U-030)

**Priority: HIGHEST — this single change gates all three units.**

What to implement in `src/services/simulation_runner.rs:run_sim_body`:

After `engine.run()` returns and before `ipc_server.stop()`, add a post-sim command service loop:

```rust
// After engine.run() succeeds, service commands until close_env or shutdown.
loop {
    // Check shutdown flag
    if shutdown.load(Ordering::Acquire) { break; }
    
    // Poll for a pending command (non-blocking)
    match ipc_server.poll_commands() {
        Some(envelope) => {
            // Dispatch: interview / batch_interview / close_env
            match envelope.command.command_type {
                CommandType::CloseEnv => {
                    SimulationIPCServer::send_success(envelope, serde_json::Map::new());
                    break; // exit the wait loop
                }
                CommandType::Interview => {
                    // Execute interview: get agent from pool, call prepare_action with interview prompt
                    // ... teri-native interview execution ...
                    let result = execute_interview(&pool, &graph, &*llm, &envelope.command).await;
                    // send result as response
                }
                CommandType::BatchInterview => { /* similar */ }
            }
        }
        None => {
            // No command pending — yield briefly
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
ipc_server.stop();
```

The interview execution itself needs a teri-native `execute_interview` that calls
`agent.prepare_action` with an interview prompt. This is the "calling an LLM with a prompt and
returning a response" contract. The agent pool is already available in `run_sim_body`.

Note: the shutdown flag is currently NOT accessible inside `run_sim_body` — it is stored in
`RunHandle.shutdown` but not passed into the body. This needs a small refactor: thread the
`Arc<AtomicBool>` into `run_sim_body` (or add it to a new `RunBodyConfig` struct).

**Work items:**
1. Thread `shutdown: Arc<AtomicBool>` into `run_sim_body`
2. Implement `execute_interview(pool, graph, llm, command) -> Result<Map<String,Value>>`
   that calls the agent's LLM with the interview prompt and returns the text response
3. Implement the post-sim poll loop with `CloseEnv` / `Interview` / `BatchInterview` dispatch
4. Add parity tests for the wait-for-commands path (send Interview, receive response; send
   CloseEnv, verify exit)

### Cycle: Verify-only flips for U-028, U-029, U-030 [≠] symbols

After the IPC loop is implemented and parity-tested:

**U-028 flip sequence:**
1. Confirm verifier runs test for S-877b (wait-for-commands loop)
2. Verifier flips S-877 `[~]` → `[x]`
3. Run left-behind sweep on U-028 symbols — all remaining are [≠] documented
4. Flip U-028 unit → `[x]`

**U-029 flip sequence (verify-only):**
1. S-903 is already satisfying — verifier confirms `TimeActivationPolicy` covers reddit case
2. S-904 is a structural mirror of S-877; once S-877 flips, S-904 is verify-only
3. All other U-029 symbols are [≠] mirrors of U-028 [≠]s
4. Flip U-029 unit → `[x]` after U-028 flips (single verify pass needed)

**U-030 flip sequence:**
1. Owner decision on S-934 (dual-LLM boost config): approve as `[≠]` or port
2. Verify S-935 (`get_active_agents_for_round` = verify-only flip like S-903)
3. Verify S-938/S-939 producer (producer parity proven for twitter in C3b)
4. Parity test for S-940 parallel execution (both platforms' jsonl written correctly)
5. Flip U-030 unit → `[x]`

### Cycle: Owner decision — dual-LLM boost config (S-934)

The `use_boost` parameter in `create_model` allows reddit agents to use a different LLM API
(faster/cheaper) when `LLM_BOOST_API_KEY` is set. This is a performance optimization.

Decision options:
- **[≠]** (diverge): teri uses a single LLM client — simpler architecture, no parallelism
  benefit needed. Mark S-934 as intentional divergence.
- **Port**: add platform-specific `LlmClient` configuration to `RunInputs` or
  `SimulationRunner::start_simulation`, allowing different API configs per platform.

This decision does NOT block the IPC loop work. It can proceed in parallel.

---

## Critical Questions — Explicit Answers

### Q1: What of the run-loop is NOT ported in S-877/S-938 (twitter .run coroutine)?

**(a) Wait-for-commands/IPC poll loop after rounds complete:** UNPORTED. `run_sim_body` does
not enter a command service loop after `engine.run()` returns. Bucket 3 — genuinely unported.

**(b) `env.step()` world execution:** `[≠]`. OASIS `env.step()` mutates the simulation world
(agent feeds, social graph, DB writes). teri's `WorldState` is an accumulator, not an OASIS
world. The producer-stream records ARE ported; the world state mutation is `[≠]`.

**(c) Signal-shutdown integration:** SATISFIED. `SimEngine::run` cooperative stop at
`src/sim/mod.rs:933-937` + `terminate_handle` at `src/services/simulation_runner.rs:1576-1600`.

**(d) Anything else:**
- `ipc_handler.update_status("running")` / `"alive"` / `"stopped"` → satisfied by
  `ipc_server.start()` / `stop()` with the `AtomicBool` liveness flag.
- `env.reset()` initialization → `[≠]` (OASIS env setup).
- `generate_twitter_agent_graph()` → `[≠]` (OASIS agent graph init; teri uses `load_agent_pool`).
- `await self.env.step(initial_actions)` initial_posts world injection → `[≠]U028-OASIS-INTERNALS`
  (the records ARE produced by `SimEngine::run` round-0; the world mutation is `[≠]`).

### Q2: Are S-903/S-935 byte-identical to S-876 (TimeActivationPolicy)?

**YES — confirmed.**

Reading Python sources:
- S-876 (`TwitterSimulationRunner._get_active_agents_for_round`, `run_twitter_simulation.py:462`)
- S-903 (`RedditSimulationRunner._get_active_agents_for_round`, `run_reddit_simulation.py:469`)  
- S-935 (`get_active_agents_for_round`, `run_parallel_simulation.py:1040`)

All three have the same `time_config` key reads, same multiplier logic, same per-agent
`active_hours`/`activity_level` gating, same `random.sample(candidates, min(target, len))` call.
S-935 only differs in receiving `config` as a direct parameter rather than `self.config`.

`TimeActivationPolicy` in `src/sim/activation.rs` covers all three:
- `from_config(config)` reads the same keys
- `active_agents(hour)` implements the same algorithm
- `ActivationPolicy::active_agent_ids(tick)` calls `simulated_hour(tick)` then `active_agents(hour)`

S-903 and S-935 are **verify-only flips**: no new porting work, only a confirmation test run.

### Q3: Can U-029 flip purely as verify-only?

**YES, after U-028's IPC loop gap is filled.**

U-029 is a structural mirror of U-028. Every non-[≠] symbol in U-029 either:
- Matches a U-028 symbol already satisfied by teri substrate (CommandType, signal handlers,
  producer stream, get_active_agents_for_round)
- Is a [≠] mirror of a U-028 [≠] symbol

Once the IPC command service loop is implemented (gating U-028), U-029 requires only:
1. A single verification pass confirming the reddit platform's `actions.jsonl` is written
   correctly (S-939/S-904 producer parity)
2. S-903 verify-only flip (already proven structurally by the S-876 parity note "reddit≡twitter diffed")
3. A wait-for-commands interview execution test for the reddit case

No reddit-specific unported behavior exists (the initial_posts multi-action edge case is handled
by the existing round-0 code).

---

## Summary of New [≠] Tags

| Tag | Scope | Description |
|-----|-------|-------------|
| `U028-LOGGING` | S-853/854/855/856/857/858, S-880-885, S-907/908/909/910 | OASIS Python logging infrastructure (UnicodeFormatter, MaxTokensWarningFilter, setup_oasis_logging, disable_oasis_logging, init_logging_for_simulation). teri uses tracing; no OASIS loggers. |
| `U028-SUBPROCESS-RUNNER` | S-869-875, S-896-902, S-911/912, S-934, S-936/937, S-878, S-905 | Class/function artifacts that exist only in the subprocess model (runner class, AVAILABLE_ACTIONS, entrypoint, OASIS model creation, PlatformSimulation container). Teri's in-process lifecycle covers the observable contract. |
| `U028-SUBPROCESS-IPC` | S-860-864, S-887-891, S-914-919 | Filesystem-based IPC transport (ipc_commands/ dirs, env_status.json, mtime-ordered scan). Replaced by mpsc+oneshot in-process channel. Already partially documented; this tag consolidates all three scripts' copies. |
| Existing `U028-OASIS-INTERNALS` | S-867, S-874, S-894, S-901, S-923, S-929-933 | OASIS SQLite trace DB reads (_get_interview_result, _get_post_info, _get_user_name, _get_comment_info, fetch_new_actions_from_db, _enrich_action_context). Already tagged; confirms S-929-933 fold under it. |
