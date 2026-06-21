# U-024 sub-cycle (g2) — `ReportConsoleLogger` → teri logging substrate

**Unit:** U-024 `ReportAgent`, sub-cycle (g2) `ReportConsoleLogger`
**Source X:** `MiroFish/backend/app/services/report_agent.py:307-388` (`class ReportConsoleLogger`)
**Dest Y:** `teri` (branch `port/mirofish`, worktree `.worktrees/mirofish-port/teri`)
**Type:** runtime-construct mapping (Python global-logger `FileHandler` → tracing substrate). Design-before-port.
**Class:** `port-fresh` (no teri equivalent of per-report capture exists).
**Contractual?** YES — `console_log.txt` content is read back by `ReportManager.get_console_log` /
`get_console_log_stream` (ported in sub-cycle f) and surfaced to the frontend. No `[≠]` is available here:
the content is observable. A `[≠]` would be a disguised feature-skip and would FAIL the parity gate.

---

## 0. What X actually does (ground truth)

`ReportConsoleLogger(report_id)`:
- Opens `{UPLOAD_FOLDER}/reports/{report_id}/console_log.txt` (append mode, utf-8), `mkdir -p` the dir.
- Attaches **one** `logging.FileHandler` (level **INFO**, formatter `'[%(asctime)s] %(levelname)s: %(message)s'`,
  datefmt `'%H:%M:%S'`) to **two named module loggers**: `'mirofish.report_agent'` and `'mirofish.zep_tools'`.
- `close()`/`__del__` detach the handler from those loggers and close the file.

Net effect: for the duration of one report run, the INFO+ output of those two modules is **tee'd** into a
per-report plain-text file. The handler is INFO-level, so **DEBUG is excluded** (e.g. `report_agent.py:1322`
`logger.debug("LLM响应…")` is NOT captured). WARNING/ERROR are captured (they are ≥ INFO).

### The Python module logger surface that feeds it
`report_agent.py:33` `logger = get_logger('mirofish.report_agent')` — a single module-level logger. **Every**
`logger.info/warning/error` in the file emits on `mirofish.report_agent` and is therefore captured (when
≥ INFO). The full leveled inventory (line → level → i18n key) is in §4. `mirofish.zep_tools` is the *other*
captured target — see §3 for its teri mapping.

---

## 1. THE MAPPING — chosen: **(a) a per-report `tracing_subscriber` Layer gated on a shared sink**

> Option (a) — a `tracing_subscriber` per-report file **Layer** (a `MakeWriter` to `console_log.txt`), gated on
> an active-report context via a process-global `Arc<Mutex<Option<ReportConsoleSink>>>` that the layer checks
> each event; `ReportConsoleLogger::new` installs the sink, `Drop`/`close` clears it. Filtered to the
> report/zep targets at INFO+, formatted as `'[%H:%M:%S] LEVEL: message'`. The **layer is installed once at
> startup**; the **sink is toggled per report**.

### Why (a), not (b) or (c)

- **(c) `[≠]` is illegal here.** `console_log.txt` is contractual (read back + surfaced to frontend). The
  content is faithfully producible. `[≠]` would be a disguised feature-skip → parity-gate FAIL. Rejected.
- **(b) "pass a writer down and write directly"** preserves the file content but **diverges from Python's
  global-logger model** and, critically, **cannot capture `mirofish.zep_tools`** output without threading the
  writer through every tool/service call site — exactly the global-capture behavior Python gets for free.
  It also double-maintains the message strings (the port already emits the same i18n messages as `tracing`
  events for the agent log; (b) would re-emit them by hand). Higher coupling, drift risk, narrower capture.
  Rejected as a downgrade of the *capture scope*.
- **(a) preserves the exact Python semantics**: "attach a sink to named module loggers for the duration of a
  report." teri's `tracing` targets ARE the named loggers (this is already documented in
  `src/logging.rs:20-24` — `setup_logger(name)` ↔ `tracing::…(target: "name", …)`). A `Layer` that filters by
  `target ∈ {teri::report, teri::services::zep_tools}` at INFO+ and writes to the per-report sink is the
  literal one-to-one of "FileHandler attached to those two named loggers at INFO." It captures globally
  (any event on those targets, from anywhere) — matching Python — with **no call-site threading**.

