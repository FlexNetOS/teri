# U-024 sub-cycle (h) — `generate_report` orchestration + `ReportSink` design

Architect decision for the **top-level ReACT report pipeline** (`ReportAgent.generate_report`,
`report_agent.py:1532-1765`) and the **`ReportSink` streaming-unification seam** (deferred here from
the master decision §3d/§5, item 5).

Grounds in the landed (a)–(g) deliverables (`src/report/mod.rs`, `manager.rs`, `logger.rs`,
`console_logger.rs`) and teri's existing SSE substrate (`src/api/mod.rs::TickStreamEvent`,
`src/api/streaming.rs`). No-downgrade is **bidirectional**: preserve MiroFish's
plan→per-section→assemble streaming pipeline AND teri's existing template `generate_stream` SSE path
(both already coexist on `ReportAgent` per master §1).

---

## 0. Key facts established by grounding (READ, not assumed)

- **uuid available** (`Cargo.toml:76` `uuid = { version = "1", features=["v4","serde"] }`); teri already
  uses `Uuid::new_v4()` in `mod.rs`. Python uses `f"report_{uuid.uuid4().hex[:12]}"` — a 12-hex-char
  suffix, NOT a full UUID string. **Port the exact shape** (see §2).
- **`async_stream` + `futures::Stream` are in use** (`mod.rs:10-11`); teri's `generate_stream` returns
  `Pin<Box<dyn Stream<Item=Result<PredictionReport>> + Send>>`. **tokio broadcast** is teri's existing
  multi-subscriber SSE substrate (`src/api/streaming.rs` `StreamAdapter` bridges `SimEngine`'s broadcast
  channel → `TickBuffer` → axum SSE). **The ReACT report stream is single-consumer** (one HTTP client
  per report run) — broadcast's fan-out is not needed here; see §1 decision.
- **`TickStreamEvent { tick, data, event_id }`** (`api/mod.rs:56`) is teri's uniform SSE wire shape, with
  the established sentinel-in-`data` convention (`lag_gap`, `sim_end`). U-027's report SSE route should
  reuse this shape family — the ReportSink event maps onto it 1:1 (see §1).
- **`ReportManager` is INSTANCE-based** (`manager.rs:50` `pub struct ReportManager { reports_dir }`,
  `new(upload_folder)`), unlike Python's `@classmethod`s. `ensure_report_folder` is **private**
  (`manager.rs:113 fn ensure_report_folder`). save_*/update_progress/assemble all take `&self`. So (h)
  threads a `&ReportManager` (or owns one) — see §2.
- **`ReportLogger::new(report_id, upload_folder)`** and **`ReportConsoleLogger::new(report_id,
  upload_folder)`** both take `upload_folder: &Path` and build `{upload_folder}/reports/{id}/…`. Both
  already mkdir the dir. So (h)'s "ensure folder" is **already covered** by logger construction +
  `manager.save_*`; the explicit Python `ReportManager._ensure_report_folder(report_id)` first-call maps
  to either a new public `ReportManager::ensure_report_folder` OR is implicitly satisfied by the first
  `save_report`/logger construction. **Decision: add `pub fn ensure_report_folder` to ReportManager**
  (make the existing private fn public) so the explicit Python ordering is preserved 1:1 and the folder
  exists before `update_progress`/`save_report`. Zero blast radius (instance method, internal).
- **`report_logger` is an `Option<ReportLogger>` field on `ReportAgent`** (`mod.rs:544`), already wired
  for the 7 section-loop log points in (e). **`console_logger` is NOT yet a field** — it must be added
  in (h) (see §2/§5). Console capture is process-global (`REPORT_CONSOLE_SINK`) toggled by the
  `ReportConsoleLogger` guard's lifetime; (h) holds the guard for the run and drops it at the end.
