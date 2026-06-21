# Implementation log: U-024 `interview_agents` full port

Worktree: `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch `port/mirofish`).
Source of truth for verbatim Chinese literals: `/home/drdave/Desktop/meta/MiroFish/backend/app/services/zep_tools.py:1272-1763` (present in the meta tree — read directly, NOT reconstructed from the doc).

## Changes
- `src/services/zep_tools.rs`: widened `AgentInterview` + `InterviewResult` DTOs; added `runner` field + `with_runner` ctor to `ReportTools`; ported 5 private helpers + the full async `interview_agents`; added `execute_by_name_async`; reworked the sync `InterviewAgents` arm (honest-error, runner=None); added 3 free helpers + `INTERVIEW_AGENTS_PROMPT_PREFIX` const; replaced/added tests.
- `src/services/simulation_runner.rs`: added `pub fn sim_data_dir(&self) -> &Path` accessor.
- `src/report/mod.rs`: both production dispatch sites (`generate_section_react`, `chat`) now `.await execute_by_name_async`; added `Send + Sync + 'static` bound to `plan_outline`/`generate_section_react`/`generate_report`/`chat`/`make_tools_fixture` (propagated from the `SimulationRunner<L>` bound).
- `src/api/report.rs`: `chat_route` → `with_runner(Some(&state.sim_runner))`; threaded `Option<Arc<SimulationRunner<OpenAiAdapter>>>` through `spawn_report_generation` → `report_generate_worker` (route passes `Some(state.sim_runner.clone())`; worker uses `with_runner`); test call site passes `None`. Debug `tools_search_route`/`tools_statistics_route` left on `new`.

## Engine API (the parity contract)
- `AgentInterview { agent_name, agent_role, agent_bio, question, response, key_quotes: Vec<String> }` + faithful `to_dict` (6 keys) / `to_text` (bold name/role, bio, Q/A, `关键引言` block with quote-char + leading-punct stripping, `问题{1-9}` skip, >150-char truncation at first `。` after pos 80, `len<10` drop).
- `InterviewResult { interview_topic, interview_questions, selected_agents, interviews, selection_reasoning, summary, total_agents, interviewed_count }` + `new(topic, questions)`, `to_dict` (8 keys), `to_text` (`## 深度采访报告` skeleton; `（自动选择）`/`（无采访记录）`/`（无摘要）` placeholders).
- `ReportTools::with_runner(graph, llm, Option<&SimulationRunner<L>>)`; `new` delegates with `runner: None` (byte-compatible).
- `ReportTools::interview_agents(...).await -> Result<InterviewResult>` (now `async`; `custom_questions: Option<Vec<String>>`).
- `ReportTools::execute_by_name_async(...).await -> String` — intercepts only `interview_agents`, delegates all else to sync `execute_by_name`.
- `SimulationRunner::sim_data_dir() -> &Path`.

