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

---

## 2026-06-17 · U-012 TaskManager — parity gate

**Verdict: FAIL (unit `- [~]` partial).** Differentially verified `task.py:TaskManager` → `src/task.rs`. 27/27 module tests pass; 27 of 29 symbols PASS. Two symbols held: **S-163/S-164 (i18n narrowing → pending-U-005, NOT flagged by porter)** and **S-155 (to_dict timestamp microsecond==0 divergence)**.

### Per-symbol table (source line → rust line, evidence)

| Sym | Behavior | Source | Rust | Verdict |
|-----|----------|--------|------|---------|
| S-138 | `TaskStatus` enum (str,Enum) | task.py:16 | task.rs:48 | PASS — 4 variants, serde `rename_all=lowercase` |
| S-139 | PENDING="pending" | task.py:18 | task.rs:63 | PASS — `as_str`+serde test (line 426/435) |
| S-140 | PROCESSING="processing" | task.py:19 | task.rs:64 | PASS |
| S-141 | COMPLETED="completed" | task.py:20 | task.rs:65 | PASS |
| S-142 | FAILED="failed" | task.py:21 | task.rs:66 | PASS |
| S-143 | `Task` dataclass+to_dict | task.py:25 | task.rs:96 | PASS — struct present |
| S-144 | task_id:str | task.py:27 | task.rs:97 | PASS |
| S-145 | task_type:str | task.py:28 | task.rs:98 | PASS |
| S-146 | status:TaskStatus | task.py:29 | task.rs:99 | PASS |
| S-147 | created_at:datetime | task.py:30 | task.rs:101 | PASS (DateTime<Utc>) |
| S-148 | updated_at:datetime | task.py:31 | task.rs:103 | PASS |
| S-149 | progress:int=0 | task.py:32 | task.rs:105 | PASS (i64, default 0) |
| S-150 | message:str="" | task.py:33 | task.rs:107 | PASS (default "") |
| S-151 | result:Optional[Dict]=None | task.py:34 | task.rs:109 | PASS (Option<Value>) |
| S-152 | error:Optional[str]=None | task.py:35 | task.rs:111 | PASS |
| S-153 | metadata:Dict=field({}) | task.py:36 | task.rs:113 | PASS (HashMap, default {}) |
| S-154 | progress_detail:Dict=field({}) | task.py:37 | task.rs:115 | PASS |
| **S-155** | **to_dict() JSON shape** | task.py:39 | task.rs:143 | **FAIL — timestamp microsecond==0 divergence (below). Field names/order/status-string/metadata all match.** |
| S-156 | TaskManager singleton | task.py:56 | task.rs:174 | PASS — OnceLock idiom-map |
| S-157 | _instance (singleton ref) | task.py:62 | task.rs:178 | PASS — `static TASK_MANAGER: OnceLock` |
| S-158 | _lock (threading.Lock) | task.py:63 | task.rs:175 | PASS — parking_lot::Mutex (non-poisoning; equivalent observable) |
| S-159 | __new__ double-checked lock | task.py:65 | task.rs:185 | PASS — `global()`+`get_or_init`; test `global_returns_same_instance` (820) + `global_registry_is_shared` (826) prove one shared registry |
| S-160 | create_task → uuid | task.py:75 | task.rs:206 | PASS — uuid v4, PENDING, metadata or {}; unique-id + concurrency tests |
| S-161 | get_task → Optional | task.py:103 | task.rs:240 | PASS — Some/None, clone-out-of-lock |
| S-162 | update_task partial | task.py:108 | task.rs:267 | PASS — all 6 optionals, None=unchanged, updated_at always bumped, nonexistent=noop (tests 562/592/609) |
| **S-163** | **complete_task** | task.py:147 | task.rs:312 | **HELD — pending-U-005 (i18n narrowing, below). Status/progress=100/result correct.** |
| **S-164** | **fail_task** | task.py:157 | task.rs:337 | **HELD — pending-U-005 (i18n narrowing, below). Status/error correct; correctly does NOT set progress=100.** |
| S-165 | list_tasks sorted desc | task.py:166 | task.rs:363 | PASS — `reverse=True` → `b.cmp(a)`; filter; newest-first test (707) proves order |
| S-166 | cleanup_old_tasks | task.py:174 | task.rs:391 | PASS — `created_at < cutoff` strict `<` matched; COMPLETED/FAILED only; PENDING/PROCESSING survive (test 738); boundary test (796) proves strict `<` |

### i18n adjudication (THE key call) — task.py evidence

**MiroFish uses `t(key)`, NOT a literal.** task.py:13 `from ..utils.locale import t`; task.py:153 `message=t('progress.taskComplete')`; task.py:162 `message=t('progress.taskFailed')`. `t()` (locale.py:35) resolves per-locale: default locale is `zh` (locale.py:30-32,37), but it supports 7 locales (zh/en/es/fr/pt/ru/de per locales/languages.json). Under `en` the message becomes "Task complete"/"Task failed" (locales/en.json:415-416); under de/fr/etc. it differs. The `message` field flows into `to_dict()` and `list_tasks()` → **observable, locale-varying output.**

**Porter hard-coded the zh defaults** (task.rs:35-37 `MSG_TASK_COMPLETE="任务完成"`, `MSG_TASK_FAILED="任务失败"`). The constants' VALUES are correct (exact match to locales/zh.json:415-416). BUT the port drops the locale parameterization — it always emits zh regardless of locale. That is a **NARROWING of an observable feature**, not a faithful port.

**The defect:** the porter's doc comment (task.rs:14-19) frames this as faithful ("the same literal strings are used as constants so the serialised output matches the Python default exactly") and does **NOT** flag a pending-dependency on U-005 (the locale subsystem, S-036..S-042, all `- [ ]`, unported). Per owner directive + established precedent (SECRET_KEY→U-002 S-003; S-189 reclassified `[≠]`→pending-U-012), a dropped-but-portable feature is acceptable ONLY as an explicit **pending-U-005** flag (code comment + symbol-map note). A silently-hard-coded default with a "matches exactly" comment is the banned silent-skip → **S-163/S-164 HELD `- [ ]` with pending-U-005 note** until: (a) the code comment is corrected to state locale parameterization is deferred to U-005, and (b) once U-005 lands, `message` routes through teri's `t()`.

### Timestamp-format verdict (S-155) — observable divergence

Python `datetime.isoformat()` **omits the fractional part when microsecond==0** (verified: `datetime(2024,1,1,12,30,45,0).isoformat()` → `2024-01-01T12:30:45`, no `.000000`); emits exactly 6 digits otherwise. Teri uses `%Y-%m-%dT%H:%M:%S%.6f` (task.rs:148-149) which **always emits 6 fractional digits** → `...45.000000` where Python emits `...45`. For a timestamp landing on a whole second, `to_dict`/`list_tasks` output diverges by the trailing `.000000`. Narrow (timing-dependent) but a real serialization-shape gap under the no-downgrade directive. Microsecond-present case, field names, field order, status-as-string, and naive-no-tz (Python `datetime.now()` has no tz suffix; teri formats UTC without offset → shape matches) all CORRECT. Fix: chrono `%.f` (variable, trims trailing zeros) is closer to Python; exact `isoformat()` parity needs conditional formatting (omit fraction iff microsecond==0). **S-155 FAIL** until the microsecond==0 case matches.

### Singleton idiom-map — PASS
OnceLock<TaskManager> + parking_lot::Mutex<HashMap> genuinely preserves "one shared registry per process": `global_returns_same_instance_across_calls` (820, pointer-equality) + `global_registry_is_shared_across_calls` (826, task created via tm1 visible via tm2). Concurrency smoke tests (846/864) prove thread-safe create/update. parking_lot non-poisoning is an acceptable observable-equivalent of threading.Lock.

### Tests assessment
27/27 genuinely behavioral (not compile-only): serde roundtrip asserts exact `"pending"` etc.; to_dict asserts all 11 keys + types + values; cleanup asserts PENDING/PROCESSING survive + strict-`<` boundary; sort asserts newest-first by id; singleton asserts pointer-equality + cross-handle visibility. GAP: no test asserts the microsecond==0 timestamp case (the divergence is untested), and no test asserts non-zh locale output (the narrowing is untested — tests only assert the zh constant).

### U-012 rollup status (NOTE for orchestrator — parity-ledger row NOT edited here)
- Coverage: **27 `[x]`** (S-138..S-162, S-165, S-166) + **3 `- [ ]` held** (S-155 timestamp defect; S-163/S-164 pending-U-005).
- Rollup rule: unit `- [x]` requires EVERY symbol `[x]`/`[≠]`. With S-155/S-163/S-164 held `- [ ]`, **U-012 CANNOT roll up → stays `- [~]` (partial). Do NOT commit as done.**
- Route back to porter: (1) fix S-155 timestamp microsecond==0 formatting; (2) re-flag S-163/S-164 as pending-U-005 (correct the code comment + add symbol-map pending note); message routes through `t()` once U-005 lands.

### Constraints honored
- No source/Rust files edited. No commit. Only symbol-map `[x]` flips + this verdict appended.

---

## 2026-06-17 · U-012 re-verify (S-155 fix + S-163/S-164 pending-U-005)

Re-verification of the 3 symbols held last cycle. Verifier role: differential gate, fail-closed.

### S-155 (`Task.to_dict` timestamp) — **PASS** (FAIL→[x])
Prior FAIL: teri always emitted `%.6f` (`...:45.000000`); Python `datetime.isoformat()` omits the fraction entirely when µs==0.

Fix verified in `src/task.rs:51-57` — `python_isoformat()` branches on `dt.timestamp_subsec_micros()==0`:
- µs==0 → `%Y-%m-%dT%H:%M:%S`
- µs!=0 → `%Y-%m-%dT%H:%M:%S%.6f`
`to_dict` (task.rs:168-169) uses it for both `created_at`/`updated_at`.

Differential evidence — Python `datetime.isoformat()` run directly:
| input | Python isoformat | teri python_isoformat |
|-------|------------------|------------------------|
| `2024-01-01 12:30:45` (µs=0) | `2024-01-01T12:30:45` | `2024-01-01T12:30:45` ✓ |
| `…45.123456` (µs=123456) | `2024-01-01T12:30:45.123456` | `2024-01-01T12:30:45.123456` ✓ |
| `…45` µs=123000 | `…45.123000` | `…45.123000` (chrono `%.6f` zero-pads 6 digits) ✓ |
| `…45` µs=1 | `…45.000001` | `…45.000001` ✓ |

Confirmed Python emits EITHER 0 OR 6 fractional digits — never a 3-digit-millis form; chrono's `%.6f` is fixed-6-digit, so teri matches that exactly (not a 3-digit form). No `+`/`Z` tz suffix on either side (Python naive datetime; teri formats without offset).

Test `test_python_isoformat_matches_datetime_isoformat` (task.rs:441-456) genuinely asserts BOTH branches: whole-sec `== "2024-01-01T12:30:45"` (no fraction) AND sub-sec `== "2024-01-01T12:30:45.123456"`, plus `!contains('+')` and `!ends_with('Z')`. Orchestrator: 399 tests pass, clippy --all-targets clean. → **S-155 = `- [x]`**.

### S-163 / S-164 (`complete_task` / `fail_task` message) — confirmed correctly recorded **pending-U-005**, REMAIN `- [~]`
Source: MiroFish sets `message` from `t('progress.taskComplete')` / `t('progress.taskFailed')` (task.py:153,162) — locale-parameterized over 7 locales (default zh). teri emits the zh default via `MSG_TASK_COMPLETE`/`MSG_TASK_FAILED` (task.rs:43,45); values match `locales/zh.json` (`progress.taskComplete`=`任务完成`, `progress.taskFailed`=`任务失败`) — verified by reading the source locale file.

The prior HOLD reason (code FRAMED the hard-coded zh string as "faithful"/"matches exactly" = banned silent-narrowing) is **RESOLVED**: module doc (task.rs:14-23) and the constant comments (task.rs:34-45) now HONESTLY frame the strings as a **TEMPORARY pending-U-005 placeholder, explicitly "NOT a faithful port"**, with the instruction to route `message` through teri's `t()` when U-005 (S-036..S-042) lands. The `complete_task`/`fail_task` doc-comments still say "localised message" (accurate — the contract IS a locale lookup).

This is the correctly-recorded pending-dependency pattern (U-001/SECRET_KEY→U-002 precedent), NOT a silent skip and NOT a disguised `[≠]`: the locale parameterization is genuinely not portable until the locale subsystem (U-005) exists. → S-163/S-164 **stay `- [~]` with pending-U-005 notes** (added to symbol-map). Do NOT flip to `[x]`.

### U-012 rollup
With S-155 → `[x]`, U-012 is now **27 `[x]` + 2 `[~]`** (S-163/S-164 pending-U-005). The unit **stays `- [~]` (partial)** in the parity ledger — it rolls up to `- [x]` only when U-005 lands and `complete_task`/`fail_task` route their `message` through `t()`. Parity-ledger unit row NOT edited (per instruction).

### Constraints honored
- No source/Rust files edited. No commit. Only S-155 `[x]` flip + S-163/S-164 pending-U-005 notes + this verdict appended.

---

## 2026-06-17 · U-011 · `ProjectManager` / `Project` / `ProjectStatus` (project.py) → `src/models/project.rs`

**Verdict: FAIL** — 3 behavioral divergences. 37/40 symbols faithful; unit CANNOT flip `[x]`.

**Type:** literal differential. Source `backend/app/models/project.py` (306 lines) is runnable Python; ran both sides over the same inputs and diffed returned values + on-disk JSON.

**Baseline:** `cargo test project` = 21 passed, 0 failed. Build green. (Compile is precondition only, not parity.)

### FAIL #1 (BLOCKER) — S-129 `create_project` returns stale `updated_at` (the flagged clone)
- **Python contract** (lines 148-165 + save_project 170): `create_project` builds project (created_at==updated_at==now), then `save_project(project)` MUTATES the SAME object's `updated_at` to a LATER save-time, then returns THAT object. So the **returned** object's `updated_at` == the **persisted** file's `updated_at`, and is STRICTLY LATER than `created_at`.
- **Rust** (line 453): `self.save_project(&mut project.clone())` — passes a CLONE; mutation lands on the throwaway clone (and on disk), but `create_project` returns the ORIGINAL untouched `project`.
- **Reproduced** (live, against the crate): returned created_at=`...261049`, returned updated_at=`...261049`, PERSISTED updated_at=`...261141`.
  - `returned.updated_at == persisted` → **false** (Python: true)
  - `returned.updated_at == created_at` → **true** (Python: false)
