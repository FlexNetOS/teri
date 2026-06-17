# Parity Verdict Trail

Audit record proving no-downgrade across the MiroFish → teri port.
Append-only: each dated block is a witnessed verdict from the parity gate.

---

## 2026-06-14 · U-015 · `GraphBuilderService` (Zep graph-build) → `KnowledgeGraph::build()`

**Verdict: PASS** (map-onto-substrate, behavioral-equivalence verified) — with **one tracked `- [!]` follow-up** (chunking, owned by U-013, not a U-015 downgrade).

**Type:** map-onto-substrate. Source X delegates to Zep Cloud (external SaaS, not runnable here), so this is behavioral-equivalence of the mapped petgraph extraction path, not a literal Zep diff. Each SOURCE branch behavior was read from `graph_builder.py` + `file_parser.py` (not assumed) and compared to the Rust `build()` at `src/graph/mod.rs:237`.

**Baseline / no-downgrade:** full `cargo test` = **156 passed, 2 ignored** — exactly the TRUE current baseline (the DISCOVER "142 on c894de8" was false-green; c894de8 was a non-compiling bad merge). No regression.

### Per-branch differential (source behavior → Rust behavior)

| Branch | SOURCE (MiroFish) behavior — evidence | Rust `build()` behavior — evidence | Result |
|---|---|---|---|
| Happy path | Zep resolves entities + edges from episodes; `_get_graph_info` reports node/edge counts. Contract: "Alice/Acme/NY" + rel set → 3 ent + 2 rel, Alice→Acme edge | 3-ent contract: `test_entity_extraction_with_mock_llm` (3 ent); 2-rel: `test_relation_extraction_with_mock_llm`; full pipeline `test_build_from_seed_document` (2 ent/1 rel, `neighbors[0]=="Acme Corp"`) + `test_graph_construction_with_mock_llm` (Alice.neighbors=[Bob]) | **PASS** |
| Empty extraction | `_wait_for_episodes` `if not episode_uuids: return` (graph_builder.py:354); `split_text_into_chunks` returns `[]` for empty/whitespace (file_parser.py) → completes, NOT an error | `if graph.entity_count()==0 { return Ok(graph) }` (mod.rs:254) — valid empty graph | **PASS** `test_build_empty_extraction_tolerates` |
| Duplicate entity | Zep entity-resolution merges same-name nodes; no fault | `if !graph.index.contains_key(&entity.name)` guards add (mod.rs:248); first-wins, no abort | **PASS** `test_build_duplicate_entity_is_skipped` (1 ent, kind=Person) |
| Unknown-entity relation | Zep only edges resolved nodes; dangling ref doesn't fault | inline `match graph.index.get(from_name){Some=>..,None=>continue}` (mod.rs:280-287) — skip, not fault. **Correctly uses the tolerant inline path, NOT `parse_relations_json` (which faults on unknown)** | **PASS** `test_build_unknown_entity_relation_is_skipped` (1 ent, 0 rel) |
| LLM error | `_build_graph_worker` catches all → `fail_task` (graph_builder.py:188-191): error surfaced via task status, not swallowed | `llm.complete(...).await?` propagates `TeriError` (mod.rs:242,262) | **PASS** `test_build_propagates_llm_error` |

All 5 U-015 tests confirmed RUN (not filtered): `test result: ok. 35 passed; 0 failed`.

### Symbol rollup — U-015 (S-181..S-197)

Rule: a unit PASS needs every symbol `- [x]`/`- [≠]` (covered) or explicitly mapped-to-another-unit. Outcome: **17/17 accounted for — 0 dropped, 0 unmapped gaps.**

- **`- [x]` covered by `build()`:** **S-190** (`_build_graph_worker` → the create→ontology→split→batch→wait→info→complete worker) — its **extraction-pipeline essence** (2-pass entity→relation, dedup, unknown-skip, empty-tolerance, error-propagation) is verified above. Flipped `- [~]` → `- [x]`.
- **`- [≠]` intentional-divergence (Zep SaaS / not-applicable-to-in-process-petgraph), already adjudicated in symbol-map with rationale + rust-target mapping:** S-181..S-188, S-191, S-192, S-195, S-196, S-197 (GraphInfo type/fields/to_dict → `entity_count()`/`relation_count()`/`serialize_to_json()`; `__init__` → llm-as-generic-param; `create_graph`/`set_ontology`/`_get_graph_info`/`get_graph_data`/`delete_graph` → Zep SaaS calls with no in-process equivalent). These are owner-style explicit divergences, NOT silent drops.
- **Distributed to OTHER units (Zep-async mechanics not needed on synchronous petgraph), cited:**
  - **S-189** `build_graph_async` (background-thread → task_id) → task-management layer = **U-012 `TaskManager`** (`backend/app/models/task.py`, ledger Layer 2, still `- [ ]`). Marked `- [≠]` in symbol-map (task layer not yet ported; `build()` is async/caller-managed). **Not dropped — tracked at U-012.**
  - **S-193** `add_text_batches` (1s sleep between batches) + **S-194** `_wait_for_episodes` (poll processed=True / 600s timeout) → Zep episode-batching/async-processing mechanics; on in-process petgraph the LLM call is synchronously awaitable so there is no episode lifecycle. `- [≠]` with rationale. **Correctly not reimplemented.**
  - **split_text / chunking** (called from `_build_graph_worker` step 3) → owned by **U-013 `TextProcessor.split_text`** (`text_processor.py` → `teri::services::text_processor`, ledger Layer 3, still `- [ ]`). See chunking adjudication below.

### Chunking adjudication (CRITICAL no-downgrade check)

MiroFish `split_text_into_chunks(chunk_size=500, overlap=50)` (file_parser.py:161) chunks text **before Zep ingestion**; the ledger labels it "for Zep episode batching." `build()` extracts over the WHOLE doc.

- **Primary purpose = Zep-episode-size artifact** (fine to omit for in-process petgraph — there is no episode size limit).
- **BUT it also bounds per-call text size**, which on a local/real LLM is genuine context-overflow protection for large docs. So it is **not purely cosmetic.**
- **Ownership:** the chunking *logic* is a distinct unit — **U-013 `TextProcessor.split_text`** — which is **`- [ ]` not yet ported.** Chunking is therefore **DISTRIBUTED to U-013, not silently dropped from the port.**
- **Verdict:** does NOT block U-015 (the extraction-pipeline contract is fully met for in-scope doc sizes, and chunking was never a `build()`-internal behavior in the symbol contract — it was a sibling worker step). **It IS recorded as a `- [!]` extend-`build()` follow-up:** when U-013 lands, `build()` should chunk large docs (split → extract per chunk → merge, dedup across chunks) before LLM extraction, to avoid context overflow on big inputs. Tracked below.

### Gaps for next cycle (`- [!]`)

- **`- [!]` GAP-U015-1 (chunking, deferred to U-013):** `build()` extracts over the whole doc; large docs (> LLM context) will overflow. Resolution lands with **U-013** (`text_processor.split_text`) — at that point extend `build()` to chunk→extract-per-chunk→merge. NOT a U-015 blocker (sibling-worker behavior, owned elsewhere); flagged so it is not forgotten. No owner-approval needed (it is a real future behavior, correctly sequenced after its owning unit).

### Result

PASS. Every `build()` branch matches MiroFish's resilience contract; 17/17 U-015 symbols are covered (`- [x]`/`- [≠]`) or explicitly distributed to U-012/U-013 with citations; chunking adjudicated (distributed to U-013, `- [!]` follow-up logged); 156-test baseline intact. U-015 flips `- [x]`; S-190 flips `- [x]`; merge-ledger U-015 → `- [x]` (verified-in-teri).

## 2026-06-14 · ITERATE cycle 2 · GAP-1 (`Relation.valid_at`) + GAP-2 (`query_vec_similarity`) — substrate de-stub

**Verdict: PASS (both capabilities)** — map-onto-substrate behavioral-equivalence (Zep is external SaaS, not runnable; verified the mapped Rust path + correctness, not a literal Zep diff). GAP-OQ3-EMBED stays `- [!]` (honestly deferred).

**Baseline / no-downgrade:** `cargo test` = **171 passed, 2 ignored, 0 failed** (lib 162 + bins 4+3+2). 156-test baseline intact; the 156 reference is NOT regressed (lib grew 156→162 with the 6 new memory tests; 10 new graph tests bring graph coverage; cross-binary total 171). All 16 cycle-2 tests confirmed RUN by name (not filtered). Clippy clean (lib).

### Claim A — `Relation.valid_at` (GAP-1/OQ-2), src/graph/mod.rs

| Check | SOURCE contract (zep_tools.py) | Rust behavior + test | Result |
|---|---|---|---|
| `is_active_at` None | n/a (always-valid edge) | `None → true` for t=0/MAX/now · `test_relation_is_active_at_none_always_true` | PASS |
| `is_active_at` open-ended | `valid_at` set, `invalid_at`/`expired_at` None → active | `Some((s,None)) → t>=s` (999✗/1000✓/9999✓) · `..._open_ended` | PASS |
| `is_active_at` closed window | edge `is_expired`/`is_invalid` → historical | `Some((s,Some(e))) → s<=t<e` half-open (999✗/1000✓/1500✓/2000✗/9999✗) · `..._closed_window` | PASS |
| active/historical split | `panorama_search` (zep_tools.py:1185-1206): `is_historical = is_expired or is_invalid` → active_facts vs historical_facts | `partition_edges_at(t)` → (active, historical); expired edge lands historical · `test_partition_edges_at` | PASS |
| **serde backward-compat (no-downgrade)** | n/a | OLD JSON `{"kind":"RelatedTo","weight":0.5}` (no valid_at) → deserializes, `valid_at=None` · `test_relation_serde_backward_compat_no_valid_at_field` (+ full graph JSON+bincode roundtrip) | **PASS** |
| weight validation parity | n/a | `with_validity` rejects weight 1.5 same as `::new` · `test_relation_with_validity_weight_validation` | PASS |
| LLM-JSON parse | edges carry temporal fields | array `[s,e]`/`[s,null]` + object `valid_from`/`valid_until` + graceful None · `..._array_form`/`..._object_form` | PASS |

**Active/historical adjudication:** the half-open `[start,end)` rule is the correct Rust analogue of Zep's `is_expired or is_invalid → historical` boolean — an edge whose window has closed (`t>=end`) is exactly Zep's expired edge. None=always-active maps Zep's null-temporal (always-valid) edge. CORRECT.

### Claim B — `query_vec_similarity` (GAP-2/OQ-3), src/memory/mod.rs

| Branch | Rust behavior + test | Result |
|---|---|---|
| empty store | `Ok(vec![])` · `test_query_vec_similarity_empty_store_returns_empty` | PASS |
| ranking | query `[0,1,0]`: B identical→1st, C `[0.6,0.8,0]`→2nd, A orthogonal→last · `test_query_vec_similarity_ranking` | PASS |
| top_k limiting | 5 stored, top_k=2 → 2 returned · `..._top_k_limiting` | PASS |
| top_k ≥ available | 3 stored, top_k=100 → all 3 · `..._top_k_ge_available_returns_all` | PASS |
| dimension mismatch | 3-dim entry skipped when query is 2-dim; 2-dim returned — no crash · `..._dimension_mismatch_skipped` | PASS |
| zero-norm | code skips entry_norm==0.0 and query_norm==0.0 (mod.rs:323,364) — no div-by-zero/NaN; exercised implicitly (test fixtures use i+1 to avoid zero) | PASS |
| identical → ~1.0 | unit vec `[0.6,0.8]` query==entry → ranks first/only · `..._identical_vector_similarity_near_one` | PASS |

**Cosine adjudication (CRITICAL — magnitude, not dot):** the in-tree tests use unit-norm/co-linear vectors so they alone don't distinguish cosine from raw dot. Verified the formula `dot/(query_norm·entry_norm)` (mod.rs:374) by an **independent differential**: query `[1,1,0]`, stored P=`[10,0,0]` (high magnitude, wrong direction) vs Q=`[0.7,0.71,0]` (aligned). Raw dot → P wins (10 > 1.41, WRONG). Cosine → Q wins (0.997 > 0.707, CORRECT). The implementation is genuine magnitude-normalized cosine. Reproduces the SEARCH half of Zep `insight_forge`/`quick_search` (`search_graph` zep_tools.py:464 returns relevance-ranked, `limit`-capped facts).

### Claim C — GAP-OQ3-EMBED honestly flagged (no fake-pass)

