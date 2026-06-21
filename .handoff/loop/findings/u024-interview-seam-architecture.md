# U-024 — `interview_agents` ReACT-tool seam architecture

Cycle 67, U-024 leaf wiring. READ-ONLY design doc. Source X = MiroFish (Python),
dest/target = teri (Rust), worktree `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri`,
branch `port/mirofish`.

The tracked `[!]` frontier: `ReportTools::interview_agents`
(`src/services/zep_tools.rs:1259-1273`) returns an honest `Err`. This doc decides whether and
how to wire it.

---

## 1. REACHABILITY VERDICT — **REACHABLE**

A `SimulationRunner` handle (as `Arc<SimulationRunner<OpenAiAdapter>>`) **is in scope** at every
production site where `ReportTools` is constructed and where `generate_section_react` runs.

### Traced call chain (UP from `generate_section_react`)

| Layer | Symbol | File:line | Has runner in scope? |
|---|---|---|---|
| Tool leaf | `ReportTools::interview_agents` (stub) | `src/services/zep_tools.rs:1259` | no (struct holds only `graph`, `llm`, `reader` — `:498-506`) |
| Dispatcher | `ReportTools::execute_inner` → `ReportTool::InterviewAgents` arm | `src/services/zep_tools.rs:1820-1839` | n/a (sync) |
| Dispatcher entry | `ReportTools::execute` / `execute_by_name` | `src/services/zep_tools.rs:1731`, `:1921` | n/a |
| ReACT loop | `ReportAgent::generate_section_react` calls `tools.execute_by_name(...)` **sync, no `.await`** | `src/report/mod.rs:1246` | — |
| ReACT loop (chat) | `ReportAgent::chat` calls `tools.execute_by_name(...)` **sync** | `src/report/mod.rs:2242` | — |
| Report route | report-generation handlers build `ReportTools::new(&graph, &llm)` | `src/api/report.rs:517, 551, 698` | **YES** |
| App state | `ApiState { config, sim_manager, sim_runner }`; `sim_runner: Arc<SimulationRunner<OpenAiAdapter>>` | `src/api/mod.rs:285, 298-300, 320-328` | **YES** |

**Load-bearing evidence:** every report axum handler takes
`State(state): State<Arc<ApiState>>` (`src/api/report.rs:497, 536, 627`), and `ApiState` owns
`sim_runner` (`src/api/mod.rs:298`). The runner is the **same instance** that started/holds the
live sims (`runs` map; comment `src/api/mod.rs:291-300`, `:320-328`). So at the
`ReportTools::new(&graph, &llm)` site (`report.rs:517/551/698`) `&state.sim_runner` is directly
available with zero new app-state plumbing.