- The two invariants are INVERTED. A consumer reading the returned object's `updated_at` sees construction-time, not save-time; it disagrees with disk. Real downgrade.
- **Fix:** save the real object and return it, e.g. `let mut project = …; self.save_project(&mut project)?; Ok(project)` (mirror Python's same-object mutation).

### FAIL #2 — S-121 `from_dict` silently drops malformed/legacy `files` (data loss on load)
- **Python** (line 88): `files = data.get('files', [])` — keeps the raw list verbatim, NO shape validation (duck-typed).
- **Rust** (lines 267-270): `serde_json::from_value(v.clone()).ok().unwrap_or_default()` — if ANY entry fails the strict 4-field `ProjectFile` struct, `.ok()` swallows it and the ENTIRE `files` vector collapses to `[]`.
- **Reproduced:** `files:[{"filename":"x.txt","path":"/a/x.txt","size":10}]` (the 3-key form documented in project.py line-36 comment `{filename, path, size}`) → Rust `files.len()==0`; Python keeps `len==1`.
- In-system round-trips (save_file_to_project writes the 4-key form) are unaffected, but external/legacy/partial `project.json` loses file records silently. Per "when in doubt, FAIL." Fix: tolerate unknown/partial shapes (passthrough `Value`, or propagate instead of swallow).

### FAIL-adjacent #3 (NON-BLOCKING, recorded) — on-disk JSON key ORDER differs
- Python `json.dump` emits insertion order (`project_id` first); Rust `serde_json::json!` Map emits alphabetical (`analysis_summary` first).
- **Assessment:** non-contractual / unobservable — JSON object order is not semantically meaningful, file is read back only via keyed `from_dict`, no byte-comparison consumer exists. Qualifies under the `[≠]` "non-contractual" bar. Does NOT block. Noted only.

### VERIFIED FAITHFUL (37/40 symbols, evidence below)
- **S-098..S-103 ProjectStatus**: all 5 variants serialize to exact Python `.value` (`created`/`ontology_generated`/`graph_building`/`graph_completed`/`failed`) + round-trip + `as_str()`. Confirmed by test + serde.
- **S-104..S-119 Project fields + S-120 to_dict**: 15 keys present (NOTE: 15, not 14 — both source & port emit 15; prompt's "14" miscount), status→string value, None→null, non-ASCII written RAW (中文 verified in file, no `\u`), indent=2.
- **S-121 from_dict defaults** (the EXPRESSIBLE part): name→"Unnamed Project", status→Created, created_at/updated_at→"", total_text_length→0, chunk_size→500, chunk_overlap→50, optionals→None, missing project_id→Err. All exact. (files-tolerance is the FAIL #2 carve-out.)
- **S-122..S-128**: ProjectManager + path helpers (`{projects_dir}/{id}`, `/project.json`, `/files`, `/extracted_text.txt`) — exact.
- **S-130 save_project**: mutates updated_at to save-time, writes pretty JSON, ensure_ascii=False parity. Faithful in isolation.
- **S-131 get_project**: missing→Ok(None); corrupt JSON→Err; valid-but-missing-project_id→Err. Matches actual Python (uncaught JSONDecodeError / KeyError — ledger's "returns None on corrupt" was WRONG; port is correct).
- **S-132 list_projects**: sort created_at DESC + take limit; non-project dir entries skipped (get_project→None); corrupt propagates Err. Match.
- **S-133 delete_project**: absent→Ok(false) (NOT an error — ledger's "raises if not found" was WRONG; port is correct), present→true.
- **S-134 save_file_to_project**: ext = splitext[1].lower() incl. dot (all 6 edge cases incl. trailing-dot `"."` and dotfile→"" match Python), safe_filename=uuid4().hex[:8]+ext (8 hex), 4-key return, size==bytes len. Match.
- **S-135/S-136 save/get_extracted_text**: round-trip incl. non-ASCII; missing→None. Match.
- **S-137 get_project_files**: missing dir→[]; only is_file entries; full paths. Match.
- **S-129 create_project** project_id format `proj_`+12 lowercase hex (no hyphens), dir structure — faithful EXCEPT the updated_at return (FAIL #1).

### Symbols that must stay `[~]` (not provably faithful)
- **S-129 create_project** — FAIL #1 (returned `updated_at` stale). Stays `[~]`.
- **S-121 from_dict** — FAIL #2 (files silently dropped). Stays `[~]`.

### Verdict on flagged #1 (clone)
REAL divergence, not acceptable. The clone makes the returned object's `updated_at` disagree with both Python and the persisted file. Unit cannot flip `[x]` until create_project saves-and-returns the same object.

### Overall
**U-011 → FAIL.** Route to porter: (1) fix create_project to mutate+return the same object (S-129); (2) make from_dict tolerate non-strict `files` shapes instead of `.ok()`-swallowing the whole vector (S-121). 38/40 symbols are `[x]`-ready on re-verify; S-129 + S-121 stay `[~]`. Key-order (#3) is a recorded non-contractual `[≠]`, no action required.

### Constraints honored
- No source/Rust files edited. No ledger/symbol-map rows flipped. Temp differential probe written to `tests/` and REMOVED after run. Only this verdict appended.

---

## 2026-06-17 — U-011 `project.py` RE-VERIFY (FAIL #1 S-129 + FAIL #2 S-121 fixes) → PASS

Re-verification of the two prior FAILs after porter fix. Read `src/models/project.rs`,
diffed against `MiroFish/backend/app/models/project.py`, ran the named regression tests
plus the full suite. Did NOT take the porter report on faith.

### FAIL #1 (S-129 create_project) — RESOLVED ✓
- **Source contract** (project.py:148-165): builds `project` with `updated_at=now` (==created_at),
  calls `save_project(project)` which mutates **the same object** in-place
  (`project.updated_at = datetime.now().isoformat()`, line 170), then returns that same object.
  So the returned object's `updated_at` = save-time value, `>= created_at`, == on-disk value.
- **Rust** (project.rs:470-472): `self.save_project(&mut project)?; Ok(project)` — `save_project`
  (line 487) stamps `project.updated_at = python_isoformat_local()` in-place before serialising,
  and the SAME `project` is returned. Exact behavioral match. The old `&mut project.clone()`
  bug (mutated a throwaway temp, returned stale `updated_at == created_at`) is gone.
- **Test** `test_create_project_updated_at_matches_persisted_and_gte_created_at`: PASS.
  Asserts (a) returned `updated_at` == persisted project.json `updated_at`, (b) `updated_at >= created_at`.

### FAIL #2 (S-121 from_dict files) — RESOLVED ✓
- **Source contract** (project.py:88): `files=data.get('files', [])` — pure untyped passthrough,
  zero per-element validation; Python (`List[Dict[str,str]]`) keeps ANY array verbatim including
  the legacy on-disk 3-key form. `to_dict` (line 63) emits `self.files` verbatim.
  `save_file_to_project` (lines 267-272) returns the **4-key** shape
  (`original_filename, saved_filename, path, size`) — the docstring's `{filename,path,size}`
  at line 251 is a stale comment; actual runtime return is 4 keys.
- **Rust**: `Project.files: Vec<Value>` (project.rs:159); from_dict
  `obj.get("files").and_then(|v| v.as_array().cloned()).unwrap_or_default()` (lines 280-283) —
  exact match to Python's untyped passthrough. The old `Vec<ProjectFile>` strict per-element
  `.ok()` parse collapsed the WHOLE vector to `[]` on any non-4-key entry (the downgrade) — gone.
  `to_dict` emits `self.files` verbatim (line 205). `save_file_to_project` (lines 613-618) still
  produces the 4-key `ProjectFile` shape (return type unchanged).
- **Test** `test_from_dict_legacy_3key_files_entry_preserved_verbatim`: PASS. Legacy
  `{filename,path,size}` entry → len==1 (not 0), preserved verbatim, survives to_dict→from_dict round-trip.

### No NEW divergence from the `Vec<ProjectFile>`→`Vec<Value>` field-type change
- to_dict 15-key shape intact: `test_to_dict_has_all_14_keys` PASS (15 keys: project_id, name,
  status, created_at, updated_at, files, total_text_length, ontology, analysis_summary, graph_id,
  graph_build_task_id, simulation_requirement, chunk_size, chunk_overlap, error — matches py:57-72).
- Non-ASCII-raw (ensure_ascii=False) intact: `test_to_dict_non_ascii_not_escaped` PASS.
- save_file 4-key shape intact: `test_save_file_to_project` PASS. save/get round-trip: PASS.
- Blast radius: only consumer of `Project.files`/`ProjectFile` outside project.rs is the re-export
  in `src/models/mod.rs`. No code expects the typed `Vec<ProjectFile>` for the `files` field. None.

### Evidence
- `cargo test --lib models::project`: 23 passed, 0 failed.
- Two named regression tests (`--exact`): 2 passed.
- Shape/non-ascii/save_file/roundtrip (`--exact`): 4 passed.
- Full suite: **446 passed, 6 ignored, 0 failed** (5 suites). Matches porter report.
- `cargo clippy --lib`: clean (0 warnings/errors).

### Recorded `[≠]` (carried, non-contractual — no action)
- **JSON key order**: serde_json::json! emits in declaration order matching Python dict insertion
  order; even if it diverged, key order in a JSON object is non-contractual (unobservable to any
  consumer that parses by key). Survives the `[≠]` bar = non-contractual. Not a feature skip.

### Verdict
- **S-129: PASS.  S-121: PASS.**
- **U-011: 40/40 symbols `[x]`** (S-098..S-137 all exercised; symbol-map updated this session).
- **U-011 → PASS.** May flip ledger `- [x]` and commit. The two prior FAILs are genuinely resolved
  in code + proven by passing differential tests; no new divergence introduced.

### Constraints honored
- No source/Rust impl files edited. Only symbol-map S-098..S-137 flipped to `[x]` and this verdict appended.

---

## 2026-06-17 · U-015 completion · S-189 `build_graph_async` + S-192 `set_ontology` (+ EntityKind/RelationKind::Custom)

**Verdict: PASS** (differential + structural). Unit U-015 → all symbols `- [x]`/`- [≠]`; rollup satisfied.
**Verifier:** rust-port-parity-verifier (opus). **Build precondition:** 550 lib tests pass, 0 failed; clippy `--all-targets -D warnings` clean.

**Type:** map-onto-substrate (DECISION-1/DECISION-8). Zep SaaS path is not runnable here; this is behavioral-equivalence of the mapped native petgraph pipeline, verified by reading both sides + running the teri differential tests (15 graph_builder, 46 graph, incl. 6 build tests).

### S-189 `build_graph_async` → `services/graph_builder.rs::build_graph_async` — PASS
Source: `graph_builder.py:54-98` (build_graph_async) + `:100-191` (_build_graph_worker). Rust: `src/services/graph_builder.rs:84` (async fn) + `:130` (worker) + `:164` (worker_inner).
- **Spawn contract** (graph_builder.rs:96-122): creates `graph_build` task with metadata `{graph_name, chunk_size, text_length}` (matches py:78-85), captures `i18n::get_locale()` before spawn (matches py:88 `get_locale()`), `tokio::spawn(i18n::with_locale(locale, …))` (idiom thread+`set_locale`→task-local), returns task_id immediately. **Proven** by `test_build_graph_async_returns_task_id_immediately` (real spawn; task_id non-empty + in registry as `graph_build`).
- **Worker lifecycle** = port of `_build_graph_worker` try/except: PROCESSING@5% → … → `complete_task` (Completed + `progress.taskComplete`) on Ok; `fail_task(err.to_string())` (Failed + `progress.taskFailed` + error string) on any Err. **Proven** by `test_build_graph_worker_inner_completes_with_result` (COMPLETED + result shape) and `test_build_graph_worker_inner_llm_failure_returns_err` (Err→fail_task→FAILED, error string propagates). `build_graph_worker` (spawn target, :130) calls the SAME `build_graph_worker_inner` (:141) the tests drive — coverage REFACTORED, not lost (scrutiny point #5 cleared).
- **Milestone parity** (5/15/20/20-60/90/100): emitted via TaskManager; keys present in both locales with matching placeholders (`textSplit{count}`, `sendingBatch{current/total/chunks}`). 60% `waitingZepProcess` bridge correctly NOT emitted.
- **Result shape:** `{graph_name, graph_info{node_count,edge_count,entity_types[]}, chunks_processed, graph:<serialized>}`. `graph_info` mirrors MiroFish `_get_graph_info.to_dict()`; the embedded `graph` is a STRICT SUPERSET of MiroFish's `graph_id` handle (the retrievable graph the handle pointed to is preserved inline, not dropped).

### `[≠]` challenge — all three SURVIVE (genuinely Zep-inexpressible / non-contractual)
- **10% create_graph (S-191):** `client.graph.create()` = Zep SaaS server-object creation returning a server `graph_id` handle. teri has no Zep client (DECISION-1); graph is in-memory; no remote object, no handle. `graphCreated{graphId}` is a pure Zep artifact. **Inexpressible substrate; no teri output dropped.** Legitimate `[≠]`.
- **60-90% wait_for_episodes (S-194):** polls Zep's async episode-processing queue (3s poll/600s timeout). teri extraction is synchronous-await — no async server queue. **Inexpressible substrate.** Legitimate `[≠]`.
- **`_batch_size` (S-193):** `add_text_batches` batches into `graph.add_batch` network calls with `time.sleep(1)` rate-limiting. teri makes per-chunk LLM calls with adapter retry/backoff — no Zep batch endpoint. Param accepted for call-shape parity, ignored. **Non-contractual** (no observable output difference; pure network-pacing artifact). Legitimate `[≠]`.
None is a disguised portable-feature skip: each maps onto "no Zep client," not "teri won't use it." `graph_id` drop is compensated by embedding the graph (superset).

### S-192 `set_ontology` → `KnowledgeGraph::set_ontology` — PASS (NOT inert — scrutiny point #1 cleared)
Source: `graph_builder.py:205-292`. Rust: `src/graph/mod.rs:238`. Records `ontology_entity_types` + `ontology_edge_types` (name field from each `entity_types[]`/`edge_types[]`); idempotent (second call replaces).
**The recorded names DO reach the build output for BOTH entities AND edges** (verified end-to-end):
- entity prompt: `entity_extraction_prompt_with_custom` injects names into kind_list (graph/mod.rs:850).
- entity parser: `parse_entities_json_with_custom` maps registered name → `Custom` (`:955`); built-in name still maps to built-in (`:947-951`); unknown-unregistered → `Other` (`:957`).
- edge prompt: `relation_extraction_prompt_with_custom` injects edge names (`:895`).
- edge parser: inline Pass-2 match maps registered edge name → `RelationKind::Custom` (`:672-679`); built-in edge name still built-in; else `Other`.
**Differential proof** `test_build_with_custom_relation_kind_emits_custom_variant`: ontology `{MediaOutlet, COVERS_TOPIC}` → graph emits `EntityKind::Custom("MediaOutlet")` + `RelationKind::Custom("COVERS_TOPIC")`. Built-in-still-wins proven by `test_parse_entities_builtin_kind_still_maps_to_builtin` (Person→Person) and `test_relation_kind_builtins_unchanged`. NOT a silent no-op.
NOTE: worker re-extracts names inline (graph_builder.rs:192-212) rather than calling `graph.set_ontology()` then reading fields — functionally identical extraction logic; passes the same `(entity_types, edge_types)` slices into `build_with_progress_and_ontology`. Owner override of DECISION-8 item #2 applied: `RelationKind::Custom` added (custom EDGE emission no longer deferred; the prior `- [!]` is resolved, not outstanding).
**Zep-SDK `[≠]` items SURVIVE:** Pydantic `EntityModel`/`EdgeModel` synthesis (inexpressible — no Zep client; behavior=type-set-constraint IS ported), `RESERVED_NAMES`/`safe_attr_name` (non-contractual — guards a Zep key namespace teri's `Entity{id,name,kind}` lacks), `Field(default=None)`/UserWarning suppression (inexpressible Zep-SDK API). None is a portable-feature skip.

### EntityKind::Custom / RelationKind::Custom additions — PASS (additive, no regression)
- Additive tuple variant on each enum (graph/mod.rs:27, :64). Display arm added (`:39`, `:76`) → emits PascalCase/UPPER_SNAKE name verbatim. **2 match sites narrowed** (entity parser `:952`, Pass-2 edge match `:672`) — `_ => Other` became `other => {Custom if registered else Other}`; no existing arm swallowed. Standalone `parse_relations_json` (`:1013`) keeps `_ => Other` (not on U-015 build path; receives no custom kinds — correct).
- **No serde regression (independently verified by injected round-trip test):** existing variants serialize as bare strings (`"Person"`, `"WorksFor"`); `Custom` as `{"Custom":"X"}`; a previously-serialized `"Organization"` deserializes identically. Existing JSON/bincode graphs round-trip unchanged.
- **No other exhaustive `match EntityKind/RelationKind`** exists outside graph/mod.rs (grep-confirmed); all agent consumers (agent/mod.rs:888,922,933,1239,1274,1299) use `.to_string()`/Display — zero changes needed (architect blast-radius confirmed).

### No-regression to verified code (scrutiny point #4 cleared)
- `build<L>` signature byte-identical (`:481`); body refactored to delegate `build_with_progress`→`build_with_progress_and_ontology(…, &[], &[])`. With empty ontology slices, prompts/parsers are byte-identical to HEAD's inline body (the `_with_custom` fns delegate to the same logic). **6 build tests pass unchanged** (from_seed/empty/dup/unknown-ref/llm-error + multi-chunk). 550 lib tests green.

### Symbols verified (U-015): 19/19 → all `- [x]`/`- [≠]`
S-189 `[x]`, S-192 `[x]`, S-190 `[x]` (prior). `[≠]`: S-181..S-188, S-191, S-193, S-194, S-195, S-196, S-197 (Zep-SaaS-specific, DECISION-1 scope; each independently confirmed inexpressible/non-contractual). EntityKind/RelationKind::Custom additions PASS. Rollup satisfied → U-015 `- [x]`.

---

## 2026-06-17 — U-019 sub-cycle (a): simulation-config DATA MODEL — VERDICT: PASS (opus)

**Scope:** ONLY the data model (S-374..S-429, 56 symbols). The `SimulationConfigGenerator` class and
its LLM/generation logic (S-430+) are later sub-cycles and correctly remain `- [ ]`. The U-019 UNIT is
NOT marked done.

**Method:** Differential / golden. The Python source's top-level imports (`openai`, `..config`)
prevent direct import, so the 5 dataclasses + 2 consts + `to_dict`/`to_json` (which have NO external
deps) were extracted verbatim into a standalone module and run as the authoritative source-of-truth.
Rust outputs were emitted from a temporary `examples/` harness (since removed) and byte-diffed.

### Byte-exact differential results (Python `json.dumps(..., ensure_ascii=False, indent=2)` vs Rust `to_json()`)
| Scenario | Result |
|----------|--------|
| `CHINA_TIMEZONE_CONFIG` (`china_timezone_config()`) | **IDENTICAL** (`diff` clean) |
| `SimulationParameters` defaults (generated_at pinned) | **IDENTICAL** |
| Full: twitter+reddit PlatformConfig + 1 agent + Chinese `narrative_direction`/`hot_topics` | **IDENTICAL** |

All three `diff`s returned zero differences. This single byte-diff proves, simultaneously:
- **Every default is byte-exact** — `active_hours` = `(8..23)` → 15 elements ending at 22, NO 23
  (range exclusive, empirically confirmed `len 15 last 22 has23 False`); all 12 TimeSimulationConfig
  defaults; PlatformConfig 0.4/0.3/0.3/10/0.5; EventConfig all-empty; AgentActivityConfig
  0.5/1.0/2.0/5/60/0.0/"neutral"/1.0; required fields have no default (constructor signatures).
- **`to_dict` emits EXACTLY 13 keys in declaration order** (`obj.len()==13`, key-order assert passes)
  AND every nested struct (`time_config`, each `agent_configs[]`, `event_config`, platform configs)
  is recursively a dict with ITS fields in declaration order — proven by the full-scenario byte-diff,
  not just the top level. `None` → `null` (twitter/reddit when unset).
- **Float fidelity** — `0.05`, `0.4`, `0.7`, `1.5`, `1.0`, `2.0`, `0.0`, `0.3` all render identically
  to Python (serde_json float formatting matches `json.dumps`).
- **`to_json`** = 2-space indent + `ensure_ascii=False` — Chinese (`需求描述`/`舆论引导`/`人工智能`)
  appears RAW UTF-8; `grep -c '\u'` = 0 escapes.
- **`generated_at`** reuses `project::python_isoformat_local()` (confirmed `pub(crate)`,
  local-naive, µs-omitted-when-zero) — correct reuse; shape asserted (no tz suffix, T separator).

19 in-crate `simulation_config` unit tests pass; full suite **579 passed, 0 failed, 6 ignored**.

### preserve_order BLAST-RADIUS — CLEARED (it is a crate-wide PARITY GAIN, zero regression)
The porter added `serde_json` feature `preserve_order` (Cargo.toml:35), switching every `Value::Object`
/`Map`/`json!` from `BTreeMap` (alphabetical) to insertion-ordered. Decisive evidence it is safe:
- **MiroFish app code uses NO `sort_keys=True` anywhere** — `grep -rn sort_keys backend/app` is empty
  (only `.venv` third-party hits). Every `json.dumps` in MiroFish emits Python-dict INSERTION order.
- **Flask is 3.1.2** (`requirements.txt: flask>=3.0.0`, venv `flask-3.1.2`). Flask ≥2.3 defaults
  `JSON_SORT_KEYS=False` → route responses are insertion-ordered too (e.g. `/health` returns
  `{'status':'ok','service':...}` in that order).
- Therefore the OLD BTreeMap behavior was a LATENT divergence from Python on every multi-key
  `json!`/Map whose declared order ≠ alphabetical. `preserve_order` RETIRES that latent risk.

Empirically confirmed across previously-verified units (re-run, all green, output order now matches Python):
- **U-010 action_logger** (`json!` entries `round,timestamp,agent_id,…`): emitted order now `round,
  timestamp,agent_id,agent_name,action_type,action_args,result,success` — byte-matches Python's
  `json.dumps(entry)` order (which under old BTreeMap would have been the alphabetical
  `action_args,action_type,…` — a divergence preserve_order FIXES). 33 tests pass.
- **U-011 project.to_dict** (`json!` in declaration order): the recorded `[≠]` "JSON key order
  non-contractual" is now ACTUALLY-ORDERED to match Python — the `[≠]` could be retired as a parity
  GAIN. 23 tests pass.
- **U-002/U-003 `/health` + server JSON** (`server.rs:77 json!{status,service}`): now emits
  `status,service` = Flask 3.x insertion order (old BTreeMap → `service,status`, divergent). Gain.
- **U-012 task to_dict, U-014 ontology validate_and_process, S-005 ensure_ascii**: full suite green;
  no code builds an object relying on sorted keys, and no MiroFish output is sorted, so insertion
  order can only match-or-improve. No test anywhere asserts a SORTED key order (`grep` swept; the only
  `sorted` hits are list-element ordering by `created_at`, unrelated to Map type).

**Conclusion:** preserve_order introduced NO regression to any verified unit; it is strictly MORE
faithful (Python dicts/Flask 3.x are insertion-ordered). The U-011 `[≠]` for key-order is now a
parity gain and may be retired by the porter/cartographer if desired (not required for this verdict).

### `[≠]` challenge
No `- [≠]` rows were proposed for this sub-cycle — every data-model symbol was ported faithfully and
exercised, so all 56 are `- [x]` (none `- [≠]`). Nothing to challenge.

### Symbols verified: 56/56 → S-374..S-429 all flipped `- [x]` in symbol-map.md. S-430+ remain `- [ ]`.
### VERDICT: **PASS** (data model). U-019 UNIT remains `- [ ]` — sub-cycles (b)/(c)/(d) outstanding.
### Constraints honored: no source/Rust impl files edited; temporary `examples/_parity_*.rs` harness removed; only symbol-map S-374..S-429, the U-019 ledger note, and this verdict block written.

---

## 2026-06-17 — U-019 sub-cycle (b) + EntityNode DTO (U-016 rows) — VERDICT: PASS

**Gate:** rust-port-parity-verifier (opus). **Build precondition:** GREEN (630 passed, clippy --all-targets -D warnings clean). **Method:** differential — source Python re-implemented and run against the same inputs as the Rust; prompts diffed byte-for-byte; salvage logic adversarially probed for a Python-salvages-but-Rust-doesn't counter-example.

### Part 1 — EntityNode / FilteredEntities DTOs (S-198..S-213) → src/services/entity_reader.rs

| Check | Source (zep_entity_reader.py) | Rust (entity_reader.rs) | Result |
|---|---|---|---|
| get_entity_type: first label ∉ {Entity,Node}, else None; order-preserving | L46-51 | L165-172 (`label != "Entity" && label != "Node"`, in-order, `None` fallback) | PASS — case-sensitive exact match confirmed (test L316 `entity`/`node` lowercase NOT filtered) |
| EntityNode.to_dict: 7 keys in order uuid,name,labels,summary,attributes,related_edges,related_nodes | L35-44 | L120-142 (explicit insert order) | PASS — test asserts exact key order + len==7 (L327) |
| attributes is JSON object; related_edges/nodes default to [] | L29,31,33 default_factory=list | Map<String,Value>; Vec<Value> #[serde(default)]; `new()` empties | PASS (tests L379, L472) |
| FilteredEntities.to_dict: 4 keys entities,entity_types(list),total_count,filtered_count | L62-68 | L221-246 | PASS — key order + counts as numbers (tests L392,L458) |
| entity_types = list(set) unordered | L65 `list(self.entity_types)` | HashSet<String> → Vec, no order asserted; test sorts before compare (L453) | PASS — faithful: neither side guarantees order; Rust does NOT assert a spurious order |

### Part 2 — LLM foundation + time/event stages (S-430..S-449, excl S-439) → src/services/simulation_config.rs

| Check | Result + evidence |
|---|---|
| Class constants 50000/15/10000/8000/300/300/20 | PASS — source L214-223 vs Rust L1079-1097 (associated consts), exact. Test `class_constants_match_python` L2297. |
| Verbatim Chinese prompts (time L543-586, event L676-703) | PASS — diffed byte-for-byte via difflib: time 1207==1207 (zero diff after stripping `"""` delimiter), event 562==562 (zero diff). All system-prompt prefixes + the English PascalCase tail + format fragments (## 模拟需求, ## 实体信息, ## 原始文档内容, ...(文档已截断), ### x个), ... 还有 k 个, default-reasoning string) confirmed present on BOTH sides. |
| get_language_instruction incorporated | PASS — time L1487-1490 (`\n\n{lang}`), event L1679-1682 (`\n\n{lang}\nIMPORTANT:...`) match source L589 / L705-706 ordering exactly. i18n::get_language_instruction (i18n.rs L274) faithfully ports locale→llmInstruction→zh-fallback→`请使用中文回答。`. |
| CHAR-based truncation (not byte) | PASS — build_context (L1144,L1151-1153 `.chars()`), summarize_entities (L1211-1214 `.chars().count()`/`.take()`), time L1436, event L1646 all use `.chars()`. Test `summarize_entities_char_truncates_long_summary` uses 301 Chinese chars → 300+`...`. Critical for 3-byte CJK; verified no byte-indexing anywhere in the unit. |
| max_agents_allowed = max(1, int(n*0.9)) | PASS — Rust `(n as f64 * 0.9).max(1.0) as usize` (L1438). Order differs (Rust max-then-trunc vs Python trunc-then-max) but converges for all n≥0 (verified n=0,1,2). |
| get_default_time_config // floor div + max | PASS — `(n/15).max(1)`, `(n/5).max(5)` (L1513-1514) = Python floor `//` + max. Tests L2271 (n=30→2/6), L2284 (n=0→1/5). |
| _parse_time_config all clamp branches | PASS — min>n → max(1,n//10) (L1566); max>n → max(min+1,n//2) (L1573, uses corrected min, same as Python L624); min>=max → max(1,max//2) (L1578). Tests L2104,L2119. Defaults max(1,n//15)/max(5,n//5) (L1553-54). |
| _parse_event_config scheduled_events=[] | PASS — source L723 HARDCODES `scheduled_events=[]` (NOT a porter shortcut). Rust L1730 `scheduled_events: vec![]`. Test L2191 confirms LLM-supplied scheduled_events is ignored. initial_posts/hot_topics/narrative_direction extraction matches (L1709-1727). |
| _fix_truncated_json brace/bracket balance + trailing-quote | PASS — count `{`-`}`, `[`-`]` (char-based L1335-1338); if last char ∉ `",}]` append `"` (L1343-1350); append `]`×brackets THEN `}`×braces (L1352-1357, order matches source L496-497). Tests L1983-2017. |
| _try_fix_config_json two regexes | PASS — string-literal regex `"[^"\\]*(?:\\.[^"\\]*)*"` (L1390) + newline→space + `\s+`→` ` collapse (L1397-1399); then control-char `[\x00-\x1f\x7f-\x9f]` strip + collapse + reparse (L1409-1413). Rust `regex` crate syntax verified equivalent to Python `re`. Tests salvage newline-in-string (L2024), control chars (L2032), embedded object extraction (L2047), garbage→None (L2040). |

### _call_llm_with_retry (S-442) + finish_reason mapping — SCRUTINIZED (highest-risk)

- 3 attempts; temperature 0.7−attempt*0.1 → 0.7/0.6/0.5 via ChatOptions.temperature; max_tokens=None (MiroFish sets none) — PASS (L1262-1268).
- Uses `chat` (raw String), NOT `chat_json` — PASS (salvage path preserved; chat_json would bypass it).
- Backoff `sleep(2*(attempt+1))` — PASS. Python sleeps ONLY on `except` (LLM-call exception). Rust sleeps on BOTH the Err branch AND the all-parse-failed soft-error branch (L1293,L1305). **Minor divergence:** Python on parse-failure does NOT sleep before the next loop iteration; Rust does. This is a *timing-only* difference on the retry path (a 2/4s extra wait when an attempt returns parseable-failing content), not an output/behavior difference — accepted as non-contractual (both still perform exactly 3 attempts and return identical final value/error).

**finish_reason mapping (strategy a) — salvage equivalence, adversarially verified:**
Python: if finish_reason=='length' → `_fix_truncated_json` BEFORE first json.loads; else parse raw, on fail → `_try_fix_config_json`. teri's `chat` CANNOT surface finish_reason (DECISION-7 — OpenAI adapter discards it). Rust strategy (a): parse raw → on fail `fix_truncated_json`+parse → on fail `try_fix_config_json`.
- Ran a faithful Python reimpl of both salvage fns over 8 inputs incl. valid, newline-in-string, control-char, garbage, embedded-object, truncated brace+bracket, truncated string, multi-space-truncated. **All contractual cases (JSON objects) produce identical results on both sides.**
- Identified ONE non-contractual edge divergence: input `{"msg": "hello  world"` (brace-unbalanced, ends in `"`, internal double-space) — Rust step-2 `fix_truncated_json` parses cleanly and PRESERVES `"hello  world"`; Python (only if finish_reason≠'length') routes to `_try_fix_config_json` which collapses to `"hello world"`. This requires a provider returning unbalanced JSON while reporting a non-`length` finish_reason — brace-imbalance ⟺ truncation ⟺ finish_reason=='length', in which case Python ALSO fix-first and matches Rust. Under the operative contract (`response_format=json_object`, content always an object, truncation⟹length) the two are equivalent; Rust loses NO salvage capability (it salvages a strict superset and, in this edge, mangles the string LESS). The dropped finish_reason signal is genuinely inexpressible in teri's substrate.
  - **[≠]-class acceptance (documented, NOT a downgrade):** the residual whitespace-preserve-vs-collapse difference is non-contractual + rooted in an inexpressible substrate signal. No symbol is marked `- [≠]` (every in-scope symbol is independently `- [x]`); this is recorded here as the finish_reason rationale.
  - **Doc-accuracy nit (non-blocking):** the module docstring (L1042) claims "This loses NO salvage behaviour" — precise statement is "no salvage *capability* lost; one non-contractual whitespace-collapse difference exists in the (unreachable-under-contract) unbalanced+non-length case." Recommend the porter tighten the comment; does NOT affect the PASS.

### Minor non-blocking note
- `parse_time_config` extracts ints via `as_i64`: a JSON float (e.g. `5.0`) for an int field falls to the Python default rather than truncating the float. Non-contractual (prompt + json_object schema specify `(int)`); accepted.

### Symbols verified: 35/35 in scope → S-198..S-213 (16) + S-430..S-449 excl S-439 (19) all flipped `- [x]` in symbol-map.md.
### Out of scope, left `- [ ]`: S-214..S-219 (ZepEntityReader machinery), S-439 (generate_config orchestration), S-450/S-451/S-452 (agent-config stages, sub-cycles c/d). Confirmed still `- [ ]`.
### VERDICT: **PASS**. U-016 UNIT remains `- [ ]` (EntityNode DTO done; ZepEntityReader machinery outstanding). U-019 UNIT remains `- [ ]` (sub-cycles a+b done; c/d outstanding).
### Constraints honored: no source/Rust impl files edited; differential harness was external (Python), removed. Only symbol-map flips + this verdict block + the ledger annotations written.

---

## 2026-06-17 — U-019 sub-cycle (c): agent-config generation (S-450, S-451, S-452) — opus parity-verifier

### Unit: U-019 sub-cycle (c) | Source: simulation_config_generator.py L728-989 | Rust: src/services/simulation_config.rs (impl<L: LlmClient> SimulationConfigGenerator<L>)
### Method: differential vs standalone Python (alias-resolution model + byte-diff of rendered prompt/system_prompt against Python golden via temporary in-test capture mock; temp tests removed after verification — src tree contains only the porter's code).

### VERDICT: **FAIL** — 2/3 symbols PASS (S-451, S-452); S-450 has a confirmed runtime behavioral divergence (downgrade). U-019 sub-cycle (c) does NOT complete; U-019 stays `- [ ]`.

---

### S-451 `generate_agent_configs_batch` — **PASS** ✅
- **Prompt byte-IDENTICAL.** Rust prompt (`simulation_config.rs:1931-1933`) == Python prompt (`L833-867`): **1657 bytes, zero diff**, including the embedded `serde_json::to_string_pretty(entity_list)` == `json.dumps(entity_list, ensure_ascii=False, indent=2)` — UTF-8 Chinese preserved (no \uXXXX escaping), 2-space pretty, key insertion order (agent_id, entity_name, entity_type, summary) preserved (serde_json `preserve_order` feature confirmed in Cargo.toml:35), empty-summary `""` case correct.
- **system_prompt byte-IDENTICAL.** Rust (`1935-1939`) == Python (`L869-870`): **400 bytes, zero diff**, including `get_language_instruction()` placement (zh default `请使用中文回答。`) + the English stance IMPORTANT note. (get_language_instruction is the same ported fn used identically on both sides.)
- **entity_list build** (`1908-1925` vs `L823-831`): agent_id=start_idx+i, entity_name, entity_type=`get_entity_type()||"Unknown"`, summary char-truncated to AGENT_SUMMARY_LENGTH (`.chars().take()`), "" when empty. ✓
- **try/except → rule fallback** (`1942-1960` vs `L872-877`): any LLM/parse error ⇒ `llm_configs = {}` (HashMap::new), no error propagated; loop uses rule per entity. ✓
- **cfg precedence** (`1974-1998` vs `L883-902`): `cfg = llm_configs.get(agent_id)` if Some+non-null+non-empty-object (mirrors Python `if not cfg` treating `{}` as falsy) else `generate_agent_config_by_rule(entity)`; then each field = `cfg.get(field).unwrap_or(BATCH_DEFAULT)` — same `.get(field,default)` extraction feeds BOTH the LLM-cfg and rule-cfg paths. ✓
- **BATCH defaults DIFFER from dataclass — CONFIRMED CORRECT** (`2007-2015` vs `L894-902`): activity_level 0.5, **posts_per_hour 0.5** (NOT dataclass 1.0), **comments_per_hour 1.0** (NOT 2.0), **active_hours `(9..23)` = [9..=22] = 14 elems** (NOT range(8,23)=15), delay 5/60, sentiment 0.0, stance "neutral", influence 1.0. No regression to dataclass defaults. ✓
- Existing tests (happy-path, llm-failure-fallback, missing-agent_id-fallback, batch-defaults-differ, start_idx, entity-fields, summary-truncation) all green.

### S-452 `generate_agent_config_by_rule` — **PASS** ✅
All 6 branches diffed value-by-value vs `L912-989`; every numeric + active_hours list EXACT:
- university/governmentagency/ngo (`2048-2062`): 0.2/0.1/0.05/[9..=17]/60/240/0.0/neutral/3.0 ✓ (`range(9,18)` expanded to 9 elems)
- mediaoutlet (`2063-2077`): 0.5/0.8/0.3/[7..=23] 17 elems/5/30/0.0/observer/2.5 ✓ (`range(7,24)`)
- professor/expert/official (`2078-2091`): 0.4/0.3/0.5/[8..=21] 14 elems/15/90/0.0/neutral/2.0 ✓ (`range(8,22)`)
- student (`2093-2106`): 0.8/0.6/1.5/[8,9,10,11,12,13,18,19,20,21,22,23]/1/15/0.0/neutral/0.8 ✓
- alumni (`2108-2121`): 0.6/0.4/0.8/[12,13,19,20,21,22,23]/5/30/0.0/neutral/1.0 ✓
- else (`2123-2137`): 0.7/0.5/1.2/[9,10,11,12,13,18,19,20,21,22,23]/2/20/0.0/neutral/1.0 ✓
- Branch match on `get_entity_type()||"Unknown".lower()` (`2042-2047` vs `L910`). ✓

### S-450 `assign_initial_post_agents` — **FAIL** ❌ (behavioral downgrade in alias resolution)
Most of S-450 is faithful: empty-posts unchanged ✓; agents_by_type lower-keyed insertion-order ✓; type_aliases table EXACT + ordered (Vec-of-pairs, `1784-1793` vs `L750-759`) ✓; round-robin `idx = used.get(key,0) % len; used[key]=idx+1` ✓ same counter keying direct & alias ✓; influence fallback STABLE-sort tie-break replicated with strict `>` reduce (first-in-original wins) ✓; empty agent_configs → 0 ✓; output `poster_type` = ORIGINAL cased value, default "Unknown" ✓.

**THE DEFECT — alias inner-loop break is unconditional (`simulation_config.rs:1843`); Python's outer break is conditional on a match (`L789-790`).**
- Python `L780-790`: when a poster_type matches an alias *group* whose members are ALL absent from agents_by_type, `matched_agent_id` stays None and the `if matched_agent_id is not None: break` does NOT fire → the outer loop **continues to the next matching group**.
- Rust `1828-1844`: after the inner loop, it `break;` **unconditionally**, exiting the outer loop with matched=None → falls through to the influence-max fallback. The code comment (`1841-1843`) misreads Python ("Python breaks the outer loop here too") — Python breaks only on success.
- **Reachable & observable.** poster_type is LLM-emitted from the ontology entity vocabulary (Student/Person/Alumni/…). Enumeration found **3 divergent cases** (single-type agent pool):
  - poster_type=`person`, only `alumni` agents → Python alias-matches the alumni agent; Rust → influence fallback.
  - poster_type=`alumni`, only `student` agents → Python matches; Rust → fallback.
  - poster_type=`student`, only `alumni` agents → Python matches; Rust → fallback.
- **Proven at runtime** with a temporary test (poster=`person`; agents = [id=100 alumni infl 1.0, id=200 official infl 9.0]): Rust returned `poster_agent_id=200` (influence fallback), Python golden = `100` (alumni alias match). The divergence changes an observable output field. NOT non-contractual, NOT inexpressible, NOT a superset → a downgrade.

### FIX (route back to porter — precise):
In `assign_initial_post_agents` (`simulation_config.rs:1828-1845`), make the OUTER-loop break conditional on a match, mirroring Python's `if matched_agent_id is not None: break`. Replace the unconditional `break;` at line 1843 with: break the outer loop only when `matched_agent_id.is_some()` (i.e. after the inner `break 'outer`, which already handles the success case — so simply REMOVE the trailing unconditional `break;` and let the outer `for` continue to the next group when no member was found). Keep the inner `break 'outer` for the success path. Then add a regression test for poster=`person`/only-`alumni` (expect the alumni agent's id, not the influence fallback).

### Symbol-map flips: S-451 → `- [x]`, S-452 → `- [x]` (independently PASS). S-450 left `- [~]`.
### U-019 row: sub-cycle (c) is 2/3 — NOT complete. U-019 stays `- [ ]`. Sub-cycle (d) (generate_config orchestration S-439 + save) still remains regardless.
### Constraints: no source/Rust impl edited; temp capture+probe tests added then REMOVED (src tree = porter's code only, verified via git diff); only symbol-map flips + this verdict + ledger annotation written.

## 2026-06-17 · U-019 sub-cycle (c) RE-VERIFY · S-450 `assign_initial_post_agents` — FAIL→fix→**PASS** ✅
Re-verification of the symbol FAILed on 2026-06-14 (alias inner-loop unconditional break → influence-max fallthrough). Porter applied the prescribed fix. Re-traced the alias scan against Python `_assign_initial_post_agents` (`simulation_config_generator.py:728-811`) — did NOT pass on faith.

### Fix confirmed (item 1 — control flow now mirrors Python L780-790)
- The unconditional `break;` after the inner alias loop is GONE. `simulation_config.rs:1828-1848`: `'outer: for (alias_key, aliases) in &type_aliases` → guard `aliases.contains(poster_type_lower) || *alias_key == poster_type_lower` → inner `for alias in aliases { if let Some(agents) = agents_by_type.get(alias) { ...match...; break 'outer; } }`. The `break 'outer` lives INSIDE the `if let Some(agents)` block, so it fires ONLY on a successful match. On no-match, the inner loop exhausts and the OUTER `for` continues to the next group — exactly Python's `if matched_agent_id is not None: break` (L789-790), which breaks only on success. No unconditional break remains that would stop the scan early. ✓

### Original divergent case now matches (item 2)
- poster=`person`, agents=[id100 alumni infl 1.0, id200 official infl 9.0]. Alias-group insertion order: official, university, mediaoutlet, **student** (idx3, members [student,person] — both absent → continue), professor, **alumni** (idx5, members [alumni,person] — alumni present → match id100). Resolves to **100**, NOT influence-max 200. This is the exact case that returned 200 last cycle (Python golden 100). New regression test `assign_initial_post_agents_continues_past_empty_alias_group` (`simulation_config.rs:2828-2846`) asserts `poster_agent_id == 100` and documents that influence-max would give 200 — it genuinely distinguishes the correct alias-scan from the bug. ✓

### No new regression (item 3)
- Direct-match round-robin (`idx = used.get(key,0) % len; used[key]=idx+1`), influence-max fallback (strict `>` reduce → first-in-original on ties), empty agent_configs→0, original-cased poster_type default "Unknown", empty initial_posts→unchanged — all still present and covered by 11 prior tests + the new regression test. `cargo test --lib assign_initial_post_agents` = **12 passed, 0 failed**.

### S-451 / S-452 undisturbed (item 4)
- `cargo test --lib generate_agent_config` = 17 passed; full `simulation_config` module = 82 passed; full lib suite = **648 passed, 0 failed**. Edit was local to the alias branch; no S-451/S-452 path touched.

### Result — **PASS**
Symbol-map: S-450 flipped `- [~]` → `- [x]`. U-019 sub-cycles a+b+c are now ALL parity-verified; only sub-cycle (d) (`generate_config` S-439 + save) remains. U-019 stays `- [ ]` (NOT marked done — sub-cycle d outstanding). No source/Rust impl edited by the verifier (porter's regression test is the only test added; it ships in the crate). Verified inside worktree `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch port/mirofish).

---

## 2026-06-17 — U-019 sub-cycle (d): `generate_config` (S-439) — FINAL symbol of U-019

**Verifier:** rust-port-parity-verifier (opus). **Worktree:** `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch `port/mirofish`).
**Source:** `MiroFish/backend/app/services/simulation_config_generator.py:243-379`.
**Rust:** `teri::services::simulation_config::SimulationConfigGenerator::generate_config` (`src/services/simulation_config.rs:2058-2237`).
**Method:** differential re-trace of source vs Rust (file:line both sides) + read of the 14 added `generate_config_*` tests + full run.

### Checks (all PASS)

1. **Step math (total_steps = 3 + num_batches).** Python `num_batches = math.ceil(len/15)` (py:275); Rust `entities.len().div_ceil(15)` (rs:2086). Cross-checked 0/1/14/15/16/17/30/31/45/46 — `div_ceil` ≡ `math.ceil` on all (0→3, 15→4, 16→5, 17→5, 30→5, 31→6). **0-entities edge:** `math.ceil(0/15)=0` → total_steps=3, agent-batch loop runs 0×, platform reported at step 3 — identical Rust; tested by `generate_config_zero_entities_total_steps_3` asserting step seq `[1,2,3]`.

2. **Step numbering + progress_callback.** py:279-284 `report_progress(step,msg)` sets current_step, calls `progress_callback(step,total_steps,message)` if present; rs:2096-2105 `report_progress!` macro does the same. Steps: 1=time (py:296/rs:2113), 2=event (py:303/rs:2125), 3+batch_idx=agent (py:315-318/rs:2143-2153), total_steps=platform (py:337/rs:2184). Idiom `Option<&mut dyn FnMut(i64,i64,&str)>` accepted. `generate_config_total_steps_formula` (n=17) and `_progress_callback_invoked_correct_sequence` (n=16) assert seq `[1,2,3,4,5]`, all callbacks see total=5, and msg1/msg2/msg_last match time/event/platform.

3. **i18n keys + placeholders EXACT.** All present in `src/i18n/locales/{en,zh}.json`: progress.generatingTimeConfig, timeConfigLabel, generatingEventConfig, eventConfigLabel, generatingAgentConfig (`{start}-{end}/{total}`), agentConfigResult (`{count}`), postAssignResult (`{count}`), generatingPlatformConfig, common.success. `generatingAgentConfig` args: start=start_idx+1, end=end_idx, total=entities.len() (py:317 / rs:2145-2152) — exact. `t` used for no-arg keys, `t_args` for placeholder keys — arity-correct. `t_args` (`src/i18n/mod.rs:209`) `{name}`→`str(v)` replacement ≡ Python `t(key,**kwargs)` (`utils/locale.py:35-63`).

4. **reasoning-or-success fallback.** Python `result.get('reasoning', t('common.success'))` time (py:300) + event (py:306). Rust `.get("reasoning").and_then(Value::as_str).map(to_string).unwrap_or_else(|| t("common.success"))` time (rs:2117-2121) + event (rs:2129-2133) — uses the JSON string only when present AND a string, else fallback. Present-path tested via `make_multi_stage_gen` (responses carry `"reasoning":"test-time-reasoning"`); fallback-path tested by `_reasoning_uses_success_fallback_when_no_reasoning_key`.

5. **batch loop.** start_idx=batch_idx*15 (py:311/rs:2139); end_idx=min(start+15,len) (py:312/rs:2140); batch=entities[start..end] (py:313/rs:2141); `generate_agent_configs_batch(context,batch,start_idx,sim_req).await` with start_idx as i64 (py:320-325/rs:2155-2162); extend all_agent_configs (py:326/rs:2163). `_agent_configs_length_equals_entities` (n=15) confirms all N flow through.

6. **assigned_count.** Python `len([p ... if p.get("poster_agent_id") is not None])` (py:333). Rust `.filter(|p| !p.get("poster_agent_id").map(Value::is_null).unwrap_or(true)).count()` (rs:2173-2177). Equivalence: key absent → None/`unwrap_or(true)`→excluded; key present + JSON null → `is_null`→excluded; present non-null → included. Exact.

7. **Platform config literals.** twitter recency=0.4/popularity=0.3/relevance=0.3/viral=10/echo=0.5 (py:342-349/rs:2187-2194) — equals struct defaults but written as explicit literals. reddit recency=0.3/popularity=0.4/relevance=0.3/viral=15/echo=0.6 (py:352-359/rs:2202-2209) — DIFFER from struct defaults (0.4/0.3/.../10/0.5), written as explicit literals (NOT `..Default`). enable_twitter/enable_reddit gating both default-true; tested: twitter_present_correct, reddit_non_default_literals, twitter_false, reddit_false, both_disabled.

8. **SimulationParameters construction (py:362-375 / rs:2215-2229).** All fields field-for-field. llm_model=self.model_name (rs:2225), llm_base_url=self.base_url (rs:2226) — tested `_llm_model_and_base_url_in_params`. generation_reasoning=reasoning_parts.join(" | ") (rs:2228) — tested `_generation_reasoning_joins_with_pipe` (exactly 3 separators / 4 parts, all labels present). generated_at=python_isoformat_local() (rs:2227): Python field default_factory `datetime.now().isoformat()` (dataclass L173) is NOT overridden in generate_config → calling the same factory is the faithful equivalent (a struct literal cannot omit a non-Default field); tested `_generated_at_is_isoformat` (T-separator, local-naive, no Z). simulation_id/project_id/graph_id/simulation_requirement passthrough tested.

9. **No regression.** Edit only ADDED `generate_config` + 14 `generate_config_*` tests. `cargo test --lib`: 662 passed. `cargo test --lib services::simulation_config`: 96 passed. `cargo clippy --all-targets -- -D warnings`: clean. Stage methods (S-430..S-452) undisturbed.

### `[≠]` challenge
None. U-019 had ZERO `[≠]` rows; S-439 was the sole `- [~]`. No disguised feature-skip to challenge.

### Result — **PASS**
S-439 flipped `- [~]` → `- [x]` in symbol-map.md. **All U-019 symbols S-374..S-452 now `- [x]` (73 symbols, zero `[≠]`)** — confirmed via tally (72 `[x]` + S-439 = 73, the only prior open row was S-439). **U-019 UNIT flipped `- [ ]` → `- [x]` in parity-ledger.md** (rollup rule satisfied). Verifier edited no source/Rust impl. Orchestrator may mark the unit done and commit.

---

## 2026-06-17 — U-016 completion (S-214..S-222 `KnowledgeGraphEntityReader`) — **FAIL**

**Verifier:** rust-port-parity-verifier · **Unit:** U-016 (reader machinery; DTOs S-198..S-213 already `[x]`)
**Source:** `MiroFish/backend/app/services/zep_entity_reader.py:71-435` · **Rust:** `src/services/entity_reader.rs:514-928` + `src/graph/mod.rs:1056 get_entity_by_id`
**Contract:** DECISION-9 (target-architecture.md:507-619). Baseline: `cargo test --lib` 701 passed (green). FAIL is a parity divergence, NOT a build break.

### Differential method
Read both implementations symbol-by-symbol; for the equivalence-critical enrich path I ran teri directly via two throwaway probe tests (since removed, tree clean) over a self-loop + bidirectional graph, capturing `related_edges`/`get_node_edges` counts and comparing to MiroFish's documented edge-iteration semantics.

### Self-loop divergence — the FAIL (the exact case the unit spec told me to check)

DECISION-9 Q5 asserts `get_neighbor_relations` (O(degree)) is "provably equivalent" to MiroFish's O(n·e) `all_edges` scan — "same edge set, same direction assignment". **This is FALSE for a self-loop**, and self-loops are REACHABLE in teri (see reachability below).

**MiroFish** uses an **exclusive `if/elif`**: `if edge.source==node: outgoing  elif edge.target==node: incoming` (`filter_defined_entities` L288-303; `get_entity_with_context` L370-385 is `if source: outgoing else: incoming`). A self-loop edge `X→X` matches `if source==X` → emitted **ONCE** (outgoing); the elif/else never fires. `get_node_edges`→Zep `get_entity_edges` returns the incident edge **once**.

**teri** `get_neighbor_relations` (`graph/mod.rs:320-350`) does two non-exclusive passes: `edges_directed(idx, Outgoing)` THEN `edges_directed(idx, Incoming)`. petgraph 0.6 returns a self-loop edge in BOTH queries, so the self-loop is emitted **TWICE** (one outgoing, one incoming).

**Empirical evidence** (graph: `Acme -[RelatedTo]-> Acme` self-loop), run through the actual reader:

| Method (input = self-loop on `Acme`) | teri actual | MiroFish (source) | match? |
|---|---|---|---|
| `get_node_edges("…aa")` len | **2** | 1 | **≠** |
| `filter_defined_entities(None,true)` → Acme.related_edges len | **2** (dir=outgoing, dir=incoming) | 1 (outgoing only) | **≠** |
| `get_entity_with_context("…aa")` → related_edges len | **2** | 1 | **≠** |
| related_nodes len | 1 (deduped) | 1 | = (this part matches) |

(Also confirmed at the substrate: a probe of `get_neighbor_relations` on `A` with self-loop A→A + bidir A→B/B→A returned **4** entries — outgoing→B, outgoing→A, incoming→B, incoming→A — i.e. the self-loop A→A counted twice. The bidirectional pair A↔B is handled CORRECTLY by both, 2 distinct edges → 2 related_edges, B deduped to 1 related_node — that case is a genuine MATCH.)

**Reachability (why this is a real bug, not an unreachable branch):** `parse_relations_json` (`graph/mod.rs:977-1031`) and `add_relation` (`:286`) have **NO self-loop guard**. If the relation-extraction LLM emits `{"from":"Acme","to":"Acme",...}` (a common LLM output — an entity related to itself), `from_idx == to_idx` and a self-loop is created. So this divergence is on the real extraction-pipeline output surface, unlike the `{Entity,Node}` skip (Q3) which is genuinely unreachable given typed entities.

**Classification:** NOT a `[≠]` (it is fully expressible — teri can match MiroFish) and NOT owner-approved. It is an unflagged observable divergence in `related_edges`/`get_node_edges` output count → **FAIL** per the no-downgrade gate.

### Required fix (route back to porter)
Make teri's enrichment match MiroFish's exclusive-direction semantics for self-loops. Cheapest correct fix: in `get_neighbor_relations` (`graph/mod.rs:320-350`), make the Incoming pass skip edges whose `source == idx` (i.e. skip self-loops in the incoming pass), so a self-loop is emitted once as outgoing — exactly MiroFish's `if/elif`. (Alternatively dedup at the reader boundary in `enrich_entity_node` / `get_node_edges`, but fixing the shared accessor is cleaner and benefits every consumer.) Add a self-loop test (`X -[k]-> X`) asserting `related_edges.len()==1` & `get_node_edges(X).len()==1` with `direction=="outgoing"`. NOTE: `get_neighbor_relations` is shared — re-verify U-018/`_build_entity_context` consumers after the change (no behavior loss expected; they only gain self-loop correctness).

### Per-symbol verdict
- `- [≠]` S-215 `__init__` — CONFIRMED legitimate. api_key + `ZEP_API_KEY` validation + `Zep(api_key)` client = network-client auth; an in-process petgraph read has no auth/client/remote handle. Genuinely inexpressible substrate, no observable output. Constructor itself is ported (`new(&KnowledgeGraph)`).
- `- [≠]` S-216 `_call_with_retry` — CONFIRMED legitimate AS RETRY. Retry/backoff exists only to survive transient Zep network failures; an in-process `index_by_id`/petgraph lookup has no I/O and cannot transiently fail (absence → `None`/`[]`, not retried). Non-contractual, no observable difference. **AND the error-fallback it wrapped IS preserved**: `get_entity_with_context` returns `None` on bad/missing uuid (`entity_reader.rs:766,769`), `get_node_edges` returns `[]` on unparseable/missing uuid (`:617-619,636`) — except→None / except→[] contracts PORTED. The retry drop did NOT drop the fallback. ✓
- `- [x]`-grade (clean, but blocked by unit FAIL) S-214 struct, S-217 `get_all_nodes` (5-key dict, summary="" / attributes={} [≠] legit, labels=[kind.to_string()], graph_id [≠]), S-218 `get_all_edges` (6-key dict, name=kind Display, uuid/fact/attributes empties [≠] legit) — these do NOT touch the enrich path and are individually correct. Left `- [~]` because the UNIT cannot PASS (rollup rule) until S-219/S-220/S-221 are fixed.
- `- [~]` (FAIL) **S-219 `get_node_edges`** — self-loop double-count (2 vs 1). except→[] contract correct; shape correct; divergence on self-loop edge count.
- `- [~]` (FAIL) **S-220 `filter_defined_entities`** — self-loop double-count in `related_edges`. Everything ELSE verified CORRECT: total_count=entity_count, filtered_count, entity_types HashSet membership, {Entity,Node}-skip ported verbatim (always-pass in teri — correct), defined_entity_types ∩ matching + first-match entity_type, Display-string match incl. Custom(name)→name, related_nodes dedup-by-uuid set, direction labels, bidirectional pair MATCHES. Only the self-loop case diverges.
- `- [~]` (FAIL) **S-221 `get_entity_with_context`** — self-loop double-count in related_edges. except→None contract correct (bad + missing uuid → None, verified). Otherwise enrich shape matches.
- `- [~]` (FAIL, inherited) **S-222 `get_entities_by_type`** — 1:1 delegation to `filter_defined_entities` is correct; fails only because its delegate (S-220) carries the self-loop divergence.

### `[≠]` challenge (every field, against the no-downgrade rule)
All field-level `[≠]`s CONFIRMED legitimate (genuinely inexpressible Zep-server/SDK artifacts, each with a verified consumer-side graceful fallback — none is a portable-feature skip):
- `summary=""` (node + related_node) — Zep auto-generates per-entity summaries server-side at ingestion; teri `Entity{id,name,kind}` carries no summary and there is NO portable source to derive one faithfully (deriving from facts would fabricate; `fact` is itself empty). Consumer U-018 `_generate_profile_with_llm` falls back to `"A {type} named {name}"` (L261); `_build_entity_context` omits the summary line. Inexpressible. ✓
- `attributes={}` (node + edge) — teri `Entity`/`Relation` have no KV attribute bag; Zep attributes are server-extracted. No portable source; consumer guards `if entity.attributes:` (L426) → skips block. Inexpressible. ✓
- edge `fact=""` — Zep `fact` is an LLM-generated NL sentence produced during Zep ingestion; teri stores only `(kind,weight)`. Consumer reads `fact` first then falls back to `edge_name`+`direction` template (L439-450) — and teri DOES emit `edge_name`(=kind Display)+`direction`, so the observable "relationships" output is still produced via the same fallback MiroFish itself uses when fact is empty. Deriving `"{from} {kind} {to}"` would diverge from MiroFish's empty-fact→template path. Correctly kept `""`. ✓
- edge `uuid=""` — read by NO consumer of `get_all_edges`/`get_node_edges` in MiroFish (dict-shape filler; Zep's own value is usually `""` for these reads); teri `Relation` has no uuid. Non-contractual; synthesizing one = fabricated observable with no reader. ✓
- `graph_id` param dropped — Zep server-graph selector; the bound `&KnowledgeGraph` is the teri selector. Inexpressible. ✓

So: zero disguised feature-skips among the `[≠]`s. The FAIL is **solely** the self-loop edge-count divergence in the enrich/edge-list path.

### Additive accessor (Q6) — VERIFIED clean
`KnowledgeGraph::get_entity_by_id(&self, id: Uuid) -> Option<&Entity>` (`graph/mod.rs:1056-1060`) reads the existing private `index_by_id` map (O(1)); adds a new pub fn only. NO change to `Entity`/`Relation`/`EntityKind`/`RelationKind`/`KnowledgeGraph` existing signatures or fields. Zero blast radius on verified types; 701 lib tests green. ✓

### Result — **FAIL** (unit U-016 stays `- [~]`; S-219/S-220/S-221/S-222 stay `- [~]`)
Single defect: self-loop edges are double-counted (outgoing+incoming) in `get_neighbor_relations`, diverging from MiroFish's exclusive `if/elif` (one entry, outgoing). Reachable via the guard-less extraction pipeline. Fix the accessor (or dedup at the reader boundary), add a self-loop test, re-verify. Everything else in U-016 — DTO mapping, counts, set membership, dedup-by-uuid, direction labels, bidirectional pairs, the except→None/[] error contracts, and every `[≠]` field — is verified correct.

---

## 2026-06-17 — U-016 RE-VERIFY (self-loop FAIL→fix→PASS) + U-018 no-regression — **PASS** (opus parity gate)

### Context
Prior cycle FAILED U-016: a self-loop `X→X` was double-counted in `get_neighbor_relations`
(petgraph returns the edge in BOTH the Outgoing and Incoming directed passes), so all three
reader paths emitted it twice (teri 2 vs MiroFish 1). MiroFish's exclusive if/elif
(`zep_entity_reader.py:288-303`) classifies a single edge once — a self-loop hits the
`if source==node` (outgoing) branch and never the `elif target==node` branch.

### The fix (re-verified)
`src/graph/mod.rs` `get_neighbor_relations`, Incoming pass (graph/mod.rs:348):
`if edge.source() == *idx { continue; }` — skips a self-loop in the incoming pass so it is
emitted ONCE, as outgoing. Regression test `self_loop_edge_emitted_once_as_outgoing`
(`src/services/entity_reader.rs:990`) asserts across all three reader paths.

### 1. Fix is correct & complete
- **get_node_edges** (entity_reader.rs:623): self-loop → `edges.len()==1` (was 2). Source→MiroFish
  `if source==node` outgoing-only. MATCH.
- **filter_defined_entities** enrich (enrich_entity_node, entity_reader.rs:870): self-loop →
  `related_edges.len()==1`, `direction=="outgoing"`, `target_node_uuid==self`. MATCH MiroFish
  L288-303 (single outgoing entry, target=self).
- **get_entity_with_context** (entity_reader.rs:764→785 enrich): self-loop → `related_edges.len()==1`,
  `direction=="outgoing"`. MATCH.
- All asserted by `self_loop_edge_emitted_once_as_outgoing` — PASS.

### Guard fires ONLY for self-loops (independently proven)
Temporary 4-case trace test (built, run PASS, then reverted — graph/mod.rs back to +0 vs the
committed fix) over node X with: self-loop X→X, normal outgoing X→Out, normal incoming In→X,
bidirectional X↔Bi. Result: `get_neighbor_relations(X)` returned exactly 5 entries:
- self-loop: 1 outgoing, 0 incoming  (guard FIRED: source==X==idx in incoming pass)
- X→Out: 1 outgoing, 0 incoming      (unchanged)
- In→X: 0 outgoing, 1 incoming       (guard did NOT fire: source=In≠X; still emitted incoming)
- X↔Bi: 1 outgoing AND 1 incoming    (guard did NOT fire on Bi→X: source=Bi≠X; both preserved)
Proves a normal incoming edge A→X (A≠X) is NOT skipped and bidirectional pairs are intact.

### 2. NO U-018 regression (get_neighbor_relations is SHARED) — CONFIRMED
- **Call sites** (grep): `src/services/entity_reader.rs:623,870` (U-016) and
  `src/agent/mod.rs:1253` (U-018 `PersonaGenerator::build_entity_context`, Part 2 related-edges).
- **U-018 parity contract requires the self-loop counted ONCE, not twice.** MiroFish U-018
  source `_build_entity_context` (`oasis_profile_generator.py:434-453`) iterates
  `entity.related_edges` — and that list is itself produced by the SAME exclusive if/elif in
  `filter_defined_entities` (`zep_entity_reader.py:288-303`), which emits a self-loop ONCE
  (outgoing). So MiroFish U-018 sees the self-loop once. teri's old double-count was therefore
  a regression AGAINST U-018's contract too; the fix brings U-018 to parity, and CANNOT regress
  it (the fix only removes a spurious duplicate that MiroFish never produced).
- teri U-018 `build_entity_context` consumes `get_neighbor_relations` directly (agent/mod.rs:1253)
  and renders each entry via `_relation_line` (outgoing: `entity --[kind]--> (neighbor)`;
  incoming: `(neighbor) --[kind]--> entity`). Post-fix a self-loop yields one outgoing line —
  matching MiroFish's single outgoing `related_edges` entry rendered at L447-448.
- **Tests**: all 104 `agent::` tests pass, incl. `test_generate_social_part2_outgoing_relation_in_prompt`
  and `test_generate_social_part2_incoming_relation_in_prompt` (outgoing source=idx and incoming
  source≠idx cases — both unchanged by the guard, confirming no U-018 directional regression).

### 3. Rest of U-016 unchanged
Edit was localized to `get_neighbor_relations` (+1 guard) + one regression test. The earlier-verified
parts hold: `[≠]` fields (summary="", attributes={}, edge fact="", edge uuid="", edge attributes={}),
the Entity/Node-skip filter (always-pass, ported verbatim), counts/entity_types/dedup-by-uuid,
direction labels, bidirectional pairs, and the except→None/except→[] error contracts. The two `[≠]`
rows S-215 (Zep api_key/client construction) and S-216 (`_call_with_retry`) survive the `[≠]`
challenge: genuinely inexpressible (in-process petgraph read has no auth client) / non-contractual
(no transient I/O to retry) — NOT a disguised feature-skip (no distinct observable output dropped;
the except→None/[] fallback CONTRACTS are ported).

### Baseline
713 passed / 0 failed / 6 ignored (full `cargo test`); reader_tests 40, agent:: 104, graph:: 46 —
all green. `cargo clippy --all-targets -- -D warnings`: No issues found.

### Result — **PASS** (unit U-016 → `- [x]`)
S-214, S-217, S-218, S-219, S-220, S-221, S-222 → `- [x]`; S-215, S-216 remain challenge-surviving
`- [≠]`. All S-198..S-222 are now `[x]`/`[≠]` → rollup rule satisfied. U-018 verified NOT regressed
(brought closer to parity). No downgrade.

---

## 2026-06-17 — U-018 OASIS profile EXPORT layer (S-367,S-369,S-370,S-371,S-372,S-373) — opus parity gate

**Verifier:** rust-port-parity-verifier (opus) · **Branch:** port/mirofish · worktree `.worktrees/mirofish-port/teri`
**Unit:** U-018 export layer (the wrongly-`[≠]`'d symbols, ported per DECISION-10). Build GREEN, clippy clean, 36 export-layer tests pass.
**Source:** `MiroFish/backend/app/services/oasis_profile_generator.py` L851-1205 · **Rust:** `src/services/oasis_profile_export.rs`

### VERDICT: **PASS** — 6/6 symbols verified; S-368 stays `[≠]` (challenge-survived: non-contractual stdout). One real-but-non-contractual byte divergence found (CSV terminator) — proven unobservable through the contractual read path; doc-comment correction required (recorded below, non-blocking).

### Per-symbol differential (file:line BOTH sides)

**S-371 `normalize_gender` — PASS.** rs:65-84 vs py:1121-1144. Exact map verified: None/empty/whitespace→other; 男→male, 女→female, 机构→other, 其他→other; male/female/other passthrough; default→other. Python `gender.lower().strip()` mirrored (rs `g.trim().to_lowercase()`); Chinese chars unaffected by to_lowercase → match. 11 tests pass (incl. Male/MALE case-fold, whitespace, garbage).

**S-372 `save_reddit_json` — PASS (the load-bearing forced-default contract).** rs:112-196 vs py:1146-1193. FORCED OASIS defaults all confirmed: age = `age.filter(>0).unwrap_or(30)` (UNCONDITIONAL) vs py `age if age else 30`; gender = `normalize_gender(...)` ALWAYS present (NOT conditional); mbti = `or "ISTJ"`; country = `or "中国"`; karma = `or 1000`; bio = `chars().take(150)` (CHAR truncation, not bytes) or name-fallback vs py `bio[:150]`; persona = `or "{name} is a participant in social discussions."`; user_id = profile.user_id (u64, always present). Optional profession/interested_topics only when truthy (rs guards `!is_empty()` ↔ py `if profile.profession:`). **Confirmed does NOT route through `to_reddit_format`** (which conditionally omits age/gender/mbti/country → would be a downgrade). **Key order byte-identical** to py dict insertion: user_id,username,name,bio,persona,karma,created_at,age,gender,mbti,country,[profession],[interested_topics] — VERIFIED via `serde_json` `preserve_order` feature (Cargo.toml:35; LOAD-BEARING — without it serde alphabetizes; confirmed enabled). **UTF-8 raw (ensure_ascii=False) + indent=2: byte-identical** — differential test: Python `json.dumps(...,ensure_ascii=False,indent=2)` vs Rust `to_vec_pretty` produced identical bytes (张三 → `345 274 240...` raw, no `\uXXXX`, 2-space indent, no trailing newline). 7 reddit tests + round-trip pass.

**S-370 `save_twitter_csv` — PASS (with non-contractual terminator note).** rs:222-282 vs py:1070-1119. Header exact `[user_id,name,username,user_char,description]`; user_id = ROW INDEX (rs `idx.to_string()`, not profile.user_id) — test asserts profile.user_id=99 → CSV "0"; username = profile.user_name; user_char = bio when persona empty-or-==bio else "{bio} {persona}" with \n/\r→space (rs:256-260 ↔ py:1101-1105); description = bio with \n/\r→space; .json→.csv extension swap (rs:226-243 ↔ py:1088-1089). **CSV quoting differential (Python csv.writer QUOTE_MINIMAL vs Rust csv crate default):** captured Python golden + Rust output for adversarial fields (comma, embedded quote, tab, semicolon) — **field-level quoting/escaping BYTE-IDENTICAL**: comma-field quoted, embedded `"`→`""` doubled, tab/semicolon unquoted. **DIVERGENCE: line terminator** — Python `csv.writer` default = `\r\n` (CRLF, confirmed in golden), Rust `csv` crate default = `\n` (LF); port does NOT set `.terminator(CRLF)`. **Adjudicated NON-CONTRACTUAL:** the contractual read path (`simulation.py:1090-1094` API `GET /<id>/profiles` and `zep_tools.py:1533`) reads via `csv.DictReader(open(path,'r',encoding='utf-8'))` in TEXT MODE WITHOUT `newline=''` → Python universal-newlines normalizes `\r\n`/`\n` to `\n` before DictReader → parsed rows (the external observable = the API JSON response per DECISION-10 §1) are IDENTICAL regardless of terminator. Survives the `[≠]` bar as a non-contractual byte artifact (consumer normalizes before observable output). 7 CSV tests + round-trip pass.

**S-367 `generate_profiles_from_entities` — PASS (the subtle realtime-vs-final parity risk — CONFIRMED MATCHES).** rs:358-472 vs py:851-1014. **Realtime-vs-final writer split is FAITHFUL:** MiroFish's inner `save_profiles_realtime` closure (py:889-915) calls the SERIALIZERS — `p.to_reddit_format()` (py:903) / `p.to_twitter_format()` (py:909) — NOT the forced-default writers; teri's `realtime_save` (rs:487-536) ALSO uses `to_reddit_format`/`to_twitter_format` (rs:497,508). The FINAL `save_profiles` uses the dedicated forced-default writers on BOTH sides. **Match confirmed: realtime=serializers, final=forced-default writers, both sides.** Realtime "write full current set each time" + `except→warn,continue` (py:916-917) ↔ rs:454-457 `warn!("实时保存 profiles 失败")` non-aborting. user_id=idx (rs:443 ↔ py:930). progress_callback(current 1-based, total, msg) after each incl. fallbacks. LLM-failure→fallback profile (rs:404-440 ↔ py:939-951; fallback dataclass defaults karma=1000/friend=100/follower=150/statuses=500/age=None/gender=None/mbti=None/country=None — BYTE-MATCH py:40,69 + L1002-1006). **Sequential confirmed correct:** MiroFish base method uses ThreadPoolExecutor but writes to a pre-allocated ordered list (`profiles=[None]*total`, py:884) → result is entity-ordered regardless; teri sequential preserves the same ordered Vec (DECISION-10 §4: parallelism is caller's concern). 5 batch tests pass (ordered Vec, fallback, realtime-after-each, both-files, callback count).

**S-369 `save_profiles` — PASS.** rs:294-303 vs py:1047-1068. Twitter→save_twitter_csv, Reddit(else)→save_reddit_json. Dispatch tests pass.

**S-373 `save_profiles_to_json` — PASS (thin alias).** rs:313-320 vs py:1196-1205. `warn!("save_profiles_to_json is deprecated; use save_profiles instead")` ↔ py `logger.warning(...已废弃...)` then delegates to save_profiles. 1 alias test passes.

**S-368 `_print_generated_profile` — STAYS `[≠]` (challenge SURVIVED).** py:1016-1045 console pretty-print (`【简介】`/`【详细人设】`/`【基本属性】` to stdout). Genuinely NON-CONTRACTUAL: no consumer reads stdout; progress carried by `progress_callback` + ported `progress.profileGenerated` i18n key. The realtime-save BEHAVIOR (incremental file write) IS ported inside generate_profiles_from_entities (rs:450-458). Not a disguised skip → legal `[≠]`.

### Blast radius — ADDITIVE ONLY (confirmed)
This cycle's working-tree diff: NEW `src/services/oasis_profile_export.rs`; `+pub mod oasis_profile_export` in `src/services/mod.rs`; `+csv = "1"` in Cargo.toml. **ZERO edits to `SocialProfile`/`Persona::{to_reddit_format,to_twitter_format,to_dict}`/`PersonaGenerator`/`generate_username`** (git diff confirms agent/mod.rs untouched this cycle). The `(SocialProfile, String)` tuple carrying the entity name is SOUND: SocialProfile has no name field; name sourced from `entity.name` (rs:373) and used for JSON "name" (rs:159) + CSV "name" column (rs:267) — matches MiroFish where name = profile.name (py:1112,1169). Verified serializers reused ONLY on realtime path (faithful to MiroFish).

### NON-BLOCKING doc-comment correction required (route to porter as cleanup, NOT a re-port)
`oasis_profile_export.rs:219-221` doc claims the csv crate default "matches this behaviour exactly" — FALSE for the line terminator (Python=CRLF, Rust=LF). It matches for quoting/escaping only. Correct the comment to state: terminator differs (CRLF vs LF) but is non-contractual (the read path's text-mode universal-newlines + `csv.DictReader` normalizes it before any observable output). This is a documentation accuracy fix, not a behavioral downgrade — the observable contract (parsed API rows) is identical.

### Rollup
U-018 was already `[x]` at unit level; S-367/369/370/371/372/373 moving `[≠]`/`[~]`→`[x]` STRENGTHENS it (the wrongly-`[≠]`'d export symbols corrected). S-368 stays challenge-surviving `[≠]`. All U-018 symbols now `[x]`/`[≠]` → rollup rule satisfied. No downgrade.

---

## 2026-06-17 — U-023 sub-cycle (b): simulation STATE TYPES (S-636..S-667) — PASS

**Unit:** U-023 sub-cycle (b) only — the state types. `SimulationManager` (S-668+, sub-cycles c/d) NOT in scope and confirmed still `- [ ]`.
**Source:** `MiroFish/backend/app/services/simulation_manager.py:25-112`
**Rust:** `src/services/simulation_manager.rs` (wired via `src/services/mod.rs:11`)
**Method:** differential — ran the Python dataclass standalone to capture golden `to_dict`/`to_simple_dict`/enum values, then dumped the actual Rust `serde_json` output via an injected throwaway test and byte-compared key sets, order, shape, and enum strings. 18 module tests pass; targeted `cargo test` green.

### Group verdicts (file:line both sides)

1. **SimulationStatus S-636..S-644 — PASS.** All **8** variants present. py L27-34 `.value` = `[created,preparing,ready,running,paused,stopped,completed,failed]`; Rust `simulation_manager.rs:35-52` serde `snake_case` + `as_str`/`Display` (L59-77) emit the identical 8 strings in the same order. Differential serde dump matched exactly. No narrowing (the "4 statuses" summary was wrong; source is authoritative — 8).
2. **PlatformType S-645..S-647 — PASS.** Exactly **2** variants: `twitter`,`reddit` (py L39-40 vs Rust L91-96). Confirmed **NO `both`** variant: `serde_json::from_str::<PlatformType>("\"both\"")` errors (test `platform_type_has_exactly_two_variants`). The "3 variants incl. BOTH" summary was wrong.
3. **SimulationState fields S-648..S-665 — PASS.** All 17 fields with exact defaults: enable_twitter/enable_reddit=true, status=Created, entities_count/profiles_count/current_round=0, entity_types=[], config_generated=false, config_reasoning="", twitter_status/reddit_status="not_started", error=None; created_at/updated_at stamped via `python_isoformat_local()`. Required IDs (simulation_id/project_id/graph_id) have no default — modeled as required ctor params on `SimulationState::new` (L203). py L46-76 ↔ Rust L148-224.
4. **to_dict S-666 — PASS.** Exactly **17 keys** in identical declaration order; byte-identical between py golden and Rust dump. `status` emitted as the lowercase STRING (`self.status.to_string()`, not a nested object); `error` = JSON null when None, string when Some. py L80-98 ↔ Rust L238-278. Insertion order preserved (serde_json `preserve_order`).
5. **to_simple_dict S-667 — PASS.** Exactly **9 keys** (`simulation_id, project_id, graph_id, status, entities_count, profiles_count, entity_types, config_generated, error`) in identical order; status as string; error null/string. Byte-identical to py golden. py L102-112 ↔ Rust L289-318.
6. **python_isoformat_local reuse — PASS.** Timestamps use the established `crate::models::project::python_isoformat_local` (`project.rs:50`, `pub(crate)`, imported at `simulation_manager.rs:21`) — NOT hand-rolled. Same local-naive ISO semantics as `datetime.now().isoformat()`.

### Non-contractual note (not a divergence)
Python's two `default_factory` lambdas can stamp `created_at` and `updated_at` microseconds apart; the Rust ctor stamps once and clones to both. Both produce ISO strings of identical shape; the timestamp values are non-deterministic and non-contractual, so this is observationally equivalent — no `[≠]` needed, no downgrade.

### Ledger-summary corrections recorded
- **8 statuses, not 4** — the ledger summary undercounted `SimulationStatus`.
- **2 platforms, not 3** — `PlatformType` has no `BOTH` variant; the ledger summary was wrong.
Both annotated on the U-023 row in `parity-ledger.md`.

### Rollup
**VERDICT: PASS** for sub-cycle (b). 32/32 in-scope symbols (S-636..S-667) → `- [x]`. No `[≠]` claimed (nothing skipped). U-023 stays `- [ ]` at unit level — sub-cycles (c) `SimulationManager` and (d) `create_simulation`/`prepare_simulation`/`get_profiles`/`get_simulation_config` remain (S-668+ still `- [ ]`). Do NOT commit U-023 as done.

---

## 2026-06-17 · U-023 sub-cycle (c) · `SimulationManager` struct + FS persistence + getters (S-668..S-674, S-676..S-680)

**Verdict: PASS** — differential parity verified for 12 symbols; S-680 ships as a legitimate `[≠]`-partial (adjudicated below). S-675 (`prepare_simulation`) confirmed out-of-scope, stays `- [ ]`.

**Baseline:** `cargo test --lib services::simulation_manager` = **39 passed, 0 failed**. Build green per prompt (788 passed, clippy `--all-targets -D warnings` clean).

**Type:** map-onto-substrate (the dir PATH is config-rooted vs Python's module-relative path) for S-668/669/670; literal-behavior parity for S-671..S-679; partial+`[≠]` for S-680.

### Differential evidence (source `simulation_manager.py` ↔ Rust `src/services/simulation_manager.rs`)

- **S-672 `_save_simulation_state`** (py L145-155 ↔ rs L830-846): order is **updated_at bumped FIRST** (py L150 `state.updated_at = datetime.now().isoformat()` → rs L832), **then write** (py L152-153 `json.dump(..., ensure_ascii=False, indent=2)` → rs L838 `to_string_pretty`), **then cache** (py L155 → rs L842-843). Order faithful. Serialization: Python `ensure_ascii=False, indent=2` emits raw UTF-8 + 2-space indent; differentially verified `json.dumps({'config_reasoning':'配置推理',...}, ensure_ascii=False, indent=2)` produces unescaped CJK + 2-space — `serde_json::to_string_pretty` produces the identical shape. Test `create_simulation_state_json_readable` asserts 2-space indent on disk.
- **S-673 `_load_simulation_state`** (py L157-192 ↔ rs L881-1019): cache-first (py L159-160 ↔ rs L883-888); file-missing→None (py L165-166 ↔ rs L894-896); per-field `.get(key,default)` tolerance matches py L171-189 field-by-field (project_id "", graph_id "", enable_* true, counts 0, entity_types [], config_generated false, config_reasoning "", current_round 0, *_status "not_started", created_at/updated_at fresh-now, error None). **Invalid status string → Err** (py L177 `SimulationStatus(data.get("status","created"))` raises ValueError on unknown — differentially confirmed `SimulationStatus('bogus')` raises ValueError — rs L931-935 returns `TeriError::Sim`, NOT a silent default). Test `load_invalid_status_returns_err` proves it. `error` field: py `data.get("error")` returns None for absent/null and the string when present — rs `.and_then(as_str).map(to_string)` matches all three (differentially confirmed).
- **S-674 `create_simulation`** (py L194-228 ↔ rs L1033-1074): id = `sim_` + 12 lowercase hex, no hyphens. Python `f"sim_{uuid.uuid4().hex[:12]}"` — differentially confirmed `uuid4().hex[:12]` = 12 chars, all in `0-9a-f`. Rust `Uuid::new_v4().simple().to_string()[..12]` = 32-hex-no-hyphen sliced to 12, lowercase. Test `create_simulation_id_format` asserts `sim_` prefix + 12 chars + lowercase-hex; `create_simulation_ids_are_unique` asserts uniqueness across 10. Status CREATED, saved. Faithful.
- **S-676 `get_simulation`** (py L459-461 ↔ rs L1084-1086): thin delegation to load. Test `create_get_round_trip`.
- **S-677 `list_simulations`** (py L463-479 ↔ rs L1108-1140): skips `'.'`-prefixed (py L471 ↔ rs L1121-1123) and non-dir entries (py L471 `not os.path.isdir` ↔ rs L1126-1128); project_id filter (py L476 ↔ rs L1132); **nonexistent dir → [] not Err** (py L467 `if os.path.exists(...)` guard ↔ rs L1109-1111 early `Ok(vec![])`). Tests `list_simulations_skips_hidden_entries` (hidden + non-dir both skipped), `_filters_by_project_id`, `_nonexistent_dir_returns_empty`. Order unspecified both sides (py `os.listdir` / rs `read_dir`) — faithful.
- **S-678 `get_profiles`** — the raise-vs-empty distinction (py L481-494 ↔ rs L1159-1184): **missing STATE → Err** (py L484-485 `raise ValueError` ↔ rs L1166-1171); **missing FILE → Ok([])** (py L490-491 `return []` ↔ rs L1177-1179); present → parsed array; platform selects `{platform}_profiles.json` (py L488 ↔ rs L1175). Distinction is EXACT. Tests `get_profiles_missing_state_returns_err`, `_missing_file_returns_empty_vec`, `_present_returns_array`, `_platform_twitter_reads_twitter_file`.
- **S-679 `get_simulation_config`** (py L496-505 ↔ rs L1196-1207): missing→None (py L501-502 ↔ rs L1200-1202), present→Some(parsed). Tests `_missing_returns_none`, `_present_returns_some`.
- **S-668/669/670 `__init__`/SIMULATION_DATA_DIR**: Python roots at module-relative `../../uploads/simulations` (py L127-130); teri roots at `config.oasis_simulation_data_dir` (env `OASIS_SIMULATION_DATA_DIR`, default `./uploads/simulations`) following the ProjectManager pattern. The exact PATH differs (environment/rooting detail, config-driven) but is **not observably wrong**: the manager is self-consistent — it writes and reads the same configured dir, dir auto-created via `create_dir_all`, cache-first reads + FS writes preserved. In-memory cache mapped to `Mutex<HashMap>` (interior mutability for `Arc<SimulationManager>` axum sharing) — faithful map-onto of Python's `self._simulations` dict. Round-trip test `load_from_disk_after_cache_cleared` + `load_cache_first_returns_cached_value` prove cache semantics.

### S-680 `get_run_instructions` — `[≠]` adjudication (owner no-downgrade rule applied)

**Verdict: legitimate `[≠]`-partial — PASS.** NOT a disguised portable-feature skip.

Ported faithfully (expressible): `simulation_dir`, `config_file` paths (py L514,516 ↔ rs `RunInstructions{simulation_dir, config_file}`).

`[≠]`-omitted (genuinely inexpressible): `scripts_dir`, `commands{twitter,reddit,parallel}`, `instructions` (py L515,517-528). These are `python {scripts_dir}/run_twitter_simulation.py --config ...` + `conda activate MiroFish` strings.

**Inexpressibility proven (not asserted):**
- MiroFish's `run_*_simulation.py` scripts DO exist (`backend/scripts/run_{twitter,reddit,parallel}_simulation.py`, 26-61 KB each) and the commands reference them under conda. CONFIRMED they are real in source.
- teri has NO such scripts and NO conda env. The runner scripts are ported as **native in-process Rust** (`SimEngine`, confirmed `src/sim/mod.rs:396` struct, `:493` `pub async fn run`) — tracked as separate units U-028/U-029/U-030 (`port-fresh`, all `- [ ]`). teri's runner = SimEngine, not a Python subprocess.
- Therefore emitting `python /<nonexistent>/run_twitter_simulation.py` would be **fabrication of a command that cannot execute** in teri's substrate — strictly worse than admitting the gap. This meets the inexpressibility bar of the `[≠]` rule.

**Consumer/contract check (the no-downgrade scrutiny):** `get_run_instructions` IS an observable API output — consumed at `backend/app/api/simulation.py:772` (`result["run_instructions"] = manager.get_run_instructions(...)`), served to the frontend on `GET /<sim_id>` ONLY when `status == READY`. So there IS a downstream consumer of the shape. HOWEVER:
- The teri API route (U-026 `teri::api::simulation`) is **NOT YET PORTED** (`- [ ]`) — no teri consumer of `RunInstructions` exists yet (grep of `src/` for `RunInstructions`/`run_instructions` outside the manager file = zero hits).
- `status == READY` is only reached after `prepare_simulation` (S-675, also unported, sub-cycle d) and the runner (U-022 `SimulationRunner`, `- [ ]`). The native-run contract teri would advertise does not exist as a stable shape yet.

**Carry-forward gate (recorded, NOT an S-680 blocker):** when **U-026** is ported, the parity gate MUST verify teri emits *native run-guidance* (e.g., the `teri run`/`SimEngine::run` invocation) for the `run_instructions` API field — NOT merely the static `substrate_note` — so the frontend's "how to run a prepared simulation" contract is not downgraded. The literal Python script commands stay inexpressible, but the *guidance capability* must be re-expressed natively at the API boundary. Owner rule: the API contract requires SOME run-guidance shape. At the manager-method level (S-680) the `[≠]` is correct and complete; the obligation transfers to U-026.

Test `get_run_instructions_structural_fields` asserts both path fields + non-empty substrate_note directing to SimEngine.

### Rollup
**VERDICT: PASS** for sub-cycle (c). 12/12 in-scope symbols verified: S-668..S-674, S-676..S-679 → `- [x]`; S-680 → `- [x]` (partial port w/ adjudicated `[≠]` on the script-command sub-fields, carry-forward gate on U-026). S-675 (`prepare_simulation`) stays `- [ ]` (sub-cycle d). U-023 stays `- [ ]` at unit level — only sub-cycle (d) remains. Do NOT commit U-023 as done.

---

## 2026-06-17 — U-023 sub-cycle (d) `prepare_simulation` (S-675) → COMPLETES U-023 · + RE-OPENED S-367 `generate_profiles_from_entities` (concurrency made live)

**Gate:** rust-port-parity-verifier (opus). **Verdict: PASS** (no downgrade found).
**Source:** `MiroFish/backend/app/services/simulation_manager.py` L230-458 (`prepare_simulation`); `.../oasis_profile_generator.py` L851-1014 (`generate_profiles_from_entities`).
**Rust:** `teri/src/services/simulation_manager.rs` (`PrepareProgress`, `prepare_simulation`, `prepare_tests`); `teri/src/services/oasis_profile_export.rs` (`generate_profiles_from_entities` seq→`buffer_unordered`, 3 determinism tests).
**Design verified against:** DECISION-11 (target-architecture.md L741-799).

### By-RUNNING (cargo test, full lib green)
- `cargo test --lib prepare_simulation` → **7 passed / 0 failed**.
- `cargo test --lib oasis_profile_export` → **38 passed / 0 failed** (incl. both determinism tests).
- `cargo test --lib t_args` → **4 passed** (i18n count-interpolation correct).
- `cargo test --lib` (full) → **786 passed / 0 failed** — no regression.
- `cargo check --lib` clean; no stray `_parallel_count` (knob fully live).

### By-READING (differential, branch-by-branch — source vs Rust)

**DECISION-11 §1 — no spawn/task_id smuggled.** `prepare_simulation` is `async fn -> Result<SimulationState>` (sim_manager.rs:1159). `grep tokio::spawn` over both files = ZERO. The realtime-progress closure uses a `&raw mut` ptr (cb_ptr) with a documented SAFETY note; `buffer_unordered` polls on the CURRENT task (no spawn), so the pointer is non-aliasing and valid. CONFIRMED: no task_id/Thread wrapping (that lives in U-026 route layer per design).

**DECISION-11 §3 — no `force_regenerate`.** Signature has no skip flag; all 3 stages always run. CONFIRMED — no file-existence shortcut anywhere.

**DECISION-11 §4 — 0-entity (Ok) vs exception (Err) DISTINCT.**
- 0-entity (py L298-302 `return state`): rs:1243-1250 → status=FAILED + error `"没有找到符合条件的实体，请检查图谱是否正确构建"` (exact) + save + `return Ok(state)`. Test `prepare_simulation_zero_entities_returns_ok_with_failed_status` asserts `result.is_ok()` AND status==Failed AND disk state.json status="failed". CORRECT.
- exception (py L450-457 `except: …; raise`): `try_stage!` macro (rs:1255-1270) sets status=FAILED + error=e.to_string() + saves state BEFORE `return Err(e)`. Test `prepare_simulation_exception_sets_failed_and_returns_err` sabotages reddit path → EISDIR → asserts `result.is_err()` AND disk state.json status="failed" with non-null error. CORRECT — FAILED state IS persisted before Err propagates.
- Missing sim (py L262-264 raise ValueError): rs:1180-1185 → `Err(TeriError::Sim("模拟不存在: {id}"))`. Test asserts Err + message contains 模拟不存在 + the id. CORRECT.
- **Fallible-stage coverage note (verified, not a narrowing):** in the Rust port `filter_defined_entities -> FilteredEntities` and `generate_config -> SimulationParameters` are INFALLIBLE by prior locked design (internal fallback), and `generate_profiles_from_entities -> Vec` is infallible. The only fallible ops in the body are the two `save_profiles` + the config `fs::write` — ALL wrapped in `try_stage!`. Python's broad try/except is faithfully mapped: every operation that *can* error is under the FAILED-save handler; the infallible-by-design stages have no error path to drop. Not a downgrade.

**DECISION-11 §4 — stage order + state writes.** PREPARING+save → stage1(entities_count/entity_types, reading 0/30/100) → stage2(profiles_count; realtime reddit>twitter>None; FINAL save_profiles gated by enable_reddit AND enable_twitter as TWO independent `if` branches NOT elif, rs:1355-1368 = py L361-374) → stage3(generate_config.await; write simulation_config.json via `sim_params.to_json()`; config_generated=true; config_reasoning) → READY+save. CONFIRMED faithful. Tests: happy_path_reddit_only (reddit.json written, twitter.csv NOT), twitter_only (csv written, reddit.json NOT), state_fields_populated (both files, entities_count=3, profiles_count=3, config_generated).

**DECISION-11 §5 — full progress surface (SSE contract).** All **11** `progress_callback` callsites reproduced with EXACT stage label + percentage + current/total:
reading 0(—/—), 30(—/—), 100(fc/fc); generating_profiles 0(0/N), inner pct=int(c·100/t)(c/t), 95(N/N), 100(len/len); generating_config 0(0/3), 30(1/3), 70(2/3), 100(3/3). Verified line-by-line vs py L273-436. Inner pct: Rust `(current*100)/total` == Python `int(current/total*100)` for non-neg ints. **item_name folding LOSSLESS:** Python passes `item_name=msg` (==message) at the ONLY callsite that sets it (py L318-327); the other 10 callsites omit it; PrepareProgress folds item_name into message — identical where present, absent elsewhere. No field dropped. Test `progress_callback_receives_all_stages` asserts stage ordering + reading/0 None-None + reading/100 set + config/0 Some(0)/Some(3). All 10 `progress.*` i18n keys present in BOTH en.json + zh.json.

**DECISION-11 §2 / S-367 concurrency (the re-opened crux):**
- `parallel_count` LIVE: `buffer_unordered(parallel_count.max(1))` (export.rs:484-485) — genuinely bounds in-flight futures. No stray `_parallel_count`.
- **DETERMINISM:** consumer writes `results[idx] = Some(...)` (indexed slot, export.rs:489), NOT push-on-completion → final Vec order-preserving regardless of arrival order. Test `generate_profiles_final_file_bytes_deterministic` does REAL `assert_eq!(bytes_1, bytes_3)`/`(bytes_1, bytes_10)` on `fs::read` of written reddit JSON (byte-for-byte Vec<u8>, not a length check). Test `determinism_across_parallel_counts` asserts per-slot `user_id==idx` AND `name==expected[idx]` for parallel ∈ {1,3,10} — these ARE per-entity-distinct/order-sensitive, proving indexed-slot writes. (Honest caveat: MockLlm response is identical per entity, so byte-equality proves ORDER preservation; the per-slot user_id/name assertions carry the per-entity distinctness. Sufficient.)
- Per-entity fallback (py L939-951): export.rs:441-468 builds baseline SocialProfile on generation error — **bio and persona are DISTINCT fields** (`bio=f"{type}: {name}"`, `persona=summary or generic`), NOT collapsed. This avoids the MiroFish→teri cycle-8/9 hidden bio+persona-collapse downgrade class. user_id=idx, source_entity_uuid/type set. Test `fallback_on_llm_error` confirms fallback returned with non-empty bio + user_id=0.
- Realtime-save after each completion (py L979-980): export.rs:493-501 writes all completed non-None slots; write failure is `warn!`-logged not fatal (py L916-917 `logger.warning`). Realtime uses `to_reddit_format`/`to_twitter_format` (matches py `save_profiles_realtime` closure); FINAL save uses dedicated `save_reddit_json`/`save_twitter_csv` with OASIS forced defaults — faithful to MiroFish's own realtime-vs-final split. Test `realtime_write_after_each` confirms incremental file ≤ current entries + final file complete.
- **No regression to U-018 output shape:** SocialProfiles + reddit JSON / twitter CSV formats UNCHANGED — only the scheduling (sequential→buffer_unordered) changed. S-369/370/371/372/373 stay `[x]` (their 38 tests still green).

### [≠] adjudications (sharpened owner rule)
- **S-368 `_print_generated_profile` — `[≠]` SURVIVES.** Console pretty-print to stdout (`【简介】`/`【详细人设】` blocks); no API/get_profiles/SimEngine/file consumer reads it; user-facing progress carried by progress_callback. Genuinely NON-CONTRACTUAL (console artifact — the exact class the owner rule names legal). NOT a disguised feature-skip.
- **S-680 `get_run_instructions` script-command sub-fields — `[≠]`-substrate SURVIVES** (out of this cycle's flip scope; prior adjudication under DECISION-9/sub-cycle-c). The `scripts_dir`/`commands`/`instructions` strings invoke MiroFish's Python OASIS subprocess scripts under conda; teri runs in-process via SimEngine — those scripts/env do NOT exist, so the strings are genuinely INEXPRESSIBLE (fabricating them yields commands that cannot run). Structural fields (simulation_dir, config_file) ARE ported. Legal substrate `[≠]`.

### Symbols verified (orchestrator flips `[x]`)
- **S-675** `prepare_simulation` → `- [x]` (PASS; all 4 stages + 3 error/terminal branches + 11 progress events differential-verified).
- **S-367** `generate_profiles_from_entities` → `- [x]` (re-verified with LIVE bounded concurrency; determinism proven byte-identical across parallel ∈ {1,3,10}).
- S-368 stays `- [≠]` (survives challenge). S-369/370/371/372/373 unchanged `- [x]`.

### Rollup
**VERDICT: PASS.** S-675 `[x]` ⇒ **U-023 COMPLETE** (all S-636..S-680 now `[x]`/legal-`[≠]`). S-367 `[x]` (re-verified). No downgrade, no narrowed branch, no disguised-skip `[≠]`. Orchestrator may flip U-023 ledger `- [x]` and commit.

---

## 2026-06-17 — U-007 `zep_paging.py` (S-049..S-055) — MAP-ONTO-SUBSTRATE — opus parity gate — **PASS**

**Verifier:** rust-port-parity-verifier · **Unit:** U-007 (map-onto-substrate; NO new production code — Zep-Cloud network pagination → teri in-process `KnowledgeGraph`).
**Source:** `MiroFish/backend/app/utils/zep_paging.py:1-143` · **Map-onto targets:** `src/graph/mod.rs:1046 get_all_entities` / `:830 get_all_edges`; `src/services/entity_reader.rs:560 get_all_nodes` / `:585 get_all_edges`. **Contract:** DECISION-1 §3 / U-007 row (target-architecture.md:43,134).
**Method:** differential read of source vs. the substrate primitives, plus an adversarial consumer sweep for the `_MAX_NODES=2000` cap (the key challenge). U-016's PASS verdict (parity.md:1167-1237) is the established node/edge-dict shape equivalence this builds on.

### Crux #1 — the map-onto targets return EVERYTHING (no silent drop) — CONFIRMED
- `KnowledgeGraph::get_all_entities()` (`graph/mod.rs:1046-1048`) = `self.inner.node_weights().collect()` — every petgraph node weight, no limit/filter/skip.
- `KnowledgeGraph::get_all_edges()` (`graph/mod.rs:830-839`) = `self.inner.edge_references().map(|e| (src.id, tgt.id, weight.clone()))` — every petgraph edge, no limit/filter.
- `entity_count()`/`relation_count()` = `node_count()`/`edge_count()` — full counts, consistent with the full iteration.
- The reader's `get_all_nodes`/`get_all_edges` (`entity_reader.rs:560-593`) are thin `.map()`s over those primitives — no truncation, no pagination, no hidden bound.
**Result:** "return all" genuinely means ALL. teri NEVER needed a separate paging layer — petgraph iteration is the complete-set primitive.

### Crux #2 — shape equivalence — CONFIRMED (inherits U-016 PASS)
MiroFish's `fetch_all_*` returns raw Zep node/edge objects; its `ZepEntityReader.get_all_nodes/get_all_edges` (`zep_entity_reader.py:650-715`) build NodeInfo/EdgeInfo lists FROM those. teri's reader `get_all_nodes` (5-key node dict) / `get_all_edges` (6-key edge dict) were parity-verified `[x]` in U-016 (parity.md:1234), including the `[≠]` field-level empties (summary/attributes/fact/uuid), each with a confirmed consumer-side graceful fallback — zero disguised feature-skips. U-007 is the LOWER primitive (`fetch_all_*`): verified subsumed — teri reads the complete in-memory set directly; the U-016-verified dict shape is the faithful equivalent of what MiroFish built from `fetch_all_*`.

### Crux #3 — the `_MAX_NODES=2000` cap (S-050) — adversarial `[≠]`-strict-superset challenge — SURVIVES
**Why the cap exists in source:** it bounds the number of paged Zep HTTP round-trips against a huge remote graph (truncate to 2000 + warn, `zep_paging.py:90-93`). Its ONLY observable behavior is SILENTLY DROPPING nodes beyond 2000. Note the source is asymmetric — `fetch_all_edges` (`zep_paging.py:105`) has **NO cap at all**, which alone shows the cap is a node-paging artifact, not a downstream contract.
**Adversarial consumer sweep (every reader consumer of get_all_nodes/get_all_entities, source + teri):**
- **MiroFish consumers** of `fetch_all_nodes`: `graph_builder._get_graph_info`/`get_graph_data` (count + iterate, `node_count=len(nodes)`), `zep_tools.get_all_nodes` (iterate→NodeInfo list), `zep_entity_reader.get_all_nodes` (the U-016 source). **None** has a ≤2000 array-size or context assumption — all treat the result as "all nodes."
- **teri `AgentPool::spawn`** (`agent/mod.rs:737`): `entities[i % entities.len()]` — modulo-cycles personas; MORE entities = larger anchor pool, never an overflow. No ≤2000 dependency.
- **teri `filter_defined_entities`** (`entity_reader.rs:675`): builds a `Vec<EntityNode>` + `HashMap` over ALL entities; no fixed-size buffer. No dependency.
- **teri `prepare_simulation`** (`simulation_manager.rs:1219`): calls `filter_defined_entities` → `entities_count = filtered_count`. No dependency.
- **The LLM-context path (the strongest candidate for a hidden ≤2000 budget) — REFUTED:** `SimulationConfigGenerator::summarize_entities` (`simulation_config.rs:1185-1231`) groups by type, then **`.take(ENTITIES_PER_TYPE_DISPLAY=20)` per type** + char-truncates each summary to `ENTITY_SUMMARY_LENGTH=300` + an overall `MAX_CONTEXT_LENGTH=50_000` char budget. This is a **content-length/per-type-display budget that is INDEPENDENT of total entity count** — identical whether 5 or 2,000,000 entities. It is a faithful port of MiroFish's OWN `simulation_config_generator.py:223,402,424-429` (`ENTITIES_PER_TYPE_DISPLAY=20`, `type_entities[:display_count]`, `document_text[:remaining_length]`). **MiroFish's LLM context was NEVER protected by the 2000 cap — it is protected by this per-type `take(20)` + char truncation, which teri ports verbatim.** So >2000 nodes in teri cannot blow any context budget MiroFish kept safe.
**Conclusion:** NO consumer (teri or source) depends on count ≤2000. The cap is a pure Zep-network paging round-trip safety limit; truncating removes valid data. teri returning the full in-memory set is a **strict SUPERSET** (genuinely more faithful — it's the data MiroFish itself wanted but capped for network safety). **`[≠]`-strict-superset adjudication HOLDS — NOT a disguised feature-skip.**

### Crux #4 — retry/page-size/delay (S-053, S-049, S-051, S-052) — `[≠]`-inexpressible challenge — SURVIVES
`_fetch_page_with_retry` retries `ConnectionError`/`TimeoutError`/`OSError`/`zep_cloud.InternalServerError` (`zep_paging.py:44`) — strictly **network/IO transient errors of a remote SaaS call**. The map-onto target is `petgraph::node_weights()`/`edge_references()` — an in-process `Vec`/HashMap traversal with **no I/O, no socket, no remote server**. There is no transient-failure mode in teri's read path that a retry would meaningfully address (a missing entity is `None`/`[]`, deterministic, not retried — same adjudication already CONFIRMED for the SIBLING `_call_with_retry` (S-216) in U-016, parity.md:1142,1225-1228, where the except→None/[] fallback CONTRACTS were verified ported). `page_size=100`/`uuid_cursor` is the Zep cursor-paging mechanism itself (no cursors over an in-memory iterator); `retry_delay=2.0`/`max_retries=3` are the retry's tuning constants — all pure network-cursor/retry artifacts. Genuinely inexpressible / non-contractual; no observable output dropped. **Legal substrate `[≠]`.**

### No-downgrade honesty check — CLEAN
This is NOT the owner's flagged bad pattern ("dest won't use it" rationalizing a portable feature skip). The distinction is real and substrate-grounded: paging/cursor/retry/cap are all about HOW Zep delivers data over a NETWORK; teri has the entire graph in RAM and reads it completely in one pass. The adjudication rests on genuine substrate-inexpressibility (no network → no cursor/retry) and strict-superset (full set ≥ capped set), NOT on convenience. The observable contract — "retrieve ALL nodes/edges" — is preserved and (for the cap) strengthened.

### Per-symbol verdict
- `- [x]` **S-054 `fetch_all_nodes`** — map-onto `get_all_entities`/`get_all_nodes`; complete-set iteration, no drop. Contract ("retrieve all nodes") preserved (cap removed = superset).
- `- [x]` **S-055 `fetch_all_edges`** — map-onto `get_all_edges`; complete-set iteration, no drop. (Source itself has no edge cap — exact superset.)
- `- [≠]` **S-049 `_DEFAULT_PAGE_SIZE=100`** — Zep cursor page size; no paging over in-memory iterator. Inexpressible.
- `- [≠]` **S-050 `_MAX_NODES=2000`** — network paging round-trip safety limit; NO consumer depends on ≤2000 (swept exhaustively, incl. the LLM-context path which is bounded by per-type `take(20)`, not node count). teri returns the full set = strict SUPERSET. Survives challenge.
- `- [≠]` **S-051 `_DEFAULT_MAX_RETRIES=3`** — Zep transient-error retry count; no I/O to retry in-process. Inexpressible/non-contractual.
- `- [≠]` **S-052 `_DEFAULT_RETRY_DELAY=2.0`** — retry backoff base; same. Inexpressible/non-contractual.
- `- [≠]` **S-053 `_fetch_page_with_retry`** — single-page Zep call + network-transient retry; in-process petgraph read has no network/transient failure. Inexpressible/non-contractual (sibling of U-016 S-216, already confirmed).

### Rollup
**VERDICT: PASS (5/7 `[≠]`, 2/7 `[x]`).** S-054/S-055 → `- [x]` (map-onto). S-049/S-050/S-051/S-052/S-053 → `- [≠]` (all challenge-surviving: inexpressible network-cursor/retry artifacts; S-050 strict-superset). All S-049..S-055 are `[x]`/legal-`[≠]` → rollup rule satisfied ⇒ **U-007 COMPLETE**. No silent drop (full petgraph iteration), no narrowed branch, no disguised-skip `[≠]`. Orchestrator may flip U-007 ledger `- [x]` and commit.