CONFIRMED. Parity-ledger row `- [!] GAP-OQ3-EMBED` exists and is untouched (left `- [!]`). `query_vec_similarity` takes a **precomputed** `query_embedding: &[f32]` (mod.rs:312) — the SEARCH half. The GENERATION half (text→vector) is genuinely deferred: `grep -rniE` over `src/` for any embed/vectorize/fake/random generator returns **none** — every `embedding:` literal is a test fixture or the `VectorEntry` field; `config.embed_model="text-embedding-3-small"` is an unused config string with no generator consuming it. **No silent fake/random embedder exists.** This is a correct, owner-visible deferral, not a downgrade.

### Result

**PASS.** Both capabilities correct; every branch tested; serde backward-compat holds (no-downgrade); cosine is genuine magnitude-normalized; no fake embedder; 156 baseline not regressed (171 total green, clippy clean). Flipped: symbol-map S-G1-001..006 + S-G2-001 → `- [x]`. parity-ledger GAP-1 + GAP-2 → `- [x]` (ENABLE U-017/U-021/U-024, which remain `- [ ]`). GAP-OQ3-EMBED stays `- [!]`.

## 2026-06-14 · ITERATE cycle 3 · GAP-ACTION-TAXONOMY (`Action::Social(SocialAction)`) — MiroFish/OASIS social action taxonomy

**Verdict: FAIL (taxonomy incomplete + one semantic narrowing).** Default-skeptical, fail-closed. The 11+DoNothing variants that ARE present parse/Display/serde/apply correctly and the generic-5 + GAP are honest — but the taxonomy DROPS one agent-chosen action (TREND) and NARROWS a behavior U-021 needs (Like/Dislike post-vs-comment). Unit stays `- [~]`; S-TAX-001..018 stay `- [~]` (NOT flipped). Build is GREEN (209 passed, 2 ignored, 0 failed; clippy clean) — green build is necessary, not sufficient.

**Baseline / no-downgrade:** `cargo test` (raw) = **209 passed, 2 ignored, 0 failed** (lib 200 + main 0 + graph_integration 4 + memory_tests 3 + 2). 171 reference NOT regressed (lib grew with 38 new social tests). 16 sim-side social tests confirmed RUN by name (184 filtered out); 25+ agent-side `parse_social_*`/`social_memory_importance_*`/`unknown_social_action` tests enumerated and present. Clippy: no issues.

### Adjudication 1 — TREND / REFRESH omission → **SPLIT verdict** (REFRESH correct, TREND is a DROPPED action)

| Action | Agent-selectable? | Reaches activity log / `to_episode_text`? | Source evidence | Verdict |
|---|---|---|---|---|
| **REFRESH** | yes (`agent_action.py:133` `perform_action(None, ActionType.REFRESH.value)`; `REDDIT_ACTIONS:198`) | **NO** — `run_parallel_simulation.py:611` `FILTERED_ACTIONS = {'refresh','sign_up'}` → `continue` at :699-700 before the action is appended to `actions` (:735). Never becomes an `AgentActivity`, never rendered. | config.py:58; run_parallel_simulation.py:611,699-700 | **OMISSION CORRECT → record `- [≠]`** (env/UI poll op, produces no agent activity) |
| **TREND** | yes (`agent_action.py:507` `perform_action(None, ActionType.TREND.value)`; `REDDIT_ACTIONS:197`) | **YES** — NOT in `FILTERED_ACTIONS`; IS in `ACTION_TYPE_MAP` (`'trend':'TREND'`, :627) → passes filter → appended to `actions` (:735) → `AgentActivity` → `to_episode_text` dispatches to `_describe_generic` ("performed TREND operation") since no `TREND` key in the 12-type table (zep_graph_memory_updater.py:43-56). | config.py:58; run_parallel_simulation.py:197,627,730,735; agent_action.py:507; zep_graph_memory_updater.py:43-56,197-199 | **DROPPED ACTION = DOWNGRADE.** TREND is an agent-chosen action that produces recorded agent activity. Must be added (e.g. `SocialAction::ViewTrending` / `Trend`, no-target). |

The porter's in-code rationale ("`TREND` and `REFRESH` … are platform config hints for OASIS internals, not agent-observable actions recorded in the activity log", sim/mod.rs:17-20) is **factually correct for REFRESH but wrong for TREND** — TREND survives `FILTERED_ACTIONS` and IS recorded. This is exactly the no-downgrade trap: the two were lumped together but the source treats them differently.

### Adjudication 2 — `LIKE_POST + LIKE_COMMENT → one Like{target_id}` (and Dislike) → **SEMANTIC NARROWING (downgrade)**

`to_episode_text` renders the two distinctly: `_describe_like_post` → "点赞了{author}的帖子：「{post_content}」" (liked **post**) vs `_describe_like_comment` → "点赞了{author}的评论：「{comment_content}」" (liked **comment**) (zep_graph_memory_updater.py:70-81 vs 153-164). The source IDs are **separate namespaces** — `_enrich_action_context` keys LIKE on `post_id` for posts (:766-772) while comment likes carry `comment_id`/`like_id` (config simplified_args :714,724). Collapsing both into a single untyped `Like{target_id}` **erases the post-vs-comment discriminant**, so U-021 cannot reconstruct which episode-text branch to emit. **This is a narrowing of a behavior a downstream unit needs.** Fix: add a target-kind discriminant — `Like{target_kind: TargetKind, target_id}` (TargetKind::Post|Comment) or split into `LikePost`/`LikeComment` (same for Dislike). **Comment←CREATE_COMMENT is FINE** (its parent is always a post; `post_id` is the correct single key — `_describe_create_comment` only ever references a post, :137-151).

### Per-variant taxonomy completeness (what IS present — verified correct)

| Variant | Parse (str→SA) | Display intent | apply no-panic | importance | serde | Result |
|---|---|---|---|---|---|---|
| CreatePost{content} ←CREATE_POST | bare + key=value (`test_parse_social_create_post_*`) | "Posted: …" | ✓ (11-case test) | 0.85 (content-creation) | ✓ roundtrip | PASS |
| Like{target_id} ←LIKE_POST+LIKE_COMMENT | both names parse (`like_post`,`like_comment`) | "Liked: …" | ✓ | 0.30 | ✓ | **NARROWED** (adj. 2) |
| Dislike{target_id} ←DISLIKE_POST+DISLIKE_COMMENT | both parse | "Disliked: …" | ✓ | 0.30 | ✓ | **NARROWED** (adj. 2) |
| Repost{post_id} ←REPOST | bare + kv | "Reposted: …" | ✓ | 0.65 | ✓ | PASS |
| Quote{post_id,content} ←QUOTE_POST | kv | "Quoted post …: …" | ✓ | 0.70 | ✓ | PASS |
| Follow{user_id} ←FOLLOW | bare + kv | "Followed user: …" | ✓ | 0.75 | ✓ | PASS |
| Comment{post_id,content} ←CREATE_COMMENT | kv | "Commented on …: …" | ✓ | 0.70 | ✓ | PASS (parent=post, correct) |
| SearchPosts{query} ←SEARCH_POSTS | bare + kv | "Searched posts: …" | ✓ | 0.25 | ✓ | PASS |
| SearchUser{query} ←SEARCH_USER | bare + kv | "Searched user: …" | ✓ | 0.25 | ✓ | PASS |
| Mute{user_id} ←MUTE | bare + kv | "Muted user: …" | ✓ | 0.75 | ✓ | PASS |
| DoNothing ←DO_NOTHING | parses | "Did nothing" | ✓ | 0.05 | ✓ | PASS |
| **TREND** | **ABSENT** | — | — | — | — | **MISSING (adj. 1 — dropped action)** |

Importance weights are sensible & monotone with behavioural significance (creation > graph-mod > amplification > passive > search > no-op). Display strings are readable-English placeholders; exact `to_episode_text` natural-language fidelity is correctly deferred to U-021 (noted in code :380).

### Generic-5 intact + GAP honest (both CONFIRMED)

- **Generic-5 unaltered:** `test_generic_actions_still_intact` asserts Speak/Move/Interact/Observe/Think still push events and compare-equal (sim/mod.rs:949-968); the parser's generic match arm (agent/mod.rs:296-303) is unchanged and returns BEFORE the social path; `store_action_in_memory` generic arms (Speak 0.6/0.8, Move 0.7, Interact 0.8, Observe 0.5, Think 0.4/0.9) untouched. The nested-enum design (`Action::Social(SocialAction)`) adds exactly one arm, no churn to the 5. **No-downgrade on generics: CONFIRMED.**
- **GAP-SOCIAL-WORLDSTATE honest:** symbol-map row `- [!] S-SWS-001` records it as DEFERRED; `apply`/`apply_at` route social actions through the SAME generic `events.push(Event{...})` path (sim/mod.rs:153-161) — no fake social-world-state (no timeline/post-store/follower-graph stub). `test_action_social_apply_no_panic` proves all 11 route without panic (:922-947). U-022/028/029/030 stay `- [ ]`. **GAP honestly `- [!]`: CONFIRMED.**

### Result

**FAIL.** Two no-downgrade defects block the taxonomy `- [x]`:
1. **TREND dropped** (agent-chosen action that IS recorded → add a no-target `SocialAction` variant; record REFRESH as `- [≠]`).
2. **Like/Dislike narrowed** (post-vs-comment discriminant erased, needed by U-021 → add `target_kind` or split variants).

The 11 present variants are otherwise correct (parse/Display/apply/importance/serde all verified); generic-5 intact; GAP honest; 171 baseline not regressed (209 green). **S-TAX-001..018 stay `- [~]`** (NOT flipped). **Routes back to porter** with the two exact fixes above. U-022/028/029/030 remain `- [ ]` and should NOT be marked taxonomy-ready until TREND + the Like/Dislike discriminant land (U-021 depends on the discriminant).

## 2026-06-14 · ITERATE cycle-3 RE-VERIFY · GAP-ACTION-TAXONOMY — both FAIL defects re-verified

**Verdict: PASS (FAIL→PASS).** The prior cycle-3 FAIL had 2 exact no-downgrade defects (TREND dropped; Like/Dislike post-vs-comment collapsed). The porter applied both fixes; this focused re-verify reads `src/sim/mod.rs` + `src/agent/mod.rs`, runs the differential, and confirms both are resolved with nothing regressed. Default-skeptical, fail-closed — confirmed by RUN tests, not by existence.

**Baseline / no-downgrade:** `cargo test` = **220 passed, 2 ignored, 0 failed** (5 suites). 209→220 (+11). 171 reference NOT regressed. Clippy `--all-targets -D warnings`: **clean** (independently confirmed). lib subset: 211 passed; every cited test confirmed RUN by name (not filtered).

### FIX-1 — TREND now genuinely represented (defect 1 resolved)

`SocialAction::Trend` (no-arg) exists (sim/mod.rs:60). Parser `"TREND" | "trend" => Some(SocialAction::Trend)` (agent/mod.rs:381). Display `"Performed trend operation"` (sim/mod.rs:92) — matches the source's `_describe_generic` ("performed TREND operation") intent for the action that survives `FILTERED_ACTIONS` and IS recorded as an `AgentActivity`. `store_action_in_memory` Trend arm = 0.25 browse/discovery band (agent/mod.rs:453). `apply_at` routes it no-panic (generic event path). Tests RUN+PASS: `test_parse_social_trend_uppercase`, `_lowercase`, `_apply_no_panic`, `test_social_memory_importance_trend`, `test_social_action_display_trend` (asserts bare + `"Social: Performed trend operation"` wrapped). **Genuine agent action, not a fake.** REFRESH correctly stays `- [≠]` S-TAX-020 (it IS in `FILTERED_ACTIONS`, never an activity).

### FIX-2 — Like/Dislike post-vs-comment discriminant restored (defect 2 resolved)

`TargetKind { Post, Comment }` added (sim/mod.rs:13). `Like { target_kind, target_id }` + `Dislike { target_kind, target_id }` (sim/mod.rs:42-44). Parser: `LIKE_POST`→`Like{Post,..}`, `LIKE_COMMENT`→`Like{Comment,..}`, same for Dislike (agent/mod.rs:342-357). **Distinct render — DUAL evidence:** (a) Display impl has separate per-TargetKind arms → `"Liked post: X"` vs `"Liked comment: X"`, `"Disliked post:"` vs `"Disliked comment:"` (sim/mod.rs:69-80); (b) `store_action_in_memory` likewise has 4 distinct arms (agent/mod.rs:433-444). The distinct-tests **prove inequality, not existence**: `test_parse_social_like_post_vs_comment_are_distinct` (+ Dislike twin) assert `assert_ne!(post_action, comment_action)` on the parse result AND `assert_ne!(...to_string())` on the Display string, plus `contains("post")`/`contains("comment")`. Both RUN+PASS. **The discriminant U-021 needs to pick the `_describe_like_post` vs `_describe_like_comment` render path is fully restored — no residual narrowing.** CREATE_COMMENT→`Comment{post_id,..}` stays single-key post (correct; its parent is always a post).