- **`update_progress` PARITY BUG (h-blocker, `- [!]`):** teri narrowed `progress` to **`u32`**
  (`manager.rs:400`), but Python's signature is `progress: int` and the **failed path writes `-1`**
  (`report_agent.py:1753` `update_progress(report_id, "failed", -1, …)`). `u32` **cannot represent -1**
  — `progress.json` would diverge (it would need a cast that changes the value). See §5/§6: (h) MUST
  widen `ReportManager::update_progress`'s `progress` param to **`i32`** (`Value::Number(progress.into())`
  works for `i32`). Low blast radius — `update_progress` has no callers yet outside (f)'s own tests.

---

## 1. ReportSink design — the unification seam

### Decision: a `ReportSink` **trait** with a single `event(&ReportEvent)` method, NOT a channel and NOT an `async_stream` baked into `generate_report`.

`generate_report` takes `sink: &dyn ReportSink` (or `&mut dyn ReportSink`) and calls `sink.event(&e)` at
every progress milestone. The **three concrete consumers fan out behind the trait**, not inside the
orchestration:

```rust
// src/report/sink.rs  (new module, sub-cycle h1)

/// One progress/lifecycle event emitted by `generate_report`.
/// Mirrors Python's progress_callback(stage, progress, message) PLUS the
/// structured fields the jsonl + SSE consumers need (so one event feeds all sinks).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReportEvent {
    pub stage: ReportStage,        // pending|planning|generating|completed|failed
    pub progress: i32,             // 0..100, or -1 on failure (matches Python int)
    pub message: String,           // i18n-resolved, == progress_callback msg
    /// Section-scoped context when the event is about a section (else None).
    pub section_title: Option<String>,
    pub section_index: Option<usize>,
    /// On a `SectionComplete` event, the freshly-generated section markdown
    /// (the save-immediately payload — see §3). None otherwise.
    pub section_content: Option<String>,
    pub report_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStage { Pending, Planning, Generating, Completed, Failed }

/// A consumer of report progress events. `generate_report` emits to ONE sink;
/// fan-out (jsonl + SSE + console) is the sink implementation's concern.
pub trait ReportSink: Send {
    fn event(&mut self, ev: &ReportEvent);
}
```

**Why a trait (over a channel / async_stream):**

1. **No-downgrade of Y's `generate_stream`:** teri's template path returns its OWN
   `Stream<Item=Result<PredictionReport>>` and is byte-stable + test-locked. A `ReportSink` trait is
   *orthogonal* — `generate_report` is a separate entry; it does not touch `generate_stream`'s
   signature or its async_stream body. (A shared channel/stream would tempt collapsing the two paths →
   the §1-refused downgrade.)
2. **Maps `progress_callback` 1:1.** Python's `generate_report` threads
   `progress_callback(stage, progress, message)` down into `plan_outline` and
   `_generate_section_react`. teri's (d)/(e) already accept `progress: Option<&ProgressCallback>` where
   `ProgressCallback = dyn Fn(&str, u32, &str)`. **The ReportSink does not replace that callback** — it
   *is the thing the callback closes over*. (h) builds a closure `|stage, pct, msg| sink.event(&…)` and
   passes it as the `progress` arg to `plan_outline`/`generate_section_react` exactly as Python does. So
   the existing (d)/(e) signatures are **unchanged** (no re-port of e).
3. **Fan-out is trivially composable + testable.** The default production sink is a small struct holding
   `Option<&mut ReportLogger>` + the SSE emitter; a `Vec<Box<dyn ReportSink>>` / tee sink fans out. A
   **NullSink** (`impl ReportSink { fn event(&mut self,_){} }`) is the parity-test default so (h) is
   driver-testable with zero I/O. A channel would force every test to spin a receiver task; a trait does
   not.