### The no-downgrade justification
(a) reproduces: (i) the file path + append/utf-8 semantics, (ii) the INFO-level filter, (iii) the exact
two-target capture scope, (iv) the `'[%H:%M:%S] LEVEL: message'` line format, (v) global capture (events from
the zep tools layer too), (vi) start/stop lifecycle tied to the report. Nothing in X is dropped.

### Does it require teri to ALSO emit the loop's `logger.info/warning` lines as tracing events?
**YES — this is the load-bearing part.** `console_log.txt` is only non-empty if teri actually emits these
events on the captured target. The agent-log (g1/d/e) sink is a *separate* structured JSONL channel and does
**not** feed the console layer. So (g2) must wire the console message emissions as `tracing` events with
`target: "teri::report"`. The `// (g2):` marker at `mod.rs:1034-1036` is the canonical example. Full list: §4.
**These tracing emissions are independent of `report_logger` being `Some`/`None`** — they must fire
unconditionally (Python's module `logger` always emits; capture is what's gated). Do NOT guard them behind
`if let Some(l) = self.report_logger`.

### Does it require a startup subscriber-init change?
**YES — exactly one, additive, non-breaking.** `src/logging.rs::init_logging` currently composes a console
layer (+ optional rotating-file layer) and calls `.init()`. The per-report console layer must be **added to
the registry at startup** (it is dormant when the sink is `None`). Concretely:

- Add a third composed layer in `init_logging` (in BOTH the `TERI_LOG_DIR`-set and console-only arms) built
  from a `ReportConsoleLayer` that reads a process-global sink handle.
- The sink handle is a `static` (e.g. `once_cell`/`std::sync::OnceLock<Arc<Mutex<Option<ReportConsoleSink>>>>`,
  or a `LazyLock`) so `ReportConsoleLogger` can toggle it without re-initializing the subscriber (which would
  panic — `init()` is once-per-process).
- The console-only arm currently uses the terse `fmt().with_env_filter(…).init()` builder; it must be
  converted to the `registry().with(console_layer).with(report_console_layer).init()` form so a second layer
  can be attached. This is a mechanical refactor of `logging.rs:125-128`; behavior of the existing console
  output is unchanged (same filter, target, level).

This is the only init change. It does not alter existing log output. It is safe because the report-console
layer is a no-op whenever the sink is `None` (i.e. outside a report run) — which is the default.

---

## 2. THE FORMAT — `'[%H:%M:%S] LEVEL: message'`

Python: `logging.Formatter('[%(asctime)s] %(levelname)s: %(message)s', datefmt='%H:%M:%S')`.

- `%(asctime)s` with `datefmt='%H:%M:%S'` → **local wall-clock time-of-day**, zero-padded `HH:MM:SS`, **no
  date, no fraction, no timezone**. teri MUST use **local** time (not UTC) to match: use
  `chrono::Local::now()` (already a dep, `Cargo.toml:79`; already used by `models/project.rs:33,51`
  `python_isoformat_local`). Format string: `now.format("%H:%M:%S")` → e.g. `14:07:33`.
  **Do NOT reuse `python_isoformat_local()`** — that emits a full ISO datetime; the console format is
  time-of-day only. Use a fresh `chrono::Local::now().format("%H:%M:%S")`.
- `%(levelname)s` → Python uppercase level names: `INFO`, `WARNING`, `ERROR`, `CRITICAL`, `DEBUG`. teri's
  `tracing::Level` Display gives `INFO`/`WARN`/`ERROR`/`DEBUG`/`TRACE`. **`WARN` ≠ `WARNING`** — Python writes
  `WARNING`. The layer MUST map `tracing::Level` → Python level string:
  `ERROR→"ERROR"`, `WARN→"WARNING"`, `INFO→"INFO"`, `DEBUG→"DEBUG"`, `TRACE→"DEBUG"` (DEBUG/TRACE are filtered
  out anyway by the INFO floor, but map them for completeness). This level-name mapping is contractual (the
  frontend renders these strings).
- `message` → the **fully-formatted, i18n-resolved** event message (the `t(...)`/`t_args(...)` output), with no
  target/span fields. The layer must render ONLY the event's message field, not tracing's default
  `target: msg` decoration. Build a minimal custom `Layer` visitor that extracts the `message` field.
