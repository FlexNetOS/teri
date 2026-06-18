# U-024 ReportAgent (extend-Y) — Architecture & Sub-Cycle Decomposition

Architect decision for the extend-Y unification of MiroFish's ReACT `ReportAgent`
(`backend/app/services/report_agent.py`, 2573 lines) with teri's template-based
`teri::report::ReportAgent` (`src/report/mod.rs`). No-downgrade is **bidirectional**:
preserve MiroFish's plan→ReACT→assemble pipeline AND teri's existing streaming template path.

Consistent with the standing architecture (`target-architecture.md`): **Decision-1** (U-017 zep
tools map-onto `src/graph`+`src/report`), **DECISION-9** (`KnowledgeGraphEntityReader<'a>{graph:&'a KnowledgeGraph}`
substrate; `graph_id:str` is `[≠]` inexpressible — the bound `&KnowledgeGraph` IS the selector),
**DECISION-11** (caller-constructs llm/graph handles), and line 72 (U-024 = ADD plan_outline + per-section
ReACT + chat + section writes onto the `generate_stream` substrate).

---

## 1. Reuse-vs-narrowing call — does ReACT SUBSUME the template path?

**Decision: BOTH COEXIST. The ReACT pipeline does NOT subsume teri's template path — they have
different inputs and serve different verified contracts. Unify under one `ReportAgent` with two
entry families; neither is downgraded.**

Evidence the two are distinct (not one narrowing the other):

| Axis | teri template path (Y, existing) | MiroFish ReACT path (X) |
|------|----------------------------------|--------------------------|
| Input | `&SimulationResult` (in-memory history) | `graph_id`+`simulation_id`+`simulation_requirement` + graph tools |
| Shape | single LLM render of one Jinja template → `PredictionReport` JSON (summary/timeline/agent_highlights/confidence) | plan_outline → N sections, each a bounded ReACT tool loop → markdown `Report` |
| Output type | `PredictionReport` (structured) | `Report`/`ReportOutline`/`ReportSection` (markdown, status machine) |
| Streaming | `generate_stream` SSE (≥2 chunks, partial→final) | per-section incremental (planning/generating/completed stages) |
| LLM call | `complete_json` (one shot) | multi-turn `chat`/`chat_json` with injected Observations |
| Tools | none | insight_forge/panorama_search/quick_search/interview_agents |

teri's `PredictionReport`/`generate`/`generate_stream` are already **parity-verified** against teri's own
contract and have live tests (`test_generate_stream_yields_multiple_chunks`). Collapsing them into the
ReACT path would (a) regress teri's structured `PredictionReport` API consumed by U-027 report routes and
(b) require a `SimulationResult`→graph adapter that doesn't exist. **That is a downgrade — refused.**

Symmetrically, the ReACT path cannot be reduced to "the template with tools": its outline-planning,
bounded multi-round tool loop, conflict/insufficient-tool control flow, section-file persistence, and
chat mode are all observable behaviors. Dropping any is a downgrade. So this is genuinely `extend-Y`,
**not** `reuse-Y` — confirmed by full-contract divergence, exactly the "near-fit ⇒ extend, not reuse"
bar.

### Unified target shape of `teri::report::ReportAgent`

`ReportAgent` becomes a **stateful struct** (it is currently a ZST `pub struct ReportAgent;`). The two
entry families live on it; the existing template fns stay as **associated functions** (no `&self`) so
their signatures and tests are byte-stable.