### No new regression + GAP honest (confirmed)

- **Exhaustive matches, no hidden arm:** `SocialAction` Display (sim/mod.rs:67-94) and the memory match (agent/mod.rs:410-455) enumerate all 13 variants with NO catch-all `_`. The only `_ =>` is `parse_social_action`'s `_ => None` (agent/mod.rs:383) — the correct unknown-name fallthrough, not a missing variant.
- **Generic-5 intact:** the parser matches Speak/Move/Interact/Observe/Think and `return`s BEFORE the social path (agent/mod.rs:296-303) — unchanged; `test_generic_actions_still_intact` + `test_generic_actions_unaltered_after_social_extension` RUN+PASS.
- **GAP-SOCIAL-WORLDSTATE honestly `- [!]`:** `apply_at` pushes ALL actions (incl. `Social`) through the same generic `Event` path (sim/mod.rs:184-185) — no fake timeline/post-store/follower-graph. `S-SWS-001` stays `- [!]`; U-022/028/029/030 stay `- [ ]`. `test_action_social_apply_no_panic` RUN+PASS.

### Result

**PASS.** Both cycle-3 defects resolved & differentially confirmed; 11 prior-correct variants + 5 generic untouched; exhaustive matches; 171 baseline not regressed (220 green); clippy `--all-targets` clean; GAP honest. Flipped: symbol-map S-TAX-001..019 + S-TAX-021 → `- [x]` (S-TAX-020 stays `- [≠]`); parity-ledger GAP-ACTION-TAXONOMY → `- [x]` RESOLVED, taxonomy READY for U-022/028/029/030 (which stay `- [ ]` — they need the full sim). GAP-SOCIAL-WORLDSTATE stays `- [!]`.

---

## Cycle 4 — 2026-06-14 — U-008 (chat/chat_json) + U-006 (retry) gap closures

**Scope:** FOCUSED re-verification of the two gaps the differential verifier flagged: U-008 `<think>`/JSON-fence strip (GAP-6), U-006 retry recovery-path coverage + max_delay clamp. Default-skeptical, fail-closed. Source: MiroFish `llm_client.py` + `retry.py`. Rust: `teri/src/llm.rs`.

**Baseline (independently confirmed):** `cargo test --lib llm` = **33 passed, 0 failed** (the 21-test +Δ from the cycle is inside this). Suite GREEN; no regression. (Full-suite 241 claimed by the loop; the llm subset is the unit under test here.)

### U-008 — PASS (proven SUPERSET)

**S-058 `strip_think` (llm.rs:21) ⇔ llm_client.py:67** `re.sub(r'<think>[\s\S]*?</think>','',content).strip()`. Differential behaviors confirmed by reading BOTH sides:
- Single block removed → bare answer. (test_strip_think_single_block; test_openai_complete_strips_think)
- **Multiple** blocks all removed, content between preserved (`<think>a</think>Mid<think>b</think>End` → `MidEnd`). Non-greedy first-close-after-open scan == Python non-greedy `*?`. (test_strip_think_multiple_blocks; test_openai_complete_strips_multiple_think_blocks)
- Multiline block removed (`[\s\S]*?` ⇔ Rust scan is newline-agnostic). (test_strip_think_multiline_block)
- **No-block pass-through UNCHANGED** + trailing `.strip()` ⇔ `.trim()`. (test_strip_think_no_block_unchanged, _trims_whitespace) → **strip is a NO-OP on think-free content** (regression-safe).
- Applied in `complete` for all 3 adapters: OpenAI llm.rs:186, Anthropic llm.rs:401, Gemini llm.rs:599. Anthropic/Gemini think-strip proven by HTTP tests (test_anthropic_complete_strips_think, test_gemini_complete_strips_think).

**S-059 `strip_json_fence` (llm.rs:49) ⇔ llm_client.py:94-97** (`^```(?:json)?\s*\n?` + `\n?```\s*$`, IGNORECASE):
- ```` ```json\n…\n``` ```` stripped (test_strip_json_fence_json_labeled; test_openai_complete_json_fenced).
- bare ```` ```\n…\n``` ```` stripped (test_strip_json_fence_bare_backticks; test_gemini_complete_json_fenced).
- **case-insensitive** `json` label (```` ```JSON ````) (test_strip_json_fence_json_label_case_insensitive).
- **unfenced UNCHANGED** → **NO-OP on fence-free content** (test_strip_json_fence_unfenced_unchanged; test_openai_complete_json_plain).
- think-THEN-fence combined (reasoning model) parses correctly — strip_think∘strip_json_fence composed (test_openai_complete_json_think_and_fence: `<think>reasoning</think>```json…``` → {v:99}). Applied: OpenAI llm.rs:215, Anthropic llm.rs:410, Gemini llm.rs:608.

**Already-matching behaviors re-confirmed still hold:** JSON-mode (`response_format:{type:json_object}` OpenAI llm.rs:199; prompt-suffix fallback for Anthropic/Gemini); parse-fail → `TeriError::Llm("Failed to parse JSON response…")` ⇔ Python `raise ValueError` (llm_client.py:102); missing api_key → `LlmConfig::validate()` ⇔ `ValueError("LLM_API_KEY 未配置")` (llm_client.py:28).

**Verdict:** all 4 symbols S-056..S-059 differentially proven, every contract branch exercised, strips are no-ops on clean content (no regression to the existing openai/anthropic/gemini complete/stream tests — confirmed unaffected). **U-008 = PASS. teri's LLM adapter is a PROVEN SUPERSET of MiroFish chat/chat_json** (adds Anthropic + Gemini + real SSE streaming). GAP-6 RESOLVED. Symbol rows S-056..S-059 → `- [x]`; parity-ledger U-008 → `- [x]`.

### U-006 — `- [~]` (STAYS OPEN) + the retry-recovery ADJUDICATION

**max_delay clamp — CONFIRMED parity.** `MAX_BACKOFF_SECS=30` (llm.rs:74) applied via `(2_u64.pow(retries)).min(MAX_BACKOFF_SECS)` at llm.rs:140,151 (OpenAI), 356,367 (Anthropic), 554,565 (Gemini) ⇔ retry.py:59 `current_delay = min(delay, max_delay)` (max_delay=30.0). Matches.

**jitter — ACCEPTED intentional-divergence `- [≠]`.** retry.py:61 `current_delay * (0.5 + random.random())` is stochastic and NOT an observable behavioral contract (it only perturbs sleep duration, never output/error/side-effect). Omitting it is correct and improves test determinism. Confirmed acceptable.

**Tested branches (green):** retry fires on 5xx + cap honored → Err after N (test_openai_retry_exhausted_returns_err `assert_hits(2)`, test_openai_retry_hits_cap `assert_hits(3)`); no spurious retry on success → exactly 1 attempt (test_openai_retry_no_retry_on_success `assert_hits(1)`).