- Line terminator: one `\n` per record (Python `FileHandler` appends the formatter's terminator `\n`).

Exact output shape per line: `[HH:MM:SS] LEVEL: <message>\n`  e.g.
`[14:07:33] INFO: ReACT generating section: Market Overview\n`.

---

## 3. CAPTURE SCOPE — targets + level

Python captures `mirofish.report_agent` + `mirofish.zep_tools` at **INFO+**.

| Python named logger      | teri tracing `target`            | Notes |
|--------------------------|----------------------------------|-------|
| `mirofish.report_agent`  | `teri::report`                   | The report module. ALL §4 emissions use this target. |
| `mirofish.zep_tools`     | `teri::services::zep_tools`      | The Zep tools service. **Not yet ported in teri** (`src/services/` has no `zep_tools`). |

**Level filter:** the layer's per-event check passes only events at **INFO, WARN, ERROR** (i.e. `<= INFO` in
tracing's ordering). DEBUG/TRACE are excluded — matching `FileHandler.setLevel(INFO)`. (So `report_agent.py:1322`
`logger.debug(…)` correctly produces NO console line; the port must keep that emission at `tracing::debug!` so
it stays excluded.)

**Target filter:** the layer passes only events whose `metadata().target()` is exactly `"teri::report"` or
`"teri::services::zep_tools"` (or a child like `teri::services::zep_tools::…` — match by prefix on
`teri::services::zep_tools` to be safe, exact on `teri::report`). Events from any other target are ignored even
when the sink is active — matching Python's per-logger attachment (it does NOT capture the root logger).

**zep_tools handling (de-risk):** `teri::services::zep_tools` does not exist yet. This is NOT a blocker for
(g2): the layer simply lists it as a captured target now, and zep-tools events flow into the console file
automatically once that module emits `tracing` events on that target. (g2) should:
- Register `teri::services::zep_tools` as a captured target **now** (zero cost; no events until the module
  exists).
- The porter MUST NOT invent zep_tools emissions in (g2). They land when the zep-tools service is ported.
  Record this as a forward-dependency note in the unit's parity row: "console capture of `zep_tools` target is
  wired; emissions arrive when `teri::services::zep_tools` is ported." This is a wiring-ready seam, not a
  downgrade — the capture scope is faithfully reproduced; the *producer* is a separate unit.

---

## 4. THE LOOP LOG LINES — `tracing` events (g2) must wire

These are the `mirofish.report_agent` (`teri::report`) emissions that must be emitted as `tracing` events for
`console_log.txt` to match Python. **Level + i18n key are contractual** (level → `LEVEL:` token in the file;
message → i18n-resolved text). All i18n keys below **already exist** in `src/i18n/locales/{en,zh}.json`.
Emit with `tracing::<level>!(target: "teri::report", "{}", crate::i18n::t_args("report.<key>", &[...]))`.

Within `generate_section_react` / `plan_outline` / the tool-exec path (the scope of (g2)/the marker at
`mod.rs:1034`), the required emissions:

| Py line | level   | i18n key                          | args                                   | teri site (current) |
|---------|---------|-----------------------------------|----------------------------------------|---------------------|
| 1249    | INFO    | `report.reactGenerateSection`     | `title`                                | start of `generate_section_react` |
| 1313    | WARNING | `report.sectionIterNone`          | `title`, `iteration`(=iter+1)          | LLM-None branch |
| 1322    | DEBUG   | (`f"LLM响应: {response[:200]}…"`)  | n/a (literal, NOT i18n)                | keep at `tracing::debug!` — excluded by INFO floor; do NOT promote |
| 1332    | WARNING | `report.sectionConflict`          | `title`, `iteration`(=iter+1), `conflictCount` | conflict branch |
| 1352    | WARNING | `report.sectionConflictDowngrade` | `title`, `conflictCount`               | 3rd-conflict downgrade |
| 1393    | INFO    | `report.sectionGenDone`           | `title`, `count`(=tool_calls_count)    | section-done branch |
| 1421    | INFO    | `report.multiToolOnlyFirst`       | `total`(=tool_calls.len()), `toolName`(=call.name) | **`mod.rs:1034` marker** — inside `if tool_calls.len() > 1` |
| 1490    | INFO    | `report.sectionNoPrefix`          | `title`, `count`(=tool_calls_count)    | no-Final-Answer-prefix branch |
| 1503    | WARNING | `report.sectionMaxIter`           | `title`                                | max-iterations branch |