```rust
// src/report/mod.rs  (extend, do not replace)

pub struct ReportAgent {
    graph_id: String,                 // retained as an opaque label only (Zep handle is [≠]); see §2
    simulation_id: String,
    simulation_requirement: String,
    // llm + graph handles are NOT stored at construction (DECISION-11) — passed per call OR
    // bound via a ReportTools facade constructed by the caller. See §2 for the exact wiring.
}

impl ReportAgent {
    // ── NEW: ReACT family (ported from X) ──
    pub fn new_react(graph_id, simulation_id, simulation_requirement) -> Self;
    pub async fn plan_outline<L,G>(&self, tools:&ReportTools<'_,L,G>, llm:&L) -> ReportOutline;
    async fn generate_section_react<L,G>(&self, section, outline, prev, tools, llm, idx) -> String;
    pub async fn generate_report<L,G>(&self, tools, llm, sink:&mut dyn ReportSink) -> Report;
    pub async fn chat<L,G>(&self, msg, history, tools, llm) -> ChatResponse;

    // ── EXISTING template family (Y, UNCHANGED signatures/tests) ──
    pub fn new() -> Self { /* keep ZST-compatible: Default + new() still valid */ }
    pub fn create_empty_report(query: String) -> PredictionReport;     // unchanged
    pub async fn generate_stream<L: LlmClient + ?Sized>(...) -> ... ;  // unchanged (assoc fn)
    pub async fn generate<L: LlmClient + ?Sized>(...) -> ...;          // unchanged (assoc fn)
}
impl Default for ReportAgent { fn default() -> Self { Self::new() } }  // keep
```

**No-downgrade guarantee:** `generate`, `generate_stream`, `create_empty_report`, `parse_report_from_json`,
`extract_key_events`, `summarize_agents`, `PredictionReport`/`TimelineEvent`/`AgentHighlight` and all 7
existing tests remain bit-identical. `ReportAgent::new()` must still return a value usable by the existing
assoc-fn call sites (they call `ReportAgent::generate(...)` without an instance, so adding fields to the
struct does NOT break them — verify the few `ReportAgent::new()`/`default()` constructions still type-check;
the new fields get default/empty values via a separate constructor `new_react`).

---

## 2. ZepToolsService ↔ KnowledgeGraph wiring decision (CRITICAL PATH)

### Blast radius (measured)
`grep -rn ZepToolsService src/` → **zero production callers.** Only two i18n string keys
(`en.json`/`zh.json` "zepToolsInitialized") and the module's own tests reference it. **The struct is a
leaf.** Changing its constructor / adding a graph handle has essentially no downstream blast radius —
only its own test module and (future) U-024/U-027 construction sites. This de-risks the wiring entirely.

### Decision: bind the graph by reference via a `ReportTools` facade — do NOT store a handle on `ZepToolsService`, and do NOT add `Arc<KnowledgeGraph>`

Follow the established **DECISION-9 substrate pattern** (`KnowledgeGraphEntityReader<'a>{graph:&'a KnowledgeGraph}`):
an in-process read **borrows** the graph; it does not own or share-mutate it. Per **Decision-1**, the four
report tools map onto `src/graph` (+ vec similarity for insight_forge). Concretely:

```rust
// src/services/zep_tools.rs  (extend)
// Bind the graph + llm by reference at the CALL boundary, not at construct time (DECISION-11).
pub struct ReportTools<'g, L: LlmClient, G = KnowledgeGraph> {
    graph: &'g G,                 // borrowed substrate (was Zep graph_id) — DECISION-9
    llm: &'g L,                   // for insight_forge sub-query generation
    reader: KnowledgeGraphEntityReader<'g>,  // reuse U-016 reader for get_entities_by_type etc.
}
```

The existing `ZepToolsService<L>` DTOs (`SearchResult`, `NodeInfo`, `EdgeInfo`, `InsightForgeResult`,
`PanoramaResult`, `AgentInterview`, `InterviewResult` + their `to_dict`/`to_text`) are **kept verbatim**
(already parity-ported in U-017). The stub methods that currently return `TeriError::Unknown(...)` are
**re-homed**: the graph-touching ones move onto `ReportTools` (which holds `&KnowledgeGraph`), so they
stop being stubs. `ZepToolsService<L>` itself may remain as the DTO/`call_with_retry` holder, OR the
graph methods migrate to `ReportTools`; **preference: migrate the graph methods to `ReportTools`** and
leave `ZepToolsService` as the retry/DTO namespace — this keeps the no-graph-handle invariant clean and
matches DECISION-11. (If the cartographer's U-017 ledger requires the methods to stay named on
`ZepToolsService`, the alternative is `ZepToolsService` gaining a `&'g KnowledgeGraph` field — also
acceptable given zero blast radius — but the facade is cleaner and reuses the U-016 reader.)