4. **The jsonl + console sinks are NOT driven through `ReportEvent`** — they are *already* wired:
   `ReportLogger` is called at its own typed log points (`log_start`, `log_planning_*`,
   `log_section_full_complete`, `log_report_complete`, `log_error`) which carry richer structured fields
   than a flat progress event; and the console log is captured passively via the `tracing` layer (g2).
   **So ReportSink's job is narrow: it is the progress/SSE surface** (the `progress_callback` replacement
   + the U-027 HTTP stream), while the jsonl/console sinks stay on their existing typed seams. This is
   the correct decomposition — do NOT route the structured jsonl through `ReportEvent` (it would lose
   the per-helper `details` key-order contract that (g1) parity-locked).

### How the three consumers fan out (the unification, concretely)

| Consumer | Driven by | Seam |
|----------|-----------|------|
| (i) `agent_log.jsonl` | typed `ReportLogger.log_*` calls wired in (h) | `self.report_logger` field (already `Option`) |
| (ii) `console_log.txt` | passive `tracing` layer (g2) | `ReportConsoleLogger` guard held for the run |
| (iii) HTTP SSE (U-027) | `ReportSink::event` | a `ChannelSink`/`SseSink` impl the route owns |

The **single event stream** the prompt asks for is the sequence of `progress_callback` invocations
(planning 0→100, generating per-section, completed 100 / failed -1). `ReportSink` *is* that stream's
abstraction. U-027's route supplies the sink that forwards each `ReportEvent` to its axum SSE response
(it can wrap a `tokio::sync::mpsc::Sender<ReportEvent>` inside a `ReportSink` impl — the channel lives
in the ROUTE, not in `generate_report`, keeping the core sync-callable + test-friendly).

### `[≠]` / decision flags on ReportSink

- **NOT a `[≠]`.** ReportSink ADDS a capability (a structured progress stream) without removing any.
  Python's `progress_callback` is a plain `Callable[[str,int,str],None]`; teri's `ReportEvent` is a
  superset (adds section_title/index/content/report_id) — a **strict superset the dest provides**, legal.
- **U-027 route shape decision (flag to merge-integrator/U-027):** the report SSE route should emit each
  `ReportEvent` as a `TickStreamEvent`-shaped frame (reuse teri's uniform `{ event_id, data }` wire
  convention) — recommend `event_id = format!("report-{stage}-{progress}")`, `data = serde_json(ev)`.
  This mirrors MiroFish's `/api/report` progress SSE. Recorded as the master §5 item-5 decision: the
  ReACT report route exposes the **per-section progress stream**, NOT the template `PredictionReport`
  chunks. (The template `generate_stream` keeps its own route.)

---

## 2. `generate_report` signature + state threading

### Signature

```rust
impl ReportAgent {
    /// Port of `ReportAgent.generate_report(progress_callback, report_id)`
    /// (report_agent.py:1532-1765).
    ///
    /// `&mut self` — generate_report MUTATES self (sets self.report_logger and the
    /// new self.console_logger guard, matching Python's `self.report_logger = ReportLogger(...)`).
    /// `manager` — the instance-based ReportManager rooted at upload_folder.
    /// `tools`/`llm` — bound per-call (DECISION-11), threaded into plan_outline + section loop.
    /// `sink` — the progress/SSE surface (NullSink in tests).
    /// `report_id` — Option; None → auto-generate `report_{uuid12}`.
    pub async fn generate_report<L: LlmClient>(
        &mut self,
        tools: &ReportTools<'_, L>,
        llm: &L,
        manager: &ReportManager,
        sink: &mut dyn ReportSink,
        report_id: Option<String>,
    ) -> Report;
}
```