Plus the `plan_outline` / agent-init / tool-exec emissions on the same target that fall inside the report run
and must appear in the console file (Python lines 917, 968, 1027, 1044, 1061(ERROR), 1152, 1205, 1209(ERROR)):

| Py line | level | i18n key                       | args                              |
|---------|-------|--------------------------------|-----------------------------------|
| 917     | INFO  | `report.agentInitDone`         | `graphId`, `simulationId`         |
| 968     | INFO  | `report.executingTool`         | `toolName`, `params`              |
| 1027    | INFO  | `report.redirectToQuickSearch` | (none)                            |
| 1044    | INFO  | `report.redirectToInsightForge`| (none)                            |
| 1061    | ERROR | `report.toolExecFailed`        | `toolName`, `error`               |
| 1152    | INFO  | `report.startPlanningOutline`  | (none)                            |
| 1205    | INFO  | `report.outlinePlanDone`       | `count`(=sections.len())          |
| 1209    | ERROR | `report.outlinePlanFailed`     | `error`                           |

> Note on scope: the lines in the §4 second table belong to `plan_outline` / agent-init / `execute_by_name`
> wiring, several of which are already partly present as port sites. (g2) owns wiring the **tracing emission**
> at every one of these sites on `target: "teri::report"` at the listed level. Sites that the porter finds are
> already emitting an equivalent `tracing` event need only target/level verification, not a second emission.
> Sites later than the report run (1628+, save/assemble/delete) are also on `teri::report`; they are captured
> too if they fire while the sink is active — wire them with the same target when their port lands, but the
> **(g2)-critical** set for the parity fixture is the `generate_section_react`/`plan_outline` set above.

**Verification floor:** the parity fixture only needs to assert the *captured-during-run* subset; the porter
must ensure every §4 emission uses `target: "teri::report"` and the correct level so capture is faithful.

---

## 5. VERIFIABILITY (timestamps are non-deterministic)

The leading `[HH:MM:SS]` is wall-clock and non-deterministic, so the verifier asserts **structure, level, and
which messages appear — never the timestamp value**. Recommended differential checks against the X-produced
`console_log.txt`:

1. **Line-shape regex** — every non-empty line matches
   `^\[\d{2}:\d{2}:\d{2}\] (INFO|WARNING|ERROR|DEBUG): .+$`. (Asserts format + that the level token is a
   Python-style name, catching the `WARN`-vs-`WARNING` bug.)
2. **Level-name parity** — assert the file contains `WARNING:` (not `WARN:`) for the warning lines, proving the
   `tracing::Level → "WARNING"` mapping is correct.
3. **Message-set parity (timestamp-stripped)** — strip the `^\[\d{2}:\d{2}:\d{2}\] ` prefix from each line and
   compare the resulting `LEVEL: message` multiset (or ordered list, since the loop is deterministic given
   fixed inputs) between Python and teri. Run both X and teri over the **same fixed report input** (mock LLM /
   recorded transcript so the loop branches deterministically) and diff. The i18n-resolved messages must match
   byte-for-byte in the chosen locale (assert under `en` to avoid locale ambiguity, then spot-check `zh`).
4. **Capture-scope negative** — assert a DEBUG line (`LLM响应…`, Py:1322) does **NOT** appear (proves the INFO
   floor), and that an event on a non-captured target (e.g. `teri::request` from `server.rs`) does NOT leak in.
5. **Lifecycle** — assert the file is empty/non-existent before `ReportConsoleLogger` is installed and that it
   stops growing after `close()`/drop (toggle-off works). A unit test: install sink → emit on `teri::report`
   → line appears; drop → emit again → no new line.

Because timestamps differ, (3) is the core differential and must operate on the timestamp-stripped projection.

---

## 6. ORDERED PORT STEPS for (g2)  (+ what folds into (h))

**(g2) steps — in `src/logging.rs` + a new `src/report/console_logger.rs` + emission wiring in `src/report/mod.rs`:**