### Why NOT `Arc<KnowledgeGraph>` / `Arc<Mutex<…>>`
Report generation is **read-only** over a finished graph. `Arc<Mutex<…>>` is reserved (DECISION for
`GraphMemoryUpdater`, target-arch line 939) for the **write-back** path that mutates the graph during
simulation. A reader taking `&KnowledgeGraph` is zero-cost, needs no lock, and composes with the U-016
reader. Using `Arc` here would be a gratuitous sharing/ownership choice with no behavior benefit.

### `graph_id` mapping
`graph_id: str` is a Zep **server handle** → **`[≠]` inexpressible** (consistent with target-arch lines
474–484, 519, 614). teri binds the actual `&KnowledgeGraph`. We retain a `graph_id: String` **label**
field on `ReportAgent` only for log lines / report metadata parity (`Report.graph_id` is serialized — it
IS observable in the JSON `to_dict`, so the *field* is PORTED as a string; only its *Zep-handle semantics*
are `[≠]`). The tool methods ignore it for selection and use the bound graph.

### insight_forge / panorama_search / quick_search / interview_agents target mapping (Decision-1)
- **quick_search** → keyword scan over `graph.get_all_entities()` + `get_all_edges()` facts (the
  existing `local_search` body, now with a real bound graph). No external dep.
- **panorama_search** → `graph.partition_edges_at(t)` (already exists, line 1237) gives
  (active, historical) split → `active_count`/`historical_count`. Requires GAP-1 `Relation.valid_at`
  (already landed — line 88/139 shows the field is present in `Relation`). **Unblocked.**
- **insight_forge** → LLM (`chat_json`) splits query into sub-queries (parity with X's sub-query
  decomposition) → each sub-query runs semantic search → entity enrichment. Semantic ranking needs
  **GAP-2 `query_vec_similarity`** (OQ-3, shimmy embeddings). **DEPENDENCY — see §4 ordering.** If
  OQ-3 is not yet landed, insight_forge falls back to keyword scan (same as quick_search) and emits a
  `- [!]` ledger note; it must NOT silently drop the multi-sub-query structure.
- **interview_agents** → native simulation IPC (U-020), per target-arch line 61. If U-020 not present,
  this tool is **deferred to its own sub-cycle** and stubbed-with-honest-error within ReportTools (the
  ReACT loop already tolerates a tool returning an error string — Python `_execute_tool` wraps
  exceptions as `"工具执行失败: …"`; teri mirrors that, so the loop degrades gracefully, no downgrade
  to the *loop*, only a `- [!]` on the *tool*).

---

## 3. Idiom map for the ReACT pieces

### 3a. Tool dispatch (`_define_tools`/`_execute_tool`/`_parse_tool_calls`/`_is_valid_tool_call`)
- **Tool identity → `enum ReportTool`** (`InsightForge`, `PanoramaSearch`, `QuickSearch`,
  `InterviewAgents`). The legacy back-compat redirects (`search_graph`→quick_search,
  `get_simulation_context`→insight_forge, `get_graph_statistics`, `get_entity_summary`,
  `get_entities_by_type`) are PORTED as additional match arms so an LLM emitting an old name still
  dispatches (observable behavior — preserve it). Unknown tool → the Python `"未知工具: …"` string,
  byte-identical.
- **`_execute_tool` → `ReportTools::execute(tool, params_json) -> String`** — a `match` over the enum,
  each arm parsing its params from a `serde_json::Map` (params arrive as JSON from the tool-call parse).
  Parameter coercions are contractual and PORTED: `include_expired` str→bool (`"true"/"1"/"yes"`),
  `limit`/`max_agents` str→int, `max_agents = min(n, 10)`, `interview_topic` falls back to `query`.
  Errors caught and returned as the `"工具执行失败: {e}"` text (NOT propagated as `Err`) — matches the
  Python `try/except` so the ReACT loop keeps going.
- **`_parse_tool_calls` → a free fn `parse_tool_calls(&str) -> Vec<ToolCall>`** preserving the exact
  3-tier priority: (1) `<tool_call>…</tool_call>` regex (`regex` crate, `(?s)` DOTALL), (2) bare whole-
  response JSON, (3) trailing `{"name"|"tool": …}` regex. The `{"tool"/"params"}`→`{"name"/"parameters"}`
  key-normalization in `_is_valid_tool_call` is PORTED. `VALID_TOOL_NAMES` set gates tiers 2–3.