**THE ADJUDICATION — recovery path (retry-THEN-succeed): porter's "untestable in httpmock 0.7" claim is REFUTED.**
- Investigated httpmock-0.7.0 source: `find_mock` (server/web/handlers.rs:95) returns the **first** matching mock via `mocks.values().find(...)`; there is no built-in respond-N-times. `when.matches` (api/spec.rs:782) takes `MockMatcherFunction = fn(&HttpMockRequest)->bool` (common/data.rs:171) — a **non-capturing fn pointer**, evaluated **per incoming request** (request_matches, handlers.rs:141). A `fn` pointer CAN read a module-level `static`.
- **Therefore a clean stateful approach EXISTS:** module-scope `static C: AtomicUsize = AtomicUsize::new(0);` + a 503 mock `.matches(|_req| C.fetch_add(1, SeqCst) == 0)` (matches only request #1) + a plain 200 mock (no extra matcher) on the same path. Request #1 → 503 mock matches first (counter→1) → adapter's internal retry loop sleeps 2s, retries; request #2 → 503 matcher now `false`, so `find_mock` falls through to the 200 mock → recovered.
- **PROVEN, not asserted:** the verifier added this exact test as a temporary probe in src/llm.rs, ran it → **green** (`1 passed`, 2.02s, the 2s = the real `2^1` backoff), then **removed it** (zero residue confirmed via grep). The recovery path is cleanly testable.

**Decision:** the recovery path is the core retry value (retry only matters if it can SUCCEED on a later attempt). Since it is cleanly testable, fail-closed mandate says it MUST be tested before U-006 counts. **U-006 stays `- [~]`** (parity-ledger U-006 stays `- [ ]`). The exact recovery-test recipe is recorded in symbol-map S-043 for the porter to add next cycle; on its addition + green, U-006 → `- [x]`.

**No-downgrade:** strips are no-ops on think-free/fence-free content (proven by the *_unchanged tests); existing openai/anthropic/gemini complete + stream tests unaffected; llm subset 33 green, 0 regressions.

### Result

**U-008 = PASS** (GAP-6 RESOLVED, proven superset; S-056..059 → `- [x]`). **U-006 = `- [~]` (open)** — recovery path is testable (technique proven & handed to porter); cannot pass on faith.

---

## Cycle-5 parity verdict — U-013 (text_processor) + GAP-U015-1 (build() chunking)

**Date:** 2026-06-14 · **Verifier:** rust-port-parity-verifier · **Method:** differential vs MiroFish source (not existence-check). Golden fixtures generated by loading `app/utils/file_parser.py` + the verbatim `preprocess_text`/`get_text_stats` bodies directly (bypassing the Flask-importing package `__init__`), then run through a Rust harness (`examples/parity_diff_cycle5.rs`, compiled+run via `cargo run --example`, removed after) that diffs Rust output against the Python golden. Baseline green: **263 tests** (was 242, +21), clippy `--all-targets -D warnings` clean.

### U-013 — split_text (S-169) = PASS

13-case differential vs `split_text_into_chunks` (file_parser.py:161-202), ALL match:
- empty→`[]`, blank(`"   \n\n  "`)→`[]` (matches `[text] if text.strip() else []`), short→`[text]`.
- exact_multiple (`"0123456789"`,5,0)→`['01234','56789']`; remainder (11 chars,5,0)→`['01234','56789','0']`.
- overlap (`"ABCDEFGHIJKLMNOPQRST"`,10,3)→`['ABCDEFGHIJ','HIJKLMNOPQ','OPQRST']` — overlap carries the last 3 chars (`start = end - overlap`), no off-by-one.
- no_sep_longblock (1200×'x',500,50)→3 hard-cut chunks; mixed_seps→4 chunks.
- **Boundary-backtrack adjudication vs source:** period_space_boundary→first chunk ends exactly at `". "` (`'This is a long sentence with some words in it.'`), NOT at char 50; chinese_fullstop (`。` sep)→3 chunks; sep_below_30pct (`"A. "` at index 1, below 0.3×500=150)→backtrack rejected, hard cut taken. Separator priority list `["。","！","？",".\n","!\n","?\n","\n\n",". ","! ","? "]` is identical to MiroFish's loop order; `rfind` (rightmost) + first-priority-that-qualifies wins, same as Python.
- **30%-threshold equivalence proven:** Rust `min_sep_char = (chunk_size as f64*0.3) as usize` then `chars_before_sep > min_sep_char` vs Python `last_sep > chunk_size*0.3`. For integer separator positions these are equivalent (n>floor(x) ⟺ n>x when x non-integer; n>3.0 ⟺ n>3 when x integer). Probe (cs=11, sep at index 3, 0.3×11=3.3): both reject (3>3.3 False / 3>3 False) → hard cut. Match.
- **UTF-8 safety proven:** multibyte_repeat (Chinese, 3-byte chars)→39 chunks, hiragana (`"あいうえおかきくけこ"`,3,0)→4 chunks, all valid &str, no panic. Rust uses `char_indices()` byte-boundary table for every slice — no raw byte-offset slicing into a multibyte char. Python str indices are codepoint-based; Rust converts window byte-offset→`chars().count()` to match. Aligned.

### U-013 — preprocess_text (S-170) = PASS

9-case differential vs text_processor.py:37-61, ALL match: CRLF→LF, bare-CR→LF, `\n{3,}`→`\n\n` (runs of 4 and 3 both collapse to 2; a run of 2 is preserved), per-line trim, final trim, combined. **`leading_indent` case (`"  indented line  \nnext"`→`"indented line\nnext"`) proves Rust `l.trim()` matches Python `line.strip()` — BOTH strip leading+trailing per line.** Defect (non-blocking): the rust doc-comment at `src/seed/text_processor.rs:152-154` claims it "preserve[s] leading whitespace for indented content" — that is WRONG (the code correctly uses `l.trim()`), comment-only, no behavioral impact, parity holds.

### U-013 — get_text_stats (S-171) = PASS

4-case differential vs text_processor.py:64-70, ALL match: `chars` = Unicode scalars (`"你好\nworld"`→8, NOT 12 bytes), `words` = whitespace-split count, `lines` = `\n`-count+1 (empty→`[0,0,1]`, simple→`[19,4,2]`, single→`[13,3,1]`). No byte-vs-char divergence. Each behavior has a proving test.

**U-013 type wrapper (S-167):** free-fn + `TextStats` struct form == static-class methods, parity-proven across all 3 methods. **S-168** `- [≠]` (extract_from_files = FileParser delegation, no Rust equiv needed). U-013 → `- [x]`.

### GAP-U015-1 — build() chunking = PASS (RESOLVED, no U-015 downgrade)

1. **Small-doc unchanged:** `split_text` returns 1 chunk for ≤500 chars, so the pipeline is the EXACT pre-chunking single-pass path. The 5 cycle-1 build tests (from_seed_document, empty, duplicate, unknown-ref, llm-error) pass UNCHANGED — no regression to U-015's verified behavior.
2. **Chunk-merge proven:** `test_build_large_doc_multi_chunk_merge` builds a >500-char doc, uses a call-counting mock returning distinct entities per chunk; asserts `entity_call_count > 1` (chunking actually happened) AND all 4 cross-chunk entities (Alice/Sunrise Corp from chunk 1, Bob/Sunset Inc from chunk 2) are present in the merged graph with `entity_count()==4` (none dropped). Pass-1 entity merge is name-deduped (`graph.index.contains_key`); pass-2 relations run per chunk over the full deduped entity set.
3. **All branches preserved across chunked path:** empty-tolerance (`[]`→empty graph), dup-skip (cross-chunk now), unknown-ref-skip (continue), LLM-error propagate (`?`). New tolerant branch: blank doc → `split_text` returns `[]` → valid empty graph.
4. **No-downgrade / no-truncation:** a large doc is no longer processed as a single (overflowing/truncated) pass — entities and relations come from ALL chunks (the MiroFish chunk-then-process contract). This is a sibling-worker extension, not a U-015 behavior change.

S-190 → `- [x]` (re-verified with chunking).

### No-regression confirmation

242→263 (+21) tests all green; the +21 are the new U-013 text_processor tests (20 in `src/seed/text_processor.rs`) + the multi-chunk build test. clippy `--all-targets -D warnings` clean. The pre-existing 242 (incl. the 5 build tests and the cycle-1..4 verified units) are unaffected.

### Result

**U-013 = PASS** (S-167/S-169/S-170/S-171 → `- [x]`, S-168 `- [≠]`). **GAP-U015-1 = RESOLVED** (S-190 → `- [x]`, no U-015 downgrade). Parity-ledger: U-013 `- [x]`, GAP-U015-1 RESOLVED in the U-015 row.

---

## 2026-06-14 · cycle-6 · U-009 · `FileParser` file-parsing gaps → `SeedIngestor` (encoding fallback, .md dispatch, is_supported, multi-file concat)

**Verdict: PASS.** All S-060..S-068 contract behaviors differentially verified against MiroFish `file_parser.py` + `config.py`. Two `- [≠]` intentional divergences (both no-downgrade). No regression: **263→275 (+12)** tests green; clippy `--all-targets -D warnings` clean (independently re-confirmed by build-health). The +12 are the new encoding/dispatch/is_supported/multi-file tests in `src/seed/mod.rs:619-782`.

**Method:** differential, not existence-check. GBK byte fixtures generated independently in Python (`"中文".encode("gbk")` → `[D6 D0 CE C4]`; `"你好世界"` → `[C4 E3 BA C3 CA C0 BD E7]`) and confirmed byte-exact to the Rust test fixtures. The `encoding_rs` decoder behavior was run directly via a throwaway `examples/gbk_probe.rs` (`cargo run --example`, removed after) — NOT inferred from the Python `gbk` codec (encoding_rs GBK is GB18030-family, so it had to be exercised directly).

### S-060 — `read_text_with_fallback` (encoding fallback, HIGHEST RISK) = PASS

Direct `encoding_rs` run (the load-bearing evidence):
- **UTF-8 fast path:** `"Hello, 世界! résumé"` → `std::str::from_utf8` Ok → returned unchanged. ✓
- **GBK round-trip PROVEN:** `GBK.decode([D6 D0 CE C4])` → `out="中文"`, **`had_errors=false`** → guard accepts. Correct Chinese characters, NOT mojibake, NOT a UTF-8 error. End-to-end `from_file` on a GBK `.txt` (`[C4 E3 BA C3 CA C0 BD E7]`) → `raw_text=="你好世界"`. ✓
- **had_errors IS CHECKED (the flagged risk is closed):** `src/seed/mod.rs:167-170` does `let (cow,_,had_errors)=GBK.decode(bytes); if !had_errors { return cow.into_owned(); }`. Adversarial false-positive probe: Latin-1 `café` bytes `[63 61 66 E9]` through GBK → `out="caf␦"`, **`had_errors=true`** → guard REJECTS, falls through to Windows-1252 → `"café"`. So a Latin-1 file is NOT mis-decoded as GBK. The exact defect the gate was told to hunt for is **absent**. ✓
- **Windows-1252 backstop never errors:** `[63 61 66 E9]` → `had_errors=false` (`0xE9→é`); lone `0x80` → `€`. Every byte maps; no `String::from_utf8_lossy` mojibake anywhere in the path (confirmed absent — strict `from_utf8` + two checked `encoding_rs` decoders only). ✓

Parity vs MiroFish `_read_text_with_fallback` (file_parser.py:11-58): MiroFish is UTF-8 → charset_normalizer/chardet best-guess → UTF-8+replace. Rust is UTF-8 → GBK(checked) → Windows-1252(total). For the real-world cases that matter (valid UTF-8, valid GBK Chinese, valid Latin-1) both produce the correct decode; neither errors/panics. The deterministic Rust order is a sound, non-downgrading equivalent of MiroFish's heuristic detector — both honor the "never raise on a text file" contract.

### S-064/S-066/S-067 — .md/.markdown dispatch → text reader = PASS

`from_file` "md"|"markdown" arm (`src/seed/mod.rs:84`) routes to `read_plain_text` (with encoding fallback), matching MiroFish `_extract_from_md`/`_extract_from_txt` (both plain text via `_read_text_with_fallback`). Tested: `.md`→raw_text exact, `file_format=="md"`; `.markdown`→exact, `file_format=="markdown"`. ✓

### S-062/S-063 — `is_supported` = PASS with TWO `- [≠]` (no behavior loss)

- **`- [≠]` (a) permissive `from_file`** (task-described): unknown ext → plain-text read (teri resilience) vs MiroFish `extract_text` raising `ValueError`. `is_supported` is the caller-side API gate (mirrors MiroFish `allowed_file`/`FileParser.is_supported`), so the policy split hides no loss — the gate still exists, it's just decoupled from the reader.
- **`- [≠]` (b) json superset** (adjudicated this cycle): MiroFish `Config.ALLOWED_EXTENSIONS={pdf,md,txt,markdown}` (config.py:41) and `FileParser.SUPPORTED_EXTENSIONS={.pdf,.md,.markdown,.txt}` — **no json** (MiroFish has no json reader). teri's set is `{txt,md,markdown,pdf,json}` — a **superset**: for the 4 shared extensions behavior is identical (nothing MiroFish accepts is rejected). teri ADDS json because teri genuinely ingests json (`read_json` at mod.rs:221-234, `test_json_file_format`/`test_integration_examples` pass). Declaring json supported is *consistent with teri's own capability* — it would be a bug to gate out a format teri ingests. Superset, no downgrade. Tested: all 5 known→true, unknowns (exe/zip/png/noext)→false, case-insensitive (`DOC.TXT`/`Report.MD`→true). ✓
- **Nit (non-blocking, comment-only):** the doc at `src/seed/mod.rs:10-12` says the const "mirrors `Config.ALLOWED_EXTENSIONS`" — it actually mirrors teri's *superset* (adds json). No behavioral impact; suggest amending the comment to "mirrors+extends".

### S-061/S-068 — `FileParser` type + `extract_from_multiple` multi-file concat = PASS

Concat format byte-exact (orchestrator pre-confirmed; re-confirmed in code): `format!("=== 文档 {idx}: {filename} ===\n{text}")` joined `"\n\n"`, error line `"=== 文档 {idx}: {path} (提取失败: {e}) ==="` — identical to MiroFish `extract_from_multiple` (file_parser.py:138-158), including 1-based index and `Path::file_name` for the per-file name. **Per-file error tolerance proven:** a missing file in a 2-file batch → `from_files` returns `Ok`, good file's content present AND `提取失败` marker present (one bad file does NOT abort the batch — matches the Python `try/except` per-file). In-order header check (`pos1<pos2`) passes. ✓

### S-065 — `_extract_from_pdf` page-skip = PASS (carried from cycle-5)

`read_pdf` page-skip-on-error already parity-verified cycle-5; unaffected by this cycle's changes.

### No-regression

Full `cargo test` (all targets) = **275 passed, 0 failed** (was 263, +12). Seed module: 45/45. The pre-existing seed tests (PDF invalid→err, URL non-200→err, json/malformed-json, web extraction, basic metadata) all unaffected. clippy `--all-targets -D warnings` clean.

### Result

**U-009 = PASS.** S-060..S-068 → `- [x]` except S-062/S-063 → `- [≠]` (json superset + permissive policy, both no-downgrade). S-069 stays distributed to U-013 (already `- [x]`). The flagged GBK-Latin-1 false-positive risk is **closed**: `had_errors` is checked, GBK round-trip proven, no mojibake.

---

## Cycle 7 — U-048 (extend-Y: in-band end-of-sim terminal signal) — 2026-06-14

**Unit:** U-048 · S-1057 (+ S-1057-A/B/C). Files: `src/sim/mod.rs`, `src/api/mod.rs`.
**Verdict: PASS** (4/4 symbols proven; SSE-handler *consumer* wiring correctly deferred to U-026).

### Source contract (MiroFish)
- `action_logger.py:105` `log_simulation_end(total_rounds, total_actions)` writes a terminal log entry `{event_type:"simulation_end", platform, total_rounds, total_actions}` — an EXPLICIT end marker on the action stream (not a bare stream-close).
- `simulation_runner.py:623` monitor detects `event_type=="simulation_end"` → sets `*_completed=True`, `*_running=False`. The contract: consumers get an explicit terminal event with the executed-round count.

### Differential confirmations (run, not asserted-to-exist)
1. **Completion fires at the right time + correct count.** `run()` sends `completion_tx.send(Some(SimCompletion{total_ticks}))` at `sim/mod.rs:565` — AFTER the tick loop (517-554) where the last snapshot is broadcast (`:547`) and pushed to history (`:553`), and BEFORE `run()` returns (`:567`). `total_ticks = history.len() as u32` (`:558`) — the ACTUAL executed count, not `max_ticks`. Test `test_completion_signal_fires_with_correct_total_ticks` (N=5, parallelism=1) asserts `sc.total_ticks==N` AND cross-checks `result.history.len()==N`. PASS.
2. **Ordering (fires-after-last-snapshot).** `test_completion_signal_fires_after_last_snapshot` (N=3) reads the shared history Arc at the instant completion is observed and asserts `history.len()==N` — proving history is fully populated when the signal lands. PASS.
3. **Late-subscriber safety (watch > broadcast).** `test_late_subscriber_sees_completion` (N=2) runs FIRST, subscribes AFTER, and observes `Some(SimCompletion{total_ticks:2})` immediately via `watch::Receiver::borrow()`. This also PROVES `_completion_anchor` works: tokio `watch::send` is a no-op when no receiver is alive, so if the anchor (`sim/mod.rs:414,426,433`) were absent the late subscriber would see `None` — it sees `Some`, so the anchor persisted the value. PASS. `test_subscribe_completion_initial_value_is_none` confirms the pre-run `None`. PASS.
4. **sim_end event maps the terminal marker.** `TickStreamEvent::sim_end` (`api/mod.rs:105`) → `{tick:total_ticks, data:{"sim_end":true,"total_ticks":n}, event_id:"sim-end"}`. Sentinel-in-data, same uniform wire format as `lag_gap`. `event_id` fixed `"sim-end"` (no suffix → exactly one per sim). 4 api tests PASS (tick, event_id, data fields, zero-ticks edge). Maps `log_simulation_end`'s explicit marker + round count.
5. **No regression (additive).** `test_snapshot_broadcast_unaffected_by_completion_channel` (N=4): `subscribe()` and `subscribe_with_history()` both still deliver all 4 ticks in order, history len==4. The completion channel is purely additive. Full suite **275 → 285 passed** (+10), 3 ignored, clippy `--all-targets -D warnings` clean.

### Ignored-test finding (2 → 3)
The +1 ignored is NOT a `#[ignore]` unit test — it is a **doctest** with an ` ```ignore ` code fence: `SimEngine::subscribe_completion` usage example at `sim/mod.rs:478`. It is illustrative SSE-handler pseudo-code (`?` outside a fn, references the not-yet-wired U-026 consumer) — legitimately non-runnable, NOT a real test silenced to dodge a failure. The other two pre-existing ignored doctests: `api/streaming.rs:134` (`StreamAdapter::as_hook`) and `sim/mod.rs:275` (`SimConfig`). Zero `#[ignore]` attributes anywhere in `src/`/`tests/`; the U-048 diff added no `#[ignore]`. No hidden downgrade.

### Symbols
- [x] S-1057 (rollup) · [x] S-1057-A `SimCompletion` · [x] S-1057-B `subscribe_completion` · [x] S-1057-C `sim_end` — 4/4.

---

## Cycle 8 — U-018 (extend-Y: Persona social fields + OASIS profile generation + serializers) — 2026-06-14

**Unit:** U-018 · `oasis_profile_generator.py` → `src/agent/mod.rs`. 26 symbols `[~]`-ported, 23 `[≠]` (audited below — NO real feature skipped). Files: `src/agent/mod.rs` (+7 `social: None` test-literals in `sim/mod.rs`).
**Verdict: PASS** (26/26 ported symbols proven by differential, exact-shape tests; all 4 owner-flagged anti-pattern candidates confirmed NOT `[≠]`-skipped).

### Differential confirmations (run, not asserted-to-exist) — 310 passed / 0 failed / 3 ignored

1. **`to_reddit_format` EXACT key-shape** (Rust mod.rs:115-159 vs Python l61-87). Always-present: `user_id`, **`username` (NO underscore — OASIS lib requirement, mod.rs:119)**, `name`, `bio`, `persona`, `karma`, `created_at`. `test_to_reddit_format_keys_and_no_underscore_username` asserts `v["username"]=="alice_wonder_123"` AND `v.get("user_name").is_none()` (proves the no-underscore requirement) AND `friend_count` absent (Reddit excludes Twitter counts, has `karma`). Conditional demographics mirror Python falsy guards (`if self.age:` → `age>0`; non-empty str; non-empty vec). **OMISSION proven** by `test_to_reddit_format_conditional_demographics_absent_when_none`: sets age/gender/mbti/country/profession=None + topics=[] → asserts each `.get(k).is_none()` (absent, not null). PASS.

2. **`to_twitter_format` EXACT key-shape** (Rust mod.rs:171-216 vs FULL Python l89-117 read in entirety). Always-present: `user_id`, `username` (no underscore), `name`, `bio`, `persona`, `friend_count`, `follower_count`, `statuses_count`, `created_at`. **`karma` NOT present** (`test_to_twitter_format_keys_and_no_underscore_username` asserts `v.get("karma").is_none()`). All 6 conditional demographics present (none dropped — full method read; `test_..._present_when_set` + `..._absent_when_none` cover both directions). PASS.

3. **`to_dict` full-flat format** (Rust mod.rs:226-248 vs Python l119-140). Uses **`user_name` (WITH underscore)** — `test_to_dict_complete_flat_format` asserts `v["user_name"]` present AND `v.get("username").is_none()`. All fields unconditional; `test_to_dict_null_optionals_present` proves None optionals serialize as JSON `null` (not omitted) and empty topics as `[]`. PASS.

4. **bio ≠ persona DE-NARROWED**: `SocialProfile.bio: String` (:41) and `SocialProfile.persona: String` (:43) are DISTINCT fields. `test_bio_and_persona_are_distinct_fields` uses load-bearing distinct values ("Short public bio line" vs "Detailed and distinct persona description...") and asserts `reddit["bio"] != reddit["persona"]` AND `twitter["bio"] != twitter["persona"]`, plus exact-value checks. The earlier collapse-into-`Persona.background` narrowing is REVERSED and proven distinct. PASS.

5. **`generate_social` + rule-based fallback + `generate_username`** (mod.rs:984-1247 vs `generate_profile_from_entity` l212-274 / `_generate_profile_rule_based` l774-845 / `_generate_username` l276-284). LLM→JSON→populate; on LLM-error OR parse-failure→`generate_social_rule_based`. `test_generate_social_with_mock_llm` (valid JSON → all fields populated from LLM). `test_generate_social_rule_based_fallback_on_llm_error` (university → age30/other/ISTJ/China-defaults match Python l822-832) + `..._on_invalid_json` (student → age22/INFP match Python intent; note Rust student age=22 vs Python random(18,30) — within contract band). Entity-type branches: student/alumni, publicfigure/expert/faculty/professor, university/org/ngo/media/company/institution/group/community, default — all present (mod.rs:1124-1203). `generate_username`: lowercase + `_` + alphanumeric-filter + numeric suffix 100..=999 — `test_generate_username_deterministic` (charset + suffix-range) + `..._distinct_for_different_names`. (Deterministic hash-suffix vs Python `random.randint` is documented S-354 — same SHAPE, no contract on the random value itself.) PASS.

6. **Generic Persona preserved + serde backward-compat**: 4 generic fields (`name/background/traits/role`) UNCHANGED; `social: Option<SocialProfile>` carries `#[serde(default)]` (:101). `test_persona_serde_backward_compat_no_social_field` deserializes OLD 4-field JSON (no `"social"` key) → asserts `social.is_none()`. `test_persona_generic_still_works_social_none` + all 8 pre-existing persona/agent tests pass unchanged. CONFIRMED.

### NO-FEATURE-SKIP audit (owner-flagged anti-pattern this session)

**The 4 candidates are NOT `[≠]`-skipped — all genuinely ported (`[~]`):** S-326 `user_id` → `SocialProfile.user_id: u64`; S-344 `to_reddit_format`; S-345 `to_twitter_format`; S-346 `to_dict` — each with passing exact-shape tests above. CLEAN.

**The 23 `[≠]` rows judged — all legitimate, three categories:**
- **REUSE (real, not dropped):** S-328 `name`→`Persona.name` (serializers emit `self.name`); S-352 `__init__`→`PersonaGenerator::new()`.
- **Architectural substitution (genuinely inexpressible in teri):** S-355/356/366 Zep graph-search / build_context / set_graph_id — Zep is an external service teri replaces with native `KnowledgeGraph`; declared at unit level (deps note). S-348/349/350/351/357/358 const-lists/_is_individual/_is_group → behavior PRESERVED as match-arm branches in `generate_social_rule_based` (form differs, behavior identical). S-362/363/364 prompt builders → unified prompt (prompt wording is non-contractual; LLM-dependent output).
- **Deferred-to-U-023/export-path (carry-forward obligations — NOT lost):** S-367 batch `generate_profiles_from_entities` (parallelism+ordering is U-023's `prepare_simulation`, ledger dep-noted); S-369/370/372/373 OASIS file export (the serialization SHAPE is `[~]`-ported & tested; only the `json.dump`/`csv.writer` I/O wrapper defers); S-371 `_normalize_gender` (中文→male/female/other map, used only by `_save_reddit_json` export); S-360/361 `_fix_truncated_json`/`_try_fix_json` (Rust narrows partial-JSON-salvage → rule-based fallback; the *contract* "produce a valid profile on parse failure" is preserved). S-368 `_print_generated_profile` (debug console output, non-contractual).

**Two QUALIFIED divergences flagged for the cartographer's pre-DONE left-behind sweep (real behaviors that MUST travel with U-023's export path, not vanish):** (a) **S-371 gender normalization** — Rust stores gender as-is; if a Chinese gender value reaches OASIS export, Rust would emit "男" where Python emits "male". Inert for English sims; must land when the export I/O lands. (b) **S-360/361 JSON-salvage** — Python regex-extracts bio/persona from truncated LLM JSON; Rust discards → rule-based defaults. Graceful-degradation difference, not a dropped path (both yield a valid profile). Neither is a downgrade of a *contractual* output (LLM text + export I/O are both out of this struct-port unit's scope); both are correctly scoped to U-023 and recorded here so they are not forgotten.

### Baseline
Full `cargo test` (all targets) = **310 passed, 0 failed, 3 ignored** (was 285, +25). 23 new U-018 tests (Platform serde, SocialProfile defaults/serde, 3× generate_social, 2× generate_username, 6× to_reddit, 6× to_twitter, 4× to_dict, bio≠persona, backward-compat). No stub markers (`todo!`/`unimplemented!`/"simplified") in mod.rs:13-1248. clippy `--all-targets -D warnings` clean (independently confirmed). The 3 ignored are the same pre-existing illustrative doctests from cycle-7 (unchanged by U-018).

### Symbols
26 ported symbols → `- [x]` (S-325/326/327, S-329..S-347, S-353/354/359/365). 23 `[≠]` rows retained as audited (no real feature among them). U-018 ledger → `- [x]`.

---

## Cycle 9 — U-004 `backend/app/utils/logger.py` (rotating-FILE logging — [≠]→[~]→extend-Y no-downgrade correction) — 2026-06-14

**Verdict: PASS.** The rotating-file-logging capability genuinely EXISTS, matches MiroFish's `RotatingFileHandler(maxBytes=10MB, backupCount=5)`, and the previously-skipped feature is now ported and parity-proven. No `[≠]` feature-skip remains on the file-logging row. 10/10 U-004 symbols → `- [x]`.

### 1. Rotating-file contract matches MiroFish (size-based 10MB × 5)
- `MAX_LOG_BYTES = 10 * 1024 * 1024` (`src/logging.rs:49`) == MiroFish `maxBytes=10*1024*1024` (logger.py:70). Asserted by `test_constants_match_mirofish_contract`.
- `LOG_BACKUP_COUNT = 5` (`src/logging.rs:52`) == MiroFish `backupCount=5` (logger.py:71).
- Writer built via `FileRotate::new(path, AppendCount::new(5), ContentLimit::Bytes(MAX_LOG_BYTES), Compression::None, None)` (`src/logging.rs:88-94`). **Confirmed SIZE-based, not time-based** by reading file-rotate 0.8.0 source: `ContentLimit::Bytes(N)` is the byte-count rotation variant (lib.rs:47-62 "Rotating by Bytes"); `AppendTimestamp`/`FileLimit::Age` (the time path) is NOT used.
- **File layer writes DEBUG+** — `EnvFilter::try_new("debug")` on the file layer (`src/logging.rs:121-127`), matching MiroFish `file_handler.setLevel(logging.DEBUG)` (logger.py:74). Console layer keeps its own EnvFilter/`RUST_LOG`/`level` (logger.py console_handler at INFO is caller-driven here).

### 2. Rotation genuinely WORKS (not a stub)
- `test_rotation_produces_backup_after_size_limit` writes 11 × 1 MB = 11 MB through the **real `FileRotate` writer** (no mock) with a 10 MB limit, then asserts `teri.log.1` exists. This exercises the actual `file_rotate` rotation path and proves a backup file is produced once the byte limit is surpassed. PASS.
- `test_rotation_keeps_at_most_backup_count_files` uses a tiny `ContentLimit::Bytes(100)` to force ≥8 rotations, then asserts the count of `teri-small.log.*` backups `<= LOG_BACKUP_COUNT` (5) — proving the backup-count CEILING. Confirmed against file-rotate source: `AppendCount` enforces `file_number >= max_files` deletion (suffix.rs:113-139: "if max_files is 3 … log.3 may exist but not log.4"). PASS.
- `test_writer_produces_expected_content` proves written bytes land in `teri.log` legibly. No `todo!`/`unimplemented!`/`stub`/`simplified` markers in `src/logging.rs`.

### 3. Opt-in + default preserved (NO regression)
- `TERI_LOG_DIR` UNSET (or empty) → the `_ =>` arm (`src/logging.rs:139-146`) builds **console-only** via `fmt().with_env_filter(...).with_target(true).with_level(true).init()` — byte-for-byte the prior `init_logging` console behavior. No file layer, no `file-rotate` writer opened. Default path UNCHANGED.
- `TERI_LOG_DIR` SET → `Ok(dir) if !dir.is_empty()` arm composes console layer + DEBUG+ file layer via `registry()` (`src/logging.rs:114-138`). File logging is purely additive/opt-in. Matches the no-hardcoded-path teri config-is-env design (MiroFish's hardcoded `LOG_DIR` becomes the opt-in env var — no capability lost, directory still auto-created via `create_dir_all`).

### 4. Idiomatic mappings — judged TRUE equivalents (not dropped capabilities)
- `get_logger(name)` (S-029) → tracing `target:` field. tracing is process-global; per-name routing IS the `target:` on each macro. True equivalent.
- module-level `logger` instance (S-030) → tracing's global subscriber addressed directly by macros. Equivalent.
- `debug/info/warning/error` shortcuts (S-031..S-034) → `tracing::debug!/info!/warn!/error!`. Confirmed tracing 0.1 provides each level (tracing-core `Level::{TRACE,DEBUG,INFO,WARN,ERROR}`). Equivalent.
- `critical` (S-035) → `tracing::error!`. **Confirmed correct**: tracing's `Level` has NO level above ERROR (read tracing-core metadata.rs:513-529 — ERROR is the highest of 5). Python CRITICAL maps to the highest available severity. No-downgrade.
- `_ensure_utf8_stdout` (S-027) → N/A. Confirmed Windows-only Python workaround; Rust stdout is UTF-8 on all platforms teri targets. No reconfiguration capability is lost (Rust never had the mojibake problem).

### 5. NO `[≠]` feature-skip remains on U-004
The rotating-FILE row (S-026/S-028 + `build_rotating_writer`) is the genuine CAPABILITY and is now `- [x]` (was wrongly `[≠]`-adjacent / skipped). The remaining `[≠]`-noted symbols (S-027/S-029..S-035) are API-SURFACE idiomatic mappings, NOT dropped features — each judged a true tracing equivalent above. No real feature is skipped.

### 6. No regression — 310 → 315
Full `cargo test` (all targets): **315 passed, 0 failed, 3 ignored** (was 310, +5 = the 5 logging tests). lib = 306, integration suites = 9. clippy `--all-targets -- -D warnings` clean (independently confirmed). main.rs: both init sites (`src/main.rs:73` run_cmd, `src/main.rs:102` serve_cmd) call `init_logging(&config.logging.level)` which composes the SAME console behavior when `TERI_LOG_DIR` is unset — behavior-preserving. The 3 ignored are the pre-existing illustrative doctests (unchanged).

### Symbols
10 U-004 symbols → `- [x]` (S-026..S-035, incl. `build_rotating_writer` + the file layer). U-004 ledger → `- [x]`. get_logger / shortcuts / utf8 recorded as idiomatic-equivalent mappings, NOT gaps.

---

## 2026-06-14 · `[≠]` RE-AUDIT (owner-flagged, harness PR #34 tightened bar)

Re-challenged every `- [≠]` row under the tightened test: `[≠]` legal ONLY as (a) inexpressible→really `[!]`, (b) non-contractual/unobservable, (c) strict-superset, or pure code-ORGANIZATION where the contract is fully ported elsewhere. Default-skeptical: each `[≠]` assumed a disguised feature-skip until proven. Source read at MiroFish `services/oasis_profile_generator.py`, `services/graph_builder.py`, `utils/retry.py`; teri read at `src/agent/mod.rs`, `src/graph/mod.rs`.

### Verdict table — borderline rows

| Row | Symbol | Verdict | Rule / Reason |
|-----|--------|---------|---------------|
| S-360 | `_fix_truncated_json` | **DISGUISED-SKIP → port-now** | Repairs truncated LLM JSON to SALVAGE a partial response before rule-based fallback. teri discards a salvageable LLM response on any parse error → quality/resilience DOWNGRADE with distinct observable output. Portable (brace/bracket/quote-closing string surgery). |
| S-361 | `_try_fix_json` | **DISGUISED-SKIP → port-now** | Aggressive JSON repair: extract `{...}`, strip control chars, collapse whitespace, and field-level `bio`/`persona` regex salvage. Recovers content teri throws away. Distinct observable output. Portable. |
| S-371 | `_normalize_gender` | **KEEP-[≠] (b) — but NARROWED scope: tie to OASIS export** | The 中文→en map (`男`→male, `女`→female, `机构`/`其他`→other, default→other) is REAL normalization, BUT its ONLY call site is `_save_reddit_json` (oasis_profile_generator.py:1177) — an OASIS file-EXPORT format concern. teri has no OASIS export path (S-369/S-372/S-373/S-344/S-345 all out of scope). With no export consumer there is no in-process contract today. **Legit keeper as (b) ONLY while OASIS export stays unported.** RE-FLAGGED: if/when OASIS Reddit/Twitter export is ported, `_normalize_gender` MUST port with it (it is contractual to that output) — recorded as a dependency, not a permanent divergence. |
| S-355 | `_search_zep_for_entity` | **KEEP-[≠] (b)** | Zep-SaaS hybrid search (`self.zep_client.graph.search` scope=edges/nodes, rrf reranker, parallel ThreadPool, 30s timeout). Pure Zep-server-side machinery; no in-process analogue. The IN-PROCESS enrichment it feeds is the graph-traversal half (see S-356). |
| S-356 | `_build_entity_context` | **DISGUISED-SKIP → port-now (enrichment narrowed in `generate_social`)** | `_build_entity_context` assembles entity attributes + `related_edges` (facts) + `related_nodes` (neighbor summaries) into the LLM prompt context for `generate_profile_from_entity`. teri's MAPPED method `generate_social` (agent/mod.rs:984) takes a FLAT `entity_summary: &str` and its prompt embeds ONLY name/type/summary — it NEVER enriches from the graph. The graph-traversal enrichment DOES exist in teri (`generate_entity_description`→`get_neighbors`, agent/mod.rs:926) but feeds the **Persona** path (`generate`, :886), NOT `generate_social`. So the social-profile path is NARROWED: it requires the caller to pre-supply context. The (b) part (Zep search) stays `[≠]`; the (in-process related_edges/related_nodes neighbor enrichment) is a dropped quality behavior on the social path → port. |
| S-048 | `call_batch_with_retry` | **DISGUISED-SKIP → port-now (port to U-006)** | Not just "callers own batching": carries a REAL resilience contract — per-item retry via `call_with_retry`, partition into `(results, failures)` with per-failure `{index,item,error}`, and `continue_on_failure` (isolate a bad item vs abort the batch). Distinct observable behavior (partial-success result shape). No current teri consumer, but "when in doubt → port it"; belongs in U-006 retry utilities. |

### Pending-dependency reclassifications (NOT divergences)

| Row | Symbol | Reclassify → | Reason |
|-----|--------|--------------|--------|
| S-189 | `build_graph_async` | **pending-U-012** | Real async-task-with-progress FEATURE (spawns background task via `self.task_manager.create_task`, returns `task_id`, drives progress). TaskManager = U-012 (S-138..S-167, all `- [ ]`). Port when U-012 lands. |
| S-192 | `set_ontology` | **pending-U-014** | Dynamic ontology (builds entity/edge Pydantic models from an ontology dict). Dynamic ontology IS in scope (OQ-5/GAP-3: EntityKind::Custom + ontology generator). OntologyGenerator = U-014 (S-172..S-176, all `- [ ]`). Port when U-014 lands. |

### KEEP-[≠] — confirmed legit (Zep-SaaS-lifecycle / code-org / superset)

- **S-191 `create_graph`** — KEEP (b): mints Zep `graph_id=mirofish_{uuid16}` + `self.client.graph.create()`. Pure Zep SaaS; teri's petgraph is in-process, no server graph to create.
- **S-193 `add_text_batches` 1s sleep** — KEEP (b): rate-limit between Zep `graph.add_batch` POSTs. No in-process analogue (no remote API to throttle). The batching/progress contract itself rides on U-012/U-013, not here.
- **S-194 `_wait_for_episodes` (poll processed=True every 3s, 600s timeout)** — KEEP (b): polls Zep SERVER-SIDE async episode processing. teri's LLM extraction is synchronous/awaitable in-process — nothing to poll.
- **S-195 `_get_graph_info`** — KEEP (b/c): paginated Zep node/edge fetch → counts. teri exposes `entity_count()`/`relation_count()` directly (graph/mod.rs:517/521). Superset/equivalent.
- **S-196 `get_graph_data`** — KEEP (b/c): paginated Zep read with Zep temporal fields (valid_at/invalid_at/expired_at/episodes). teri uses `serialize_to_json()`/`get_all_entities()`/`get_all_edges()`; Zep-specific temporal columns are server-side artifacts.
- **S-197 `delete_graph`** — KEEP (b): `self.client.graph.delete()`. teri's KnowledgeGraph is dropped when out of scope; no remote graph to delete.
- **S-181..S-188 `GraphInfo` dataclass + `GraphBuilderService` form** — KEEP code-org: Zep-result DTO + service wrapper; contract ported as methods on `KnowledgeGraph` (entity_count/relation_count/serialize_to_json, EntityKind enum). Same behavior, different form.
- **S-045/S-046/S-047 `RetryableAPIClient` (+__init__/call_with_retry)** — KEEP code-org: per-adapter inline retry in `call_api` (llm.rs); same single-call retry contract, different form. (Distinct from S-048, which carries the EXTRA batch-partition contract.)
- **S-062 `SUPPORTED_EXTENSIONS`** — KEEP (c) strict-superset: `{txt,md,markdown,pdf,json}` ⊇ MiroFish `{pdf,md,txt,markdown}`; adds json (genuinely ingested), rejects nothing MiroFish accepts.
- **S-063 `is_supported`** — KEEP code-org/(c): caller-side gate preserved; `from_file` stays permissive (unknown→plain-text) = resilience, hides no loss.
- **S-168 `extract_from_files`** — KEEP code-org: delegation plumbing; extraction ported in `SeedIngestor` (U-010).
- **S-328 `OasisAgentProfile.name`** — KEEP code-org: reuses `Persona.name`, not duplicated.
- **S-348/S-349/S-350/S-351 (MBTI/COUNTRIES/INDIVIDUAL/GROUP const arrays)** — KEEP code-org: inlined as rule-based match arms / free-LLM choice; values preserved.
- **S-352 `__init__`** — KEEP code-org: `PersonaGenerator::new()` covers init; LLM passed per-call.
- **S-357/S-358 `_is_individual_entity`/`_is_group_entity`** — KEEP code-org: classification inlined as match arms in rule-based fallback.
- **S-362/S-363/S-364 (system/individual/group prompt builders)** — KEEP code-org: prompt logic folded into `generate_social`'s single template.
- **S-366 `set_graph_id`** — KEEP (b): Zep graph_id not applicable; graph passed by ref.
- **S-367 `generate_profiles_from_entities`** — KEEP code-org: batch loop is the orchestrator's (`AgentPool::spawn`) job. (NOTE: enrichment-per-entity contract is S-356's concern, addressed above.)
- **S-368 `_print_generated_profile`** — KEEP (b): console/debug print, unobservable contract → tracing layer.
- **S-369/S-370/S-372/S-373 (OASIS save_profiles / twitter_csv / reddit_json / save_profiles_to_json)** — KEEP code-org: OASIS file export out of scope (S-344). (Coupled to S-371 re-flag above.)
- **retry jitter (S-043 note)** — KEEP (b): stochastic, no observable contract.
- **S-TAX-020 REFRESH omission** — KEEP (b): in `FILTERED_ACTIONS={refresh,sign_up}` (run_parallel_simulation.py:611), filtered BEFORE reaching actions.jsonl/`to_episode_text`. Not a downgrade.

### PORT-NOW list (exact source → target)

1. **S-360 `_fix_truncated_json`** — oasis_profile_generator.py:583 → new helper in `src/agent/mod.rs` (e.g. `fix_truncated_json(&str)->String`), called in `generate_social` before `serde_json::from_str`.
2. **S-361 `_try_fix_json`** — oasis_profile_generator.py:606 → `try_fix_json(...)->Option<Value>` in `src/agent/mod.rs`, attempted on parse failure in `generate_social` before rule-based fallback (field-level bio/persona salvage incl.).
3. **S-356 (in-process enrichment half)** — `_build_entity_context` parts 1-3 (attributes + related_edges + related_nodes) → enrich `generate_social` to pull `KnowledgeGraph::get_neighbors` context (reuse/mirror `generate_entity_description`) instead of requiring a flat caller-supplied `entity_summary`. Zep-search half (S-355) stays `[≠]`.
4. **S-048 `call_batch_with_retry`** — retry.py:195 → `src/llm.rs` (U-006 retry utilities): batch helper returning `(Vec<Ok>, Vec<Failure{index,item,error}>)` with `continue_on_failure`.

### Symbol-map mutations applied (this re-audit)
- S-360, S-361 → `- [ ]` (port-now); S-356, S-048 → `- [ ]` (port-now); each row note updated to DISGUISED-SKIP rationale. None marked `- [x]`.
- S-189 → `- [ ]` pending-U-012; S-192 → `- [ ]` pending-U-014 (reclassified from `[≠]`).
- S-371 retained `- [≠]` with an added re-flag (port-with-OASIS-export dependency).
- S-355 retained `- [≠]` (Zep-SaaS-search half only).
- All other `[≠]` rows: KEEP confirmed (table above), unchanged.

---

## 2026-06-17 · [≠]-audit ports — parity gate (resume)

Re-run of the differential parity gate for the 4 `[≠]`-audit ports committed GREEN at `20e2e48`
(the prior verifier was interrupted by session budget; rows were left `- [~]`). Method: read each
MiroFish source symbol at the cited file:line, read the teri port (`src/agent/mod.rs`, `src/llm.rs`),
enumerate every contract branch in the source, and confirm the Rust handles each — preferring
executable differential checks (the existing unit tests, all GREEN). Fail-closed: a symbol PASSES
only when every contract branch matches with NO downgrade and NO dropped branch.

### Verdict table

| Symbol | Verdict | Evidence (source vs teri) — branches checked |
|--------|---------|----------------------------------------------|
| **S-360** `_fix_truncated_json` | **PASS** | src `oasis_profile_generator.py:583-604` vs teri `src/agent/mod.rs:969-995`. Branches: (a) strip — `.strip()`↔`.trim()`✔; (b) unbalanced-brace/bracket count via `count('{')-count('}')`↔`filter(=='{').count() - filter(=='}').count()`✔; (c) dangling-string close — Python `content[-1] not in '",}]'` (chars `"` `,` `}` `]`) ↔ Rust `last != '"' && != ',' && != '}' && != ']'` — **exact char-set match**✔; (d) close brackets THEN braces (inner-before-outer, `']'*open_brackets` then `'}'*open_braces`)↔`for 0..max(0) push ']'` then `'}'`✔; (e) `.max(0)` guards negative (over-closed) counts — Python `'x'*n` with n≤0 yields `""` (same no-op)✔. Tests (4 GREEN): closes-open-brace, closes-dangling-string+brace, closes-array+brace, valid-input-unchanged. No divergence. |
| **S-361** `_try_fix_json` | **PASS** | src `oasis_profile_generator.py:606-670` vs teri `src/agent/mod.rs:1013-1218`. All 7 steps matched: (1) `fix_truncated_json` first✔; (2) extract first `{…}` — Python `re.search(r'\{[\s\S]*\}')` (greedy to last `}`) vs teri brace-depth scan to the *matching* `}` — **noted divergence**: Python greedily takes to the LAST `}`, teri stops at the first balanced close. Non-downgrading: teri's is *stricter/safer* (extracts a well-formed object), and when the outer object IS balanced both yield the same string; when trailing garbage with extra `}` exists, teri's is the correct salvage. No observable contract loss for the bio/persona recovery the symbol guarantees; (3) normalize newlines inside string values — Python `fix_string_newlines` (replace `\n`/`\r`→space, `\s+`→single) applied only inside `"…"` regions ↔ teri `normalize_json_string_newlines` walks quoted regions, replaces CR/LF→space, `split_whitespace().join(" ")`✔; (4) parse + set `_fixed=true`✔; (5) strip control chars `[\x00-\x1f\x7f-\x9f]`→space + collapse `\s+` then retry ↔ teri `strip_control_chars` (`cp<=0x1f \|\| 0x7f..=0x9f`)✔; (6) field-level salvage — **bio uses CLOSED-quote pattern** `r'"bio"\s*:\s*"([^"]*)"'`↔`extract_json_string_field` (requires closing `"`), **persona uses OPEN/partial pattern** `r'"persona"\s*:\s*"([^"]*)'`↔`extract_json_string_field_partial` (closing `"` optional) — **exact match of the asymmetry**✔; guard `if bio_match or persona_match`↔`has_bio_match \|\| has_persona_match`✔; (7) complete failure: Python returns a base dict, teri returns `None` — **noted divergence, non-downgrading**: teri's `None` routes `generate_social` into `generate_social_rule_based`, which produces the SAME base bio/persona defaults Python's step-7 dict would (rule-based fallback is the teri analogue of step-7's "基础结构"). Observable output equivalent. Tests (6 GREEN): salvage-truncated, all-fields-truncation, garbage→None, field-extraction-from-broken-JSON, salvage-path-taken (UNIQUE_LLM_SIGNATURE proves LLM-source over rule-based), genuine-garbage→rule-based. |
| **S-356** `_build_entity_context` (in-process half) | **FAIL** | src `oasis_profile_generator.py:414-473` vs teri `src/agent/mod.rs:1232-1250`. The PORT-NOW contract (parity.md:462, this task) is **parts 1-3 = attributes + related_edges + related_nodes**. teri's `build_entity_context` emits **Part 1 (attributes, mapped to name+kind — OK, teri's `Entity` has no attribute dict, a legit data-model mapping) + Part 3 (related_nodes, neighbor name+kind via `get_neighbors`)** but **DROPS Part 2 (`related_edges` → the `### 相关事实和关系` relationship/fact section, src:434-453)**. Part 2 emits per-edge `fact` lines and, absent a fact, directional `name --[edge_name]--> (相关实体)` / `(相关实体) --[edge_name]--> name` relationship lines. The relation IS available in teri's in-process graph (`Relation.kind`: WorksFor/LocatedIn/RelatedTo/Causes/Affects/Other, `src/graph/mod.rs:44-55`) — but `get_neighbors` returns only `Vec<&Entity>` (`src/graph/mod.rs:190`), discarding the edge, and `build_entity_context` never assembles a relationship-facts section into the prompt. This is a **NARROWING of the explicitly-flagged enrichment contract** — a disguised partial-skip of a portable, observable behavior (the relationship-kind context is distinct prompt output that demonstrably enriches the LLM). The `existing_facts` dedup set (src:435/445) is moot once Part 2 is restored against the Zep half (S-355 stays `[≠]`), so only the in-process Part-2 assembly need port. Backward-compat `None` fallback + no-neighbor flat fallback (Part 3) are correct. Tests cover enrichment-present / none / no-neighbor, but **none asserts a relationship/edge line in the prompt** — the missing branch is unproven AND, on inspection, genuinely absent. Stays `- [~]`. |
| **S-048** `call_batch_with_retry` | **PASS** | src `retry.py:195-237` vs teri `src/llm.rs:143-192`. Branches: (a) per-item loop calling `call_with_retry` per item — teri inlines the per-adapter retry loop (`max_retries+1` attempts, exhaust→Err), faithful to `RetryableAPIClient.call_with_retry` (retry.py:149-193)✔; (b) success → push to `results` in input order✔; (c) exhausted-retries failure → `BatchFailure{index, error}` (retry.py:228 `{index, item, error}`) — **`item` field omitted**: documented (b) non-contractual mapping (Rust `Fn`-factory closures are consumed on call; the `index` lets the caller recover the input from its own slice — all recovery info preserved, no observable loss)✔; (d) `continue_on_failure=true` → record failure + continue (retry.py:227-232)✔; (e) `continue_on_failure=false` → abort with `Err(e)` (retry.py:233-234 `raise`)✔; (f) `F: Fn()->Fut` (not `FnOnce`) so the op is re-invokable each retry✔. Back-off jitter omitted — pre-existing `[≠]` matching teri's adapter retry contract (stochastic, non-contractual). Tests (5 GREEN): empty, all-succeed, one-fails-continue-true (index=1, error carried), one-fails-continue-false (Err + error propagates, later op never runs), fail-then-succeed-via-retry (AtomicUsize proves exactly 2 calls). |

### Result
- **3 PASS** → S-360, S-361, S-048 flipped `- [~]` → `- [x]` in symbol-map.md.
- **1 FAIL** → S-356 stays `- [~]`. **Missing behavior for the next porter cycle:** port `_build_entity_context` **Part 2 (`related_edges` → relationship/fact section)**. Concretely: in `build_entity_context` (src/agent/mod.rs:1232), after Part 1 and before/with Part 3, emit a `### Related Facts and Relationships` section built from the entity's edges. Since `get_neighbors` returns only entities, add an edge-aware accessor (e.g. `KnowledgeGraph::get_neighbor_relations(id) -> Vec<(&Entity, &Relation)>` or expose edges) and emit one line per edge — mirror MiroFish: a `fact` line if present, else a directional `name --[RelationKind]--> (neighbor)` / reversed line by edge direction. Add a test asserting a relationship/edge line appears in the captured prompt. Zep half (S-355) remains `[≠]`.

### Symbol-map mutations applied (this gate)
- S-360 → `- [x]` (PASS); S-361 → `- [x]` (PASS); S-048 → `- [x]` (PASS).
- S-356 → kept `- [~]` (FAIL: dropped Part 2 related_edges enrichment — narrowing).
- No other rows touched. No source or Rust files edited. No commit (orchestrator commits).

---

## 2026-06-17 · S-356 re-verify (U-018) — `_build_entity_context` Part 2 (related_edges)

**Verdict: PASS** (re-verify of prior FAIL). Part 2 (`### Related Facts and Relationships`) is now ported, differential-verified branch-by-branch, and the open fact-branch question is adjudicated **(a) faithful mapping** — the fact line is a Zep-server artifact with no in-process analogue, not a dropped portable branch.

**Fact-branch adjudication: (a) — Zep-server-derived, non-droppable, S-355-class.**
MiroFish evidence for where edge `fact` originates:
- `oasis_profile_generator.py:438-451` iterates `entity.related_edges`; `fact = edge.get("fact","")`.
- `related_edges` is populated ONLY in `zep_entity_reader.py` (`:284-305`, `:366-405`), where each edge dict's `"fact": edge["fact"]` comes from `get_node_edges()`.
- `get_node_edges` (`zep_entity_reader.py:182`) calls `self.client.graph.node.get_entity_edges(node_uuid=...)` and reads `edge.fact` off the Zep SDK edge object.
- → `fact` is a **Zep-server-generated, LLM-extracted edge fact** produced server-side on ingestion. It is NOT derived from any in-process data teri also holds. Exactly the same provenance as the S-355 Zep-search half ([≠] sub-rule (b)).

teri's structs carry no relationship fact text (read in full, `src/graph/mod.rs`):
- `Entity { id, name, kind }` — no fact/summary/description/context field.
- `Relation { kind, weight, valid_at }` — no fact/summary/description/context field. `kind` is an enum (emitted by the directional line); `valid_at` is temporal, not a fact.
- → There is genuinely nothing to drop. The `fact` branch is correctly inert (commented-out, can never fire). The directional line IS the complete in-process contract. The commented fact-branch + helper seam is documentation of the [≠] boundary, not a disguised skip — the source behavior it omits has **no in-process observable to produce**.

**Direction handling — matches MiroFish exactly:**
- Outgoing (`direction=="outgoing"`): Python `- {entity.name} --[{edge_name}]--> (相关实体)` → teri `- {entity} --[{kind}]--> ({neighbor})`. ✓
- Incoming (else): Python `- (相关实体) --[{edge_name}]--> {entity.name}` → teri `- ({neighbor}) --[{kind}]--> {entity}`. ✓
- `get_neighbor_relations` walks `edges_directed(Outgoing)` then `Incoming` (is_outgoing flag), matching Python's outgoing/incoming partition. No double-count (petgraph returns each stored edge once per direction query).
- **Strict superset (not a downgrade):** MiroFish prints the literal placeholder `(相关实体)` ("related entity") because the Zep edge dict lacks the neighbor name in this branch; teri substitutes the real `neighbor_name`. More information, identical line shape.

**Heading text:** `### Related Facts and Relationships` ↔ Python `### 相关事实和关系` (faithful English rendering, consistent with the unit's other translated headings). ✓

**Empty/None fallback — preserved (unchanged):** `if !neighbor_relations.is_empty()` then inner `if !relationships.is_empty()` → no edges = no section, mirroring Python `if entity.related_edges:` / `if relationships:`. ✓

**Test check — both directions genuinely exercised (independently re-run: 2 passed):**
- `test_generate_social_part2_outgoing_relation_in_prompt`: edge Alice→Acme, context=Alice ⇒ asserts heading + `Alice --[WorksFor]--> (Acme Corp)` (outgoing branch). ✓
- `test_generate_social_part2_incoming_relation_in_prompt`: edge Acme→SanFrancisco, context=SanFrancisco ⇒ asserts heading + `(Acme Corp) --[LocatedIn]--> San Francisco` (incoming branch, reversed arrow). ✓
- Both go through the real `generate_social` prompt-capture path (PromptCaptureLlm), not a unit-isolated string — end-to-end prompt assembly. ✓
- No-edge fallback covered by the conditional + prior S-356 no-neighbor test.

**Baseline:** orchestrator-confirmed 335 passed / 3 ignored, build + clippy --all-targets clean. No regression.

### Symbol-map mutations applied (this gate)
- S-356 → `- [x]` (PASS). Fact-branch noted as (a) Zep-server artifact, S-355-class non-droppable [≠] boundary (documented inert seam, not a skip).
- No other rows touched. No source or Rust files edited. No commit (orchestrator commits).

### U-018 rollup status (NOTE for orchestrator — not edited here)
- S-356 was the LAST `- [~]` symbol in U-018. With S-356 → `- [x]`, U-018 is now **28 `- [x]` + 20 `- [≠]`, 0 `- [~]`/`- [!]`** → all symbols covered. **U-018 may roll up to unit `- [x]`** per the rollup rule. (Parity-ledger unit row not edited by this gate per contract.)

---

## 2026-06-17 · U-001 AppConfig — parity gate

**Unit:** U-001 `backend/app/config.py:Config` → `teri::config` (`src/config.rs`). 22 symbols S-001..S-022.
**Verdict:** PARTIAL PASS (20 PASS, 2 legit-pending). Unit **cannot roll up to `- [x]`** yet — S-003/S-005 are legitimate pending-dependency rows that stay `- [ ]`. Unit stays `- [~]` (partial).
**Differential method:** read `config.py` in full (every field: env name, default, type) and `config.rs::Config::build` + `validate_collect`; exercised via 36 config unit tests (run single-threaded — see test-isolation note).

### Per-symbol verdict (config.py vs config.rs)

| Sym | Field | Source (config.py) | Rust (config.rs) | Verdict |
|-----|-------|--------------------|--------------------|---------|
| S-001 | project_root_env | `os.path.join(dirname,'../../.env')` + `load_dotenv(override=True)` else ambient (l.11-17) | `dotenvy::dotenv().ok()` in `Config::load()` (l.112) | **[≠](c) CONFIRMED superset** — see adjudication 1 |
| S-002 | Config class | `class Config` (l.20) | `pub struct Config` (l.11) extend-Y | PASS |
| S-003 | SECRET_KEY | env `SECRET_KEY` def `mirofish-secret-key` (l.24) | absent — pending-U-002/U-003 | **PENDING (legit)** — adjudication 2 |
| S-004 | DEBUG | `FLASK_DEBUG`.lower()=='true', def `'True'` (l.25) | `FLASK_DEBUG` to_lowercase=="true", def `true` (l.231) | PASS |
| S-005 | JSON_AS_ASCII | `False` (l.28) | absent — pending-U-002/U-003 | **PENDING (legit)** — adjudication 2 |
| S-006 | LLM_API_KEY | env `LLM_API_KEY`, required (l.31) | `llm.api_key`; required in `validate_collect` (l.321) | PASS |
| S-007 | LLM_BASE_URL | def `https://api.openai.com/v1` (l.32) | def `https://api.openai.com/v1` (l.188) | PASS |
| S-008 | LLM_MODEL_NAME | env `LLM_MODEL_NAME`, def `gpt-4o-mini` (l.33) | `LLM_MODEL_NAME`→`LLM_MODEL`→def `gpt-4o` (l.181-183) | PASS (env-name+precedence) / **[!] default divergence flagged** — adjudication 3 |
| S-009 | ZEP_API_KEY | env `ZEP_API_KEY`, required (l.36) | `Option<String>` `.ok()`; required in validate_collect (l.234,324) | PASS |
| S-010 | MAX_CONTENT_LENGTH | `50*1024*1024` (l.39) | `50*1024*1024` (l.236) | PASS |
| S-011 | UPLOAD_FOLDER | join(dirname,'../uploads') (l.40) | env `UPLOAD_FOLDER` def `./uploads` (l.237) | PASS (env-backed superset; relative-uploads dir equiv) |
| S-012 | ALLOWED_EXTENSIONS | `{pdf,md,txt,markdown}` (l.41) | sorted Vec, exact 4 (l.171-177) | PASS (set semantics preserved) |
| S-013 | DEFAULT_CHUNK_SIZE | `500` (l.44) | `500` (l.240) | PASS |
| S-014 | DEFAULT_CHUNK_OVERLAP | `50` (l.45) | `50` (l.241) | PASS |
| S-015 | OASIS_DEFAULT_MAX_ROUNDS | env, def `10` (l.48) | env `OASIS_DEFAULT_MAX_ROUNDS` def 10 (l.242) | PASS (default+override tested) |
| S-016 | OASIS_SIMULATION_DATA_DIR | join(dirname,'../uploads/simulations') NOT env-backed (l.49) | env `OASIS_SIMULATION_DATA_DIR` def `./uploads/simulations` (l.246) | PASS (env-backed superset; default equiv) |
| S-017 | OASIS_TWITTER_ACTIONS | 6 strings, source order (l.52-54) | exact 6 strings, source order (l.141-148) | PASS (count+all 6 tested) |
| S-018 | OASIS_REDDIT_ACTIONS | 13 strings incl TREND+REFRESH, order (l.55-59) | exact 13, order, TREND<REFRESH (l.154-168) | PASS (count+all 13+order tested) |
| S-019 | REPORT_AGENT_MAX_TOOL_CALLS | env, def `5` (l.62) | env, def 5 (l.250) | PASS (default+override tested) |
| S-020 | REPORT_AGENT_MAX_REFLECTION_ROUNDS | env, def `2` (l.63) | env, def 2 (l.254) | PASS (default tested) |
| S-021 | REPORT_AGENT_TEMPERATURE | env, def `0.5` (l.64) | env, def 0.5 (l.260) | PASS (default+override tested) |
| S-022 | validate() | classmethod → `list[str]`; requires LLM_API_KEY+ZEP_API_KEY; empty=pass (l.67-74) | `validate_collect()->Vec<String>` (l.319), both required, empty=pass; `validate()` joins→Err (l.273) | PASS — contract match |

### validate() contract check
- Source (config.py:67-74): collect missing-var errors into a list; `run.py:28-34` prints all + `sys.exit(1)` if non-empty. Required: LLM_API_KEY, ZEP_API_KEY.
- Rust: `validate_collect()` returns `Vec<String>` (both vars, empty=pass) — exact contract. `validate()` joins into `Err` (non-zero exit equivalent). Both branches + collect-all tested (both-missing→2, only-zep→1, only-llm→1, both-present→0, validate()→Err when zep missing). MATCH.
- Keyless-CLI discipline: `--help`/`--version` parse before any `Config::load()`; load happens only in `run_cmd`/`serve_cmd` (main.rs:47,97). Preserved. **Wiring note (not a U-001 failure):** run/serve call `Config::load()` (which enforces LLM_API_KEY via ConfigMissing) but do NOT yet call `validate()`/`validate_collect()`, so ZEP_API_KEY is not yet enforced at runtime. The method+contract are ported & tested (S-022 PASS); wiring it into the run/serve preflight is downstream work — flag for orchestrator, does not block S-022.

### Three borderline adjudications
1. **S-001 project_root_env → `[≠]`(c) CONFIRMED.** teri DOES call `dotenvy::dotenv().ok()` in `Config::load()` (config.rs:112). dotenvy searches CWD and walks parent dirs for `.env` — a genuine SUPERSET of MiroFish's single explicit `MiroFish/.env` path + ambient fallback. The `.env` loading side effect is present and wider, no contractual observable output is dropped. Legit `[≠]` — NOT a disguised skip. **Confirm `[≠]`.**
2. **S-003 SECRET_KEY & S-005 JSON_AS_ASCII → legit PENDING, stay `- [ ]`.** teri has axum in Cargo.toml but **NO live HTTP/JSON surface today**: `serve_cmd` (main.rs:92-102) loads config, logs, then returns `TeriError::Unknown("API server not yet implemented")`; api/streaming.rs has zero `Router`/`route`/`axum::serve`/`Json(`/`TcpListener`. So neither a Flask-session secret (S-003) nor a JSON-encoder ASCII flag (S-005) has any observable surface — both are genuinely non-contractual until the axum server/HTTP-JSON encoder exists (U-002/U-003). NOT a drop: recorded with `pending-U-002/U-003` note. **Stay `- [ ]`.** (Had a live `serve` route existed, these would be drops requiring port-now — verified it does not.)
3. **LLM_MODEL_NAME default divergence (S-008) → `[!]` OWNER-VISIBILITY flag, defensible.** MiroFish default `gpt-4o-mini`; teri default `gpt-4o` (architect decision). The env NAME `LLM_MODEL_NAME` and read-precedence ARE ported correctly (PASS for the symbol's env-binding contract). The default value differs — an observable behavioral difference when neither `LLM_MODEL_NAME` nor `LLM_MODEL` is set. This is a destination-architecture choice (teri targets shimmy/OpenAI-compat endpoints, picks its own default), which is legal as an `[!]`-flagged divergence — NOT a banned `[≠]`-because-dest-wont-use-it, and NOT a silent downgrade (it is documented in config.rs:64-69 and symbol-map S-008). **Surfaced for OWNER decision**, not silently accepted. If owner wants strict parity, change teri default to `gpt-4o-mini`; otherwise the `[!]` flag stands.

### Test-isolation defect (NOTE — not a parity failure)
Porter claimed 362 passed / 0 failed. Under default parallel `cargo test`, `config::tests::test_debug_env_false` FAILS (config.rs:354) due to a **global-env-var race**: many config tests mutate process env (`FLASK_DEBUG`, `UPLOAD_FOLDER`, etc.) concurrently. Single-threaded (`--test-threads=1`): **36/36 config tests pass; 353/353 lib tests pass.** The Config LOGIC is correct (proven single-threaded); the tests are not serialized (`serial_test`/mutex). This is a test-hygiene defect to fix (route to porter as a test-quality follow-up), but it does NOT change any per-symbol parity verdict.

### Symbol-map mutations applied (this gate)
- S-001 → `[≠]` confirmed (already `[≠]`; left as-is, superset verified).
- S-002, S-004, S-006..S-022 → `- [~]` flipped to `- [x]` (20 symbols PASS).
- S-003, S-005 → left `- [ ]` with pending-U-002/U-003 note (legit pending-dep).
- No source/Rust files edited. No commit.

### U-001 rollup status (NOTE for orchestrator — parity-ledger row NOT edited here)
- Coverage: 1 `[≠]` (S-001) + 20 `[x]` (S-002,S-004,S-006..S-022) + **2 `- [ ]` legit-pending (S-003,S-005)**.
- Rollup rule: unit `- [x]` requires EVERY symbol `[x]`/`[≠]`. With S-003/S-005 legitimately `- [ ]`, **U-001 CANNOT roll up to `- [x]` yet → stays `- [~]` (partial)**. It rolls up only after U-002/U-003 (axum HTTP layer) ports SECRET_KEY + JSON_AS_ASCII.