1. **Sink type + global handle** (`logging.rs` or `console_logger.rs`): define `ReportConsoleSink` holding an
   open append `std::fs::File` to `console_log.txt` (+ the report_id for diagnostics). Define a process-global
   `static REPORT_CONSOLE_SINK: OnceLock<Arc<Mutex<Option<ReportConsoleSink>>>>` (init to `None`).
2. **The Layer** (`logging.rs`): implement `ReportConsoleLayer` (`impl<S> Layer<S>`). `on_event`: (a) check the
   global sink is `Some`; (b) check `event.metadata().level() <= Level::INFO`; (c) check `target` ∈
   {`teri::report`, prefix `teri::services::zep_tools`}; (d) extract the `message` field via a field visitor;
   (e) format `[{HH:MM:SS}] {PYLEVEL}: {message}\n` using `chrono::Local::now()` and the level-name map; (f)
   write+flush to the sink file under the mutex. Ignore write errors (Python silently ignores; mirror
   `ReportLogger::log`'s non-fatal write).
3. **Wire the layer into `init_logging`** (`logging.rs:101-132`): convert the console-only arm to the
   `registry().with(console_layer).with(report_console_layer).init()` form; add `report_console_layer` to the
   file-arm registry too. Initialize `REPORT_CONSOLE_SINK` to `Some(Arc::new(Mutex::new(None)))`. No change to
   existing console/file output behavior.
4. **`ReportConsoleLogger` struct** (`console_logger.rs`): `new(report_id, upload_folder)` → `mkdir -p`
   `{upload_folder}/reports/{report_id}`, open `console_log.txt` append/utf-8, store the `ReportConsoleSink`
   into the global handle (`*sink.lock() = Some(...)`). `close(&mut self)` and `Drop` → `*sink.lock() = None`
   and drop the file. (Mirror Python `__init__`/`close`/`__del__`.) **Guard against re-entry**: if a sink is
   already present, replacing it matches Python's "avoid duplicate handler" intent closely enough; document it.
5. **Wire the §4 tracing emissions** in `src/report/mod.rs`: at the `// (g2):` marker (`mod.rs:1034-1036`) emit
   `tracing::info!(target: "teri::report", "{}", t_args("report.multiToolOnlyFirst", &[("total", &tool_calls.len()), ("toolName", &call.name)]))`;
   and at every other §4 site emit the listed level/key/args on `target: "teri::report"`. These are
   **unconditional** (not gated on `report_logger`). Verify the DEBUG line (1322) stays `tracing::debug!`.
6. **Tests** (`console_logger.rs` + a differential fixture): the §5 checks — line-shape regex, `WARNING` not
   `WARN`, timestamp-stripped message-set parity over a deterministic fixture, INFO-floor negative, and the
   install/emit/drop lifecycle test.

**Folds into (h) (`generate_report` creates/closes the console_logger):**
The **construction + close lifecycle wiring at the report-run boundary** belongs to (h), where
`generate_report` (Python `report_agent.py:1582-1590` instantiates `ReportLogger` + `ReportConsoleLogger`,
and `:1736/:1762` set `console_logger = None` on completion/error) constructs `ReportConsoleLogger::new(...)`
at the top of the run and ensures `close()`/drop at the end (success AND error paths). (g2) delivers the
**type, layer, init wiring, and the emissions**; (h) delivers the **per-run instantiate/teardown** call. Keep
(g2) self-contained and parity-verifiable via a unit-level install/emit/drop test (step 6) so it does not
depend on (h) landing first.

---

## Risk flags for the no-downgrade gate
- `- [≠]` NONE legal here — console_log.txt is contractual; (c) was rejected on the `[≠]` bar.
- `- [!]` forward-dep: `teri::services::zep_tools` capture target is wired but produces no lines until the
  zep-tools service is ported (separate unit). NOT a (g2) blocker; recorded as a wiring-ready seam.
- Contractual details that WILL FAIL the gate if missed: (1) `WARNING` vs `WARN` level name; (2) local (not
  UTC) time; (3) INFO floor excluding the DEBUG line; (4) two-target (not root) capture; (5) i18n-resolved
  message text matching X byte-for-byte under a fixed locale.