## Tests added (12 new fns; lib 1533 → 1544)
1. `test_interview_agents_sync_path_returns_honest_error` — sync `execute_by_name` (runner=None) → `工具执行失败`.
2. `test_interview_agents_async_no_runner_returns_error_text` — async dispatch, no runner → error text.
3. `test_execute_by_name_async_delegates_other_tools` — async == sync for `quick_search`.
4. `test_agent_interview_to_dict_field_set` — 6-key dict.
5. `test_agent_interview_to_text_basic_format` — headers; no quote block when empty.
6. `test_agent_interview_to_text_key_quote_cleaning` — strip curly quotes/leading punct (kept), skip `问题3`, drop `len<10`.
7. `test_agent_interview_to_text_truncation_over_150` — truncate at first `。` after pos 80.
8. `test_interview_result_to_dict_field_set` — 8-key dict.
9. `test_interview_result_to_text_empty_interviews` — `（自动选择）`/`（无采访记录）`/`（无摘要）` path.
10. `test_interview_result_to_text_non_empty` — section headers + `#### 采访 #1` + summary.
11. `test_clean_tool_call_response_passthrough_when_not_json` — plain + no-`tool_name` passthrough.
12. `test_clean_tool_call_response_unwraps_arguments_content` — `arguments.content` unwrap.
(The old `test_interview_agents_returns_honest_error`, which called the now-async method synchronously, was removed/replaced by #1; the 3 pre-existing sync `execute_by_name` interview tests — `..._returns_error_text`, `..._max_agents_capped_at_10`, `..._topic_fallback_to_query` — still pass unchanged against the honest sync arm.)

## Build/test status — PASS
- `cargo build` — PASS.
- `cargo test --lib` — **1544 passed; 0 failed** (≥1533 baseline; +11 net).
- `cargo test` (all bins) — all green (1544 + 4 + 3 + 8 integration, 0 failures).
- `cargo clippy --all-targets` — **clean** (0 warnings; one scoped `#[allow(clippy::only_used_in_recursion)]` on `execute_inner` with rationale — the sync interview arm no longer threads `simulation_id`).
- `cargo fmt --check` — clean.

## Deviations
- **`INTERVIEW_AGENTS_PROMPT_PREFIX` is a NEW local const**, NOT the existing `api::simulation::INTERVIEW_PROMPT_PREFIX`. The architecture doc §2c flagged these as two distinct literals (`zep_tools.py:1352-1362` 6-rule multi-line block vs `simulation.py:23` single line) — confirmed against the Python source; ported the multi-line block byte-for-byte.
- **`Send + Sync + 'static` bound added to `ReportTools<L>`** (and the 5 report methods that take `&ReportTools<L>`). Required because the new `runner: Option<&SimulationRunner<L>>` field carries `SimulationRunner<L>`'s `L: ... + Send + Sync + 'static` bound. All real callers (`OpenAiAdapter`) and tests (`StubLlm`) already satisfy it → zero call-site breakage; `new(graph, llm)` stays byte-compatible. This is the doc §3a fallback being unnecessary (the `L` types unify).
- **Generate worker threads an owned `Arc` clone**, not a borrow (a `&runner` can't cross the detached `'static` OS-thread in `spawn_report_generation`). `report_generate_worker`/`spawn_report_generation` gained a trailing `Option<Arc<SimulationRunner<OpenAiAdapter>>>` param; the in-test direct worker call passes `None`.
- **`[!] U-024-PROD-PENDING`** (from the doc, not introduced here): the terminal `interview_agents_batch` IPC returns env-not-running until the live IPC producer lands (U-026-k). Full logic ported; only live-data flip deferred. NOT a stub.
- Python `.get(key, default)` string semantics ported precisely via `py_get_str`/`resolve_agent_name` free fns (default applies only when key ABSENT, empty-string value wins) — distinct from Rust `.unwrap_or("")`.

## Handoff notes (for the guardian)
- **`_clean_tool_call_response` except-only regex**: the regex fallback runs ONLY on `serde_json` parse failure (Python's `except JSONDecodeError`); a successful parse with no matching `arguments.<key>` returns the original response (NOT the regex). Verify against `zep_tools.py:1493-1503`.
- **Key-quote extraction** (`extract_key_quotes`): regex set + order is contractual (`#{1,6}\s+` → `\{[^}]*tool_name[^}]*\}` → `[*_`|>~\-]{2,}` → `问题\d+[：:]\s*` → `【[^】]+】`); Strategy 1 sentence split on `[。！？]`, keep `20..=150` CHAR-len, sort by char-len descending (stable), first 3 each `+ "。"`; Strategy 2 (only if S1 empty) curly `“”` then corner `「」` 15-100 char captures. Verify char-count (not byte-count) semantics.
- **Exception ladder mapping**: `Err(TeriError::Sim(...))` from `interview_agents_batch` → ValueError branch (`采访失败：…。模拟环境可能已关闭…`); any other `Err` → generic (`采访过程发生错误：…`); in-band `status != Completed || error.is_some()` → API-failure branch (`采访API调用失败：…。请检查OASIS模拟环境状态。`). The 180s timeout is passed explicitly (`Duration::from_secs_f64(180.0)`), NOT the 120s method default — verify at `zep_tools.rs` Step 4.
- **Parity oracle**: differential the Python helper (mocked `self.llm` + fixture files) vs teri `InterviewResult::to_text()`/`.to_dict()` per the doc §5 fixtures (no-profiles early-return summary, LLM-select fallback `使用默认选择策略`, api-failure summary, env-not-running summary, happy-path dual-platform `response_text` + `_clean_tool_call_response` unwrap + key_quotes).
- No new deps added; no C/TLS surface touched; no guard weakened. The fail-closed posture (no runner → honest tolerated error) is preserved on the sync path.