- **Why enum, not trait objects:** the tool set is closed and known at compile time; a dispatch enum is
  the idiomatic, allocation-free form and keeps the back-compat redirect arms trivially expressible. A
  `trait Tool` + `Box<dyn Tool>` registry would add indirection with zero behavioral gain (no plugin /
  open-set requirement in X).

### 3b. Bounded reflection loop (MAX_* constants → control flow)
- Constants port as `const`: `MAX_TOOL_CALLS_PER_SECTION: usize = 5`, `MAX_REFLECTION_ROUNDS: usize = 3`
  (note: X declares it but the section loop uses local `max_iterations=5`/`min_tool_calls=3`
  — PORT the **actual** loop values, not just the unused constant; flag that `MAX_REFLECTION_ROUNDS`
  is declared-but-unused in X — port the constant for completeness but the real bound is the local 5),
  `MAX_TOOL_CALLS_PER_CHAT: usize = 2`.
- The section loop → `for iteration in 0..max_iterations { … }` with the same branch ladder, **all
  branches preserved (no downgrade)**:
  1. LLM returns `None`/empty → retry message then `break` on last iter (teri: `chat` returns `Result`;
     map an empty string the same way the Python checks `response is None`).
  2. conflict (tool_call **and** `Final Answer:`) → `conflict_retries`: first 2 re-ask, 3rd truncates to
     first `</tool_call>` and forces execution. PORT exactly (it is observable loop behavior).
  3. `Final Answer:` with `tool_calls_count < min_tool_calls` → reject + unused-tool hint, `continue`.
  4. `Final Answer:` valid → `split("Final Answer:").last().trim()`.
  5. tool call with quota exhausted → `REACT_TOOL_LIMIT_MSG`, `continue`.
  6. tool call OK → execute **first only**, append Observation (`REACT_OBSERVATION_TEMPLATE`),
     `used_tools.insert`, unused-tool hint.
  7. neither → if under min, insufficient-tools prompt; else accept the raw text as final answer
     (the "no prefix" path).
  8. loop fallthrough → `REACT_FORCE_FINAL_MSG`, one more `chat`, force-extract.
  All Chinese prompt-template constants (`PLAN_SYSTEM_PROMPT`, `SECTION_SYSTEM_PROMPT_TEMPLATE`,
  `REACT_OBSERVATION_TEMPLATE`, etc.) are PORTED **verbatim** as `const &str` with `{}` placeholders
  filled by `format!`/a tiny templating helper — they are part of the behavior (the model's outputs
  depend on them). Truncation rule `prev[:4000]+"..."` PORTED (char-boundary-safe via `char_indices`).
- `used_tools: HashSet<&'static str>` / `all_tools` set → idiomatic; unused-hint joins with the same
  separators (`、` / `, `) — keep byte-identical since they appear in prompts.

### 3c. Data model (ReportStatus/ReportSection/ReportOutline/Report)
- `ReportStatus` → `#[derive(Serialize, Deserialize)] #[serde(rename_all="lowercase")] enum ReportStatus
  { Pending, Planning, Generating, Completed, Failed }` (serde value = the Python `str`-enum values
  `"pending"`/… exactly). `to_dict` uses `.value` → serde lowercase matches.
- `ReportSection { title: String, content: String }` + `to_markdown(level)`.
- `ReportOutline { title, summary, sections: Vec<ReportSection> }` + `to_markdown`.
- `Report { report_id, simulation_id, graph_id, simulation_requirement, status, outline: Option<…>,
  markdown_content, created_at, completed_at, error: Option<String> }` + `to_dict`.
- **Key-order on JSON: contractual.** `Report.to_dict`/`ReportOutline.to_dict`/`ReportSection.to_dict`
  hit `outline.json`/`meta.json` files and the `/api/report` response (U-027). Use **`#[serde(...)]`
  with fields declared in Python dict order** (serde_json preserves struct field declaration order),
  OR build an explicit `serde_json::Map` mirroring the Python ordering as the existing
  `zep_tools.rs::to_dict` impls already do. Match the Python order: report_id, simulation_id, graph_id,
  simulation_requirement, status, outline, markdown_content, created_at, completed_at, error. **`- [≠]`
  watch:** if U-027 (routes) consumes these structs directly rather than re-serializing, the export
  shape is STILL contractual (it is the persisted-file + API shape) — do NOT `[≠]`-skip `to_dict`/file
  writes on "the route consumes the struct directly". Port them.

