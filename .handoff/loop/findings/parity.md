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