- **`&mut self`** — Python assigns `self.report_logger` and `self.console_logger` inside the method.
  teri's `report_logger` field is already `Option<ReportLogger>`; (h) sets it here. **Add a
  `console_logger: Option<ConsoleGuard>` field** (see below). `&mut self` is the faithful mapping; the
  template assoc-fns are untouched (they don't take `self`).
- **No `progress_callback` param** — replaced by `sink` (§1). (h) constructs the closure that the (d)/(e)
  methods still want from `sink`.

### `report_id` generation (exact shape)

```rust
let report_id = report_id.filter(|s| !s.is_empty()).unwrap_or_else(|| {
    let hex = uuid::Uuid::new_v4().simple().to_string(); // 32 lowercase hex, no dashes
    format!("report_{}", &hex[..12])                     // == Python uuid4().hex[:12]
});
```
Python: `f"report_{uuid.uuid4().hex[:12]}"`. `Uuid::simple()` gives the dashless hex; first 12 chars
match `.hex[:12]`. **`- [!]` determinism:** the id is random → not byte-reproducible. Parity tests MUST
pass an explicit `report_id` (the `Option` is exactly for this) and assert the auto-gen path only by
**shape** (`^report_[0-9a-f]{12}$`), never value. Same posture as logger's nondeterministic
`elapsed_seconds`.

### console_logger lifecycle

Add to `ReportAgent`:
```rust
/// Active console-capture guard for the current run. Dropping it detaches the
/// tracing sink (g2). Held only for the duration of generate_report.
console_logger: Option<console_logger::ReportConsoleLogger>,
```
- Python sets `self.console_logger = ReportConsoleLogger(report_id)` at start and calls
  `.close()` + sets `None` in BOTH the success and except tails. teri: construct it into
  `self.console_logger`, and on every return path call `.close()` (or rely on `Drop` — the guard's
  `Drop` already calls `close()`, idempotent). **Decision: explicit `.close()` on both tails for
  faithful ordering, plus `Drop` as the safety net.** Set the field back to `None` after close.
- `new()`/`new_react()` initialize `console_logger: None`.

### Status machine (preserve exactly)

`Pending` (initial Report) → `Planning` (before plan_outline) → `Generating` (before the section loop)
→ `Completed` (after assemble, set `completed_at`) on success; → `Failed` (set `error`) in the `catch`.
Each transition mirrors a `manager.update_progress(report_id, "<status>", <pct>, <msg>, …)` + the
matching `ReportLogger` typed call + a `sink.event`. The exact progress percentages are **contractual**
(they hit `progress.json`): pending 0, planning 5, outline-done 15, per-section
`20 + (i/total)*70`, section-done `base + 70/total`, assembling 95, completed 100, failed -1. **Port the
integer arithmetic verbatim** (Python `int((i/total)*70)` — use `((i as f64/total as f64)*70.0) as i32`
with the same truncation). The nested `progress_callback` rescaling in Python (planning `prog//5`,
section `base + int(prog*0.7/total)`) is also contractual — port the closures verbatim into the sink
adapters passed to plan_outline/section.

---

## 3. Save-section-immediately streaming (preserve)

Python's loop, per section: generate → `section.content = content` → append to `generated_sections`
context (`f"## {title}\n\n{content}"`) → **`ReportManager.save_section(report_id, section_num, section)`
immediately** → `completed_section_titles.append` → `log_section_full_complete` → `update_progress`. The
report file `section_{NN}.md` lands the moment the section is done, before the next begins.

**Mapping (no buffering):**
1. `let content = self.generate_section_react(&section, &outline, &generated_sections, tools, llm,
   Some(&section_progress_closure), section_num).await;`
2. `section.content = content.clone();`
3. `generated_sections.push(format!("## {}\n\n{}", section.title, content));` (the 4000-char truncation
   for the NEXT section's context is handled INSIDE generate_section_react already — (e) does it).
4. `manager.save_section(report_id, section_num, &section)?;` ← **the immediate write.**
5. `completed_section_titles.push(section.title.clone());`
6. `if let Some(l) = &self.report_logger { l.log_section_full_complete(&section.title, section_num,
   &format!("## {}\n\n{}", section.title, content).trim()); }` (note Python `.strip()`).
7. `sink.event(&ReportEvent{ stage: Generating, section_content: Some(content), … })` — the per-section
   SSE/progress emission, carrying the section payload so U-027 streams it live.
8. `manager.update_progress(report_id, "generating", base + 70/total, sectionDone msg, None,
   Some(&completed_section_titles))?;`

`section_num = i + 1` (1-indexed), matching Python and `save_section`'s `{NN:02}` path. **The loop owns
`generated_sections: Vec<String>` and `completed_section_titles: Vec<String>`** declared before the loop
(Python parity); both are also read by the failed-path `update_progress`.

**No-downgrade note:** the per-section emit (step 7) IS the ReACT streaming behavior MiroFish has and
teri's template `generate_stream` does NOT — this is exactly why both paths coexist (master §1). Do not
collapse it into a single final emit.

---

## 4. interview_agents / U-020 dependency — confirmed honest-err `- [!]`, does NOT block (h)

- **U-020 `InterviewBus` is NOT landed** (`src/sim/ipc.rs` MISSING — verified). `interview_agents`
  (`zep_tools.rs:1259`) is therefore still the honest-err stub.
- The ReACT loop **tolerates a tool returning an error string** — (e) already routes `execute_by_name`'s
  return straight into the Observation regardless of content, and the Python parity is the
  `"工具执行失败: …"` text wrap. So a section that calls `interview_agents` simply gets an error-string
  Observation and continues; **the loop is not downgraded, only the tool is pending.**
- **Decision: (h) proceeds with interview_agents as an honest-err `- [!]`** (master §5 item 3 stands).
  (h) does NOT wire U-020. The `- [!]` is on the *tool*, recorded; `generate_report` parity is verified
  with scripted LLM tool-calls that exercise the WIRED tools (quick_search/panorama_search) and, where a
  test wants to assert the interview path, asserts the honest-error Observation byte-for-byte.

---

## 5. Remaining `ReportLogger` wiring (h wires these)

(g1) ported all 13 `log_*` helpers; (e) wired the **7 section-loop ones**
(`log_section_start`, `log_llm_response`, `log_tool_call`, `log_tool_result`, `log_section_content`).
**(h) wires the remaining orchestration-level log points**, in `generate_report`:

| Python call (line) | teri wiring point in generate_report |
|--------------------|--------------------------------------|
| `log_start(sim_id, graph_id, sim_req)` (1583) | right after constructing `self.report_logger` (before planning) |
| `log_planning_start()` (1606) | before `plan_outline` |
| `log_planning_complete(outline.to_dict())` (1618) | after `plan_outline` returns |
| `log_section_full_complete(title, num, content)` (1680) | per-section, step 6 of §3 — **already wired in (e)? NO** — (e) wired `log_section_content`; `log_section_full_complete` is an ORCHESTRATION call, wire it HERE |
| `log_report_complete(total_sections, total_time)` (1716) | after assemble, success tail |
| `log_error(str(e), "failed")` (1747) | in the catch, before saving failed state |

> Note: `log_planning_context` (1121 helper) — Python's `generate_report` does NOT call it directly;
> it is called inside `plan_outline` in Python (the master decision routes it through (d)). Verify (d)'s
> port wired `log_planning_context`; if not, it is a (d) gap, not (h)'s. **Flag for the porter to check**
> — but `generate_report` itself only calls log_start / log_planning_start / log_planning_complete /
> log_section_full_complete / log_report_complete / log_error.

**Logger construction:** `self.report_logger = Some(ReportLogger::new(&report_id, upload_folder)?)`
where `upload_folder` comes from the `manager`'s root. Since `ReportManager` stores `reports_dir =
{upload_folder}/reports`, expose `manager.upload_folder()` (the parent of `reports_dir`) OR pass
`upload_folder` explicitly to `generate_report`. **Decision: add `pub fn upload_folder(&self) ->
&Path` to ReportManager** returning `reports_dir.parent()` — cleaner than threading a second path arg;
zero blast radius. (h1 adds it alongside the `i32` widening and the `pub ensure_report_folder`.)

---

## 6. Ordered sub-step plan for (h)

Each sub-step is independently portable AND parity-verifiable in one loop cycle. Deps explicit.

> **(h1) ReportSink trait + ReportEvent/ReportStage + manager prerequisites (lowest-risk — PORT FIRST).**
> New `src/report/sink.rs`: `ReportEvent`, `ReportStage`, `trait ReportSink`, a `NullSink`, and a simple
> `TeeSink`/closure-sink helper. PLUS the three small `ReportManager` prerequisites this sub-cycle needs:
> (1) **widen `update_progress`'s `progress: u32` → `i32`** (the failed-path `-1` parity fix — §5/§0),
> (2) make `ensure_report_folder` **`pub`**, (3) add **`pub fn upload_folder(&self) -> &Path`**. Add the
> `console_logger: Option<ReportConsoleLogger>` field to `ReportAgent` (init `None` in new/new_react).
> **Parity:** `ReportEvent` serde shape (stage lowercase strings == status enum values); `NullSink`
> no-ops; `update_progress(report_id,"failed",-1,…)` now writes `"progress": -1` to `progress.json` and
> diffs byte-equal vs Python. Unit-level, no LLM/graph.
> **Deps:** (a) data model, (f) manager. **No graph/LLM dep — land first, lowest risk.**

> **(h2) generate_report skeleton: report_id, status machine, init, loggers, planning, finalize/error
> tails — WITHOUT the section loop body.**
> Build the `&mut self` method: auto-gen `report_id` (uuid12 shape), construct `Report{Pending}`,
> `manager.ensure_report_folder`, construct `self.report_logger` + `self.console_logger`, `log_start`,
> `update_progress(pending,0)`, `save_report`; status→Planning, `log_planning_start`,
> `sink.event(planning,0)`, call `plan_outline` (already landed (d)) via the planning sink-closure,
> `log_planning_complete`, `save_outline`, `update_progress(planning,15)`; status→Generating; **then
> assemble (`manager.assemble_full_report`) directly over the outline with empty section content** as a
> placeholder; status→Completed, `completed_at`, total_time, `log_report_complete`, `save_report`,
> `update_progress(completed,100)`, close console_logger, return. PLUS the full `catch`→Failed path
> (`log_error`, save failed, `update_progress(failed,-1)`, close console, return).
> **Parity:** drive with a scripted `chat_json` returning a 2-section outline + a NullSink + a temp
> upload_folder; diff `Report.to_dict`, `meta.json`, `outline.json`, `progress.json` SEQUENCE, and the
> `agent_log.jsonl` orchestration lines (start/planning_start/planning_complete/report_complete) vs
> Python for the no-section-content case; force an error (scripted chat_json error) and diff the failed
> `progress.json` (`-1`) + `log_error` line + `Report.status="failed"`/`error`. report_id passed
> explicitly for determinism.
> **Deps:** (h1), (d) plan_outline, (f), (g1)/(g2).

> **(h3) the per-section streaming loop body (replaces h2's placeholder assemble).**
> Insert the `for (i, section) in outline.sections.iter_mut().enumerate()` loop (§3): per-section
> progress arithmetic (`20 + (i/total)*70`), the section sink-closure (Python's `base + int(prog*0.7/
> total)` rescale, verbatim), call `generate_section_react` (landed (e)), `section.content=…`,
> push context, `save_section` immediately, push completed-title, `log_section_full_complete`,
> `sink.event(Generating, section_content)`, `update_progress(generating, base+70/total)`. Then the real
> assemble runs over the now-populated section files.
> **Parity:** scripted LLM (canned tool-calls + Final Answer per section) + fixture graph; diff the FULL
> file tree (`section_01.md`, `section_02.md`, …, `full_report.md`), the `progress.json` write SEQUENCE
> (every per-section update), the `agent_log.jsonl` (incl. the (e) section-loop lines AND the (h)
> `section_complete` lines), and `Report.to_dict` vs Python end-to-end. Assert sections are written
> incrementally (each `section_NN.md` exists before the next section's LLM call — drive with a mock that
> records call order).
> **Deps:** (h2), (e) generate_section_react, (b)/(c) tools+dispatch (for the wired tools in the loop).

> **(h4) hardening + U-027 sink adapter seam (optional polish, after h3).**
> The `ChannelSink`/`SseSink` reference impl (an `mpsc::Sender<ReportEvent>` wrapper) that U-027 will own
> — provided here as the documented seam so U-027 doesn't reinvent it; plus confirm console_logger
> close-ordering on both tails. NOT required for (h) parity (NullSink covers it) — schedule with/after
> U-027.
> **Deps:** (h3); coordinates with U-027.

**Critical path:** (h1) → (h2) → (h3). (h4) is insertable after (h3)/with U-027. **PORT (h1) FIRST**
— it is pure (trait + structs + three tiny manager fixes), unblocks h2/h3, and the `update_progress`
`i32` widening is a parity *bug fix* that should land regardless.

---

## 7. Parity risks / decisions-needed (`[≠]`/`[!]` flags for the no-downgrade gate)

1. **`- [!]` `update_progress` `u32`→`i32` (PARITY BUG, fix in h1).** Teri narrowed Python's
   `progress: int`; the failed path's `-1` is unrepresentable. Widening to `i32` restores parity. This is
   a *correction*, not a divergence. Verify `progress.json` `"progress": -1` byte-matches.

2. **`- [!]` report_id nondeterminism.** `report_{uuid12}` is random. Parity tests pass explicit
   `report_id`; auto-gen verified by shape `^report_[0-9a-f]{12}$` only. (Same posture as
   `elapsed_seconds`/`total_time_seconds` wall-time.) Not a `[≠]`.

3. **`- [!]` total_time_seconds nondeterminism.** `(now - start).total_seconds()` → wall time. The
   `ReportLogger.log_report_complete` already rounds via `round_half_even_2dp` (g1). Tests assert the
   field is a number of correct shape, not its value. Not a `[≠]`.

4. **`- [!]` interview_agents pending U-020** (§4). Honest-err tool; loop tolerates it; does NOT block
   (h). `- [!]` on the tool, not the orchestration.

5. **`[≠]` (NONE introduced by h).** ReportSink is a strict-superset capability add (legal, §1). Every
   observable artifact `generate_report` produces — `meta.json`, `outline.json`, `progress.json`
   sequence, `section_NN.md`, `full_report.md`, `agent_log.jsonl`, `console_log.txt`, the SSE progress
   stream — is **PORTED**, none `[≠]`-skipped. The `progress_callback` → ReportSink change is a faithful
   mapping (Python's callback is the same single event stream), not a drop.

6. **SSE shape vs U-027 (decision flag, master §5 item-5).** The report SSE route exposes the
   **per-section progress stream** (recommend `TickStreamEvent`-shaped frames carrying `ReportEvent`),
   NOT the template `PredictionReport` chunks; the template `generate_stream` keeps its own route. Flag
   to merge-integrator/U-027. Not a `[≠]` (both streams preserved).

7. **`ReportManager` instance vs Python classmethods (already resolved in (f), confirm in h).** Python
   calls `ReportManager.save_*` as classmethods; teri threads a `&ReportManager` instance. (h) owns/
   borrows one. Behavior identical (same files, same content). Not a `[≠]`.

8. **`log_planning_context` ownership check (porter task, not a risk on h).** `generate_report` does
   NOT call it; it belongs to `plan_outline` (d). Porter should confirm (d) wired it; if missing, that's
   a (d) follow-up, out of (h) scope.