### 3d. ReportLogger / ReportConsoleLogger / ReportManager (persistence + observability)
These are **observable file sinks** (agent_log.jsonl, console_log.txt, meta.json, outline.json,
progress.json, section_NN.md, full_report.md) and a status/progress API. They are **PORTED, not `[≠]`**
(producing distinct on-disk artifacts the frontend reads — a disguised-skip would be a downgrade).
- **`ReportLogger`** → a struct writing newline-delimited JSON to `…/reports/{id}/agent_log.jsonl`.
  All `log_*` helpers PORTED (each emits the same `action`/`stage`/`details` shape, ISO-8601 timestamp,
  `elapsed_seconds`). The i18n `t(...)` messages route through teri's `i18n` (U-005). Streaming
  observability = teri's `ReportSink` trait abstracting the per-event emit (so SSE in U-027 and the
  jsonl file are two sinks over one event stream — unifies with Y's `generate_stream` SSE substrate).
- **`ReportConsoleLogger`** → attaches a file appender to the `log`/`tracing` subscriber for the
  report-scoped span (teri uses `tracing`). PORT as a per-report file layer (or a buffered writer the
  ReportSink also feeds). The Python attach/detach to named loggers maps to a tracing layer guard
  dropped at report end. (Mirrors the U-004 rotating-file logging lesson: dest-architecture difference
  is not grounds to skip a file sink.)