**Why the struct doesn't have it yet:** `ReportTools<'g, L>` was deliberately built as a
borrow-facade over `&KnowledgeGraph + &L` only (DECISION-9/11, doc-comment `zep_tools.rs:482-515`),
because at sub-cycle (b) the runner dependency (U-022) was `- [ ]`. U-022 is now terminal
(`SimulationRunner::interview_agents_batch` = S-631, `src/services/simulation_runner.rs:5340-5352`).
The seam is therefore **minimal additive**: thread an optional runner handle into the facade and
keep `new(graph, llm)` byte-for-byte compatible for the 4 existing constructor callers
(`report.rs:517,551,698` + `report/mod.rs:2529`).

### Blast radius of the additive field (measured, ~0)
- Constructor callers of `ReportTools::new`: 4 (`report.rs:517,551,698`; `report/mod.rs:2529`)
  + ~30 test sites. With `new(graph, llm)` PRESERVED (delegating to a `with_runner` ctor that
  passes `runner: None`), **none of them change**. Only the 3 report-route sites that want live
  interviews call the new `with_runner(graph, llm, runner)` ctor.
- The two production dispatch sites (`report/mod.rs:1246, :2242`) are both inside `async fn`
  (`generate_section_react` `:958`, `chat` `:2124`), so `.await` is available — see §4.

---

## 2. THE PYTHON CONTRACT (distilled — porter must lose NOTHING)

Source: `MiroFish/backend/app/services/zep_tools.py:1272-1482` (`interview_agents`) + its 5
private helpers + the two dataclasses. Tool description: `report_agent.py:523-548`. Dispatch
mapping: `report_agent.py:1008-1021` (already ported into the teri `InterviewAgents` arm,
`zep_tools.rs:1820-1839`).

### 2a. ⚠️ BLOCKER — teri `InterviewResult`/`AgentInterview` are NARROWED (a hidden downgrade)

The teri structs are a **subset** of the Python ones and MUST be widened before the body can be
faithful. This is itself a no-downgrade violation in the current stub that the port must fix:

`AgentInterview` (Python `zep_tools.py:285-340`) fields:
`agent_name, agent_role, agent_bio, question, response, key_quotes: List[str]`.
teri (`zep_tools.rs:~410-447`) — **VERIFY/ADD** these fields and faithful `to_dict`/`to_text`.
Python `to_text` (`zep_tools.py:301-340`) is elaborate: bold name/role, full untruncated bio,
Q/A, then a `关键引言` block that for each quote strips `「」""""`, strips leading punctuation
`，,；;：:、。！？\n\r\t `, **skips quotes containing `问题{1-9}`**, truncates >150 chars at the
first `。` after pos 80 (else hard `[:147]+"..."`), and only emits if `len >= 10`. Byte-identical
output required.

`InterviewResult` (Python `zep_tools.py:341-398`) fields:
`interview_topic, interview_questions: List[str], selected_agents: List[Dict],
interviews: List[AgentInterview], selection_reasoning: str, summary: str,
total_agents: int, interviewed_count: int`.
teri currently has only `{agent_interviews, questions, responses}` (`zep_tools.rs:453-480`) —
**this is wrong** and must be replaced with the full field set + faithful `to_dict`/`to_text`.
Python `to_text` (`zep_tools.py:380-398`) emits a fixed `## 深度采访报告` markdown skeleton with
`**采访主题:**`, `**采访人数:** {interviewed_count} / {total_agents} 位模拟Agent`,
`### 采访对象选择理由`, `\n---`, `### 采访实录`, per-interview `#### 采访 #{i}: {name}`, and
`### 采访摘要与核心观点` with `summary or "（无摘要）"`. (`[≠]` ALERT: the existing teri
`to_text`/`to_dict` shape is a disguised feature-skip — restore the full Python shape.)

### 2b. `interview_agents` signature + body (`zep_tools.py:1272-1482`)

Python signature:
`interview_agents(self, simulation_id, interview_requirement, simulation_requirement="", max_agents=5, custom_questions: List[str]=None) -> InterviewResult`.
teri leaf param order is `(simulation_id, requirement, sim_req, max_agents: i64, custom_questions: Option<&str>)`
(`zep_tools.rs:1259-1266`). NOTE the type skew: Python `custom_questions` is `List[str]`, teri
models `Option<&str>`. The dispatch arm (`zep_tools.rs:1821-1839`) currently passes `None` always
(Python dispatch also never supplies `custom_questions` — `report_agent.py:1008-1021` only reads
`interview_topic`/`query` and `max_agents`). Keep `custom_questions = None` from the dispatcher;
model the leaf param as `Option<Vec<String>>` (or `&[String]`) for contract fidelity even though
the only caller passes empty.

Body steps (ALL must be ported):

1. `logger.info(console.interviewAgentsStart, requirement=interview_requirement[:50])`
   — char-slice `[:50]` (CJK-safe).
2. Build `result = InterviewResult(interview_topic=interview_requirement,
   interview_questions=custom_questions or [])`.
3. **Step 1 — load profiles**: `profiles = self._load_agent_profiles(simulation_id)`
   (`zep_tools.py:1505-1549`). Path = `{module}/../../uploads/simulations/{simulation_id}` →
   teri maps to `{config.oasis_simulation_data_dir}/{simulation_id}` (`config.rs:263`,
   default `./uploads/simulations`). Prefer `reddit_profiles.json` (JSON array, returned as-is);
   else `twitter_profiles.csv` via `csv.DictReader`, each row →
   `{realname: row["name"], username: row["username"], bio: row["description"],
   persona: row["user_char"], profession: "未知"}`. Read failures log a warning and fall through.
   Logs: `console.loadedRedditProfiles{count}`, `console.loadedTwitterProfiles{count}`,
   `console.readRedditProfilesFailed{error}`, `console.readTwitterProfilesFailed{error}`.
   (teri already has a profile-path pattern in `src/api/simulation.rs:1158-1160, 1901`.)
   - **Empty profiles guard** (`zep_tools.py:1313-1316`): `log console.profilesNotFound{simId}`,
     set `result.summary = "未找到可采访的Agent人设文件"`, **return `result`** (early).
4. `result.total_agents = len(profiles)`; `log console.loadedProfiles{count}`.
5. **Step 2 — select agents** (`_select_agents_for_interview`, `zep_tools.py:1551-1632`):
   build per-profile summary dicts
   `{index, name(realname||username||"Agent_{i}"), profession(||"未知"),
   bio(profile.bio[:200]), interested_topics(profile.get("interested_topics", []))}`;
   call `self.llm.chat_json` (temp **0.3**) with the verbatim Chinese system+user prompts
   (`zep_tools.py:1582-1610`, see §2c); read `selected_indices = resp.get("selected_indices",[])[:max_agents]`
   and `reasoning = resp.get("reasoning","基于相关性自动选择")`; filter to valid in-range indices
   (`0 <= idx < len(profiles)`), collecting `selected_agents` + `valid_indices`. On **any
   exception**: `log console.llmSelectAgentFailed{error}`, fallback = first `max_agents` profiles,
   `indices = range(min(max_agents, len))`, reasoning = `"使用默认选择策略"`.
   Returns `(selected_agents, selected_indices, reasoning)`.
   Set `result.selected_agents`, `result.selection_reasoning`;
   `log console.selectedAgentsForInterview{count, indices}`.
6. **Step 3 — questions** (`zep_tools.py:1338-1346`): if `not result.interview_questions`,
   `result.interview_questions = self._generate_interview_questions(...)`
   (`_generate_interview_questions`, `zep_tools.py:1634-1681`): `agent_roles = [a.profession||"未知"]`;
   `llm.chat_json` (temp **0.5**) with verbatim Chinese prompts (`zep_tools.py:1647-1665`, §2c);
   return `resp.get("questions", [f"关于{interview_requirement}，您有什么看法？"])`; on exception
   `log console.generateInterviewQuestionsFailed{error}` and return the 3-item Chinese fallback
   list (`zep_tools.py:1677-1681`). `log console.generatedInterviewQuestions{count}`.
7. **Build combined prompt** (`zep_tools.py:1348-1366`):
   `combined_prompt = "\n".join(f"{i+1}. {q}" for i,q in enumerate(result.interview_questions))`.
   Prepend the verbatim `INTERVIEW_PROMPT_PREFIX` (multi-line Chinese, `zep_tools.py:1351-1361`,
   §2c) → `optimized_prompt = f"{PREFIX}{combined_prompt}"`.
8. **Step 4 — batch interview** (`try` block, `zep_tools.py:1368-1462`):
   build `interviews_request = [{"agent_id": agent_idx, "prompt": optimized_prompt}
   for agent_idx in selected_indices]` (NO platform key → dual-platform).
   `log console.callingBatchInterviewApi{count}`. Call
   `SimulationRunner.interview_agents_batch(simulation_id=…, interviews=…, platform=None,
   timeout=180.0)` — teri: `runner.interview_agents_batch(simulation_id, interviews, None,
   Duration::from_secs_f64(180.0))` (S-631, `simulation_runner.rs:5340`). **NOTE the timeout: 180s
   here, NOT the method default 120s** — pass 180 explicitly.
   `log console.interviewApiReturned{count=api_result["interviews_count"], success}`.
   - **API failure guard** (`zep_tools.py:1386-1392`): if `not api_result.get("success", False)`:
     `error_msg = api_result.get("error","未知错误")`; `log console.interviewApiReturnedFailure{error}`;
     `result.summary = f"采访API调用失败：{error_msg}。请检查OASIS模拟环境状态。"`; **return `result`**.
   - **Step 5 — parse results** (`zep_tools.py:1394-1456`): `api_data = api_result.get("result",{})`;
     `results_dict = api_data.get("results",{}) if isinstance(api_data,dict) else {}`.
     For each `(i, agent_idx)` in enumerate(selected_indices):
     `agent = selected_agents[i]`; `agent_name = agent.realname||agent.username||f"Agent_{agent_idx}"`;
     `agent_role = agent.profession||"未知"`; `agent_bio = agent.bio||""`.
     Pull `twitter_result = results_dict[f"twitter_{agent_idx}"]||{}`,
     `reddit_result = results_dict[f"reddit_{agent_idx}"]||{}`; `*_response = *_result.get("response","")`.
     **`_clean_tool_call_response`** each (`zep_tools.py:1484-1504`, §2d). Then
     `twitter_text = twitter_response or "（该平台未获得回复）"` (same for reddit).
     `response_text = f"【Twitter平台回答】\n{twitter_text}\n\n【Reddit平台回答】\n{reddit_text}"`.
   - **Key-quote extraction** (`zep_tools.py:1421-1452`) — preserve EXACTLY, regexes are contractual:
     `combined_responses = f"{twitter_response} {reddit_response}"`; then sequential `re.sub`:
     `#{1,6}\s+`→"", `\{[^}]*tool_name[^}]*\}`→"", `[*_`|>~\-]{2,}`→"",
     `问题\d+[：:]\s*`→"", `【[^】]+】`→"" (in this order).
     Strategy 1: `sentences = re.split(r'[。！？]', clean_text)`; keep `s.strip()` where
     `20 <= len(s.strip()) <= 150` AND `not re.match(r'^[\s\W，,；;：:、]+', s.strip())` AND
     `not s.strip().startswith(('{','问题'))`; sort by `len` **descending**; take first 3, each
     `+ "。"` → `key_quotes`.
     Strategy 2 (only if Strategy 1 empty): `re.findall(r'“([^“”]{15,100})”', clean_text)` +
     `re.findall(r'「([^「」]{15,100})」', clean_text)`; drop those matching `^[，,；;：:、]`; take 3.
     `len(...)` throughout is **character** count.
   - Build `AgentInterview(agent_name, agent_role, agent_bio=agent_bio[:1000],
     question=combined_prompt, response=response_text, key_quotes=key_quotes[:5])`;
     `result.interviews.append(...)`. After loop `result.interviewed_count = len(result.interviews)`.
   - **Exception ladder** (`zep_tools.py:1457-1468`):
     `except ValueError as e` (env-not-running): `log console.interviewApiCallFailed{error}`,
     `result.summary = f"采访失败：{str(e)}。模拟环境可能已关闭，请确保OASIS环境正在运行。"`, return.
     `except Exception as e`: `log console.interviewApiCallException{error}`, log traceback,
     `result.summary = f"采访过程发生错误：{str(e)}"`, return.
     **teri mapping:** `interview_agents_batch` returns `Result<IPCResponse>`; `Err(TeriError::Sim
     "...envNotRunning..."` / not-found) maps to the **ValueError** branch (env-not-running message);
     any other `Err` → the generic Exception branch. The `api_result["success"]==false` path
     (above) is the in-band failure (distinct from a thrown error). Adjudicate exact mapping at
     port time against the `IPCResponse` shape (`send_batch_interview`).
9. **Step 6 — summary** (`zep_tools.py:1470-1478`): if `result.interviews`,
   `result.summary = self._generate_interview_summary(interviews, interview_requirement)`
   (`_generate_interview_summary`, `zep_tools.py:1683-1763`): if no interviews → `"未完成任何采访"`;
   else build `interview_texts = [f"【{name}（{role}）】\n{response[:500]}"]`;
   `quote_instruction` is **locale-dependent**: `get_locale()=='zh'` → `引用受访者原话时使用中文引号「」`
   else `Use quotation marks "" when quoting interviewees` (teri: use the i18n locale check);
   `llm.chat` (temp **0.3**, max_tokens **800**) with verbatim Chinese system+user prompts
   (`zep_tools.py:1700-1730`, §2c); on exception `log console.generateInterviewSummaryFailed{error}`
   and fallback `f"共采访了{len(interviews)}位受访者，包括：" + "、".join(agent_names)`.
10. `log console.interviewAgentsComplete{count=result.interviewed_count}`; `return result`.

### 2c. Verbatim Chinese literals the porter MUST keep byte-identical
- `INTERVIEW_PROMPT_PREFIX` (`zep_tools.py:1351-1361`) — full 6-rule multi-line block.
- agent-selection system prompt + user prompt template (`zep_tools.py:1582-1610`), incl. the
  JSON-return contract block.
- question-generation system + user prompts (`zep_tools.py:1647-1665`).
- summary system prompt (with `{quote_instruction}` interpolation) + user prompt
  (`zep_tools.py:1700-1730`).
- All `result.summary` strings: `未找到可采访的Agent人设文件`,
  `采访API调用失败：{error_msg}。请检查OASIS模拟环境状态。`,
  `采访失败：{str(e)}。模拟环境可能已关闭，请确保OASIS环境正在运行。`,
  `采访过程发生错误：{str(e)}`, `共采访了{n}位受访者，包括：…`.
- placeholders: `（该平台未获得回复）`, `（无摘要）`, `（无采访记录）`, `（自动选择）`.
- `to_text` skeleton headers: `## 深度采访报告`, `**采访主题:**`,
  `**采访人数:** {x} / {y} 位模拟Agent`, `### 采访对象选择理由`, `### 采访实录`,
  `#### 采访 #{i}: {name}`, `### 采访摘要与核心观点`.
- fallbacks: `关于{requirement}，您有什么看法？` + the 3-item question list;
  `基于相关性自动选择`, `使用默认选择策略`, `未知`, `Agent_{i}`.
- `response_text` markers: `【Twitter平台回答】`, `【Reddit平台回答】`.
- tool description `TOOL_DESC_INTERVIEW_AGENTS` (`report_agent.py:523-548`) — verify already
  ported at `zep_tools.rs:1414` (`get_tools_description`).
- i18n console keys (already in teri's locale tables — verify present):
  `console.interviewAgentsStart/profilesNotFound/loadedProfiles/loadedRedditProfiles/
  loadedTwitterProfiles/readRedditProfilesFailed/readTwitterProfilesFailed/
  selectedAgentsForInterview/llmSelectAgentFailed/generatedInterviewQuestions/
  generateInterviewQuestionsFailed/callingBatchInterviewApi/interviewApiReturned/
  interviewApiReturnedFailure/interviewApiCallFailed/interviewApiCallException/
  generateInterviewSummaryFailed/interviewAgentsComplete`.

### 2d. `_clean_tool_call_response` (`zep_tools.py:1484-1504`, static)
If response empty or `strip()` doesn't start with `{` → return as-is. If `'tool_name' not in
text[:80]` → return as-is. Else `json.loads(text)`; if dict with `'arguments'`, for key in
`('content','text','body','message','reply')` return `str(data['arguments'][key])` (first hit).
On JSON/Key/Type error, regex `"content"\s*:\s*"((?:[^"\\]|\\.)*)"` → group(1) with `\n`→newline,
`\"`→`"`. Else return original.

---

## 3. THE ADDITIVE SEAM DESIGN (REACHABLE → wire it now)

### 3a. Struct field (additive, non-breaking)
In `ReportTools<'g, L>` (`zep_tools.rs:498-506`) add:
```rust
/// Optional live simulation runner for interview_agents IPC dispatch (U-024).
/// None for the 4 graph-only construction sites (debug routes, tests) — those
/// keep `new(graph, llm)`. Some(...) only on the report-generation routes that
/// can reach a live sim. `&'g` runner: caller-owned (Arc held by ApiState).
runner: Option<&'g SimulationRunner<L>>,
```
(`SimulationRunner<L>` — generic param matches `ReportTools`'s `L`; at the route the concrete
`L = OpenAiAdapter` from `ApiState.sim_runner`, and `build_llm` already yields `OpenAiAdapter`,
per `src/api/mod.rs:295`. Confirm the type unifies at port time; if `build_llm`'s `L` and the
runner's `L` diverge, fall back to a borrowed trait object or thread only the `runs`/IPC handle.)

### 3b. Constructor (keep `new` byte-compatible)
```rust
pub fn new(graph: &'g KnowledgeGraph, llm: &'g L) -> Self {
    Self::with_runner(graph, llm, None)
}
pub fn with_runner(graph: &'g KnowledgeGraph, llm: &'g L,
                   runner: Option<&'g SimulationRunner<L>>) -> Self {
    let reader = KnowledgeGraphEntityReader::new(graph);
    Self { graph, llm, reader, runner }
}
```
All 4 existing `::new` callers + ~30 tests stay unchanged. The 3 report routes that want live
interviews (`report.rs:517` is a *debug* route — leave on `new`; the live sites are the
report-generation path and `chat_route:698`) switch to
`ReportTools::with_runner(&graph, &llm, Some(&state.sim_runner))`.

### 3c. Profiles directory
`_load_agent_profiles` needs the sim-data dir. Two options (porter picks the smaller):
(a) add `pub fn sim_data_dir(&self) -> &Path` accessor to `SimulationRunner`
(`simulation_runner.rs:995` field is private) and read it from the threaded `runner`; OR
(b) thread `config.oasis_simulation_data_dir` (`config.rs:263`) as a third borrow on `ReportTools`
(less clean — prefer (a), one accessor, zero new fields beyond the runner).

---

## 4. SYNC/ASYNC RESOLUTION

The leaf **must become `async`** (it awaits `interview_agents_batch` + 3 LLM calls via
`chat`/`chat_json`, all `async` on the `LlmClient` trait, `src/llm.rs:282-288`).

The dispatcher chain (`execute_inner`/`execute`/`execute_by_name`) is **sync** and is called sync
at exactly **two production sites**, both already inside `async fn`:
`generate_section_react` (`report/mod.rs:1246`) and `chat` (`report/mod.rs:2242`).

**Decision — localized async pre-dispatch (do NOT make the whole match async):** the other 6 tools
are sync-pure (graph reads). Converting all of `execute_inner` to `async fn` would force `.await`
on ~30 sync tests and add no value. Instead:

- Add a thin `pub async fn execute_by_name_async(&self, name, params, …) -> String` that handles
  ONLY the `interview_agents` name by `.await`ing the async leaf, and **delegates every other name
  to the existing sync `execute_by_name`**. (Mirrors how Python's single `_execute_tool` is async-
  capable but only this branch needs IPC.)
- At the two production dispatch sites, replace `tools.execute_by_name(...)` with
  `tools.execute_by_name_async(...).await`. Both are in `async fn` → no signature break to the
  loop, no change to the other 3 tools, no `Send` regression (the only borrow held across the await
  is `&self`/`&runner`/`&llm`, all `Sync`; `self.runs.lock().await` inside the runner is the only
  lock and it is dropped before returning — consistent with `report.rs`'s "no borrow across await"
  discipline noted at `report.rs:462-464`).
- Keep the sync `interview_agents` honest-error variant? **No** — replace it. The sync
  `ReportTool::InterviewAgents` arm in `execute_inner` (`zep_tools.rs:1820-1839`) becomes
  unreachable for the async path; either (i) route `InterviewAgents` out of the sync `execute_inner`
  (return an internal "use async path" marker, never hit in prod because the async dispatcher
  intercepts it first) or (ii) keep the sync arm returning the honest error ONLY for the
  `runner: None` facades (debug routes/tests that call sync `execute_by_name`). Option (ii) is the
  no-downgrade-safe default: sync callers without a runner still get the honest tolerated error;
  live callers go through `execute_by_name_async`.

(Alternative considered & rejected: make `execute_inner` fully `async`. Rejected — ~30 sync test
call sites + the 6 sync-pure tools would all need needless `.await`; larger blast radius, no
behavioral gain.)

---

## 5. PORT PLAN (ordered) + PARITY ORACLE

### Ordered steps for the porter
1. **Widen the DTOs first** (§2a): replace teri `InterviewResult` (`zep_tools.rs:453-480`) and
   `AgentInterview` (`~410`) with the FULL Python field sets + faithful `to_dict`/`to_text`
   (byte-identical headers, key-quote cleaning, locale-aware bits). Update the 3 narrow fields'
   existing callers. This removes the hidden `[≠]` downgrade. **Verify parity on `to_text`/`to_dict`
   BEFORE wiring the body.**
2. **Add the seam** (§3): `runner` field + `with_runner` ctor (keep `new`); `sim_data_dir`
   accessor on `SimulationRunner`. Confirm `cargo check` — the 4 `::new` callers + tests unchanged.
3. **Port the 5 helpers** as private async (or sync where no LLM) methods on `ReportTools`:
   `load_agent_profiles` (sync, fs+csv+json), `clean_tool_call_response` (sync, pure),
   `select_agents_for_interview` (async, `chat_json` temp 0.3), `generate_interview_questions`
   (async, `chat_json` temp 0.5), `generate_interview_summary` (async, `chat` temp 0.3/max 800).
   Keep every Chinese literal verbatim (§2c).
4. **Port `interview_agents`** as `async fn` per §2b — the full 10-step body, all guards, the
   180s timeout, the exception ladder mapped onto `Result<IPCResponse>` from `interview_agents_batch`.
5. **Async dispatch** (§4): add `execute_by_name_async`; switch the two production sites
   (`report/mod.rs:1246, :2242`) to `.await` it; resolve the sync `InterviewAgents` arm per §4(ii).
6. **Wire live routes**: switch the report-generation route + `chat_route` (`report.rs:698`) to
   `ReportTools::with_runner(&graph, &llm, Some(&state.sim_runner))`. Leave debug `tools_*` routes
   (`report.rs:517,551`) on `new` (no sim context).
7. Replace the stub test (`zep_tools.rs:2511, 2886`) expectations; add the parity tests below.
8. Update the ledger: flip the `[!]` U-024 frontier; record the DTO-widening as the corrected
   `[≠]`→PORTED. The IPC terminal call remains **producer-pending** (`[!]` U-026-k: live IPC
   producer not yet landed → env-not-running today). Per the no-downgrade rule this is STILL a full
   faithful port — the live-data flip happens when producers land; "producer-gated" is NOT a reason
   to keep the stub.

### Parity oracle (for the verifier)
Differential against Python `ZepToolsService.interview_agents` + helpers. Stub the runner/LLM
identically on both sides; diff `InterviewResult.to_text()` AND `.to_dict()` byte-for-byte.
Fixtures the verifier should drive (each = same inputs → Python output vs teri output):
- **No profiles** → `summary == "未找到可采访的Agent人设文件"`, empty interviews, early return.
- **Reddit JSON profiles** vs **Twitter CSV profiles** → identical `selected_agents` shape +
  `total_agents`.
- **LLM selection failure** → fallback first-N selection, `reasoning == "使用默认选择策略"`.
- **`api_result.success == false`** → `summary == "采访API调用失败：{err}。请检查OASIS模拟环境状态。"`.
- **env-not-running (ValueError)** → `summary == "采访失败：…。模拟环境可能已关闭…"`.
- **Happy path** (mock batch returns twitter_/reddit_ responses incl. a `tool_name`-wrapped JSON
  reply) → identical `response_text` (dual-platform markers), identical `_clean_tool_call_response`
  unwrap, identical `key_quotes` (Strategy 1 sentence-split + sort-by-len-desc; Strategy 2 quote
  fallback), identical `agent_bio[:1000]` truncation, identical `summary` from
  `generate_interview_summary` (mock LLM).
- **`to_text` key-quote edge cases**: quotes containing `问题{1-9}` skipped; >150-char truncation at
  first `。` after pos 80; `len < 10` dropped; quote-char/leading-punct stripping.
Oracle harness: run the Python helper directly (it is pure given mocked `self.llm` + fixture files)
and capture golden `to_dict`/`to_text` strings; teri must reproduce them exactly.

---

## Risk flags (for the no-downgrade gate)
- `[!] U-024-PROD-PENDING`: terminal IPC interview returns env-not-running until producers land
  (U-026-k). Full logic ported; live data deferred to producer arrival. Tracked, reasoned, NOT a
  skip.
- `[≠]→PORTED` DTO widening: the current narrowed `InterviewResult`/`AgentInterview` is a hidden
  downgrade being CORRECTED here, not preserved. The parity gate must confirm the full shape.
- `[≠]` candidate `_load_agent_profiles` path base: Python `os.path.dirname(__file__)/../../uploads`
  → teri `config.oasis_simulation_data_dir`. Same effective directory under default config; flag
  only if the verifier finds a path divergence (non-contractual — both resolve the same files).
- Type-unification risk on `runner: Option<&SimulationRunner<L>>` (§3a) — if `build_llm`'s `L` and
  the runner's `L` can't unify, port-time fallback = borrow the IPC/`runs` handle instead. Adjudicate
  at step 2.