- **`ReportManager`** → `src/report/manager.rs` (sub-module). PORT the folder layout, `save_outline`,
  `save_section` (+ `_clean_section_content`), `update_progress`/`get_progress`,
  `assemble_full_report` (+ `_post_process_report` heading-normalization), `save_report`, `get_report`,
  `get_report_by_simulation`, `list_reports`, `delete_report`, `get_agent_log`/`get_console_log`
  (+ `_stream` variants). `Config.UPLOAD_FOLDER` → teri config (env-driven; see CLAUDE.md "Config = env
  vars only"). The `_clean_section_content` / `_post_process_report` regex heading-to-bold rewrites are
  **content-shaping behavior** (observable in the emitted markdown) — PORTED verbatim (regex crate),
  including the "convert ### and below to **bold**", duplicate-heading dedupe, separator stripping, and
  ≤2-consecutive-blank-lines collapse. **Back-compat old-format paths** (`{id}.json`/`{id}.md` flat
  files in `get_report`/`list_reports`/`delete_report`) are PORTED (observable fallback behavior).

---

## 4. Ordered sub-cycle plan (one per loop cycle, deps explicit)

Each sub-cycle is independently portable AND parity-verifiable. ZepTools↔graph wiring is split so the
**pure data model lands first** (no graph dep), then the **wiring** (the blocker), then the loop.

> **(a) Data model + ReportStatus/Section/Outline/Report + to_dict/markdown.**
> Pure structs, serde, key-order, `to_markdown`. NO ZepTools / graph / llm dependency.
> Parity: differential `to_dict` JSON (key order + status string values) and `to_markdown` vs Python
> golden fixtures. Extends `src/report/mod.rs`; existing `PredictionReport` untouched.
> Deps: none. **Land first.**

> **(b) ZepTools ↔ KnowledgeGraph wiring — `ReportTools` facade (THE BLOCKER).**
> Build `ReportTools<'g,L>` binding `&KnowledgeGraph` + `&L` + the U-016 reader. Re-home
> `quick_search` (keyword over get_all_entities/edges) and `panorama_search`
> (`partition_edges_at` → active/historical; GAP-1 `valid_at` already landed) and
> `get_entities_by_type`/`get_entity_summary`/`get_graph_statistics`/`get_simulation_context` onto it,
> replacing the `TeriError::Unknown` stubs with real graph reads. `graph_id` ignored for selection
> (`[≠]` label only). DTOs unchanged.
> Parity: feed a fixture `KnowledgeGraph`, diff `quick_search`/`panorama_search` `.to_text()` vs the
> Python tool output over the same graph. Zero production blast radius (leaf struct).
> Deps: (none new) — U-017 DTOs + U-016 reader + GAP-1 already present.
> **`- [!]` insight_forge semantic ranking deferred to (b2) pending OQ-3 — do not block (b).**

> **(b2) insight_forge (semantic) — OPTIONAL/CONDITIONAL.**
> LLM sub-query decomposition (`chat_json`) + per-sub-query semantic search via `query_vec_similarity`
> (OQ-3 / GAP-2, shimmy embeddings) + entity enrichment. If OQ-3 not yet landed, ship the sub-query
> **structure** with a keyword-search backend and a `- [!]` note (no structural drop).
> Parity: sub-query count/shape + `InsightForgeResult.to_text` vs Python.
> Deps: (b); GAP-2/OQ-3 (`query_vec_similarity`). Can run after (b) independently of (c)/(d).

> **(c) Tool dispatch + parser — `ReportTool` enum, `parse_tool_calls`, `execute`, param coercions,
> back-compat redirects, `_get_tools_description`.**
> The pure ReACT plumbing minus the loop. Param str→bool/int coercions, `min(max_agents,10)`,
> unknown-tool string, `try/except`→error-text. Tool descriptions (`TOOL_DESC_*`) ported verbatim.
> Parity: golden `parse_tool_calls` over the 3 input tiers (xml / bare / trailing) incl. the
> `tool`/`params`→`name`/`parameters` normalization; `execute` dispatch table.
> Deps: (a) [for nothing structural] + (b) [execute needs ReportTools]. **After (b).**

> **(d) plan_outline.**
> `get_simulation_context` → `PLAN_SYSTEM_PROMPT` + `PLAN_USER_PROMPT_TEMPLATE` → `chat_json`
> (temperature 0.3) → `ReportOutline`; PORT the on-error 3-section fallback outline.
> Parity: given a fixture graph + mocked `chat_json`, diff produced `ReportOutline.to_dict` and the
> fallback path vs Python.
> Deps: (a), (b), (c).

> **(e) generate_section_react — the bounded ReACT loop.**
> The full branch ladder (None/empty, conflict×3, insufficient-tools, quota, observation, no-prefix,
> force-final), `used_tools`/unused-hint, prev-section 4000-char truncation, all prompt constants.
> Parity: drive with a scripted mock `chat` returning canned tool-calls/Final-Answer sequences;
> assert the message-trace and final answer match Python for: happy path, conflict-downgrade,
> quota-exhaustion, force-final.
> Deps: (a), (c), (d).

> **(f) ReportManager (manager.rs) — persistence + assembly + heading normalization + back-compat.**
> File layout, save/get/list/delete, `_clean_section_content`, `_post_process_report`,
> progress/agent-log/console-log readers, old-format fallbacks. `Config.UPLOAD_FOLDER`→teri env config.
> Parity: golden file-tree + `assemble_full_report`/`_post_process_report` output vs Python over the
> same sections; `get_report`/`get_report_by_simulation` round-trip; old-format read.
> Deps: (a). Can run in parallel with (c)/(d)/(e) (no graph/loop dep) — schedule after (a), any time
> before (g).

> **(g) ReportLogger + ReportConsoleLogger + ReportSink (observability sinks).**
> jsonl structured log, tracing file layer, the `ReportSink` trait unifying SSE (Y's stream) + jsonl.
> Parity: agent_log.jsonl line shape (action/stage/details/timestamp/elapsed) vs Python for a fixed
> run; console_log.txt presence/format.
> Deps: (a), (f) [paths], i18n U-005. Before (h).

> **(h) generate_report — orchestration (plan → per-section → assemble) wired to sinks + manager.**
> The top-level pipeline: status machine, per-section progress, save-section-immediately streaming,
> assemble, COMPLETED/FAILED handling, total-time. Unifies with Y's `generate_stream` via ReportSink.
> Parity: end-to-end differential — same graph + scripted LLM → diff the full `Report.to_dict`,
> `full_report.md`, `progress.json` sequence, section files vs Python.
> Deps: (b),(c),(d),(e),(f),(g).

> **(i) chat — conversational ReACT (MAX_TOOL_CALLS_PER_CHAT=2).**
> 2-iteration loop, report-content context (15000-char cap), `get_report_by_simulation` lookup,
> response cleaning regex (`<tool_call>`/`[TOOL_CALL]` strip), `{response, tool_calls, sources}`.
> Parity: scripted mock chat → diff the returned dict (incl. `sources` = tool query params).
> Deps: (c), (f) [get_report_by_simulation], (b) [execute].

**Critical-path order:** (a) → (b) → (c) → (d) → (e) → (h). (b2),(f),(g),(i) are insertable around the
spine: (f)+(g) any time after (a) (no graph dep), (b2) after (b), (i) after (c)+(f).

---

## 5. Parity risks / decisions-needed (`[≠]` candidates — flag for no-downgrade gate)

1. **`[≠]` `graph_id` Zep-handle semantics** (NOT the field). The `graph_id` *string* is PORTED
   (serialized in `Report.to_dict`, observable). Only its **by-id remote-graph-selection** semantics
   are inexpressible — teri binds `&KnowledgeGraph`. Consistent with target-arch 474–484/519/614.
   **Legal `[≠]`** (genuinely inexpressible Zep-server artifact).

2. **`- [!]` insight_forge semantic ranking** depends on **GAP-2/OQ-3 `query_vec_similarity`** (shimmy
   embeddings). If unresolved when (b2) runs, ship sub-query structure + keyword fallback; the
   *semantic ranking quality* is a `- [!]` owner-decision, NOT a silent drop of the multi-query
   structure. Decision-needed: confirm shimmy `/v1/embeddings` exposure (OQ-3).

3. **`- [!]` interview_agents** needs native simulation IPC (U-020). If U-020 not present at (c)/(h),
   the tool returns an honest error string (the ReACT loop tolerates it — Python parity), and a `- [!]`
   marks the tool deferred. The **loop is not downgraded**; only the *tool* is pending its dependency.

4. **LLM tool-calling format = text-protocol, NOT provider function-calling.** X uses a **prompt-encoded**
   `<tool_call>{json}</tool_call>` / `Final Answer:` text protocol over plain `chat` — it does NOT use
   OpenAI/Anthropic native tool-calling APIs. teri MUST reproduce the **same text protocol** over
   `chat(&[ChatMessage], &ChatOptions)` (U-008), NOT substitute native function-calling. Substituting
   native tool-calling would change observable model behavior and break the conflict/parse branches →
   that is a downgrade. **Decision: port the text protocol verbatim.** No `[≠]`.

5. **Streaming model difference (per-section vs single-buffer).** Y's `generate_stream` yields a partial
   then a final `PredictionReport` (≥2 chunks). X streams *per section* (planning/generating/completed
   progress events). These are **different report families** (§1) — both preserved. The unification
   point is the `ReportSink` trait (one event stream → SSE + jsonl). No capability lost on either side;
   not a `[≠]`. **Decision-needed for U-027:** which stream the HTTP route exposes for the ReACT report
   (recommend: the ReACT per-section/progress stream, mirroring MiroFish's `/api/report` SSE). Flag to
   merge-integrator/U-027.

6. **`_post_process_report` / `_clean_section_content` heading→bold rewrites are content-shaping and
   PORTED verbatim** (observable markdown output). Do **NOT** `[≠]`-skip them as "formatting niceties" —
   they change the emitted report bytes. The regexes (heading detection, duplicate-window dedupe,
   ≤2-blank collapse) port to the `regex` crate. Parity risk: Rust `regex` lacks some PCRE features but
   these patterns (`^(#{1,6})\s+(.+)$`, simple anchors) are fully supported. Low risk; verify in (f).

7. **`MAX_REFLECTION_ROUNDS` declared-but-unused in X.** The section loop's real bound is the local
   `max_iterations=5`. Port the constant for completeness but the **behavioral** bound is 5 — document
   so the parity verifier doesn't expect a 3-round reflection that X never actually runs. Not a `[≠]`;
   a note to avoid mis-porting a dead constant into live control flow.

8. **Chinese prompt/template constants & `t()` i18n.** All `*_PROMPT*`/`REACT_*`/`TOOL_DESC_*` ported
   verbatim (`const &str`); `t(...)`/`get_language_instruction()` route through teri i18n (U-005,
   U-014's locale machinery). These are behavior (model conditioning + log messages), PORTED not `[≠]`.
