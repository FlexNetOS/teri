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

---

## 2026-06-17 — U-021 sub-cycle (a) — `AgentActivity` + `to_episode_text` + 12 `_describe_*` (S-493..S-514)

**Source:** `MiroFish/backend/app/services/zep_graph_memory_updater.py` L24-199
**Rust:** `.worktrees/mirofish-port/teri/src/services/graph_memory.rs`
**Kind:** PURE PORT (no substrate mapping). Bar = byte-exact Chinese NL output.

### VERDICT: PASS — 22/22 symbols (S-493..S-514) all `[x]`. (U-021 unit stays `[~]`; sub-cycles b/c remain.)

### Evidence — verified BY READING + BY RUNNING

**By running:** `cargo test graph_memory` → **53 passed, 0 failed**. 52 of 53 use exact `assert_eq!`
against the full expected `"Alice: <chinese>"` string; the single `assert!` is the prefix
`starts_with("Alice: ")` test, which is additionally backed by an exact-equality full-format test.
No weak (non-empty / length-only) assertion masquerading as parity evidence.

**By reading — the byte-exact crux (strongest evidence):** extracted EVERY Chinese production
literal from the source describer region (L64-199, docstrings stripped) and from the Rust impl block
(pre-`mod tests`), normalised `{var}` placeholders to `{}`, and diffed the two sets:
**41 PY literals == 41 RS literals, set-equal, byte-for-byte.** Zero PY-only (no dropped literal),
zero RS-only (no drift). This covers every full-width `：`/`，`/`「」` and the colon-vs-no-colon crux.

### Adversarial checklist — all refutation attempts FAILED to find a divergence

1. **Struct fidelity (S-493..S-500):** 7 fields — `platform:String`, `agent_id:i64` (Py int),
   `agent_name:String`, `action_type:String`, `action_args:Map<String,Value>` (Py Dict[str,Any]),
   `round_num:i64` (Py int), `timestamp:String`. Types correct. `action_type` is a **plain dispatch
   String** — the only `SocialAction` mention is the L38 doc-comment explicitly DISavowing the
   coupling (`grep` = 1 hit, in a comment). NOT coupled to teri's enum. ✓

2. **`to_episode_text` (S-501):** match covers exactly the 12 action_types, `_ => describe_generic`,
   returns `format!("{}: {}", agent_name, description)` = `"{agent_name}: {description}"` — agent_name
   prefix + ": ", no simulation prefix (source L61-62). ✓

3. **12 describers (S-502..S-514) — byte-exact:**
   - Key sets verified per describer: like/dislike_post read `post_content`+`post_author_name`;
     repost reads `original_content`+`original_author_name`; quote_post reads original_content/
     original_author_name + `quote_content` OR `content`; create_comment reads `content`+`post_content`
     +`post_author_name`; like/dislike_comment read `comment_content`+`comment_author_name`;
     follow/mute read `target_user_name`; search reads `query` OR `keyword`; search_user reads
     `query` OR `username`. No wrong/missing key. ✓
   - Ladder ORDER preserved: 4-way `both → content → author → neither` (like/dislike post+comment,
     repost, quote-base); create_comment = outer-on-content then inner 4-way; quote_post appends
     `，并评论道：「{quote_content}」` after the base. ✓
   - **THE PUNCTUATION CRUX:** quote_post base = `…帖子「{content}」` (NO `：`, source L117 ↔ rust L189),
     while like/dislike/repost = `…帖子：「{content}」` (WITH `：`). Byte-confirmed both directions:
     quote-base has `帖子「` not `帖子：「`; like has `帖子：「`. ✓
   - **`or`-fallbacks (the subtle Python-falsy edge):** quote_content OR content (L113), query OR
     keyword (L181), query OR username (L186). Rust uses `arg(a)` → `if !is_empty() { a } else { arg(b) }`.
     Python `a or b` returns `b` when `a` is empty-string OR absent. The Rust `is_empty()` check on
     the `unwrap_or("")` result treats **both** the absent-key case AND the present-empty-string case
     as fall-through. Confirmed by reading the helper `arg()` (`.get→as_str→unwrap_or("")`) AND by the
     dedicated empty-string tests `test_quote_post_or_fallback_empty_quote_content_uses_content`,
     `test_search_posts_empty_query_uses_keyword`, `test_search_user_empty_query_uses_username`
     (each `{"query":""}` / `{"quote_content":""}` → asserts the fallback string). NOT an absence-only
     check. ✓
   - generic: `format!("执行了{}操作", self.action_type)` = `执行了{action_type}操作`. ✓

4. **Tests assert the RIGHT thing (spot-checked the complex ones):** quote_post with comment append
   (`引用了Carol的帖子「原文」，并评论道：「我的评论」`, L487), create_comment's 5 branches (L574-623,
   all exact), and all 3 or-fallback empty-string tests — each is an exact `assert_eq!` against the
   precise Chinese string. No weak expectation found.

### Minor non-contractual note (NOT a divergence)
The `arg()` helper returns `""` for a present-but-non-string JSON value (e.g. a number), whereas
Python `dict.get` would return the raw value and the f-string would stringify it (`「5」`). Per the
unit contract, `action_args` values are NL text strings deserialised from `actions.jsonl`; a numeric
arg is not a contractual input here. Non-contractual edge, no parity impact.

### Rollup
**PASS:** all 22 symbols S-493..S-514 exercised and byte-exact (0 `[≠]` — pure port, nothing skipped).
S-493..S-514 → `- [x]`. No silent drop, no narrowed/reordered ladder, no wrong key, no weakened test.
U-021 unit ledger stays `- [~]` (sub-cycles b = `ZepGraphMemoryUpdater` L202+, c = manager remain).

---

## 2026-06-17 — U-021 sub-cycle (b) `ZepGraphMemoryUpdater` (S-515..S-530) — VERDICT: **FAIL** (one observable divergence; 1 of 16 symbols left `- [~]`)

**Verifier:** rust-port-parity-verifier (differential, cross-boundary).
**Worktree:** `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch `port/mirofish`).
**Source:** `MiroFish/backend/app/services/zep_graph_memory_updater.py` (L202-476).
**Rust:** `src/services/graph_memory.rs` (GraphMemoryUpdater<L> L778-1289) + `src/graph/mod.rs` (extend_from_text/extract_and_merge_into/ExtendStats L559-858).
**Design:** DECISION-14 (target-architecture.md:874-1036).
**Tests run (all green):** `cargo test build_` → 23 pass (no U-015 regression); `extend_from_text` → 4 pass; `updater_tests` → 13 pass; `services::graph_memory` → 66 pass; full suite → **868 pass, 6 ignored**.

### Adversarial checklist results

1. **build byte-identical refactor — PASS.** `extract_and_merge_into` (graph/mod.rs:617) is the extracted 2-pass body; `build_with_progress_and_ontology` (:559) now builds a fresh graph and calls it (:584-594). 23 `build_` tests green; `test_build_still_byte_identical_after_refactor` (mod.rs:2616) confirms no regression. `build`/`build_with_progress`/`_and_ontology` present at :504/:527/:559.

2. **`extend_from_text` no-drop — PASS.** Merge by exact case-sensitive name: `if !self.index.contains_key(&entity.name) { add_entity; added+=1 } else { merged+=1 }` (mod.rs:668-673) — gates BEFORE add_entity (which rejects dupes at :287). Pass-2 collects `self.get_all_entities()` AFTER Pass-1 merge (:687) → resolves relations against self's FULL post-merge set, so a relation to a PRE-EXISTING entity is NOT dropped. Proven by `test_extend_from_text_merge_semantics` (mod.rs:2509): base graph={Alice}; extend introduces Bob + relation Bob→Alice; asserts `relations_added==1` and `relation_count==1` (the extend-into-self beats throwaway-subgraph property). Existing-node kind preserved (`test_extend_from_text_duplicate_name_reused`).

3. **retry `[≠]` — ADJUDICATED LEGAL.** Observable part PORTED: `flush_batch` Err arm → `failed_count.fetch_add(1)` + error log + worker continues (graph_memory.rs:1280-1287); worker never aborts. Literal 3x-backoff `[≠]` is **justified**: teri's LLM adapter `call_api` (llm.rs:317-356, used by `complete()` → `extend_from_text` → `flush_batch`) ALREADY retries internally — server-error + timeout retry with exponential backoff up to `self.max_retries` (:336-352). A second retry layer in flush_batch would be **redundant network-retry**. The source's MAX_RETRIES targeted Zep-network transients; teri's network-transient retry already lives one layer down. `[≠]` on S-519 stands. (S-518 SEND_INTERVAL, S-520 RETRY_DELAY likewise network-shaped, non-contractual — legal `[≠]`.)

4. **concurrency observable contract — MOSTLY PASS, ONE FAIL (buffer_sizes empty-state).**
   - batch-at-exactly-5: `test_batch_flush_at_batch_size` asserts `batches_sent==1, items_sent==5` for exactly 5 same-platform (one extend per 5). `test_per_platform_independent_batching` asserts 5 twitter flush=1 batch while 3 reddit waits, then stop flushes reddit → 2 batches. PASS.
   - `combined_text = join("\n")`: graph_memory.rs:1255-1256 exact; `test_combined_text_join` asserts the LLM prompt contains the `\n`-joined episode text. PASS.
   - DO_NOTHING skip BEFORE enqueue (skipped_count++, no total_activities): :1048-1051; `test_add_activity_do_nothing_skipped` asserts skipped=1/total=0. PASS.
   - add_activity_from_dict event_type skip (no counter bumped): :1072-1074; `test_add_activity_from_dict_event_type_skipped` asserts both stay 0. PASS.
   - `_flush_remaining` sends sub-5 leftovers on clean stop (no loss): worker channel-closed branch (:1203-1214) flushes every non-empty buffer; `test_flush_remaining_on_stop` asserts 3 activities → batches_sent=1/items_sent=3. PASS.
   - get_stats TOP-LEVEL key set {graph_id,batch_size,total_activities,batches_sent,items_sent,failed_count,skipped_count,queue_size,buffer_sizes,running}: byte-exact (`test_get_stats_key_set`). PASS.
   - **`buffer_sizes` nested-map content — FAIL (DIVERGENCE).** Source seeds `_platform_buffers={'twitter':[],'reddit':[]}` at `__init__` (L252-255); `get_stats` returns `buffer_sizes={p:len(b) for p,b}` (L463) → **always contains keys `twitter` and `reddit`**, even with zero activities. teri's `buffer_snapshot` starts `{}` (graph_memory.rs:966) and is only written by the worker on the first activity (`update_buffer_snapshot` is called inside the recv loop :1181 / drain :1216, NEVER at worker startup — even though the worker's own `platform_buffers` IS seeded twitter+reddit at :1166-1171, that seed never reaches the snapshot until an activity arrives). **Empirically confirmed** via temporary probe (reverted): `get_stats()` right after `start()` returns `buffer_sizes = {}` (no twitter/reddit keys) vs source `{"twitter":0,"reddit":0}`.
     - **Input:** `new(...); start(); get_stats()` (or any path with only DO_NOTHING/event_type activities — the worker never receives anything).
     - **Expected (source):** `buffer_sizes = {"twitter": 0, "reddit": 0}`.
     - **Actual (Rust):** `buffer_sizes = {}`.
     - **Offending symbol:** S-530 `get_stats` (and the worker-side snapshot init under S-527 `worker_loop`).
     - **Why contractual:** DECISION-14 §Decision 3 explicitly classifies `get_stats` as "OBSERVABLE, served via API/U-049 → PORT" and `buffer_sizes` as a named field of that contract. The nested-map key set is observable output of a symbol claimed fully PORTED (not `[≠]`). This is a narrowing of the per-platform map's guaranteed key set — exactly the disguised-narrowing class the gate fails closed on, regardless of size.

5. **U-050 locale — PASS.** `start()` captures `crate::i18n::get_locale()` (graph_memory.rs:982) BEFORE `tokio::spawn` (:995) and wraps the worker future in `crate::i18n::with_locale(locale, ...)` (:995-997). Faithful to source L281 (`current_locale = get_locale()` before Thread) + worker `set_locale(locale)` (L366).

6. **Other `[≠]` — all legal.** ZEP_API_KEY check (S-516 `__init__` ValueError) → keyless native graph, substrate-absent, legal `[≠]`. SEND_INTERVAL (S-518) → not load-bearing (pure wall-clock pacing, no output), legal `[≠]`. Zep coreference entity-resolution → teri merges by exact name, NO entity dropped (every extracted entity IS added; only the resolution key differs), genuine substrate inexpressibility, legal `[≠]`. **PLATFORM_DISPLAY_NAMES PORTED** (not skipped): `platform_display_name` (graph_memory.rs:835) twitter→"世界1"/reddit→"世界2"/else→input, used in the flush + flush_remaining log lines (:1206, :1273); `test_platform_display_name_*` cover all 3 branches incl. case-insensitivity. Correct — this is the log-observable feature that MUST be ported, and it is.

### Symbol roll-up (S-515..S-530, 16 rows)
- `- [x]` S-515 GraphMemoryUpdater struct — type present, generic `<L>` (LlmClient not dyn-safe; observable contract identical, documented).
- `- [x]` S-516 BATCH_SIZE=5 — `const BATCH_SIZE: usize = 5`; threshold proven by batch tests.
- `- [x]` S-517 PLATFORM_DISPLAY_NAMES — PORTED (世界1/世界2), log-observable, 3 tests.
- `- [≠]` S-518 SEND_INTERVAL — network rate-limit, non-contractual (no output). **Survives `[≠]` challenge.**
- `- [≠]` S-519 MAX_RETRIES — redundant: adapter `call_api` already retries (llm.rs:336-352); failed_count+continue PORTED. **Survives.**
- `- [≠]` S-520 RETRY_DELAY — network backoff cadence, non-contractual. **Survives.**
- `- [x]` S-521 __init__/new — constructor; ZEP_API_KEY `[≠]` (keyless, substrate-absent — survives).
- `- [x]` S-522 _get_platform_display_name — merged into S-517 fn.
- `- [x]` S-523 start — locale capture + spawn + with_locale (U-050); idempotent (`test_start_idempotent`).
- `- [x]` S-524 stop — drop tx + timeout-join + final log; flush-on-stop proven.
- `- [x]` S-525 add_activity — DO_NOTHING skip before enqueue + counters (producer side).
- `- [x]` S-526 add_activity_from_dict — event_type skip + Python-identical field defaults.
- `- [x]` S-527 _worker_loop — recv→per-platform buffer→threshold flush (batch-at-5 proven). *(snapshot-init gap contributes to S-530 FAIL)*
- `- [x]` S-528 _send_batch_activities — combined_text "\n".join + extend_from_text + counters; retry `[≠]` legal.
- `- [x]` S-529 _flush_remaining — channel-closed drain flushes sub-5 leftovers per platform (no loss).
- `- [~]` **S-530 get_stats — UNPROVEN / DIVERGENT.** Top-level key set byte-exact, but `buffer_sizes` nested map omits the guaranteed `twitter`/`reddit` keys in the empty/skip-only state (`{}` vs `{"twitter":0,"reddit":0}`).

### VERDICT: **FAIL** — 15/16 symbols `- [x]`/`- [≠]` (all `[≠]` survived the challenge), **S-530 stays `- [~]`**.

**Routed back to porter — the precise missing behavior:** seed `buffer_snapshot` with the two initial platforms at `start()` (or in `new`) so `get_stats().buffer_sizes` reports `{"twitter":0,"reddit":0}` from the first call — matching the source's `__init__`-seeded `_platform_buffers`. Two faithful options: (a) initialize `buffer_snapshot` to `{"twitter":0,"reddit":0}` when constructing it; OR (b) call `update_buffer_snapshot(&platform_buffers, &buffer_snapshot)` once at worker startup (before the recv loop) so the seeded twitter+reddit buffers (graph_memory.rs:1166-1171) are reflected immediately. Add a parity test asserting `buffer_sizes` contains `twitter` and `reddit` keys (both 0) immediately after `start()` with zero activities. Re-verify → flip S-530 to `- [x]`; then the unit may PASS.

**Unit ledger:** U-021 stays `- [~]` (sub-cycle (b) not yet clean; sub-cycle (c) `ZepGraphMemoryManager` also remains). No commit on this unit.

---

## 2026-06-17 — U-021 sub-cycle (b) S-530 — **FIX RE-VERIFIED** — VERDICT: **PASS** (16/16 symbols covered)

**Verifier:** rust-port-parity-verifier (differential re-verification of the single FAIL above).
**Worktree:** `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch `port/mirofish`).
**Source:** `MiroFish/backend/app/services/zep_graph_memory_updater.py` (L252-255 `__init__` seed, L376-377 dynamic-platform add, L463 `get_stats` comprehension).
**Rust:** `src/services/graph_memory.rs` (`new` L956-975, `get_stats` L1129-1143, `worker_loop` L1162-1224).

### The fix (matches the precise routed-back instruction, option a)

The porter implemented **option (a)** from the prior FAIL: `GraphMemoryUpdater::new` now pre-seeds the buffer-size snapshot at construction —
```
let mut initial_snapshot = HashMap::new();
initial_snapshot.insert("twitter".to_string(), 0usize);
initial_snapshot.insert("reddit".to_string(), 0usize);
... buffer_snapshot: Arc::new(Mutex::new(initial_snapshot)),
```
(graph_memory.rs:962-973). The worker's own `platform_buffers` was already seeded twitter+reddit (L1173-1178). `get_stats` clones this snapshot (L1130).

### Re-verification (each point independently confirmed)

1. **`buffer_sizes` ALWAYS contains `twitter`+`reddit` (=0 empty) from construction — CONFIRMED.** Snapshot seeded in `new()` BEFORE any worker activity → `get_stats().buffer_sizes` returns `{"twitter":0,"reddit":0}` from the very first call. Byte-faithful to source: `__init__` seeds `_platform_buffers={'twitter':[],'reddit':[]}` (L252-255); `get_stats` builds `{p:len(b) for p,b in self._platform_buffers.items()}` (L463) over that seeded dict. The prior divergence (`{}` vs `{"twitter":0,"reddit":0}`) is **resolved**.

2. **Dynamic non-twitter/reddit platform path UNBROKEN — CONFIRMED.** Worker L1185 `platform_buffers.entry(platform.clone()).or_default().push(activity)` still ADDS a novel platform's key on first activity — mirrors source L376-377 (`if platform not in self._platform_buffers: self._platform_buffers[platform]=[]`). The seeding only *adds* the two initial keys; it does not gate or shadow the dynamic insert. Proven by `test_buffer_sizes_third_platform_adds_key` (discord→key added =1, twitter/reddit remain =0).

3. **3 new regression tests RUN + PASS + assert the right thing — CONFIRMED (by name, not filtered):**
   - `test_buffer_sizes_seeded_twitter_reddit_at_start` ... **ok** — both keys present AND =0 immediately after `start()`, zero activities.
   - `test_buffer_sizes_seeded_after_do_nothing_activities` ... **ok** — only DO_NOTHING + event_type activities (which are skipped before enqueue and never reach the worker) → both keys still present =0.
   - `test_buffer_sizes_third_platform_adds_key` ... **ok** — discord activity adds `discord`=1 while twitter+reddit remain present =0.
   `test result: ok. 69 passed; 0 failed` (services::graph_memory suite). Assertions verified to test the correct contract (key-presence + zero-value + dynamic-add-without-loss).

4. **No regression to the rest of (b) — CONFIRMED.** Fix is localized to snapshot seeding in `new()`. Full suite: **871 passed, 6 ignored, 0 failed** (lib 860 + bins 4+3+4) — grew exactly +3 from the prior 868 (the 3 new tests); all prior (b) tests (batch-at-5, per-platform, flush-remaining, combined_text join, DO_NOTHING/event_type skip, get_stats key set, locale, idempotent start) still green.

### Symbol roll-up (S-515..S-530, 16 rows) — FINAL

- `- [x]` S-515 GraphMemoryUpdater struct
- `- [x]` S-516 BATCH_SIZE=5
- `- [x]` S-517 PLATFORM_DISPLAY_NAMES (PORTED, 世界1/世界2, log-observable)
- `- [≠]` S-518 SEND_INTERVAL — network rate-limit, non-contractual. Survives `[≠]` challenge.
- `- [≠]` S-519 MAX_RETRIES — redundant (adapter `call_api` already retries); failed_count+continue PORTED. Survives.
- `- [≠]` S-520 RETRY_DELAY — network backoff cadence, non-contractual. Survives.
- `- [x]` S-521 __init__/new — constructor + **buffer-snapshot seed (the fix)**; ZEP_API_KEY `[≠]` keyless substrate-absent.
- `- [x]` S-522 _get_platform_display_name
- `- [x]` S-523 start — locale capture + spawn + with_locale (U-050); idempotent.
- `- [x]` S-524 stop — drop tx + timeout-join + final log.
- `- [x]` S-525 add_activity — DO_NOTHING skip before enqueue + counters.
- `- [x]` S-526 add_activity_from_dict — event_type skip + Python-identical defaults.
- `- [x]` S-527 _worker_loop — recv→per-platform buffer→threshold flush; snapshot seeded twitter+reddit. *(snapshot-init gap now closed)*
- `- [x]` S-528 _send_batch_activities — combined_text "\n".join + extend + counters; retry `[≠]` legal.
- `- [x]` S-529 _flush_remaining — channel-closed drain flushes sub-5 leftovers per platform (no loss).
- `- [x]` **S-530 get_stats — NOW PASS.** Top-level key set byte-exact AND `buffer_sizes` nested map now reports `{"twitter":0,"reddit":0}` from construction (was `{}`). Flipped `- [~]` → `- [x]`.

**Final tally: 13 `- [x]` + 3 `- [≠]` (all survived the challenge) = 16/16 covered. Zero `- [~]`, zero disguised-skip `[≠]`.**

### VERDICT: **PASS**

The single observable divergence from the prior FAIL is resolved by a faithful, localized fix; no other (b) behavior regressed; 871 tests green. **Sub-cycle (b) is parity-clean.** S-530 flips to `- [x]`.

**Unit ledger:** U-021 still gated on sub-cycle (c) `ZepGraphMemoryManager` (S-531+) — sub-cycle (b) is now PASS but the unit cannot flip `- [x]` until (c) is verified. No change to ledger/symbol-map made here beyond this parity.md trail (per instruction).

---

## 2026-06-17 — U-021 sub-cycle (c) `ZepGraphMemoryManager` → `GraphMemoryManager<L>` (S-531..S-539)

**Verifier:** rust-port-parity-verifier · **Worktree:** `.worktrees/mirofish-port/teri` (branch `port/mirofish`)
**Source:** `MiroFish/backend/app/services/zep_graph_memory_updater.py` L479-554
**Rust:** `src/services/graph_memory.rs` L1886-2065 (struct + impl) + `manager_tests` L2071-2327

### Map-onto adjudication (class-level singleton → instance struct)

Python's `ZepGraphMemoryManager` is a class-level singleton: `_updaters` (class dict), `_lock` (class `threading.Lock`), `_stop_all_done` (class bool). Rust maps to an INSTANCE struct `GraphMemoryManager<L>` held in app state (e.g. `Arc<…>`). **Ruling: faithful map-onto, NOT a behavior change.** The mapping is *forced* — Rust has no generic statics, and `LlmClient` is not dyn-safe (the type is `GraphMemoryManager<L>`, generic over the client), so a class-level static keyed singleton is inexpressible. The observable contract is preserved exactly: ONE registry per process (one struct instance held in shared state), keyed by `simulation_id`, with idempotent `stop_all` as the U-049 cleanup entry point. This is a `[≠]`-class substrate adaptation of the *holder* — the per-method behavior is fully ported (so the methods themselves are `- [x]`, not `- [≠]`).

### S-538 `stop_all` — highest-stakes (U-049 cleanup + idempotency). VERIFIED.

(a) **Idempotent, no re-stop.** Rust checks `stop_all_done.compare_exchange(false,true,AcqRel,Acquire)` and `return`s on `Err` BEFORE acquiring the Mutex — exactly mirroring Python's `if cls._stop_all_done: return` (L534) which also tests before `with cls._lock` (L538). The flag flips on the FIRST call only; the second call short-circuits and never touches the (now-empty) map. Confirmed by `test_stop_all_idempotent`: registry empty after call 1, still empty after call 2, no panic.

(b) **One failing updater does NOT abort the rest.** Read `GraphMemoryUpdater::stop` (L1017-1044): returns `()` (not `Result`), no `?`, no early-return; it does `store`/`take`/`let _ = timeout(handle).await`/counter-loads/`info!`. A panicking *worker task* surfaces as `Err(JoinError)` from `handle.await` and is **swallowed by `let _ =`** — it cannot propagate into `stop_all`. The `stop_all` loop (L2029-2038) calls `updater.stop().await` directly with **no `?`** inside `for (id, mut updater) in updaters.drain()`. Adversarial mental test (3 updaters, middle one "errors" on stop): since `stop()` is infallible/non-panicking by construction, all 3 stop and the loop completes. Even in the pathological panic case, `drain()`'s `Drain` guard removes all remaining entries on drop, so the **map ends empty regardless**. This is the faithful Rust equivalent of Python's per-iteration `try/except … continue` (L541-544) — Rust achieves catch-log-continue by making `stop()` infallible+self-logging rather than wrapping each call.

(c) **Map ends empty.** `drain()` empties the HashMap (equivalent to Python `cls._updaters.clear()`, L545). Confirmed by `test_stop_all_clears_registry` (3 updaters → empty) and `test_stop_all_idempotent`.

→ **S-538 `- [x]`.**

### S-534 `create_updater` — VERIFIED. Stop-old-FIRST ordering preserved.

L1945-1964: locks map; `if let Some(old) = updaters.get_mut(id) { old.stop().await }` BEFORE constructing/starting/inserting the new updater — exact match to Python L503-504 (`if id in _updaters: _updaters[id].stop()`) then construct+start+insert (L506-508). No leaked old worker. **Return-type ruling:** Python returns the updater (L511); Rust returns `Result<(), TeriError>`. The updater holds a `JoinHandle` behind the Mutex and is not `Clone`; returning a `&`/owned instance out of the guard is impossible. **No caller depends on the returned instance** — the access path is `get_updater`/`get_all_stats` (registry-mediated). `Result<()>` is a faithful access-path adaptation (the `[≠]`-class signature change documented in the symbol-map row), **not a downgrade**. Confirmed by `test_create_updater_registers` (Some + running) and `test_create_updater_replaces_existing` (new graph_id "graph-b", total_activities=0 → old was genuinely replaced, not appended).

→ **S-534 `- [x]`.**

### S-535 `get_updater` — VERIFIED. Presence + stats is the faithful observable.

L1978-1987: returns `Option<UpdaterStats>` (snapshot) vs Python's `Optional[ZepGraphMemoryUpdater]`. **Ruling:** no caller needs the *live* updater object (the producer hot-path uses the updater handle held elsewhere; the manager's read surface is existence + stats). Returning a `&`/`&mut` through the `tokio::sync::Mutex` would force the caller to hold the lock for its entire use — a deadlock footgun. `None` vs `Some(_)` maps presence directly; `UpdaterStats` carries the readable state. Faithful composable equivalent (`[≠]`-class signature, documented). Confirmed by `test_get_updater_absent_returns_none` (None for unregistered) and `test_create_updater_registers` (Some after create).

→ **S-535 `- [x]`.**

### S-536 `stop_updater` — VERIFIED.

L1995-2001: `updaters.remove(id)` → if `Some(mut updater)` stop + log; if `None` no-op. Matches Python L521-525 (`if id in _updaters: stop(); del`). Confirmed by `test_stop_updater_removes` (present after, absent after stop) and `test_stop_updater_absent_is_noop` (no panic, registry stays empty). → **S-536 `- [x]`.**

### S-539 `get_all_stats` — VERIFIED.

L2051-2058: one entry per updater, each value = `updater.get_stats().await`; key set = registry keys. Matches Python dict-comprehension L551-553. Confirmed by `test_get_all_stats_returns_all_entries` (len==2, contains sim-a/sim-b, graph_id per entry). `get_stats` (S-530, already `[x]`) is the per-entry value. → **S-539 `- [x]`.**

### S-531/532/533/537 — struct + fields. VERIFIED.

- **S-531** struct `GraphMemoryManager<L>` (L1904-1913) — the class→instance holder. `- [x]`.
- **S-532** `updaters: tokio::sync::Mutex<HashMap<String, GraphMemoryUpdater<L>>>` — port of `_updaters`. `- [x]`.
- **S-533** `_lock` folded into the Mutex. The separate `threading.Lock` has no observable beyond mutual exclusion, which the `tokio::sync::Mutex` provides. `tokio::sync::Mutex` (not `std::Mutex`) is **required** because guards are held across `.await` points (`old.stop().await`, `get_stats().await` while iterating). Nothing observable dropped. `- [x]`.
- **S-537** `stop_all_done: AtomicBool` with `compare_exchange(AcqRel, Acquire)` — port of `_stop_all_done` check-before-lock. `- [x]`.

### Tests reviewed (9 manager_tests) — all assert the right things, none weak.

`test_stop_all_idempotent` asserts empty after both calls (the no-double-stop contract — confirmed by the AtomicBool short-circuit, and no-double-stop is structurally guaranteed since the map is already empty on call 2). `test_create_updater_replaces_existing` asserts the NEW updater's graph_id="graph-b" and total_activities=0 — proving the old was stopped+replaced (it added an activity to the old one; the new reads zero), i.e. it genuinely verifies replace-stops-old, not a weak presence check. No weakly-asserting test found. **Gap noted (non-blocking):** no test injects a *failing* `stop()` to exercise per-updater error isolation directly — but that isolation is structurally guaranteed (S-538(b): `stop()` is infallible/non-panicking, no `?`, `drain()` clears regardless), so the contract holds without a dedicated test.

### Run

`cargo test graph_memory` → **78 passed, 0 failed.** The 9 `services::graph_memory::manager_tests::*` confirmed present and passing (listed via `--list`).

### Tally

S-531 `- [x]` · S-532 `- [x]` · S-533 `- [x]` · S-534 `- [x]` · S-535 `- [x]` · S-536 `- [x]` · S-537 `- [x]` · S-538 `- [x]` · S-539 `- [x]` → **9/9 covered, zero `- [~]`, zero `- [≠]`** (all 9 methods/fields are fully ported; the holder's class→instance and the two return-type adaptations are forced/inexpressible signature shapes, documented, with behavior fully preserved — they do NOT downgrade observable contract, so the symbols are `[x]`, not skip-`[≠]`).

### VERDICT: **PASS** — and **U-021 COMPLETE**

Sub-cycle (a) S-493..S-514 = `[x]` (verified prior). Sub-cycle (b) S-515..S-530 = 13 `[x]` + 3 `[≠]` (verified above, parity-clean). Sub-cycle (c) S-531..S-539 = 9 `[x]` (this block). **All S-493..S-539 are now `[x]`/`[≠]` (every `[≠]` survived the challenge). The unit ledger U-021 may flip `- [x]` and commit.** No ledger/symbol-map edits made here beyond this parity.md trail (per instruction).

---

## 2026-06-17 · U-020 sub-cycle (a) · IPC protocol types (`CommandType`/`CommandStatus`/`IPCCommand`/`IPCResponse`) — S-453..S-476

**Verdict: PASS** — PURE PORT, byte-exact JSON parity verified line-by-line against `simulation_ipc.py` L25-92 + by running tests. 24/24 symbols (S-453..S-476) → `- [x]`. No `- [≠]` in this sub-cycle (transport `[≠]` belongs to sub-cycle b). U-020 stays `- [~]` (sub-cycle b Client/Server not yet ported).

**Method:** read both sides in full + DECISION-15 + symbol-map rows; ran `cargo test simulation_ipc` (21 unit pass, 0 fail) + `cargo test --doc simulation_ipc` (2 doctests pass). Module wired: `lib.rs:16 pub mod services` → `services/mod.rs:12 pub mod simulation_ipc`. `serde_json` has `preserve_order` (Cargo.toml:35) — confirmed by-reading AND the key-order tests pass by-running.

**Differential verification (source-line → rust):**

1. **Enum `.value` strings — PASS.** `CommandType` py L27-29 `"interview"`/`"batch_interview"`/`"close_env"` ; `CommandStatus` py L34-37 `"pending"`/`"processing"`/`"completed"`/`"failed"`. Rust uses `#[serde(rename_all="snake_case")]` AND an explicit `as_str()` (rs L75-81, L116-123). Both serde serialization (`command_type_serde_all_variants`, `command_status_serde_all_variants` assert exact quoted lowercase strings, NOT `"Interview"` Debug) AND `as_str()`/to_dict emission verified. Round-trip parse asserted in the serde tests. `to_dict` uses `.as_str()` (rs L167, L291), not enum Debug. **By-running + by-reading.**

2. **to_dict key order + null-not-omitted — PASS.** `IPCCommand::to_dict` (rs L159-175) builds `Map::new()` + 4 sequential inserts → `command_id, command_type, args, timestamp` (py L49-54 exact order). `IPCResponse::to_dict` (rs L283-313) → 5 inserts `command_id, status, result, error, timestamp` (py L76-81). result/error `None` → `Value::Null` (rs L294-308), key ALWAYS present. Tests assert: `obj.len()==4`/`==5`, `obj.keys()` exact ordered vec, AND `serialised.contains("\"result\":null")` / `"\"error\":null"` (ipc_response_to_dict_null_not_omitted L617-656), AND serialised-string key order (ipc_command_to_dict_serialised_key_order, ipc_response_to_dict_serialised_key_order). preserve_order confirmed driving real Map ordering. **By-running + by-reading.**

3. **from_dict required-vs-tolerant split — PASS, matches Python exactly.**
   - `IPCCommand::from_dict` (rs L187-241): `command_id` REQUIRED → `.get(...).ok_or_else(Err)` mirrors py L59 `data["command_id"]` (KeyError). `command_type` REQUIRED + unknown-string → `Err` mirrors py L60 `CommandType(...)` (ValueError). `args` tolerant `.unwrap_or_default()` → `{}` mirrors py L61 `.get("args",{})`. `timestamp` tolerant → `python_isoformat_local()` mirrors py L62 `.get("timestamp", now)`. Tests cover: missing command_id → Err, unknown command_type → Err, absent args → empty map, absent timestamp → non-empty default, all-fields, round-trip.
   - `IPCResponse::from_dict` (rs L328-390): command_id + status REQUIRED (py L87-88); result/error `.get(...)` → `None` mirrors py L89-90 `.get("result")`/`.get("error")`; JSON `null` ALSO → `None` (`.and_then(as_object/as_str)` drops null) — asserted by `ipc_response_from_dict_null_result_is_none` L707-718. timestamp default now. Tests cover missing command_id → Err, unknown status → Err, absent + null optional fields → None.
   - **Faithful error mapping:** porter returns `TeriError::Sim` on missing/bad. Sibling `Project::from_dict` (project.rs:242-267) uses the SAME required-`.get().ok_or_else(Err)` / tolerant-`.unwrap_or` pattern (it uses `TeriError::Config` because it is a config/model concern; `TeriError::Sim` is the correct sibling for a `services/` simulation type). The required-vs-tolerant boundary is identical to the established teri from_dict pattern. A Rust port that made `args` required or `command_id` tolerant would be a divergence — neither occurred.

4. **timestamp default — PASS.** Both default to `crate::models::project::python_isoformat_local` (rs L44, L233, L381) — the SAME `pub(crate)` helper teri uses for `datetime.now().isoformat()` (project.rs:50-58: local naive, microseconds omitted when zero, no TZ). Not a divergent format.

5. **args/result types — PASS, no narrowing.** Python `Dict[str,Any]` → `serde_json::Map<String,Value>` (rs L147, S-465). `Optional[Dict]` → `Option<Map<String,Value>>` (rs L266, S-472). `Optional[str]` → `Option<String>` (rs L268). The `result`/`args`/`error` producers in the source (`send_success` typed `result: Dict[str,Any]`, py L380) only ever emit dict/str — a non-dict `result` is outside the protocol contract (would be a producer bug in Python too), so `.and_then(as_object)` collapsing a non-object to None is faithful to the contractual dict-or-null shape, NOT a narrowing of any value the protocol produces. (Producers live in sub-cycle b; the contract boundary here is dict-or-null.)

**DECISION-15 alignment:** sub-cycle (a) is declared a PURE PORT, transport-agnostic, NO substrate decision. All five differential dimensions above match the DECISION-15 spec (4-key/5-key order, null-not-omitted, ensure_ascii=False ⇒ serde default non-escaping, python_isoformat_local, preserve_order). The file-transport `[≠]` is explicitly deferred to sub-cycle (b) — none of it leaks into (a).

**Symbols (24/24 → `- [x]`):** S-453 S-454 S-455 S-456 S-457 S-458 S-459 S-460 S-461 S-462 S-463 S-464 S-465 S-466 S-467 S-468 S-469 S-470 S-471 S-472 S-473 S-474 S-475 S-476 — all `- [x]`, zero `- [~]`, zero `- [≠]`.

### VERDICT: **PASS** — sub-cycle (a) done. S-453..S-476 = `[x]`. **U-020 stays `- [~]`** (sub-cycle b `SimulationIPCClient`/`SimulationIPCServer` S-477..S-492 remains). Symbol-map updated for these 24 symbols only; unit ledger NOT flipped.

---

## 2026-06-17 — U-020 sub-cycle (b): `SimulationIPCClient` + `SimulationIPCServer` (S-477..S-492) — opus PARITY GATE

**Unit:** U-020 sub-cycle (b). **Class:** map-onto-substrate (file-based subprocess IPC → in-process tokio mpsc+oneshot, DECISION-16; substrate LOCKED in DECISION-15).
**Source:** `MiroFish/backend/app/services/simulation_ipc.py:95-395` (read in full).
**Rust:** `src/services/simulation_ipc.rs:797-1797` (appended client/server + `IpcEnvelope` + `channel()` + `ipc_transport_tests`).
**Test run (from worktree):** `cargo test simulation_ipc` → **35 passed, 0 failed** (14 new ipc_transport_tests + 21 existing protocol tests in the same file).

### Differential parity — PORTED contract behaviors (all MATCH)

| Behavior | Source (Python) | Rust port | Verdict |
|---|---|---|---|
| `send_command` command_id | `str(uuid.uuid4())` | `Uuid::new_v4().to_string()` | MATCH (test asserts response.command_id parses as UUID) |
| timeout = REAL elapsed await | wall-clock `while time.time()-start < timeout` busy-poll | `tokio::time::timeout(timeout, reply_rx).await` | MATCH (test: undraining server + 50ms timeout → Err) |
| timeout defaults | interview 60 / batch 120 / close_env 30 | `INTERVIEW_TIMEOUT=60s`, `BATCH_INTERVIEW_TIMEOUT=120s`, `CLOSE_ENV_TIMEOUT=30s` consts; methods take `Duration` (effective defaults preserved per DECISION-16 §16.3 note) | MATCH |
| timeout → error | `raise TimeoutError("等待命令响应超时 (N秒)")` | `Err(TeriError::Sim("等待命令响应超时 ({:.0}秒)"))` | MATCH (Chinese prefix + parenthesized seconds preserved). NOTE: float→int render (`60.0秒`→`60秒`) is COSMETIC display text only — surfaced via `str(e)` into user-facing JSON `api.interviewTimeout`, never parsed/matched by any consumer (verified across `simulation.py`). Non-contractual; not a downgrade. |
| `send_interview` args + conditional platform | `{agent_id,prompt}` + `if platform: args["platform"]=platform` | `Map{agent_id,prompt}` + `if let Some(p)=platform { insert }` | MATCH (2 tests: with-platform → 3 keys incl. "platform"; no-platform → exactly 2 keys, no "platform") |
| `send_batch_interview` args + conditional platform | `{interviews}` + conditional platform | `Map{interviews}` + conditional platform | MATCH (2 tests: with platform="reddit" present; without → no platform key) |
| `send_close_env` args | `{}` | empty `Map` | MATCH (test asserts CommandType::CloseEnv received) |
| `send_success` → response | `IPCResponse{status=COMPLETED, result=Some}` | `IPCResponse{status=Completed, result=Some(result), error=None}` | MATCH (test: result {score:99}, error None) |
| `send_error` → response | `IPCResponse{status=FAILED, error=Some}` | `IPCResponse{status=Failed, error=Some, result=None}` | MATCH (test: error "something went wrong" / "agent error", result None) |
| command_id round-trip into response | `send_success/error(command_id, …)` | `command_id = envelope.command.command_id` echoed into response | MATCH (§16.4; correlation now automatic via embedded oneshot, but command_id retained for protocol + the 发送/收到 log lines) |
| FIFO oldest-first | mtime-sorted dir scan (oldest first) | `mpsc.try_recv()` preserves send order | MATCH (test: send "first" then "second" → poll yields "first" then "second") |
| liveness `check_env_alive` | reads `env_status.json` `status=="alive"` | reads shared `Arc<AtomicBool>` set by start/stop | MATCH (test: false before start → true after start() → false after stop()) |
| Client `Clone` / multi-sender | many Flask routes write one `ipc_commands/` dir | `#[derive(Clone)]` on Sender-backed client | MATCH (test: `client.clone()` + `tokio::join!` two concurrent sends, both Ok) |
| `poll_commands` empty | returns `None` (no files) | `rx.try_recv().ok()` → `None` | MATCH (test) |
| send/receive log lines | `logger.info("发送IPC命令…")` / `logger.info("收到IPC响应…")` | `info!("发送IPC命令…")` / `info!("收到IPC响应…")` | MATCH (preserved verbatim) |

### `[≠]` adjudications — CHALLENGED, all SURVIVE (genuinely inexpressible on the LOCKED in-process substrate; none are feature-skips)

The entire file-transport mechanism exists ONLY to cross the OASIS-subprocess↔Flask OS-process boundary. DECISION-15 LOCKED teri's substrate as in-process (OASIS subprocess → in-process SimEngine), eliminating that boundary. The mpsc+oneshot delivers the SAME observable protocol (command type+args → matching IPCResponse, or timeout). Precedent: U-007/U-016 Zep-network→petgraph adjudicated identically. Per-artifact:

- **ipc_commands/ + ipc_responses/ dirs + os.makedirs** — FS channel between 2 processes; one process → no boundary. Same delivery via mpsc. **INEXPRESSIBLE.**
- **env_status.json file + `_update_env_status` timestamp (S-488)** — cross-process liveness signal. CHALLENGED HARDEST: traced ALL consumers. The timestamp field IS read by `get_env_status_detail` — but that lives in `simulation_runner.py` = **U-022 (separate, unported unit, all `- [ ]`)**, and it reads the cross-process file the *subprocess* owns. The rich payload (twitter_available/reddit_available/timestamp) is written by the simulation SCRIPTS (`run_parallel_simulation.py:249-253`, `run_twitter/reddit_simulation.py` = U-028/U-029/U-030), NOT by `SimulationIPCServer._update_env_status` (which writes only `{status,timestamp}` as a fallback). Within U-020's scope, NOTHING consumes the timestamp; the file is purely the cross-process delivery of a boolean now shared in-memory. When U-022 is ported it already delegates liveness to `SimulationIPCClient.check_env_alive()` (runner L1388). **INEXPRESSIBLE within U-020; not a disguised skip.**
- **os.remove cleanup (command/response files)** — reclaim files post-delivery; mpsc consumes the envelope, oneshot self-consumes. Nothing to clean. **INEXPRESSIBLE.**
- **mtime-ordered directory scan** — imposes oldest-first over an unordered FS dir; mpsc is ALREADY FIFO, so oldest-first is PRESERVED (the observable), only the mechanism is moot. **INEXPRESSIBLE (observable preserved).**
- **poll_interval (0.5s)** — FS re-scan cadence; a channel wakes the awaiter immediately. The OBSERVABLE (timeout as real elapsed await) is preserved. **INEXPRESSIBLE (observable preserved).**
- **JSONDecodeError-retry-on-partial-file** — defends a half-written-file read race; in-process values move whole, never partially observable. **INEXPRESSIBLE (file-race artifact).**
- **S-488 `_update_env_status` method-folding** — folded into start()/stop() AtomicBool stores. Observable liveness (alive on start, stopped on stop, readable by check_env_alive) IS preserved (test-confirmed). Only the file write + timestamp (file artifacts, no in-process U-020 consumer) are dropped. **VALID fold — no observable lost.**

### Symbols (16/16) — S-477..S-492

- S-477 `SimulationIPCClient` (type, Clone) → `[x]`
- S-478 `__init__`→`channel(buffer)` factory → `[x]` (dirs/makedirs `[≠]`, inexpressible)
- S-479 `send_command` → `[x]` (uuid v4, real timeout await + faithful 等待命令响应超时 message, log lines; poll_interval/file-IO `[≠]`)
- S-480 `send_interview` → `[x]` (conditional platform key verified)
- S-481 `send_batch_interview` → `[x]` (conditional platform key verified)
- S-482 `send_close_env` → `[x]` (empty args, CloseEnv type verified)
- S-483 `check_env_alive` → `[x]` (AtomicBool; env_status.json read `[≠]`)
- S-484 `SimulationIPCServer` (type) → `[x]`
- S-485 `__init__`→`channel()` → `[x]` (dirs `[≠]`; running starts false)
- S-486 `start` → `[x]` (running.store(true); `_update_env_status("alive")` `[≠]`)
- S-487 `stop` → `[x]` (running.store(false))
- S-488 `_update_env_status` → `[≠]` (folded into start/stop AtomicBool; file write + timestamp inexpressible cross-process artifact, no in-process U-020 consumer — CHALLENGE survived)
- S-489 `poll_commands` → `[x]` (try_recv FIFO oldest-first verified; mtime scan + JSONDecodeError-retry `[≠]`)
- S-490 `send_response` → `[x]` (oneshot fire; os.remove `[≠]`)
- S-491 `send_success` → `[x]` (Completed/result/command_id-echo verified)
- S-492 `send_error` → `[x]` (Failed/error verified)

15 `[x]` + 1 `[≠]` (S-488, challenge-survived) = **16/16 covered.**

### VERDICT: **PASS** — sub-cycle (b) done.
S-477..S-487, S-489..S-492 = `[x]`; S-488 = `[≠]` (genuinely inexpressible, CHALLENGE survived). Sub-cycle (a) S-453..S-476 already `[x]` (2026-06-17 PASS above). **All U-020 symbols S-453..S-492 are now `[x]`/`[≠]` ⇒ U-020 COMPLETE — unit ledger may flip `- [x]` and commit.** No protocol behavior lost (timeout defaults, conditional platform, FIFO, liveness, command_id all preserved + differentially tested). Symbol-map updated for S-477..S-492 only.

---

## 2026-06-17 — U-022 sub-cycle (a): run-state types (S-540..S-598, S-610/S-611) — **FAIL**

**Verifier:** rust-port-parity-verifier (opus, fail-closed). Method: golden differential — real CPython `to_dict/to_detail_dict` (dataclasses extracted from `simulation_runner.py`) vs real compiled-Rust `to_detail_dict`, byte-for-byte JSON diff; + full enumeration of `progress_percent` over all reachable round ratios.

### Confirmed divergence (downgrade) — `progress_percent` rounding
- **Symbol:** S-597 `SimulationRunState.to_dict` (computed key `progress_percent`).
- **File:line:** `src/services/simulation_runner.rs:482-484`.
- **Python (`simulation_runner.py:168`):** `round(current_round / max(total_rounds,1) * 100, 1)` — CPython `round()` is **round-half-to-even (banker's)** on the IEEE-754 value.
- **Rust:** `(raw_pct * 10.0).round() / 10.0` — `f64::round()` is **round-half-away-from-zero**.
- **Golden diff (identical inputs, real both sides):** `current_round=1, total_rounds=16` (raw 6.25) → **Python `6.2`**, **Rust `6.3`**. The two `to_detail_dict` JSON blobs are identical on all 25 keys/order/types EXCEPT this one line.
- **Scope of break:** 243 distinct `(current_round, total_rounds)` pairs in round counts 1..400 diverge — every `.x5` half-cent boundary (6.25→6.2/6.3, 31.25, 81.25, 1.25, 11.25, …). Reachable: `total_rounds = int(total_hours*60/minutes_per_round)` yields 16/80/etc. for ordinary configs; `current_round` walks 0→total, so boundaries ARE hit.
- **Why a downgrade (not a `[≠]`):** `progress_percent` is a contractual `to_dict` key served to the frontend (DECISION-17.1: "PORT exactly incl. the round(…,1) one-decimal rounding"). The Rust comment at L482 even STATES Python uses round-half-to-even, then fails to implement it. This is a portable-feature narrowing, not an inexpressible/non-contractual divergence.
- **Why tests missed it:** `to_dict_progress_percent_one_decimal_rounding` only tests `1/3 = 33.3` (a non-boundary). No `.5`-tie case is exercised.

**Fix routed to porter:** implement Python `round(x, 1)` semantics (round-half-to-even on the scaled value), e.g. via `rust_decimal` banker's rounding or an explicit half-to-even scale/round/unscale, and add a parity test covering 6.25→6.2, 0.25→0.2, 6.45→6.5, 0.05→0.1.

### Everything else: PASS (shape parity confirmed by golden diff)
- **RunnerStatus (S-541..S-549):** 8 variants, exact lowercase `.value` strings, `as_str`/`Display`/serde all match. ✓
- **AgentAction.to_dict (S-550..S-560):** 9 keys, exact order, `result: None`→`null`, `action_args` object. Byte-identical. ✓
- **RoundSummary.to_dict (S-561..S-570):** 9 keys, exact order, computed `actions_count=len(actions)`, nested `actions:[to_dict]`, `end_time: None`→`null`. ✓
- **SimulationRunState.to_dict (S-571..S-597):** 23 keys, exact insertion order matches Python L161-186; `runner_status` emits `.value` string; `total_actions_count` computed; all `Option`→`null` (started_at/completed_at/error/process_pid). ✓ (except progress_percent value, above)
- **to_detail_dict (S-598):** 25-key superset + `recent_actions:[to_dict]` + computed `rounds_count`. ✓
- **add_action (S-596):** front-insert, cap-50 truncation (drops the OLDEST = tail, matching Python `[:max]` since insert was at front), per-platform counter, updated_at refresh. ✓
- **Persistence (S-610/S-611):** `save_run_state` create_dir_all + 2-space pretty + raw UTF-8; `load_run_state` `.get(key,default)` tolerance, missing file→None, parse error→None. Round-trip verified. ✓

### `[≠]` adjudication (both CHALLENGE-survived)
- **S-540 `IS_WINDOWS`** — `[≠]` non-contractual CONFIRMED: used only to select `taskkill`/`killpg` in the terminate path; no observable output, no serialized field. teri's stop is OS-agnostic.
- **S-595 `process_pid` value** — `[≠]` value-only CONFIRMED: struct field + `to_dict` key PORTED (golden shows `"process_pid": null`); value is `null` (no OS subprocess) — faithful to Python's `None` before a process is spawned; no consumer contract broken (Optional[int]).

### Coverage
Shape-verified: S-541..S-594, S-596, S-598, S-610, S-611 (would be `[x]`). `[≠]` confirmed: S-540, S-595. **BLOCKED by S-597** (progress_percent value divergence) — leave S-597 `- [~]`. Because one in-scope symbol fails parity, the unit does NOT pass; symbols NOT flipped to `[x]` this cycle.

### VERDICT: **FAIL** — return to porter.
Single defect: S-597 `progress_percent` uses round-half-away-from-zero; Python uses round-half-to-even. Fix the rounding + add `.5`-tie parity test, then re-verify. All other S-540..S-611 symbols are shape-parity-clean and ready to clear once S-597 matches.

---

## 2026-06-17 · U-022 sub-cycle (a) — RE-VERIFY of S-597 rounding fix → `SimulationRunState` + persistence

**Verdict: PASS** (differential, empirical golden diff vs real CPython 3.14.4). Re-verification of the single prior FAIL (`progress_percent` round-half-away-from-zero vs CPython `round(x,1)` half-to-even). FIX CONFIRMED CORRECT; no regression.

**The fix under test:** `round_half_even_1dp` (`src/services/simulation_runner.rs:336-397`) now backs `progress_percent` in `SimulationRunState::to_dict()` (L568-574). Algorithm: `scaled=x*10`; `frac<0.5` down / `frac>0.5` up; on IEEE `frac==0.5`, decode mantissa bits and compare `mantissa*20` vs `(2n+1)*2^(-exp)` in u128 exact integer arithmetic → Less=down / Greater=up / Equal=half-to-even.

**Empirical golden diff (decisive evidence — did NOT take the porter's word):**
- Tool: real CPython 3.14.4 `round(x,1)` vs the verbatim Rust helper (compiled standalone, bit-exact hex-float inputs).
- **Full domain sweep: 82,828 values checked → 0 mismatches.** Covers every `(current_round,total_rounds)` raw percentage for total∈0..400 plus dense `.x5` tie batteries and named edges (6.25→6.2, 0.25→0.2, 6.45→6.5, 0.05→0.1, 0.15→0.1, 0.45→0.5, 31.25→31.2, 81.25→81.2, 2.55→2.5, 2.675→2.7, etc.). Every value matches CPython.
- Hex-float parser independently bit-verified (82,828/82,828 round-trip to identical IEEE bits via Python `struct`), so the diff cannot be masked by a parse error.

**Original 243-divergence set: 0 remaining.**
- Re-enumerated counts 1..400: exactly **243** pairs where CPython diverges from round-half-away-from-zero (reproduces the original FAIL figure precisely; first pair cr=1,tr=16 raw=6.25 → CPython 6.2, old Rust 6.3).
- New helper vs CPython on all 243: **243/243 match, 0 bad.** The exact issue the prior FAIL flagged is resolved.

**`[≠]` Less/Greater branch correctness (not just "unused-and-lucky"):**
- In the percentage domain, all 481 `frac==0.5` cases are TRUE exact ties (verified via `fractions.Fraction`), so they resolve via half-to-even only. To prove the exact-compare path is *correct* (helper is documented general/reusable), constructed 50 values where IEEE `x*10` lands on `n+0.5` but the true product differs (e.g. 0.05 GT→0.1, 0.15 LT→0.1, 0.35 LT→0.3, 0.45 GT→0.5). Rust: **50/50 match CPython, 0 bad.** Less/Greater logic is genuinely correct.

**Edge cases:** `0.0→0`, `100.0→100` (no panic); `NaN→NaN`, `±inf` pass through (non-finite guard); negatives via recursion match CPython (`-2.5→-2.5`, `-6.25→-6.2`, `-0.05→-0.1`, `-0.15→-0.1`). Overflow: exp range over percentages is −54..0, so the `exp>=0` `1u128<<exp` path shifts by at most 0 in-domain — no overflow risk for the contract.

**No regression / no collateral edits:** `git diff` shows only `src/services/mod.rs` (+1 line: `pub mod simulation_runner;`); `simulation_runner.rs` is the new sub-cycle (a) file. Rest of sub-cycle (a) unchanged and shape-correct vs `MiroFish/backend/app/services/simulation_runner.py`:
- RunnerStatus (8 variants, lowercase serde, `as_str`/`Display`), AgentAction (9-key `to_dict`, null result, serde roundtrip), RoundSummary (9-key `to_dict`, computed `actions_count`, nested actions, null `end_time`), SimulationRunState defaults + `add_action` (insert-at-front, cap=50, per-platform counter, `updated_at` refresh), `to_dict` (23-key order matches source dict literal; `total_actions_count` computed), `to_detail_dict` (25 keys = superset + `recent_actions` + `rounds_count`).
- Persistence S-610/S-611 (`load_run_state`/`save_run_state`): save via `to_detail_dict()` + indent=2 (verified 2-space pretty-print), load with `.get(k,default)` exact defaults (tolerates-missing-fields test), missing-file→None. Faithful to Python `_load_run_state`/`_save_run_state`.
- **Full `cargo test --lib` = 948 passed, 0 failed.** Module suite 44 passed; 7 dedicated `round_half_even_1dp*` parity-regression tests pass.

**`[≠]` challenge (both survive):**
- **S-540 `IS_WINDOWS`** — non-contractual: feeds only the `taskkill` vs `killpg` subprocess-terminate selection; teri stop is OS-agnostic (cooperative shutdown + `task.abort()`), no branch, no observable output. (Subprocess orchestration S-599+ is sub-cycle (b), still `- [ ]`.) Confirmed `[≠]`.
- **S-595 `process_pid` value** — value-only non-contractual: field AND `to_dict` key ARE ported (shape parity; `"process_pid": null` emitted, covered by `to_dict_null_optional_fields`); only the VALUE is always null (teri has no OS subprocess PID). JSON shape identical to source → not a skipped feature. Confirmed `[≠]` value-only.

**Symbols cleared to `- [x]` (60):** S-541..S-598 (RunnerStatus + variants S-542..549, AgentAction + fields/method S-550..560, RoundSummary + fields/method S-561..570, SimulationRunState + fields/methods S-571..598), S-610, S-611. **`[≠]` confirmed:** S-540, S-595 (value-only). S-599 (`SimulationRunner` orchestrator) correctly remains `- [ ]` — sub-cycle (b).

**Sub-cycle (a) coverage: 60/60 cleared + 2 `[≠]` confirmed → unit sub-cycle (a) PASS.**

---

## 2026-06-17 — U-022 sub-cycle (b) SimulationRunner LIFECYCLE — **FAIL** (parity-verifier, opus, fail-closed)

**Scope:** S-599,600,602,603,608,612,616,617,624,625,627 (`- [~]` ported sub-cycle b) + `[≠]` rows S-601/604/606/607 + `[→U-049]` S-626. Source: `/home/drdave/Desktop/meta/MiroFish/backend/app/services/simulation_runner.py`. Worktree: `.worktrees/mirofish-port/teri`.

**Gates (ran here):** `cargo test` → **980 passed, 6 ignored, 0 failed**. `cargo clippy --all-targets -- -D warnings` → exit 0. No build/lint regression. Diff is +179/-6 sim/mod.rs, +1018 simulation_runner.rs, +120 simulation_manager.rs, +34 symbol-map.

**High-risk claims 1 & 2 — VERIFIED PASS (no regression to U-015/U-018/U-048):**
- **Claim 1 (SimEngine prepare-phase restructure, sim/mod.rs 582-604):** the ONLY logic deletion is the 6-line `stream::iter(pool.agents.iter()).map(...).buffered(n).collect().await` chain, replaced by collecting `Box::pin(agent.prepare_action(&world, llm))` into a `Vec<Pin<Box<dyn Future+Send>>>` then `stream::iter(prepared).buffered(n).collect().await`. Behavior-identical: `prepare_action` is `&self` (no eager side effect; async-fn body is lazy, so building the Vec executes no agent logic), `.buffered(n)` unchanged → SAME ordered results + SAME concurrency degree; same `Vec<Result<Action>>` shape + same `?` propagation in phase 2 zip. Heap-pin is a `Send`-bound requirement for `tokio::spawn`, not a semantic change. 44 sim tests + graph/build integration green & unchanged.
- **Claim 2 (cooperative shutdown hook):** purely additive. `shutdown: Option<Arc<AtomicBool>>` defaults `None`; per-tick check is gated `if let Some(ref flag) && flag.load(Acquire)` — unreachable for all pre-existing callers. Static check: only NEW `start_simulation:1135` calls `with_shutdown`; no existing caller/test sets it. Graceful break falls through to the SAME post-loop completion path (history cloned, partial `total_ticks`, `completion_tx.send(Some(..))`, `Ok`) → does NOT tear U-048's subscribe_completion contract. New tests assert both (full loop when None/false; graceful empty-completion when pre-set).
- **Claim 3 (RunInputs seam):** FAITHFUL. Python start_simulation builds nothing — the engine/pool/graph are constructed inside the spawned `run_*.py` child; the Rust seam moves that construction to the caller (U-024/API) and start_simulation owns only spawn+register+stop. No observable in Python start_simulation is dropped: reject-if-running, config-load/missing-Err, total_rounds compute+truncate, STARTING-persist, graph-updater (require graph_id / fail→log+disable-not-abort), platform flags, RUNNING-persist, register — all present & ordered identically. `mark_state_json_stopped` is a faithful port of L1248-1259 (missing→no-op false; partial edit status+updated_at preserving all keys; 2-space indent; +cache-invalidation, a correct teri addition). All PASS.

**>>> TWO REAL DIVERGENCES / DOWNGRADES → FAIL <<<**

**FAIL-1 — `stop_simulation` grace window NARROWED 10s → 5s (S-616/S-617).**
- Source: `_terminate_process(cls, process, sim_id, timeout: int = 10)` (py L721). `stop_simulation` calls it WITHOUT a timeout arg (py L793) → **10s** grace before SIGKILL (`process.wait(timeout=10)`, L769). Only `cleanup_all_simulations` passes `timeout=5` (py L1224).
- Rust: BOTH `stop_simulation` (rs:1219) and `cleanup_all` (rs:1292) call the same `terminate_handle`, which uses a single `const TERMINATE_GRACE = Duration::from_secs(5)` (rs:850, used at rs:1406). So `stop_simulation`'s grace window is 5s, not 10s.
- Observable downgrade: a sim that yields gracefully between 5s and 10s is force-`abort()`ed under teri but allowed to finish under MiroFish. The symbol-map S-616 claim "The 5s grace-then-force WINDOW (`TERMINATE_GRACE`) is preserved exactly" is **factually wrong** (and its Python-description "SIGKILL after 5s" mislabels the 10s stop path). This is expressible, contractual, observable timing → NOT a legitimate `[≠]`; it is a quantitative narrowing.
- **Fix:** `terminate_handle` must take a grace duration (or two constants). `stop_simulation` → 10s; `cleanup_all` → 5s. Mirror py L793 (default 10) vs py L1224 (5). Add a test asserting the stop-path force-abort fires at ~10s, not 5s.

**FAIL-2 — `cleanup_all` CLOBBERS already-finished runs (S-625).**
- Source: in `cleanup_all_simulations`, ALL state recording — run_state.json STOPPED+error (py L1234-1241) AND the state.json secondary write (py L1244-1259) — is **inside `if process.poll() is None:`** (py L1219). A process that has already exited (poll() != None, e.g. a sim that COMPLETED normally) is skipped entirely: Python writes NOTHING for it.
- Rust: rs:1285-1320 writes `run_state.json` → `Stopped` + `error="服务器关闭，模拟被终止"` AND calls `mark_state_json_stopped` (flips state.json status→`stopped`) for EVERY drained handle, including `handle.is_finished()` ones (the `is_finished()` branch at rs:1286 only skips `terminate_handle`, NOT the state writes). The code comment claiming Python "always records the shutdown for tracked runs" is incorrect — Python gates it on still-running.
- Observable downgrade: a simulation that finished (status completed/stopped) before server shutdown has its final `run_state.json` overwritten to `runner_status=Stopped, error="服务器关闭，模拟被终止"` and its `state.json` status flipped to `stopped` — corrupting the historical record of completed runs. MiroFish preserves it untouched.
- **Fix:** guard the STOPPED-state + `mark_state_json_stopped` writes behind `!handle.is_finished()` (py `poll() is None`). Only running runs get the shutdown record. Add a test: a finished run in the map is drained but its run_state.json / state.json are NOT modified by cleanup_all.

**Notes (not blockers):**
- Prompt conflated S-624 (`_cleanup_done` flag, in scope, ported) with S-615 (`_check_all_platforms_completed`, dual-platform). S-615 is correctly left `- [ ]` (sub-cycle c, action-log dependent) — its absence here is NOT a FAIL.
- `[≠]` rows S-601/604/606/607 (SCRIPTS_DIR / _action_queues / stdout+stderr file handles) are genuinely inexpressible-substrate (no child process / no pipes in-process; no observable output) — confirmed legitimate. S-626 `[→U-049]` deferral (signal handlers are Flask-WSGI-specific; cleanup_all shipped as the callable U-049 invokes) — legitimate, not a drop.
- S-609 `get_run_state` is implemented (mem-cache-then-file) but its symbol-map row is `- [ ]` (not claimed for b). Internal use only here; left for its own clearing. Not a blocker.

**Verdict: FAIL — 0 of the 11 claimed sub-cycle (b) symbols flipped to `- [x]`.** S-616, S-617, S-625 carry real downgrades; the rest (S-599/600/602/603/608/612/624/627) are individually sound but the unit cannot PASS while sibling lifecycle symbols diverge (rollup rule). All `- [~]` rows remain `- [~]`. Route back to porter with FAIL-1 + FAIL-2 (both small, localized fixes in `terminate_handle` / `cleanup_all`). The SimEngine restructure + shutdown hook + RunInputs seam are CLEARED for re-verification once the two terminate/cleanup divergences are fixed.

---

## 2026-06-17 — U-022 sub-cycle (b) RE-VERIFICATION (FAIL-1 + FAIL-2 fixes) — opus, fail-closed — **PASS**

Re-verify ONLY the two fixes that FAILed the prior block. The three structural claims (SimEngine restructure / shutdown hook / RunInputs seam) + `mark_state_json_stopped` were already cleared; confirmed undisturbed (the two fixes are surgical and localized to `simulation_runner.rs`; `mark_state_json_stopped`'s body at `simulation_manager.rs:1441-1468` and `SimEngine::with_shutdown`/`run` are byte-unchanged by the fix).

### FAIL-1 fix — grace-window split — **PASS (parity confirmed)**
- Source: `_terminate_process(process, sim_id, timeout: int = 10)` (py:721). `stop_simulation` calls it with NO timeout arg (py:793) → default **10s** (`process.wait(timeout=10)`, py:769). `cleanup_all_simulations` calls `_terminate_process(process, sim_id, timeout=5)` (py:1224) → **5s**.
- Rust: `const STOP_GRACE = Duration::from_secs(10)` (rs:856), `const CLEANUP_GRACE = Duration::from_secs(5)` (rs:864). `terminate_handle(handle, sim_id, grace: Duration)` (rs:1436) is now parameterized; `stop_simulation` passes `STOP_GRACE` (rs:1234), `cleanup_all` passes `CLEANUP_GRACE` (rs:1326). The single collapsed `TERMINATE_GRACE=5s` that narrowed stop's 10s window is gone.
- Differential: the 5–10s band where stop tolerates a graceful exit but cleanup force-aborts is now correctly distinct. Both windows match their per-caller Python `timeout`.

### FAIL-2 fix — `cleanup_all` finished-run gate — **PASS (parity confirmed)**
- Source: in `cleanup_all_simulations` the ENTIRE record-keeping body — terminate (py:1220-1231) + `run_state.json` STOPPED+error write (py:1234-1241) + secondary `state.json` write (py:1244-1259) — is gated behind `if process.poll() is None:` (py:1219). A finished run (poll() is not None) is skipped: NOTHING is written for it. `_processes.clear()` (py:1282) still drains ALL entries regardless.
- Rust: `cleanup_all` (rs:1316-1321) now `if handle.is_finished() { continue; }` — the in-process analog of `poll() is not None`. A finished handle is skipped (no terminate, no `run_state.json` write, no `mark_state_json_stopped`) and its persisted state (COMPLETED, error=None) is left INTACT; it is still drained (`runs.drain()` at rs:1291, the `_processes.clear()` equivalent, removed it before the loop and it drops at end of iteration). A still-running handle: `terminate_handle(.., CLEANUP_GRACE)` + STOPPED/clear-flags/`completed_at`/error `服务器关闭，模拟被终止` → `run_state.json`, secondary `state.json` write via `SimulationManager::mark_state_json_stopped` (U-023), per-run catch-log-continue. Faithful.

### Regression tests (read — they PROVE the fix, not just compile)
- `terminate_grace_windows_match_python_defaults` (rs:2513): asserts `STOP_GRACE==10s`, `CLEANUP_GRACE==5s`, `STOP_GRACE != CLEANUP_GRACE`, `STOP_GRACE > CLEANUP_GRACE`. Directly proves FAIL-1.
- `cleanup_all_preserves_finished_run_state` (rs:2603): starts a 1-round run, waits until finished, writes COMPLETED `run_state.json` + `state.json="completed"`, runs `cleanup_all`, then asserts run_state stays **Completed**, **error stays None**, **completed_at untouched** (`2026-06-17T12:00:00`), **state.json stays "completed"** (secondary write did NOT fire), AND the run is **drained** (`get_running_simulations().is_empty()`). Exactly the FAIL-2 assertion set required.
- `cleanup_all_stops_running_but_skips_finished` (rs:2673): mixed run — proves the gate DISCRIMINATES (finished preserved COMPLETED/no-error/"completed"; running → Stopped + `服务器关闭` error + state.json "stopped"); both drained. Proves it is not all-or-nothing.
- `stop_completes_within_grace_window` (rs:2475): cooperative graceful stop returns well under `STOP_GRACE` (between-tick shutdown, not force-abort).
- `cleanup_all_is_idempotent` (rs:2540) + `cleanup_all_terminates_and_records` (rs:2550): empty-map silent return + flag flip; running-run terminate+record path.

### Edge cases
- Empty map + idempotency: `cleanup_done.compare_exchange(false,true,AcqRel/Acquire)` (rs:1279) succeeds once; second call returns on the flag. Empty drain + no updaters → silent return (rs:1294). Covered by `cleanup_all_is_idempotent`.
- Grace-boundary race: `tokio::time::timeout(grace, &mut task)` reaps the task on either branch (Ok=graceful, Err=abort+await-cancelled) — same race semantics as Python `process.wait(timeout)`; grace window is the only observable. No state corruption.

### Gates
- `cargo test`: **983 passed, 6 ignored** (5 suites) — no regression. Targeted `services::simulation_runner`: 60 passed; the 6 named lifecycle/regression tests: all pass.
- `cargo clippy --all-targets -- -D warnings`: **no issues found**.

### `[≠]` challenge (re-confirmed survives)
- S-601 `SCRIPTS_DIR` (locate `run_*.py`) — no scripts run in-process (`SimEngine::run`); no observable output. Inexpressible-substrate. ✓
- S-604 `_action_queues` (thread→thread `Queue`) — no second thread to hand off to in-process tokio. No observable. ✓
- S-606/607 `_stdout_files`/`_stderr_files` (drain child pipes) — no child process / pipes in-process. No observable. ✓
- S-626 `register_cleanup` — `[→U-049]`, NOT a drop: SIGTERM/SIGINT/SIGHUP/atexit installation is Flask-WSGI-specific; U-049 wires teri's `ctrl_c` graceful-shutdown to CALL the shipped `cleanup_all`. ✓

### Verdict
**PASS** — both FAIL-1 and FAIL-2 fixes are behaviorally faithful to the Python source and proven by genuine differential regression tests; 983 tests green; clippy clean; structural cleared code undisturbed.
- Flipped `- [~]`→`- [x]`: **S-599, S-600, S-602, S-603, S-608, S-612, S-616, S-617, S-624, S-625, S-627** (11 lifecycle symbols).
- Confirmed `[≠]`: S-601, S-604, S-606, S-607. Confirmed `[→U-049]`: S-626.
- Left `- [ ]` for sub-cycles c-f: S-605 (`_monitor_threads`, monitor task), S-615 (`_check_all_platforms_completed`), + monitor/reader/interview symbols.
- Sub-cycle (b) symbols verified: **11/11 cleared lifecycle (`- [x]`) + 4 `[≠]` + 1 `[→U-049]`** — the unit may proceed; (b) is PASS.

---

## 2026-06-17 — U-022 sub-cycle (c): simulation MONITOR + action-log offset-tail + graph-memory firing — **PASS**

**Verifier:** rust-port-parity-verifier (opus, fail-closed, default-skeptical)
**Scope:** S-605, S-613, S-614, S-615 (U-022) + S-1056 / U-047 realization. Touches U-021 `graph_memory.rs` (NEW `GraphMemoryManager::fire_activity_from_dict`) and the `RunHandle.state` → `Arc<tokio::sync::Mutex<…>>` structural change.

**Method:** `cargo test` → **999 passed, 6 ignored** (no regression to U-010/U-021/U-048/sim suites; matches baseline). `cargo clippy --all-targets -- -D warnings` → **clean**. Sub-cycle (c) tests run + pass explicitly (6 `read_action_log_*` + 12 monitor/gate/end/graph). Differential read of Python `_monitor_simulation` / `_read_action_log` / `_check_all_platforms_completed` (`backend/app/services/simulation_runner.py:482-718`), Python `add_activity_from_dict` (`zep_graph_memory_updater.py:340-362`), and the U-010 PRODUCER `backend/scripts/action_logger.py:43-116` vs teri `src/sim/action_logger.rs`.

### HIGH-RISK adversarial findings

1. **U-047 offset-tail invariants (S-614) — the crux — PASS.** `read_action_log` (`simulation_runner.rs:1704`): opens, `seek(Start(position))`, `read_to_end` the delta, then advances the offset ONLY past newline-terminated complete lines (`while let Some(rel_nl) = buf[start..].iter().position(|&b| b==b'\n')`); a trailing fragment is left unconsumed and the offset stops at the last complete line. Tests prove (a) no-re-read across polls, (b) partial line NOT consumed + offset stops at complete-line boundary + consumed exactly once when later newline-terminated, (c) no double-fire, (d) missing-file returns `position` unchanged + growth-between-polls + IO-error robustness.
   - **Offset vs Python `f.tell()`:** On writer-produced input the two are byte-for-byte identical. The U-010 producer (`PlatformActionLogger.log_action`, `action_logger.py:65-66` / `src/sim/action_logger.rs:125-135`) writes `json.dumps(entry) + '\n'` in ONE write per record, so the file at EOF is ALWAYS at a complete-line boundary → Python's `for line in f` never yields a fragment and `f.tell()`==EOF==end-of-last-complete-line==Rust's `new_position`. The test `read_action_log_reads_new_lines_and_returns_offset` asserts `off == file len`, confirming.
   - **Divergence assessment:** Rust's explicit newline-boundary is a STRICT SUPERSET of robustness vs the torn-write edge (won't parse/error on a half-flushed final line); identical observable on all writer inputs. NOT a downgrade.

2. **U-010 ↔ U-021 field-name mapping — PASS (no drift).** Cross-checked BOTH boundaries against the PRODUCER, not just claimed: producer writes `round` (not `round_num`), `agent_id`, `agent_name`, `action_type`, `action_args`, `result`, `success`, `timestamp` and NO `platform` key. Consumer `apply_log_record` (`simulation_runner.rs:1854-1880`) and `GraphMemoryUpdater::add_activity_from_dict` (`graph_memory.rs:1020-1052`) both map `round`→`round_num` and supply `platform` from the directory — 1:1, exactly as Python `_read_action_log` L665-674 and `add_activity_from_dict` L352-360. No silent empty/wrong-data path.
   - Note: producer `log_round_end` writes `actions_count` not `simulated_hours`; the consumer's `round_end` branch defaults `simulated_hours` to 0 when absent — this is a FAITHFUL port (Python's writer/consumer behave identically), parity-preserving, not a Rust gap.

3. **simulation_end → COMPLETED + FINAL read pass — PASS.** `monitor_simulation` (1585) breaks on the U-048 `completion_rx.borrow().is_some()` (correct `process.poll()` replacement; `watch` retains the final value), then does ONE final read pass (1638-1643). Test `monitor_loop_does_final_read_after_completion` proves an action written AFTER completion fires is still captured (no trailing-action loss, Python L518-522). Test `monitor_loop_already_completed_at_start_still_reads` proves no race for a late-subscribing monitor (DECISION-17).

4. **Dual-platform gate (S-615) — PASS.** `check_all_platforms_completed` (1918) is a line-by-line port of Python L706-718. Single-platform completes alone; dual requires BOTH (`check_completed_dual_requires_both`: twitter-only-done → still false → both-done → true); no-platform-enabled is false (not vacuously true).

5. **Graph-fire only when enabled, once per action — PASS.** `graph_fire_disabled_does_not_register_activities` (graph_enabled=false → manager untouched). `graph_fire_enabled_forwards_actions_to_updater`: 2 real actions + 1 DO_NOTHING + 1 event → `total_activities==2`, `skipped_count==1` — proving fire-exactly-once, the U-021 DO_NOTHING skip (`add_activity`, `graph_memory.rs:998`) is reached, and event_type records are filtered before enqueue.

6. **`RunHandle.state` → `Arc<Mutex>` structural change — PASS, no latent deadlock.** Every state-lock in the monitor path holds ONLY synchronous work (`save_run_state` is a sync `fn`, `check_all_platforms_completed` is sync, the field mutations are sync); the ONE `.await` after a state mutation (the graph-fire) is explicitly performed AFTER the guard is dropped (`apply_log_record` 1883-1900, `{ … } // lock dropped here`). No lock held across `await` anywhere. No nested locks (`get_run_state` 1007 takes the `runs` lock, clones the `state` Arc, DROPS `runs`, THEN locks the state mutex). `get_run_state` reads through the SAME Arc the monitor writes → monitor updates are observable (the point of the change). The 5 updated call sites (1015, 1240, 1263, 1368, 1186/1199) preserve sub-cycle (a)/(b) behavior — (b) lifecycle symbols stay `[x]`, full suite green.

7. **`[≠]` adjudication — CONFIRMED inexpressible, NOT a disguised skip.**
   - `daemon=True` OS-thread flag: genuinely inexpressible — a tokio task IS tied to the runtime; the OBSERVABLE "monitor dies with the run" is PORTED (`terminate_handle` aborts+awaits `RunHandle.monitor`, 1495-1498). No dropped observable.
   - non-zero `exit_code`→FAILED branch + `simulation.log` tail (Python L524-544): inexpressible — there is no OS exit code for an in-process `tokio::spawn(SimEngine::run)`. The COMPLETED-via-`simulation_end` success observable IS ported; run-failure is carried by the sim task's own error logging + the `Failed` transitions in `start_simulation`. Substrate-true, not "dest won't use it".
   - **Producer-side deferral is clean, not a hidden gap:** the CONSUMER monitor contract is faithful on its own — the U-010 producer (`PlatformActionLogger`, S-070..S-077) ALREADY exists and writes the matching field shape, and the monitor's missing-file path is a no-op exactly as Python's behavior when no log exists. (c) does not silently depend on anything unbuilt.

### Verdict
**PASS — U-022 sub-cycle (c) is parity-verified, no downgrade.** Symbols flipped `- [~]`→`- [x]`: **S-605, S-613, S-614, S-615**. **U-047 / S-1056 REALIZED + VERIFIED** (`- [~]`→`- [x]`). `[≠]` rows on S-613 confirmed (daemon flag + exit-code branch genuinely inexpressible). The orchestrator may proceed to the next sub-cycle / commit.

---

## 2026-06-18 — U-024 sub-cycle (b): `ReportTools<'g,L>` graph-tool methods (PARITY VERIFIER)

**Scope:** differential parity of the `ReportTools<'g,L>` facade (`src/services/zep_tools.rs`) vs Python `ZepToolsService` graph methods (`MiroFish/.../zep_tools.py`). Default-skeptical, ran both sides.

**Evidence base:** `cargo test --lib` → **1020 passed, 0 failed** (no Y-regression). `zep_tools` suite **28/28**, `entity_reader` **56/56**, graph `partition` **3/3**. Python reference harness (`/tmp/parity_check.py`) over the Rust `fixture_graph()` reproduced active_count=2 / historical_count=1 at t=300 and the 100/+10 scoring — matching Rust assertions.

### Per-surface verdict
1. **local_search/search_graph/quick_search** — PASS. Score constants match (exact=100 `py:584`↔`rs:828`; per-kw=+10 `py:589`↔`rs:833`). Tokenize split on `,`/`，` len>1 (`py:575`↔`rs:815-820`). Descending sort `reverse=True` (`py:603`↔`rs:858`). `[:limit]` cap (`py:605`↔`rs:860`). scope edges/nodes/both + node-summary-as-fact `[name]: summary` (`py:635`↔`rs:902`). search_graph→local_search is Python's own fallback path (`py:544`); Zep cross-encoder is `[≠]` (server-side, inexpressible) — LEGIT.
2. **panorama_search** — PASS. active = `is_active_at` true (`py:1199 not(is_expired or is_invalid)` ↔ teri `partition_edges_at(t).0` via valid_at window). historical tag `[{valid_at} - {invalid_at}] {fact}` (`py:1205`↔`rs:1070`). include_expired gate `[:limit] if include_expired else []` (`py:1230`↔`rs:1078`); default `True`↔caller-passed bool. **str→bool coercion is OUT OF SCOPE** — it lives in `report_agent.py:986-987` (ReACT dispatch), not in panorama_search (`zep_tools.py:1149` is plain `bool=True`). Not a missing behavior of this unit.
3. **get_entities_by_type** — PASS (with recorded DECISION-8 mapping). Python `entity_type in node.labels` exact match; Rust delegates to U-016 `filter_defined_entities` matching `kind.to_string()`. Casing divergence (Zep `"Student"` ↔ teri lowercase `"person"`) is the pre-accepted DECISION-8 `[≠]`, not a new drop.
4. **get_entity_summary** — PASS. dict KEY ORDER entity_name/entity_info/related_facts/related_edges/total_relations (`py:847-853`↔`rs:643-661`). search_graph(limit=20) + case-insensitive name match + node_edges aggregation faithful.
5. **get_graph_statistics** — PASS. Key order graph_id/total_nodes/total_edges/entity_types/relation_types (`py:882-888`↔`rs:702-714`). graph_id RETAINED in output (`py:883`↔`rs:704`). entity_types excludes Entity/Node (`py:874`↔`rs:689`). Test asserts counts 4/3 + per-type.
6. **get_simulation_context** — PASS. entities `[:limit]` + total_entities = FULL typed count (`py:939-940`↔`rs:766/769-772`). limit default 30 honored. summary="" is DECISION-9 Q2 `[≠]`.
7. **get_all_nodes/edges/node_detail/node_edges** — PASS. Entity→NodeInfo, EdgeTriple→EdgeInfo. Temporal map: valid_at `Some((s,Some(e)))`→valid_at=s, invalid_at=e, expired_at=e (closed window); `Some((s,None))`→valid_at=s only; `None`→all None (`rs:1316-1327`). Test asserts "100"/"200"/"200". node_detail bad/missing uuid→None; node_edges filter by source/target.
8. **DTO key order + to_text** — PASS. InsightForgeResult 9-key order + PanoramaResult 9-key order asserted by tests; to_text Chinese headers verbatim (`## 未来预测深度分析`, `## 广度搜索结果（未来全景视图）`, `【关键事实】`, `【历史/过期事实】`).
9. **EdgeInfo::is_invalid()** — PASS. Python `invalid_at is not None` (`py:135`) ↔ Rust `self.invalid_at.is_some()` (`rs:202`). Prior bug `source_node_uuid.is_empty()` CORRECTED; `test_edge_info_is_invalid_uses_invalid_at` proves empty-source + invalid_at=None → false.

### Deferred — HONEST, not silent drops
- **insight_forge** — PASS-as-deferred. Multi-sub-query STRUCTURE preserved (sub_queries populated via the SAME keyword fallback Python uses on `_generate_sub_queries` exception, `py:1138-1143`↔`rs:1127-1135`); per-sub-query + main-query search, dedup via seen-set, entity_insights, relationship_chains all built. Only semantic ranking QUALITY is `[!]` (OQ-3). Not dropped.
- **interview_agents** — PASS-as-deferred. Returns honest `Err` the ReACT loop tolerates (mirrors Python `_execute_tool` try/except → "工具执行失败"). Does NOT fabricate a fake interview. Test asserts `is_err()`.

### `[≠]` challenge results — all LEGIT (substrate-true, not feature-skips)
- NodeInfo.summary="" / attributes={} — teri Entity is `{id,name,kind}`; no per-entity summary or attr bag exists to read. Zep summaries are server-ingestion artifacts. INEXPRESSIBLE. ✓
- EdgeInfo.uuid="" / fact="" — teri Relation is `{kind,weight,valid_at}`; no uuid, no LLM-generated fact sentence. No consumer reads edge uuid. INEXPRESSIBLE. ✓
- search_graph cross-encoder reranking — Zep Cloud server-side; teri has no Zep server. Python itself falls back to local_search. INEXPRESSIBLE. ✓
- graph_id selection — the bound `&KnowledgeGraph` IS the selector; graph_id retained where observable (get_graph_statistics output). Server-handle artifact. INEXPRESSIBLE. ✓

### Verdict
**PASS — U-024 sub-cycle (b) is parity-verified, no downgrade.** Every re-homed graph method matches Python (scoring constants, sort order, caps, partition boundary, key order, temporal mapping). Every `[≠]` is genuinely inexpressible (substrate-true), not a portable-feature skip. Both deferrals are honest (structure preserved / honest error). 1020/1020 lib tests pass — Y not regressed.

---

## 2026-06-18 · U-024 sub-cycle (d) · `ReportAgent.plan_outline` (+ PLAN_SYSTEM_PROMPT / PLAN_USER_PROMPT_TEMPLATE) → `src/report/mod.rs`

**Verdict: PASS** — differential parity proven across all 7 verification surfaces. Symbols verified: **3/3** (S-738, S-739, S-761).
**Baseline / no-downgrade:** full `cargo test --lib` = **1087 passed, 0 failed** (incl. 8 plan_outline sub-cycle-d tests + template assoc-fns + data-model tests). Y not regressed.
**Method:** char-level diff of both prompt consts via a Python extractor (`eval` of the triple-quoted literals) vs the Rust raw strings; Python `str(list(...))` and `json.dumps(...,ensure_ascii=False,indent=2)` run live and compared against the Rust `python_list_repr` + `serde_json::to_string_pretty` (temp in-crate test, since removed).

### Surface-by-surface evidence
1. **PLAN_SYSTEM_PROMPT + PLAN_USER_PROMPT_TEMPLATE verbatim — PASS.** Byte-identical. SYS = 691 chars, USER = 367 chars, `==` True both. Python `"""\` line-continuation eats the leading newline and the closing `"""` adds no trailing newline; the Rust `r#"你是…!"#` / `r#"【预测场景设定】…发现。"#` match exactly (Chinese body, JSON example block, 章节数量 reminder lines, the trailing `！` with no newline). py:552-589 / py:591-611 ≡ mod.rs:92-128 / mod.rs:130-149.
2. **system_prompt assembly — PASS.** Python `f"{PLAN_SYSTEM_PROMPT}\n\n{get_language_instruction()}"` (py:1166) ≡ Rust `format!("{}\n\n{}", PLAN_SYSTEM_PROMPT, get_language_instruction())` (mod.rs:409). Order + `\n\n` separator exact.
3. **user_prompt substitution — PASS.** All 6 slots map:
   - simulation_requirement; total_nodes/total_edges from `graph_statistics` (default 0 via `unwrap_or(0)`); total_entities from context (default 0). ✓
   - **entity_types** = `python_list_repr(keys)`: matches Python `str(list(...keys()))` exactly for `[]`, `['Person']`, `['Person', 'Organization']`, Chinese `['人物', '组织']` (single quotes, `", "` sep, `[]` empty). ✓
   - **related_facts_json** = `to_string_pretty(facts[:10])`: **byte-identical** to Python `json.dumps(facts[:10], ensure_ascii=False, indent=2)` for a multi-fact Chinese input — 2-space indent, non-ASCII unescaped (`未来事实1：群体迁移`/`危机`/`时间`), float `0.8`, key order preserved, `[:10]` truncation (`test_…truncated_to_10`), empty → `"[]"` on both. The `[≠]`-watch is **resolved as truly identical**, not a divergence. ✓
4. **chat_json call — PASS.** `vec![ChatMessage::system(sys), ChatMessage::user(user)]` → `[{role:system},{role:user}]` same order as Python `messages=[{system},{user}]`; `ChatOptions{temperature:Some(0.3)}` ≡ `temperature=0.3`; returns `serde_json::Value`. (llm.rs:217-248)
5. **outline parsing + defaults — PASS.** title default `"模拟分析报告"`, summary default `""`, section title `.get("title").unwrap_or("")` + content `""`, sections from `response["sections"]` in order; missing/empty sections → empty Vec. `test_plan_outline_happy_path` + `test_plan_outline_defaults_on_empty_sections`. py:1190-1200 ≡ mod.rs:438-467.
6. **fallback outline + try/except boundary — PASS.** On chat_json `Err` → 3-section fallback, byte-identical: title `"未来预测报告"`, summary `"基于模拟预测的未来趋势与风险分析"`, sections `["预测场景与核心发现","人群行为预测分析","趋势展望与风险提示"]` (`test_plan_outline_fallback_on_llm_error`, py:1211-1218). **Boundary confirmed:** Python's `try` starts at py:1176 (AFTER user_prompt build); a `{"sections":[]}` response parses via `.get(...,default)` and yields an empty-section outline, **NOT** the fallback — Rust matches (`test_plan_outline_defaults_on_empty_sections` → title "模拟分析报告", 0 sections, not fallback). ✓
7. **progress emissions — PASS.** Happy path: exactly 4 emissions at 0/30/80/100, all stage="planning", keys `progress.{analyzingRequirements,generatingOutline,parsingOutline,outlinePlanComplete}` (all 4 resolve in en.json + zh.json) (`test_plan_outline_progress_emissions`). Error path: 0 + 30 fire, 80 + 100 SKIPPED — matches Python's except returning before the inner callbacks (`test_plan_outline_fallback_no_progress_after_failure`). ✓

### Non-contractual divergences inspected (none block parity)
- **`python_list_repr` apostrophe edge:** Python `repr()` switches a single-quote-containing string to double-quotes (`["O'Brien"]`); Rust always single-quotes (`['O'Brien']`). **Non-contractual / unreachable:** entity_types keys are graph entity-type *labels* (ontology class names — Person/Organization/Chinese categories, zep_tools.py:874 node.labels), never free-form apostrophe'd text. Confirmed source = node labels excluding Entity/Node. Does not affect any realistic input.
- **`build_plan_user_prompt` Err → fallback:** Python builds user_prompt OUTSIDE the try, so a `json.dumps` raise would propagate (not fallback). In practice `to_string_pretty` on `Vec<Value>` is infallible and `json.dumps(ensure_ascii=False)` on JSON-derived facts never raises → the `Err` arm is effectively dead on both sides. Non-contractual.
- **present-but-null `title`:** Python `dict.get("title", default)` returns the present `None`; Rust `.and_then(as_str).unwrap_or(default)` returns the default. Only diverges if the LLM returns a non-string title — non-contractual (LLM returns strings); absent-key default (the real contract) matches exactly.

### Out-of-scope upstream note (NOT a sub-cycle-d defect, flag for zep_tools owner)
`get_graph_statistics` (`src/services/zep_tools.rs:691,713`) builds `entity_types` in a `std::collections::HashMap` then `serde_json::to_value(...)` — the into-IndexMap insertion order is the HashMap's **randomized** iteration order, whereas Python's dict preserves **node-iteration insertion order**. So the `{entity_types}` slot's key ORDER can differ run-to-run. This originates in the zep_tools symbol, NOT in S-761/S-739: `build_plan_user_prompt` faithfully preserves whatever order it is handed. Recommend the zep_tools owner switch `entity_types`/`relation_types` to an insertion-ordered map (IndexMap/Vec-of-pairs) to match Python dict order. Tracked as a follow-up; does not gate sub-cycle (d).

### Symbols
- S-738 `PLAN_SYSTEM_PROMPT` → `- [x]` (verbatim, 691 chars byte-identical)
- S-739 `PLAN_USER_PROMPT_TEMPLATE` → `- [x]` (verbatim 367 chars + all 6 substitutions verified)
- S-761 `ReportAgent.plan_outline` → `- [x]` (all 7 surfaces match; 8 differential tests green)

### Verdict
**PASS — U-024 sub-cycle (d) is parity-verified, no downgrade.** Prompts verbatim; substitutions + defaults + fallback + try/except boundary + progress-emission pattern all match Python. The one flagged `[≠]`-watch (related_facts_json) resolved to byte-identical. The only real ordering divergence is upstream in zep_tools (out of scope, flagged). 1087/1087 lib tests pass — Y not regressed.

---

## 2026-06-18 — U-024 sub-cycle (e) `generate_section_react` (bounded ReACT loop) — PASS

Differential parity vs `report_agent.py:_generate_section_react` (1221-1530). Source could be read; Rust ran (`cargo test --lib` = 1099 pass). Verdict per surface:

1. **8 constants + 3 inline msgs — PASS.** Char/AST diff each Rust const vs Python triple-quote/paren-concat: SECTION_SYSTEM (3141), SECTION_USER (509), REACT_OBSERVATION (330), INSUFFICIENT (180), INSUFFICIENT_ALT (135), TOOL_LIMIT (111, AST), UNUSED_HINT (90), FORCE_FINAL (84) all VERBATIM. Inline 格式错误 (169 chars AST) VERBATIM; （响应为空）/请继续生成内容。/（这是第一个章节）all present both sides. (S-740..S-747)
2. **Setup — PASS.** system = SECTION_SYSTEM_PROMPT.replace(5 slots: report_title/report_summary/simulation_requirement/section_title/tools_description) + "\n\n" + get_language_instruction() (rs:825-832 vs py:1255-1262). Order + 5 slots match.
3. **previous_content — PASS.** join "\n\n---\n\n" (rs:858 vs py:1271); per-section truncate at 4000 CHARS via char_indices().nth(4000) byte offset (rs:844-855), "..." only when char_count>4000; empty→（这是第一个章节）. `test_react_previous_sections_truncation_unicode` proves 4001×"中" → 4000 chars+"...", 4000×"中" → no "..." (char not byte). py `sec[:4000]` is char-based → match.
4. **None/empty — PASS (documented mapping).** Rust maps BOTH `Err` AND `Ok("")` → None path (rs:903-906); iter<max-1 → append assistant（响应为空）+ user 请继续生成内容。+ continue; last iter → break → force-final (rs:908-917 vs py:1312-1320). NOTE: Python `chat` returns a `str` (raises on null content via re.sub; the `is None` guard is defensive/effectively-dead for the real OpenAI client). The `Ok("")`→None mapping is an **accepted intentional mapping** (architecture doc l186-187: "map an empty string the same way Python checks `response is None`"). It is a fail-closed error-recovery edge, NOT a feature-left-behind (no Python serialization/CLI/export feature dropped); the only-reachable real "no usable turn" scenario converges. Tracked, not hidden. `test_react_none_empty_retry_then_break` asserts retry×4 then break→force-final→recovered.
5. **Conflict×3 — PASS.** conflict_retries++; <=2 → append assistant(response)+格式错误 user + continue; 3rd → `response.find("</tool_call>")`, truncate to `first_tool_end + len("</tool_call>")` (rs:944-946 vs py:1355-1357, offset INCLUDES closing tag), re-parse, has_final_answer=false, conflict_retries=0, **FALL THROUGH (no continue)** (rs:950-953). `test_react_conflict_downgrade_third_time` proves 3rd conflict executes the truncated quick_search then continues → force content.
6. **Situation 1 — PASS.** count<3 → append + REACT_INSUFFICIENT (inline hint `", "` join, rs:971) + continue; else `response.rsplit("Final Answer:").next().trim()` == py `split("Final Answer:")[-1].strip()` (rs:990 vs py:1392). `test_react_final_answer_last_occurrence` proves LAST occurrence; `test_react_insufficient_tools_rejection_then_accept` proves reject-then-accept.
7. **Situation 2 — PASS.** count>=5 → REACT_TOOL_LIMIT + continue (rs:997-1004); else execute **tool_calls[0] only** via execute_by_name(name,params,graph_id,sim_id,sim_req,report_context) (rs:1014-1021); count++; used_tools.insert; obs unused_hint uses `"、"` (rs:1048) gated on `unused非空 && count<5`; used_tools_str uses `", "` (rs:1059); report_context = "章节标题: {title}\n模拟需求: {req}" (rs:885 vs py:1294). Separators correct (、 vs ,). `test_react_happy_path` + `test_react_quota_five_tool_calls_then_force_final` exercise.
8. **Situation 3 — PASS.** append assistant; count<3 → REACT_INSUFFICIENT_ALT (inline hint `", "`, rs:1085) + continue; else `response.trim()` return (rs:1099 vs py:1491). `test_react_no_prefix_accept_situation3` proves trim on accept.
9. **Force-final — PASS.** append REACT_FORCE_FINAL; chat; Err/empty → t('report.sectionGenFailedContent') (i18n key resolves, en+zh present); "Final Answer:" → rsplit.trim(); else → **RAW response (s, NOT trimmed)** (rs:1119 vs py:1519). Confirmed differs from Situation-1/3 which DO trim. `test_react_force_final_plain_not_trimmed` asserts trailing whitespace preserved; `test_react_force_final_empty_returns_i18n_fallback` asserts fallback.
10. **chat params — PASS.** ChatOptions{temperature:0.5, max_tokens:4096} on BOTH calls (rs:887 loop, rs:1106 force-final vs py:1307-1308/1508-1509).
11. **(g) logging deferral — LEGIT.** All 7 Python `if self.report_logger:` sites + the multiToolOnlyFirst info log represented by `// (g):` comments (rs:821/956/991/1009/1011/1022/1100/1108), none vanished. Behavioral contract (returned string + appended messages + tool exec) preserved; logs are observability owned by sub-cycle (g) (ReportLogger S-681..S-704 still `- [ ]`). Tracked deferral, NOT a feature-skip.
12. **Set-order — LEGIT.** Python `set` joins (`、`/`, `) are nondeterministic; teri uses fixed canonical array ["insight_forge","panorama_search","quick_search","interview_agents"] + membership filter → deterministic. MEMBERSHIP identical (no member dropped); `""` sentinel excluded from both joins. Acceptable inherent-nondeterminism resolution.

**ChatMessage::assistant (llm.rs:238) — PASS (additive).** Pure addition alongside system()/user(); ChatRole closed enum; existing ChatMessage API undisturbed (1099 tests incl. all prior chat tests pass).

**Y-not-regressed:** 1099 lib tests pass (1087 prior baseline + 12 new sub-cycle (e) ReACT tests). No regression.

**Symbols:** 9/9 verified `- [x]` (S-740..S-747 constants, S-762 method).

**VERDICT: PASS.** Every branch, constant, separator, and control-flow edge matches Python. The one mapping divergence (`Ok("")`→None) is documented, contractually-convergent error-recovery, not a downgrade.

---

## 2026-06-18 — U-024 sub-cycle (f) `ReportManager` — PASS

**Scope:** 29 symbols S-765..S-793 (`src/report/manager.rs`). Differential goldens derived by
extracting the Python algorithms verbatim into a standalone harness, running BOTH sides over crafted
inputs, and diffing byte-for-byte. Temp harness files removed post-verification.

**Method:** `/tmp/parity_f/py_golden.py` (extracted `_clean_section_content` 2132–2197 +
`_post_process_report` 2301–2424 + outline shim) vs a temp `examples/parity_f_dump.rs` calling the
real Rust methods → `diff` IDENTICAL across all 28 crafted cases. JSON: `/tmp/parity_f/py_json.py`
(`json.dump(ensure_ascii=False, indent=2)`) vs real `save_report`/`save_outline`/`update_progress`
writers → on-disk bytes IDENTICAL.

### Surface verdicts

1. **`clean_section_content` — PASS.** 15 crafted cases byte-identical to Python, incl. the hard edges:
   (a) dup heading in first 5 lines dropped + following blank skipped (`a`,`g`); (b) SAME heading at
   i≥5 → `**title**` NOT dropped (`b` i=5, `m`); (c) `###`/`####` → `**title**`+blank (`c`); (d) leading
   blanks popped (`d`); (e) leading separator `---`/`***`/`___` popped WITH trailing blanks (`e`,`k`);
   (f) space-stripped dup match `replace(' ','')` (`f`); dup-no-blank-after keeps next line (`h`);
   heading-at-i4 still in-window (`l`). Evidence: py_golden.json == rust_dump.json["clean"].

2. **`post_process_report` — PASS.** 13 crafted cases byte-identical. L1==outline.title kept (`p_l1_title_kept`);
   L1∈sections → `## ` promote (`p_l1_section_promoted`); L1 other → bold (`p_l1_other_bold`);
   L2∈sections/==title kept (`p_l2_section_kept`, `p_l2_outline_title`); L2 other → bold; L≥3 → bold;
   **dup-window edge**: dup within last-5 processed_lines deduped (`p_dup_5line_edge`) but a 6th-line-back
   dup is NOT (`p_dup_beyond_5`) — off-by-one window correct; `---` after heading skipped
   (`p_sep_after_heading`); blank-after-heading ≤1; final blank-run collapse ≤2 (`p_blank_collapse`);
   full multi-section render (`p_multi_section`). Evidence: py_golden.json == rust_dump.json["pp"].

3. **JSON formats/key-order — PASS.** meta.json (`report.to_dict` order: report_id, simulation_id,
   graph_id, simulation_requirement, status, outline, markdown_content, created_at, completed_at, error),
   outline.json (title, summary, sections), progress.json (status, progress, message, current_section,
   completed_sections, **updated_at**) — all byte-identical to Python `json.dump(ensure_ascii=False,
   indent=2)`: 2-space indent, non-ASCII unescaped (中文 + 🚀 pass through verbatim), `null` for None,
   NO trailing newline. serde_json `preserve_order` feature confirmed active. Evidence: py_meta.json ==
   rust_meta.json, py_outline.json == rust_outline.json, py_progress.json == rust_progress.json.

4. **section file padding / save_section — PASS.** `format!("section_{:02}.md")` ⇒ 1→`section_01.md`,
   10→`section_10.md` (tests `test_save_section_*`). save_section applies `clean_section_content` before
   writing (manager.rs:292) — matches Python 2117. Index base **1-based** (Python: "从1开始"), preserved.

5. **get_report old-format fallback — PASS.** Both Python fallbacks reproduced: (i) meta.json missing →
   `{reports_dir}/{id}.json` flat file (manager.rs:719–725 == py 2452–2457); (ii) empty markdown_content
   → read `full_report.md` (manager.rs:754–770 == py 2478–2484). Report reconstruction field-by-field:
   defaults `''` for created_at/completed_at, `''` for missing section content, status parse via
   ReportStatus, error null→None. Tests `test_get_report_old_format_fallback`,
   `test_get_report_markdown_fallback_from_full_report_md`.

6. **from_line pagination shapes — PASS.** Both `get_console_log`/`get_agent_log` return
   `{logs, total_lines, from_line, has_more:false}` with from_line offset slice (`i >= from_line`),
   total_lines counts ALL lines, has_more always false. get_agent_log skips invalid-JSON lines
   (`except JSONDecodeError: continue` == `if let Ok(entry)`), total_lines still counts the bad line
   (test asserts total=3 with 1 bad line). Missing-file shape returns `from_line:0` (hardcoded, matches
   py). Console log strips `\n\r` per line. Tests `test_get_{agent,console}_log_*`.

7. **list_reports / delete_report / get_report_by_simulation / get_generated_sections — PASS.**
   list_reports sorted by created_at DESC (stable sort both sides), limit truncate, old+new format scan;
   delete_report removes folder (new) else flat `{id}.json`+`{id}.md` (old); get_report_by_simulation
   scans dirs + `.json` files; get_generated_sections sorted by filename, `[]` on missing folder. Tests
   `test_list_reports_*`, `test_delete_report_*`, `test_get_report_by_simulation*`,
   `test_get_generated_sections_*`.

### Minor flag (non-blocking, non-contractual)
`update_progress` `updated_at` uses `chrono::Local::now().to_rfc3339()` → offset-suffixed
(`...+08:00`), whereas Python `datetime.now().isoformat()` is NAIVE (no offset). The field is
**write-only / never parsed back** by Python (only `datetime.now().isoformat()` write site;
grep confirms zero readers of `progress['updated_at']`) → non-contractual, observable only as a
display string. The project already has `python_isoformat_local()` (models/project.rs:50, used by
U-023 simulation_manager) that produces the Python-exact naive shape. RECOMMENDATION to porter:
swap the timestamp writer to `python_isoformat_local()` for cross-unit consistency. NOT a parity
failure — value shape (ISO-8601 string) and key name (`updated_at`) match; behavior identical.

### Y-not-regressed
`cargo test --lib` ⇒ **1142 passed** (1099 prior baseline still green, +43 new manager tests).
`cargo test --lib report::manager` ⇒ 43 passed, 1099 filtered. `cargo test --lib report::` ⇒ 79 passed
(36 prior report-module tests + 43 manager) — `pub mod manager;` did not disturb the existing report
module or its tests.

**Symbols:** 29/29 verified `- [x]` (S-765..S-793).

**VERDICT: PASS.** The two regex content-shaping methods are byte-identical to Python across all
crafted edge cases; all three JSON files are byte-identical (format + key-order + non-ASCII); section
padding, pagination shapes, and every back-compat fallback match. One non-contractual timestamp-format
flag routed to the porter as a consistency nit, not a downgrade.

---

## 2026-06-18 — U-024 sub-cycle (g1) — `ReportLogger` (agent_log.jsonl) + 7 wirings into `generate_section_react`

**Surfaces verified:** entry shape/key-order; JSONL compact format; all 13 helpers
(action/stage/details key+order+message-key); timestamp; elapsed_seconds rounding; the 7
`generate_section_react` wirings; None-guard. Differential vs `report_agent.py:36–305` (logger) and
`:1221–1530` (wiring). Symbols in scope: S-681..S-698 (18 g1 symbols).

**Tests:** `cargo test --lib report::logger` ⇒ 22 passed. `cargo test --lib` ⇒ **1166 passed**
(1144 prior baseline still green, +22 new logger tests) — Y-not-regressed CONFIRMED.

### Per-surface results

1. **Entry shape + key order** — PASS. `log()` (logger.rs:185–208) inserts into `serde_json::Map`
   in exactly `timestamp, elapsed_seconds, report_id, action, stage, section_title, section_index,
   details` — identical to py:85–94. `section_title`/`section_index` → `Value::Null` when `None`
   (logger.rs:200,206 vs py:91,92). serde_json `preserve_order` keeps insertion order; asserted by
   `test_log_entry_key_order_and_top_level_shape`.
2. **JSONL format** — PASS. `serde_json::to_string` (compact, logger.rs:212) + `writeln!` (`\n`,
   :223) + `OpenOptions::append(true).create(true)` (:218–222) ≡ py:97–98 `open('a') ... json.dumps(
   ensure_ascii=False)+'\n'`. Non-ASCII unescaped (serde_json default) asserted by
   `test_log_non_ascii_unescaped` (中文/日本語 literal). Compact/single-line asserted by
   `test_log_compact_format_and_single_line`.
   - *Non-contractual nit (NOT a fail):* Python default `json.dumps` separators emit `", "`/`": "`
     (space after comma/colon); serde_json compact emits no spaces. The jsonl is machine-read by the
     frontend via `JSON.parse` (whitespace-insensitive) — observably equivalent. Same strategy already
     accepted in `sim::action_logger`.
3. **13 helpers** — PASS for action/stage/details-key/order/message-key on all 13. Verified each Rust
   helper's `details` `Map` insertion order + i18n key against Python verbatim (logger.rs:237–589 vs
   py:100–304). All 13 message keys resolve in `i18n/locales/{en,zh}.json:569–581` (not pass-through).
   Spot-confirmed details-bearing ones: `log_tool_result` includes `result_length` (5 keys, :408–424);
   `log_llm_response` includes `response_length`+`has_tool_calls`+`has_final_answer` (6 keys,
   :449–467); `log_section_content` includes `content_length`+`tool_calls_count` (4 keys, :490–505);
   `log_error` sets `section_index=None` (:588 → `self.log(..., None)`). Per-helper key-order asserted
   by 13 dedicated tests.
4. **timestamp** — PASS. `python_isoformat_local()` (project.rs:50–58) = `Local::now().naive_local()`
   with microsecond fraction emitted only when non-zero — matches Python NAIVE `datetime.now()
   .isoformat()` (CPython ground truth: `2026-06-18T12:00:00` whole / `...123456` with micros). The
   (f) fix is in place.
5. **elapsed_seconds** — PASS. `start.elapsed().as_secs_f64()` → `round_half_even_2dp` (logger.rs:181–
   182). Banker's-rounding helper verified against CPython ground truth: `round(1.234,2)=1.23`,
   `round(1.235,2)=1.24`, `round(0.045,2)=0.04`, `round(2.675,2)=2.67`, `round(12.345,2)=12.35` — the
   helper's mantissa-bit tie-resolution matches. Value is wall-time-dependent → parity is the
   rounding-function + key, both correct.
6. **Wiring (7 sites)** — PASS. Diffed line-by-line against Python `if self.report_logger:` blocks,
   all guarded by `if let Some(l) = self.report_logger.as_ref()`, same point, same args:
   - `log_section_start` pre-loop (mod.rs:825 vs py:1252).
   - `log_llm_response` after conflict-resolution, `iteration+1`+has_tool_calls/has_final_answer
     (mod.rs:960 vs py:1364).
   - `log_tool_call` before `execute_by_name`, `iteration+1` (mod.rs:1024 vs py:1423).
   - `log_tool_result` after execute, `iteration+1` (mod.rs:1047 vs py:1438).
   - `log_section_content` ×3: situation-1 valid-final return (mod.rs:998 vs py:1395), situation-3
     no-prefix return (mod.rs:1125 vs py:1493), force-final return (mod.rs:1155 vs py:1522) — each
     with the right content + `tool_calls_count`. NONE dropped or misplaced.
   - `multiToolOnlyFirst` correctly left as a (g2) console-log marker (mod.rs:1033–1036, NOT a
     ReportLogger call) — Python py:1421 is `logger.info(...)`, a console log. Correct.
7. **None-guard** — PASS. `report_logger: None` ⇒ every `if let Some` is skipped ⇒ no file, loop
   behavior identical. The (e) ReACT-loop tests run with `None` and remain green (in the 1166).

### Scope split (g1/g2/h) — legitimately tracked, NOT a silent drop
`ReportConsoleLogger` (S-699..S-704) → g2; `ReportSink`/higher-level API → h. Both remain `- [ ]`/open
in `symbol-map.md`; the g1 wiring leaves an explicit `// (g2): …` marker at the console-log site. The
deferral is recorded, not waved.

### ✗ FAIL — `*_length` keys use BYTE count, Python uses CHARACTER count (observable downgrade)

**Input:** any non-ASCII tool result / LLM response / section content (the dominant case — this is a
Chinese-language product; system prompts + outputs are Chinese).
**Expected (source):** Python `len(result)` / `len(response)` / `len(content)` = **character count**
(py:207, 230, 252, 276; CPython `len('中文')==2`).
**Actual (Rust):** `result.len()` / `response.len()` / `content.len()` / `full_content.len()` =
**byte count** (logger.rs:407, 448, 489, 526; for 3-byte UTF-8 ≈ 3× the char count).
- 4 affected sites: `log_tool_result.result_length`, `log_llm_response.response_length`,
  `log_section_content.content_length`, `log_section_full_complete.content_length`.
- **Observable**: the frontend renders this value literally as characters —
  `Step4Report.vue:1862` `formatResultSize` returns `` `${length} chars` `` / `` `${(length/1000)
  .toFixed(1)}k chars` ``. A 1000-Chinese-char result displays as "3.0k chars" instead of "1000
  chars". Distinct rendered output for the same input.
- **Project convention already settled the opposite way:** TextProcessor parity (this file, §2026
  earlier — "`chars` = Unicode scalar count") ports Python `len(str)` → Rust `.chars().count()`. The
  logger regressed to `.len()`, and a code comment (logger.rs:993) even *acknowledges* the mismatch
  ("Python len() on str = char count, but we use byte len here") — this is a documented downgrade, not
  a non-contractual artifact.
- **Fix (mechanical, route to porter):** change `.len()` → `.chars().count()` at logger.rs:407, 448,
  489, 526; tighten the 4 length-asserting tests to assert the char count on a non-ASCII fixture
  (e.g. `result text 中文` → 9 chars, currently the test only asserts `is_number()`, which is why this
  slipped the existing goldens).

**Symbols:** affected helpers **stay `- [~]`**: S-693 (`log_tool_result`), S-694 (`log_llm_response`),
S-695 (`log_section_content`), S-696 (`log_section_full_complete`). The other 14 g1 symbols
(S-681..S-692, S-697, S-698) are individually parity-clean but the **unit cannot PASS** with 4
unproven symbols (rollup rule).

**UNIT (g1) VERDICT: FAIL.** Entry shape, all 13 helpers' action/stage/details keys+order+message,
timestamp, elapsed rounding, and the 7 wirings all match Python — but 4 `*_length` values are a real
observable byte-vs-char downgrade against a convention this port already established. Route back to the
rust-port-porter for the 4-site `.chars().count()` fix + non-ASCII length-assertion in the 4 tests;
re-verify. Symbols S-681..S-698 remain `- [~]`.

---

## 2026-06-18 — U-024 sub-cycle (g2) `ReportConsoleLogger` — PASS

**Verdict:** PASS (6/6 symbols S-699..S-704 → `- [x]`). Differential vs `MiroFish/backend/app/services/report_agent.py:307-388`.

**Method:** Ran `cargo test --lib console_logger` (10/10 pass). To rule out the flagged "conditional-skip papers over a non-capturing layer" risk, I temporarily injected a hard assertion `assert!(SUBSCRIBER_INSTALLED)` into the capture tests — it PASSED both in isolation and in the full lib suite, proving the real capture path (layer install → emit → assert file content) genuinely executes in every runnable config (no other test installs a global tracing subscriber, so console_logger wins the set-once race). Also dumped a real captured `console_log.txt` end-to-end.

**Real captured output (decisive evidence):**
```
[19:03:05] INFO: ReACT generating section: Market Overview
[19:03:05] WARNING: Section X iteration 2: LLM returned None
[19:03:05] ERROR: Tool execution failed: quick_search, error: boom
```
(a `tracing::debug!` line and a `teri::server` line in the same run produced NO output → INFO floor + target filter both proven live.)

| # | Surface | Verdict | Evidence |
|---|---------|---------|----------|
| 1 | Layer GENUINELY captures | PASS | Probe proved `SUBSCRIBER_INSTALLED=true`; format dump shows real file lines; conditional-skip is a legit set-once fallback, never the only path |
| 2 | Format `[%H:%M:%S] LEVEL: msg\n` | PASS | Dump matches exactly; local time via `chrono::Local::now()`; message-only (no target/span decoration) |
| 3 | WARN→WARNING (#1 trap) | PASS | Dump shows `WARNING:` not `WARN:`; `python_level_name` maps WARN→"WARNING"; `test_warn_maps_to_warning_not_warn` asserts no `WARN:` |
| 4 | INFO+ floor (DEBUG excluded) | PASS | `tracing::debug!` produced no line; py:1322 kept as DEBUG, not promoted; `test_debug_events_excluded` |
| 5 | Target filter (report + zep prefix only) | PASS | `teri::server`/`teri::sim` excluded; `teri::report` exact + `teri::services::zep_tools` prefix captured; `test_non_report_target_excluded` + `test_zep_tools_target_captured` |
| 6 | Emission sites + LEVELS match Python | PASS | All 17 sites diffed (mod.rs ×13 + zep_tools.rs ×4); levels exact: info/warn/error per py:917/968/1027/1044/1061(ERR)/1152/1205/1209(ERR)/1249/1313(WARN)/1332(WARN)/1352(WARN)/1393/1421/1490/1503(WARN). Note: py tool-exec logs (968/1061/1027/1044) use the `mirofish.report_agent` logger (py:33), so Rust placing them in zep_tools.rs on `target:"teri::report"` is CORRECT. `iteration+1` arg matches. All fire UNCONDITIONALLY (not gated on report_logger) |
| 7 | Lifecycle (new/close/Drop) | PASS | `new` mkdir+append open+sink install; `close` flush+toggle-off (post-close not captured); `Drop`→close idempotent. Mirrors py `__init__`/`_setup_file_handler`/`close`/`__del__` |
| 8 | init change additive/non-breaking | PASS | `logging.rs` adds `report_console_layer` to BOTH arms; console-only arm converted to `registry().with(...).init()`; no-op when sink None (default); existing console/file output filter/target/level unchanged |

**Forward-dep [!]:** `teri::services::zep_tools` capture target is wired; no production module emits on it yet (only the test fixture at console_logger.rs:610). This is the architect's wiring-ready seam, NOT a downgrade — capture scope is faithfully reproduced; the producer is a separate unit (legit-tracked).

**[≠] challenge:** none claimed; none warranted — `console_log.txt` is contractual (read back by `get_console_log`/stream, surfaced to frontend), so a `[≠]` would be a disguised feature-skip. The full feature was ported.

**Regression:** full `cargo test --lib` = 1176 passed / 0 failed (1166 prior + 10 g2). Y not regressed.

---

## 2026-06-18 — U-024 sub-cycle (h1) `ReportSink` foundation + `update_progress` -1 fix + `console_logger` field — PASS

**Verdict:** PASS. h1 is pure substrate scaffolding (new `src/report/sink.rs`) + three additive manager prerequisites + one ReportAgent field. The real X-parity surface is small and is verified byte-faithful; the scaffolding is confirmed a legit substrate, not a hidden downgrade.

**Differential method:** Read Python `update_progress` body (`report_agent.py:2200-2226`), the failed-path `-1` write (`1753`), `ReportStatus` enum (`389-395`), `console_logger` init (`915`), `_ensure_report_folder` (`1916`). Dumped the **raw progress.json bytes** from teri's own `update_progress` (temp test, since removed) and diffed against Python `json.dumps(ensure_ascii=False, indent=2)`. Ran `cargo test --lib`.

| # | Surface | Verdict | Evidence |
|---|---------|---------|----------|
| 1 | update_progress -1 fix (THE parity surface) | PASS | Param + progress.json value now `i32` (manager.rs:419, `Number::from(i32)`). Raw dump byte-identical to Python: `"progress": -1` JSON integer; key order status/progress/message/current_section/completed_sections/updated_at preserved (serde_json `preserve_order` feature ON in Cargo.toml:35 → IndexMap == Python dict insertion order); `current_section: null`; `completed_sections: []` + populated-array layout match; `updated_at` naive isoformat (no offset) == `datetime.now().isoformat()`. Normal 0..100 unchanged. (Note: my first throwaway repro used a crate WITHOUT preserve_order and showed alphabetical keys — the real teri build has the feature, confirmed in Cargo.lock + indexmap present; the byte-faithful order is from teri's actual config.) |
| 2 | ReportStage serde values | PASS | All 5 variants → "pending"/"planning"/"generating"/"completed"/"failed" via `#[serde(rename_all="lowercase")]`; `to_status_str()` returns the same 5 strings. Equal to Python `ReportStatus.value` (389-395) AND to the existing `ReportStatus` enum (mod.rs:385, same rename). `test_report_stage_serde_lowercase` + `test_report_stage_all_five_variants_round_trip` green. |
| 3 | ReportSink / ReportEvent / NullSink (substrate seam) | PASS | (a) Legit abstraction of Python's `progress_callback(stage,progress,message)` — architect §1 confirms it's a strict-superset capability add (adds section_title/index/content/report_id), NOT a place a Python feature was dropped. (b) `ReportEvent.progress` is `i32` (carries -1; `test_report_event_negative_progress_failed_path` green). (c) jsonl/console sinks correctly NOT routed through ReportEvent — they keep their typed seams (`ReportLogger.log_*` / `tracing` g2 layer) per architect §1.4, preserving the (g1) details key-order contract. NullSink no-ops via dyn dispatch. h2/h3 deferral (generate_report orchestration) is a legit decomposition: S-763 stays `- [ ]`, tracked in architect §6 with explicit per-substep parity criteria; architect §7.5 confirms NO `[≠]` introduced and every observable artifact is PORTED (none skipped). |
| 4 | console_logger field | PASS | `console_logger: Option<ReportConsoleLogger>` on ReportAgent, `None` in both `new()` and `new_react()` (mod.rs:570,594). Mirrors Python `self.console_logger: Optional[ReportConsoleLogger] = None` (915). None-default → existing construction/tests byte-stable; no log* path reads it yet (h2 populates). |
| 5 | ensure_report_folder pub + upload_folder accessor | PASS | `ensure_report_folder` made `pub` (was private) — pure visibility change, same `create_dir_all`→`PathBuf` body matching Python `makedirs(exist_ok=True)` return (1916). `upload_folder()` returns `reports_dir.parent()` (the upload_folder root) — new additive accessor, no behavior change to (f). Both have tests (test_ensure_report_folder_is_pub_and_creates_dir, test_upload_folder_returns_parent_of_reports_dir) green. |

**[≠] challenge:** NONE claimed by the porter/architect for h1, and none warranted. ReportSink/ReportEvent are a strict-superset substrate (architect §7.5) — independently confirmed: every observable generate_report artifact (progress.json, meta.json, outline.json, section_NN.md, full_report.md, agent_log.jsonl, console_log.txt, SSE stream) is PORTED or scheduled (h2/h3), none `[≠]`-skipped. The `progress_callback`→ReportSink mapping is faithful (same single event stream), not a drop. The h2/h3 deferral is genuine decomposition (foundation-first), not a feature skip.

**Symbol coverage:** S-784 (update_progress) re-verified PASS under i32 widening → stays `- [x]` (annotation updated). S-769 (ensure_report_folder) re-confirmed `- [x]` (pub additive). The new substrate symbols (ReportSink/ReportEvent/ReportStage/NullSink, console_logger field, upload_folder accessor) have NO Python counterpart → no parity-ledger rows (correct: they're teri substrate, not ported Python symbols). S-763 (generate_report) correctly remains `- [ ]` — its parity is h2/h3's gate.

**Regression (Y not regressed):** full `cargo test --lib` = **1188 passed / 0 failed** (1176 prior baseline + 8 sink + 4 manager-h1 tests = 1188). The prior 1176 all still pass.

**Build-health note (for build-health-auditor, NOT a parity finding):** `cargo fmt -p teri --check` flags 2 pre-existing porter lines in the h1 deliverable (manager.rs:1584 multi-line `update_progress` test call; mod.rs:591 `new_react` `Self{...}` line). Format is a commit precondition owned by build-health — fix before the unit ledger commits. Does not affect behavioral parity.

**h1 OVERALL VERDICT: PASS.** The -1 fix matches Python byte-for-byte (key order + integer -1 + null + array), all 5 stage values are correct and agree with ReportStatus, the ReportSink scaffolding is a verified strict-superset substrate (not a drop), and nothing from generate_report was silently dropped — the orchestration is legitimately deferred to h2/h3 with tracked parity criteria.

---

## 2026-06-18 — U-024 sub-cycle (h2) ROUND-2 RE-VERIFY · `generate_report` skeleton · VERDICT: PASS

**Scope:** Re-verify the porter's fix for the Round-1 FAIL (post-planning failure dropped the built
outline from the FAILED `meta.json`). Skeleton scope only (planning + finalize/error tails); the
per-section loop is h3.

**Round-1 downgrade — CONFIRMED GONE.** `generate_report` (src/report/mod.rs:1535) now hoists the
`report` object BEFORE the async try-body (mod.rs:1591). The try-body returns `std::io::Result<()>` and
mutates `report` in place; the success arm returns it as `Completed`; the **error arm mutates the SAME
object** — `status=Failed`, `error=Some(...)` — WITHOUT resetting `outline`/`markdown_content`/
`completed_at` (mod.rs:1779-1780). This is the faithful map of Python's `except` (report_agent.py:1742-1743)
which mutates the in-scope `report` already holding `.outline` (py:1615). `Report.to_dict` serializes
`outline.to_dict() if self.outline else None` (py:462), so the retained outline lands in the FAILED
`meta.json`.

**On-disk assertion confirmed (not just in-memory).** New test `test_generate_report_h2_failed_meta_retains_outline`
(mod.rs:3409) injects EISDIR on `full_report.md` (pre-creates it as a directory) so plan_outline +
save_outline succeed but `assemble_full_report` fails post-planning. It asserts the ON-DISK `meta.json`
(written by the error tail's best-effort `save_report`) has `status="failed"` AND non-null `outline` with
`title="Future Prediction Report"` + 2 sections (mod.rs:3454-3479) — matching Python `save_report(report)`
at py:1751. Re-ran the EISDIR probe independently: `cargo test test_generate_report_h2_failed_meta_retains_outline`
= 1 passed.

**No NEW divergence from the restructure.** Happy-path side-effect ORDER diffed step-by-step vs Python
1577-1738 — identical: ensure_folder → ReportLogger+log_start → ConsoleLogger → update_progress(pending,0)
→ save_report → status=Planning → update_progress(planning,5) → log_planning_start → emit(Planning,0) →
plan_outline(prog//5 closure) → report.outline= → log_planning_complete → save_outline →
update_progress(planning,15) → save_report → info(outlineSavedToFile) → status=Generating → assemble →
status=Completed/completed_at → log_report_complete → save_report → update_progress(completed,100) →
emit(Completed,100) → info(reportGenDone) → close console. progress.json sequence = pending 0 / planning 5 /
planning 15 / completed 100. plan_cb rescale `(prog/5)` (mod.rs:1667) = Python `prog//5` (py:1613), test
asserts 30//5=6, 80//5=16, 100//5=20 reach the sink. Error tail emits NO sink event, calls log_error,
best-effort `let _ =` on save_report + update_progress(failed,-1), closes+clears console_logger
(`self.console_logger.take()`, mod.rs:1800) — exact map of py:1740-1764. Diff vs HEAD: `src/report/mod.rs`
ONLY, **884 insertions / 0 deletions** → template path (`generate_stream`/`generate`/`PredictionReport`,
mod.rs:1304/1415/81) definitionally untouched.

**Green.** `cargo test -p teri test_generate_report_h2` = **11 passed**. Full suite `cargo test -p teri` =
**1214 passed / 6 ignored / 0 failed**. `cargo clippy --all-targets -p teri` = clean.

**Legit `[!]` nondeterminism ledger (no `[≠]`):**
- `report_id` = `report_{uuid12}` — random; tests pass explicit id, auto-gen verified by shape only.
- `total_time_seconds` = wall time — asserted as number-shape, not value.
- `created_at`/`completed_at` = local isoformat wall time — same posture.
- `interview_agents` pending U-020 — honest-err `[!]` on the tool, not on generate_report. Loop tolerates it.

**No `[≠]` introduced.** Every observable artifact the skeleton produces (meta.json, outline.json,
progress.json sequence, agent_log.jsonl orchestration lines, sink events) is PORTED, none skipped.

**h2 OVERALL VERDICT: PASS.** Round-1 downgrade fixed and regression-locked; no new divergence introduced
by the restructure. S-763 cleared for the **h2 skeleton scope** (kept `- [~]` in symbol-map with the h2-PASS
annotation; the section loop = h3 keeps the symbol open for full clearance).

---

## 2026-06-18 — U-024 h3 (per-section streaming loop in `generate_report`) — PASS

**Scope:** the `for i in 0..total_sections` loop (src/report/mod.rs:1714-1868) replacing h2's placeholder
assemble. Differential vs `report_agent.py:1636-1707`. Verifier: rust-port-parity-verifier (adversarial).

**Gates:** `cargo test -p teri` 1220 passed / 6 ignored; report suite 142 passed; 6 h3 tests pass;
`cargo clippy -p teri --all-targets` clean.

### TRAP #1 — final meta.json carries section content (clone-vs-reference) — CONFIRMED FAITHFUL
- Python: `report.outline = outline` (py:1615) is a REFERENCE; after the loop sets `section.content`,
  the final `save_report` (py:1722 → 2433 `json.dump(report.to_dict())`, ReportSection.to_dict py:404-408
  includes `content`) writes meta.json with POPULATED `outline.sections[*].content`. The intermediate
  `save_report` (py:1626) runs BEFORE the loop → empty content.
- teri: pre-loop clone at mod.rs:1682 (empty content) feeds the intermediate `save_report` (mod.rs:1699).
  POST-loop RE-ASSIGN at mod.rs:1896 `report.outline = Some(outline.clone())` (now populated) feeds the
  final `save_report` (mod.rs:1913 → manager.rs:714 `to_string_pretty(report.to_dict())`). save_report also
  re-saves outline.json with populated content (manager.rs:719-721, == Python py:2436-2437).
- Locked by `test_generate_report_h3_final_meta_has_section_content`: asserts both the returned
  `Report.outline.sections[*].content` AND on-disk meta.json `outline.sections[*].content` are non-empty.

### TRAP #2 — sink-event superset policy — LEGAL STRICT SUPERSET
- Faithful events present: (a) pre-section base_progress (mod.rs:1747 == py:1648-1653),
  (b) section-closure sub-progress inside generate_section_react (mod.rs:1773-1787 == py:1656-1665),
  (c) post-loop 95 assembling (mod.rs:1873 == py:1698-1699), completed 100 (mod.rs:1924 == py:1728-1729).
- ADDED: one post-section `section_content=Some(content)` event per section (mod.rs:1856). It changes NO
  Python-observable artifact: same `progress.json` writes (the superset event is on the SINK, not a
  `manager.update_progress` call — the update_progress sequence is byte-identical), same section_NN.md,
  same full_report.md, same agent_log.jsonl, same console output. Architect §1/§3-step7/§7.5 superset bar
  met → LEGAL (a strict-superset capability the dest provides for U-027 live streaming). NOT a divergence.
- Locked by `test_generate_report_h3_sink_events`: 2 content-carrying events (idx 1,2), non-empty content,
  plus the faithful (a)/(b)/(c)/completed events at correct progress values.

### Progress arithmetic — CONTRACTUAL, VERBATIM MATCH
- base_progress: `20 + ((i as f64/total as f64)*70.0) as i32` (mod.rs:1723) == `20 + int((i/total)*70)`
  (py:1638). For total=2: i=0→20, i=1→55. For total=3: i=1→23, i=2→46 (Rust `as i32` truncates toward
  zero == Python `int()` for positive). ✓
- section closure: `base + (prog as f64*0.7/total as f64) as i32` (mod.rs:1776) == `base + int(prog*0.7/total)`
  (py:1663). ✓
- section-done: `base + (70/total as i32)` (mod.rs:1842) == `base + int(70/total)` (py:1691). Integer
  division: 70/3=23 both sides. ✓
- update_progress write SEQUENCE faithful: per-section pre (base), per-section done (base+70/total),
  post-loop 95, completed 100; failed path -1 (i32 widening, h1). Final progress.json = completed/100 with
  2 completed_sections — locked by `test_generate_report_h3_progress_json_sequence`.

### Other checks
- save-section-immediately (mod.rs:1812 == py:1673), BEFORE next section's LLM call — empirically locked
  by `test_generate_report_h3_incremental_write` (mock records section_01.md exists at section-2 call[4]).
  `save_section` runs `clean_section_content` (manager.rs:312, f-landed) == Python `_clean_section_content`.
- REAL assemble over populated section files (mod.rs:1900, manager.rs:546 reads section_NN.md) — full_report.md
  contains both sections; locked by `test_generate_report_h3_full_file_tree`.
- agent_log.jsonl: 2 section_start (e) + 2 section_complete (h3 log_section_full_complete, mod.rs:1820) —
  locked by `test_generate_report_h3_agent_log_section_complete_lines`. `.trim()` == Python `.strip()`
  (mod.rs:1820 == py:1683). ✓
- No-downgrade of Y: template `generate_stream` path untouched; h2 behaviors (status machine, failed-meta
  retains outline, report_id shape) preserved — full suite green confirms no regression.

### Ledger (`[!]`/`[≠]`)
- `- [!]` interview_agents — U-020 InterviewBus not landed; honest-err stub; loop tolerates it (NOT a fail,
  architect §4). On the TOOL (S-319), not generate_report.
- `- [!]` report_id / total_time_seconds nondeterminism — tests assert by shape, not value (architect §7.2/7.3).
- `[≠]` — NONE. Every observable artifact (meta.json, outline.json, progress.json sequence, section_NN.md,
  full_report.md, agent_log.jsonl, console_log.txt) is PORTED. The sink-superset is a legal capability ADD,
  not a `[≠]` skip.

**h3 OVERALL VERDICT: PASS.** S-763's h3 scope cleared. The section loop is the LAST core piece of
generate_report; h2 (skeleton/planning/tails) + h3 (section loop) together fully verify the orchestration.
S-763 flipped to `- [x]` in symbol-map. h4 (U-027 ChannelSink/SseSink adapter seam) is optional polish per
architect §6 — NOT required for generate_report parity (NullSink covers it).

================================================================================
## 2026-06-18 — U-024 sub-cycle (i): `ReportAgent::chat` — PARITY PASS
================================================================================

VERDICT: **PASS**. Symbols cleared `- [x]`: S-748, S-749, S-753, S-764 (4/4 for this
sub-cycle). U-024 is now functionally complete (a✓…i✓) modulo h4 deferred to U-027 and
b2 insight_forge OQ-3 pending (both pre-existing, out of (i) scope).

Source: MiroFish/backend/app/services/report_agent.py:1766-1881 (chat),
829-857 (CHAT_SYSTEM_PROMPT_TEMPLATE/CHAT_OBSERVATION_SUFFIX), 882 (MAX_TOOL_CALLS_PER_CHAT).
Rust:   src/report/mod.rs:2124-2293 (chat), 385-417 (consts), 591-625 (ChatResponse).

DIFFERENTIAL EVIDENCE (Rust real code paths vs Python, byte-level):

[Target 2 — .format() brace] PROVEN byte-identical + SHA-256-matched.
  Rust probe (real CHAT_SYSTEM_PROMPT_TEMPLATE const + 3 sequential .replace) vs Python
  CHAT_SYSTEM_PROMPT_TEMPLATE.format(simulation_requirement=, report_content=, tools_description=)
  on fixed triple {sim="试 req {x}", rc="rep 内容", td="td: 描述"} (CJK + a stray "{x}" in the value):
    PY len=313 SHA256=a772be97ffac1583 ; RUST len=313 SHA256=a772be97ffac1583 ; BYTE_IDENTICAL=True.
  JSON-example line renders with SINGLE braces both sides:
    {"name": "工具名称", "parameters": {"参数名": "参数值"}}
  teri stores the const with SINGLE braces (Python's `.format()` {{→{ unescape is pre-applied),
  then 3 .replace() — names never collide with literal braces. NOT doubled-brace divergence.

[Target 1 — CHAR vs BYTE truncation, g1 class] PROVEN char-based (CJK differential).
  message[:50]            : Rust message.chars().take(50)        — 50 chars (not 150 bytes). MATCH.
  markdown_content[:15000]: Rust char_count>15000 ? char_indices().nth(15000) slice — CHAR. MATCH.
    15001 CJK ("中"*15001) → both keep 15000 chars + "\n\n... [报告内容已截断] ..." suffix. byte-identical.
    15000 CJK ("中"*15000) → NEITHER gets the suffix (char_count>15000 is False). byte-identical.
    The >15000 comparison is a CHAR count (chars().count()), NOT .len() bytes. CONFIRMED — no .len() slip.
  result[:1500]           : Rust char_count>1500 ? char_indices().nth(1500) slice — CHAR. MATCH.

[Target 3 — regex cleanup] PROVEN byte-identical (Rust regex crate vs Python re).
  Rust (?s)<tool_call>.*?</tool_call>  +  \[TOOL_CALL\].*?\)  then .trim()
  Input1 "Some text\n<tool_call>\n{...}\n</tool_call>\nAfter\n[TOOL_CALL] foo(bar)  "
    → both: "Some text\n\nAfter"   (DOTALL multiline strip works; bracket+trim works)
  Input2 "pre <tool_call>line1\nline2</tool_call> mid [TOOL_CALL]x(a)(b) post"
    → both: "pre  mid (b) post"    (non-greedy .*?\) stops at FIRST ')', leaves "(b)")
  (?s) DOTALL on first re, \[ \] \) escaped on second — confirmed correct.

CONTRACT CHECKS (side-by-side read, all match):
  - messages order: system → chat_history[-10:] (last-10 slice, roles preserved) → user msg. ✓
  - system_prompt = render + "\n\n" + get_language_instruction(). ✓
  - empty report_content → "（暂无报告）" placeholder. ✓
  - ReACT: max_iterations=2; tool_calls[:1] (≤1 exec/round); MAX_TOOL_CALLS_PER_CHAT=2 break;
    post-loop final llm.chat + clean + return; no-tool-call early return inside loop. ✓
  - tool_calls_made accumulates EXECUTED calls only (pushed inside the take(1) exec block). ✓
  - sources = each accumulated call's parameters["query"] default "" (both early-return and post-loop). ✓
  - observation = "\n".join(f"[{tool}结果]\n{result}") + CHAT_OBSERVATION_SUFFIX, appended as
    user msg AFTER the assistant response msg. ✓
  - temperature=0.5, max_tokens=None (Python passes only temperature=0.5). ✓
  - execute_by_name called with report_context="" (Python _execute_tool default ""; only insight_forge
    consumes it and only when non-empty → "" is faithful). ✓
  - ChatResponse.to_dict key order: response, tool_calls, sources; each ToolCall → {name, parameters}. ✓

ADJUDICATIONS:
  [≠] fetchReportFailed except path (py:1800-1801): LEGIT, not a disguised skip.
      get_report_by_simulation returns Option (swallows I/O err via .ok()?), so Python's
      `except Exception → logger.warning(fetchReportFailed)` path is INEXPRESSIBLE under teri's
      chosen signature. The warning is a non-contractual internal diagnostic with NO Python-observable
      artifact (no file/serialization/CLI/render dropped). Observable behavior (None/fetch-fail →
      report_content="" → "（暂无报告）") is preserved exactly; test (i)-6 confirms None→clean response,
      no panic. CONFIRMED inexpressible/non-contractual → covered.
  [!] interview_agents (U-020 not yet ported): execute_by_name returns honest unknown-tool err string,
      tolerated by the chat loop as an observation. Upstream-dependency [!], not a chat-method downgrade.
  llm-error convention (Err/Ok("")→""): teri's chat returns ChatResponse (infallible), so it cannot
      raise as Python would. unwrap_or_default() → "" → parse_tool_calls("")=[] → no-tool early return
      → "" response. Consistent with (e) generate_section_react's None-mapping. No Python feature lost
      (Python's only behavior on llm error is to propagate the exception); the divergence is confined to
      the unobservable error channel, forced by the deliberate infallible return type. ACCEPTABLE, not a [!].

NO-DOWNGRADE OF Y: template family (generate/generate_stream/parse_report_from_json) untouched.

TESTS: 11 `test_chat_i_*` tests green. Full lib suite 1216/1216 PASS (single-threaded; the 4
parallel-run console_logger failures are pre-existing global-tracing-subscriber mutex-poison
flakiness — confirmed green single-threaded, unrelated to (i)). clippy -p teri --all-targets clean.

PROBE HYGIENE: a temporary zzz_probe_regex_and_format test was inserted to capture the real Rust
regex/render output, diffed against Python, then REMOVED. src/report/mod.rs restored to 5058 lines;
`grep -c zzz_probe` = 0; crate builds clean. No probe artifact remains.

---

# U-025 sub-cycles (a)+(b) — Shared route seam + 4 project routes — PARITY VERDICT: PASS

**Date:** 2026-06-18 · **Verifier:** opus (rust-port-parity gate) · **Source X:** `MiroFish/backend/app/api/graph.py:36-117` + `app/api/__init__.py` + `app/__init__.py:43,66-69` · **Rust Y:** `src/api/graph.rs`, `src/api/mod.rs`, `src/server.rs`

## Verdict: PASS (for the (a) seam + the 4 (b) project routes). U-025 unit stays `- [ ]` pending sub-cycles (c)–(f).

Symbols cleared to `- [x]`: **S-794, S-795, S-796, S-797** (4/4 of sub-cycle b). The (a) shared seam (ApiError/build_llm/graph_router/create_app un-stub) is verified and rolled under S-024 (U-003 create_app, which stays PARTIAL — flips only when all three blueprints U-025/026/027 land).

## Per-route differential (status + EXACT JSON body — keys, order, values), proven via real HTTP through create_app

| Route | Case | Status | Body (key order) | Source match |
|---|---|---|---|---|
| get_project | seeded | 200 | `{success,data:to_dict}` | data = Python to_dict, 15 keys IN ORDER ✓ |
| get_project | missing | 404 | `{success,error}` (2-key, no traceback) | error=`api.projectNotFound` w/id ✓ |
| get_project | corrupt json | 500 | `{success,error,traceback}` (3-key) | Flask has NO try/except → uncaught exception=500 ✓ FAITHFUL |
| list_projects | empty/seeded | 200 | `{success,data,count}` | data array, count=len, created_at desc ✓ |
| list_projects | `?limit=abc` | 200 | success envelope | Flask type=int bad→default 50 (NOT 400) ✓ |
| list_projects | absent/`?limit=N` | 200 | — | absent→50, N→N ✓ |
| delete_project | Ok(true) | 200 | `{success,message}` | api.projectDeleted w/id ✓ |
| delete_project | Ok(false) | 404 | `{success,error}` | api.projectDeleteFailed w/id ✓ |
| reset_project | missing | 404 | `{success,error}` | api.projectNotFound ✓ |
| reset_project | ok | 200 | `{success,message,data}` | status machine ✓; graph_id/task_id/error→null ✓; data key order = to_dict ✓ |

## Shared-seam adjudications

- **U025-TRACEBACK `- [≠]` — UPHELD as legit non-contractual `[≠]`.** The 3-key shape `{success,error,traceback}` is byte-preserved (proven through HTTP on the corrupt-json 500). The `traceback` VALUE being a Rust `std::backtrace::Backtrace` string (not Python `traceback.format_exc()`) is non-contractual: the contractual keys are success+error; traceback is opaque debug text a frontend renders/ignores. The KEY IS PRESENT and POPULATED — this is NOT a key-drop / feature-skip. Survives the `[≠]` challenge.
- **U025-ROUTE-ORDER `- [!]` — RESOLVED.** Proven via the real HTTP path (not a direct handler call): `GET /api/graph/project/list` returns the LIST envelope (`count`+`data` array), confirming axum 0.7 ranks static `/project/list` above the capture `/project/:project_id`. No overlap panic.
- **ApiError IntoResponse:** client→2-key (no traceback) ✓; client_with→appends extra keys ✓; server→3-key ✓. All driven through `into_response()` + real HTTP.
- **build_llm:** `OpenAiAdapter::new(&config.llm)` per-request, `#[allow(dead_code)]` (not called by (b)). Compiles, correctly shaped, mirrors MiroFish per-handler service construction (graph.py:217,390). NOT in ApiState (DECISION-U025-1). ✓
- **create_app/routing:** `/api/graph/*` mount matches Flask `/api/graph` blueprint prefix ✓; `/health` still 200 ✓ (U-002/U-003 19 server tests pass); CORS scoped to `/api/*` — `/health` carries NO `access-control-allow-origin`, `/api/graph/*` carries it (proven w/ Origin header). ✓
- **preserve_order:** `serde_json` `preserve_order` feature active (Cargo.toml:35); all body key orders byte-faithful (success before data/message/error; data before count; message before data). ✓

## No-downgrade of Y

- Full suite: **1247 passed, 6 ignored** (`cargo test -p teri`). clippy `--all-targets` clean. 16 graph tests + 19 server tests (17 U-002/U-003 + 2 added non-regression) pass.
- The OTHER 6 graph routes (c–f: ontology/generate, build, task/:id, tasks, data/:id, delete/:id) are NOT wired (only 3 `.route` lines = 4 handler bindings) and have NO handler functions defined → axum default 404, NOT fake-200 placeholders. No silent stub.

## Probe hygiene
A temporary `tests/probe_u025.rs` (5 adversarial HTTP assertions: route-order, 500 3-key, reset/get key orders, CORS scoping) was inserted, run green (4 test fns ok), then REMOVED. Tree restored; `tests/probe_u025.rs` absent; crate builds clean. No probe artifact remains.

---

## 2026-06-18 · U-025 sub-cycle (c) · `POST /ontology/generate` (S-798) + `allowed_file` → **FAIL** (opus)

**Verdict: FAIL.** One genuine, observable behavioral divergence in the ported `allowed_file` symbol (a `Path::extension` vs `os.path.splitext` semantic mismatch on leading-multi-dot basenames). Symbol S-798 stays `- [ ]`. Full suite green (1264 passed, 6 ignored), clippy `--all-targets` clean, (a)+(b) routes + /health non-regressed, OpenAiAdapter `#[derive(Clone)]` additive (llm.rs:307) — but the divergence blocks the unit.

### Adjudicated targets (file:line both sides)

1. **CHAR-count (target 1): PASS.** Rust `all_text.chars().count() as i64` (graph.rs:437) == Python `len(all_text)` (graph.py:211) — both count Unicode scalars. Happy-path test (graph.rs:1304-1310) genuinely proves char≠byte: CJK `你好世界` (4 chars / 12 bytes), asserts `total_chars < byte_count`. A `.len()` slip would have been caught; it is correct.
2. **all_text header format (target 2): PASS.** Rust `format!("\n\n=== {} ===\n{}", file_info.original_filename, text)` (graph.rs:418) byte-matches Python `f"\n\n=== {file_info['original_filename']} ===\n{text}"` (graph.py:201). Uses `SeedIngestor::from_file().raw_text` RAW (graph.rs:411) + `text_processor::preprocess_text` SEPARATELY (graph.rs:414) — NOT `from_files` (whose header `=== 文档 {idx}: {filename} ===` at seed/mod.rs:51 differs). `document_texts` holds the preprocessed per-file text (graph.rs:416). Correct.
3. **ontology 2-key projection (target 3): PASS.** Rust projects exactly `{entity_types, edge_types}` with `unwrap_or(Value::Array(vec![]))` defaults (graph.rs:457-460) == Python `{"entity_types":…, "edge_types":…}` (graph.py:229-232), dropping any other generator keys. `analysis_summary` routed separately with `unwrap_or("")` (graph.rs:461-467) == graph.py:233. `generate` returns `Result<Value>` (ontology.rs:324) so the projection does real work (analysis_summary proves >2 keys returned).
4. **Response body shape (target 4): PASS.** 6-key data in exact order project_id/project_name/ontology/analysis_summary/files/total_text_length (graph.rs:476-486) == graph.py:238-248. `files` is the 2-key `{filename,size}` shape (graph.rs:398-401) == graph.py:192-195. serde `preserve_order` proven across suite. (But see coverage gap — the 200 assembly is not exercised through the real handler.)
5. **Validation 400s + ordering (target 5): PASS.** sim_req check (graph.rs:357-362) BEFORE file check (graph.rs:367-372) == graph.py:161-173. noDocProcessed calls `delete_project` THEN 400 (graph.rs:424-431) == graph.py:203-208; test `generate_ontology_400_no_docs_processed_disallowed_ext` (graph.rs:1147-1187) asserts the project WAS deleted (`projects.len()==0`). 500 → `ApiError::server` 3-key (tested graph.rs:913-929).
6. **allowed_file (target 6): FAIL — divergence below.** Empty/no-dot/uppercase/only-dot/unknown-ext all correct, BUT leading-multi-dot basenames diverge.

### THE DIVERGENCE (blocking)

`allowed_file` (graph.rs:287-294 → `SeedIngestor::is_supported` seed/mod.rs:30-37) uses Rust `Path::extension()`, which differs from Python `os.path.splitext()[1]` on basenames that are ALL leading dots + a name with no internal dot:

| input | Python `splitext[1]` → allowed (graph.py:30) | Rust `Path::extension` → allowed (seed/mod.rs:31) | match |
|---|---|---|---|
| `..txt` | `''` → **False** | `txt` → **true** | ✗ |
| `...txt` | `''` → **False** | `txt` → **true** | ✗ |
| `..md` | `''` → **False** | `md` → **true** | ✗ |
| `.hidden.txt` | `.txt` → True | `txt` → true | ✓ |
| `.a.txt`, `. .txt` | `.txt` → True | `txt` → true | ✓ |

- **Input:** a `files` part with `filename="..txt"` (or `...txt`, `..md`) as the only upload.
- **Expected (source):** Python rejects it (ext=`''`) → if sole file, `400 api.noDocProcessed` and the project is deleted.
- **Actual (Rust):** Rust accepts it (ext=`txt`) → file saved, extracted, sent to LLM, `200`.

This is the same class as the char-vs-byte slip: a destination-stdlib primitive (`Path::extension`) whose semantics differ from the source's (`os.path.splitext`'s documented "leading dots on the basename are not an extension separator" rule). It is observable (accept-vs-reject of a real upload) and contractual (`allowed_file` is a validation gate whose job is byte-faithful accept/reject parity). Not a `[≠]` (Python's behavior is trivially expressible in Rust). FAIL.

### Minimal fix (route back to porter)

In `allowed_file` (or `is_supported`), replicate `os.path.splitext`'s leading-dot rule: strip the basename's leading dots before taking the extension, e.g. take the substring after the LAST `.` but only if a non-dot, non-empty stem precedes it on the basename. Concretely: split basename on `.`; the extension is the final segment only when there is at least one non-empty, non-all-dots segment before it. The existing `allowed_file_only_dot_rejected` test (graph.rs:1001-1004) should be extended with `..txt`/`...txt`/`..md` → reject, plus `.hidden.txt`/`.a.txt` → accept (Python parity), to lock the fix.

### Handler test-coverage gap (verdict: GAP — close it alongside the fix)

The handler `generate_ontology` (graph.rs:313) hard-calls `build_llm` (concrete `OpenAiAdapter`, graph.rs:447) inline. The doc-comment at graph.rs:309-312 claims a `generate_ontology_inner` exists for mock injection — **it does not** (no such fn in the file). Consequence: NO test drives the real axum handler to a 200. The four e2e tests stop at a 400 before the LLM boundary; the "happy path" test (graph.rs:1268-1399) manually RE-IMPLEMENTS steps 4-10 with a `MockLlmClient` rather than calling the handler. So the handler's own project-state mutation (graph.rs:457-469) and response-assembly JSON build (graph.rs:476-486) — exactly the code the 6-key/2-key/key-order claims rest on — are executed by no test. The shapes are correct by reading, but unproven through the handler. Recommendation: extract a `generate_ontology_inner<L: LlmClient>(state, fields, files, llm: L)` that the axum handler delegates to with `build_llm`, and add ONE test through that inner with a `MockLlmClient` asserting the real 200 envelope (6 data keys, order, files 2-key, ontology 2-key). This closes the gap and makes the doc-comment honest. Bundle with the `allowed_file` fix.

### Flag ledger (this sub-cycle)
- `U025-FILEPARSER` (`- [!]`): RESOLVED — file text-extraction primitive landed (`SeedIngestor::from_file` raw_text + `text_processor::preprocess_text`); called correctly (raw + preprocess separately). Not a skip.
- `U025-CLONE` (`- [!]`): DONE — `#[derive(Clone)]` on `OpenAiAdapter` (llm.rs:307); additive, fields all Clone, zero behavior change. Confirmed.

### No-downgrade of Y
- `cargo test -p teri`: **1264 passed, 6 ignored** (5 suites). `cargo clippy -p teri --all-targets`: clean.
- (a)+(b) routes + `/health` non-regressed (route-order, get/list/delete/reset, health tests all green).

**S-798 → stays `- [ ]`.** U-025 stays `- [ ]` (d/e/f also pending). Re-port the `allowed_file` extension rule + close the handler 200-path coverage gap, then re-verify.

---

## 2026-06-18 — U-025 sub-cycle (c) `/ontology/generate` ROUND-2 RE-VERIFY → **PASS**

Re-verify of the two round-1 FAILs (porter fixed both). Source X: `MiroFish/backend/app/api/graph.py` (`allowed_file` 26-31, `generate_ontology` 122-255). Rust: `src/api/graph.rs`, `src/seed/mod.rs`, `src/api/mod.rs`.

### FIX 1 — `allowed_file` now matches `os.path.splitext` — **CONFIRMED**
Python ground truth captured via `os.path.splitext(f)[1].lower().lstrip('.')`. Rust `allowed_file` (graph.rs:306) rewritten: basename → empty/no-dot guard → `stem_start` (first non-dot index) → `after_leading` = `basename[stem_start..]` → `after_leading.rfind('.')` → lowercase suffix → `seed::is_allowed_ext` (canonical `SUPPORTED_EXTENSIONS`, NOT a 2nd hardcoded list). Traced all 13 table cases by hand + verified `Path::file_name` preserves leading dots (rustc probe):

| input | Python ext | Rust verdict | match |
|---|---|---|---|
| `..txt` `...txt` `..md` | `''` | REJECT (after_leading has no dot) | ✓ |
| `.txt` | `''` | REJECT | ✓ |
| `.hidden.txt` `.a.txt` `..a.txt` | `.txt` | ACCEPT | ✓ |
| `file.txt` `FILE.TXT` `a.PDF` | ext | ACCEPT (case-insensitive via `to_lowercase`) | ✓ |
| `noext` `` (empty) `foo.exe` | — | REJECT | ✓ |

- `seed::is_supported` (seed/mod.rs:40) UNCHANGED — still its own `Path::extension` codepath; only an **additive** `pub(crate) fn is_allowed_ext` (seed/mod.rs:30) was added, sharing the same `SUPPORTED_EXTENSIONS` const (no duplication). `json` in the canonical set is a pre-existing documented superset (not introduced here); all Python-set cases match exactly.
- 6 new FIX-1 boundary tests present (graph.rs:1096-1130) covering both reject (`..txt`/`...txt`/`..md`) and accept (`.hidden.txt`/`.a.txt`/`..a.txt`) sides. Plus existing `.`/empty/no-dot/exe/uppercase coverage.

### FIX 2 — handler now has REAL 200 coverage — **CONFIRMED**
- `generate_ontology_inner<L: LlmClient>(pm, llm, simulation_requirement, project_name, additional_context, files) -> Result<Json<Value>, ApiError>` EXISTS (graph.rs:438), contains steps 4-10: create_project → per-file save/extract(`SeedIngestor::from_file`)/preprocess → noDocProcessed project-delete → `total_text_length = all_text.chars().count()` + save_extracted_text → `OntologyGenerator::new(llm).generate` → 2-key ontology projection → status `OntologyGenerated` + save → 6-key response envelope.
- axum `generate_ontology` (graph.rs:354) does steps 1-3 (multipart single-pass collect + sim_req 400 + ≥1-named-file 400) then calls `generate_ontology_inner(&pm, build_llm(&state.config), …)`. **PURE REFACTOR** — char-count (`chars().count()`), header `\n\n=== {orig} ===\n{text}` (graph.rs:489), ontology 2-key {entity_types,edge_types}, 6-key data {project_id,project_name,ontology,analysis_summary,files,total_text_length}, files 2-key {filename,size} all intact vs round-1.
- New test `generate_ontology_inner_200_real_response_envelope` (graph.rs:1143) drives the REAL inner fn with `MockLlmClient` → asserts 6-key data (count==6) + each key present + files array len==2 each 2-key + CJK char_count < byte_count + `ttl == char_count`. Old happy-path test (graph.rs:1514) now CALLS `generate_ontology_inner` (no re-implementation). Handler doc-comment (graph.rs:350) correctly references the real `generate_ontology_inner` (no phantom symbol).

### No new divergence + green
- Round-1 PASSes hold: char-count, header format, ontology projection, 6-key response, validation 400 ordering, noDocProcessed project deletion (test asserts `list_projects==0` post-400), 500 3-key shape.
- `ApiError` `#[derive(Debug)]` (mod.rs:166) is **additive** — private fields, `client` 2-key / `server` 3-key / `client_with` shapes unchanged.
- `cargo test -p teri`: **1271 passed, 6 ignored** (5 suites). `cargo clippy -p teri --all-targets`: clean. (a)+(b) routes + `/health` non-regressed. No probes/`todo!`/`dbg!` in graph.rs.
- `[≠] U025-TRACEBACK` carried (3-key 500 contract preserved, value-only Rust-string divergence, non-contractual). `[!] U025-FILEPARSER` resolved, `[!] U025-CLONE` done.

**VERDICT: PASS.** S-798 (`generate_ontology`, route 5) → `- [x]`. U-025 stays `- [ ]` (S-799–S-803 / sub-cycles d/e/f pending).

---

## 2026-06-18 — U-025 sub-cycle (e): task-query routes — PASS

**Unit:** U-025 (`backend/app/api/graph.py` → `src/api/graph.rs`)
**Symbols verified:** S-800 `GET /task/<task_id>` (get_task), S-801 `GET /tasks` (list_tasks) → both `- [x]`.
**Differential:** HTTP-level via full `create_app` router (`/api/graph/*` nested under `/api`, confirmed server.rs:198,206). Source primitives (TaskManager::global/get_task/list_tasks, Task::to_dict) confirmed CALLED correctly; internals not re-verified (landed U-012).

### get_task (py:534-550) — PASS
- 200 seeded: `{"success":true,"data":<task.to_dict()>}` — key order success,data (preserve_order on). Test `get_task_200_data_matches_to_dict` asserts `json["data"] == task.to_dict()` exact (round-trip via singleton). to_dict 11-key shape identical to py:41-53 (task_id, task_type, status, created_at, updated_at, progress, message, progress_detail, result, error, metadata; status lowercase; isoformat).
- 404 missing: `{"success":false,"error":"Task not found: <id>"}` — 2-key, NO traceback. Uses `ApiError::client(NOT_FOUND, t_args("api.taskNotFound",[("id",id)]))`. i18n value byte-identical to Python locale (`en.json:339`/`zh.json:339`: `"Task not found: {id}"`/`"任务不存在: {id}"`, {id} substituted). Test `get_task_404_missing` asserts status 404, success=false, error contains id, no traceback key.

### list_tasks (py:553-564) — PASS
- 200: `{"success":true,"data":[...],"count":N}` — key order success,data,count. Handler calls `TaskManager::global().list_tasks(None)` which returns `Vec<Value>` ALREADY to_dict'd (task.rs:382-391) — handler does NOT re-to_dict (no double-serialize). `count = tasks.len()`. Test `list_tasks_data_array_and_count_consistent` asserts count==data.len(), seeded task present, count>=1 (robust to shared global).
- Python `TaskManager().list_tasks()` (no filter) ↔ teri `list_tasks(None)`: both unfiltered, both sorted newest-first by created_at (task.rs:389 `b.created_at.cmp(&a.created_at)` == py:172 `sorted(...reverse=True)`). No filtering/ordering divergence.

### Cross-cutting
- preserve_order: serde_json built with `preserve_order` feature (Cargo.toml:35) → json! macro emits keys in insertion order on both bodies. CONFIRMED.
- 404 client error is 2-key (no traceback); server/500 would be 3-key — correct client-vs-server split (api/mod.rs:177-185 client; 215-225 server).
- Singleton: handlers use `TaskManager::global()` (OnceLock process singleton, task.rs:206) matching Python `__new__` singleton — cross-request task visibility preserved. Tests robust to shared global (assert seeded-id present / count>=1, not exact totals).

### No-downgrade of Y
- Full suite: `cargo test -p teri` → **1275 passed, 6 ignored** (5 suites, 12.24s).
- Task-route tests: get_task_200_data_matches_to_dict, get_task_404_missing, list_tasks_data_array_and_count_consistent, task_routes_non_regression_existing_routes_still_work → all ok.
- clippy `-p teri --all-targets`: clean (no issues).
- (a)/(b)/(c) routes + /health: non-regression test green.

**VERDICT: PASS.** S-800, S-801 → `- [x]`. U-025 stays `- [ ]` pending sub-cycles (d) S-799/`/build` and (f) S-802/S-803 (`/data`, `/delete`).

---

## 2026-06-18 — U-025 sub-cycle (d): `POST /build` (S-799) + completion-hook extension — PARITY VERDICT

**Scope:** route 6 `POST /build` (`build_graph` handler, `src/api/graph.rs:608-834`) + the additive
project-completion hook (`ProjectCompletion`, `apply_completion_success/failure`,
`build_graph_async_with_completion`, `src/services/graph_builder.rs:76-274`) vs Python
`backend/app/api/graph.py:260-529`.

**Method:** differential read of every guard (status + JSON shape, file:line both sides), driven
completion-hook tests that AWAIT the spawned task to terminal then RELOAD project from disk, additive-extend
non-regression checks, full-suite + clippy gate.

### Completion-hook preservation (THE no-downgrade point) — CONFIRMED
The architect's own option-(ii) would have DROPPED three project-state observables (graph_id,
status=COMPLETED/FAILED, error). The refined decision (A: additively extend with `Option<ProjectCompletion>`)
is faithfully implemented and DRIVEN:
- SUCCESS: `apply_completion_success` (graph_builder.rs:258-264) → reload project, `status=GraphCompleted`,
  `graph_id=Some(task_id)`, save. Test `completion_hook_success_sets_graph_completed_and_graph_id`
  (graph.rs:2503-2620) spawns via `build_graph_async_with_completion` + `ProjectCompletion`, polls task to
  terminal, then **reloads from disk** (`pm.get_project`, graph.rs:2608) and asserts status=GraphCompleted +
  graph_id==task_id + error=None. PASS (ran individually: 1 passed).
- FAILURE: `apply_completion_failure` (graph_builder.rs:268-274) → reload, `status=Failed`,
  `error=Some(err)`, save. Test `completion_hook_failure_sets_failed_status_and_error`
  (graph.rs:2629-2735) forces an LLM error, awaits FAILED terminal, **reloads from disk** (graph.rs:2726),
  asserts status=Failed + error contains the LLM message + graph_id=None. PASS.
- Best-effort: both helpers swallow reload/save errors (`let Ok(Some(..)) = .. else { return }` +
  `let _ = save_project`, graph_builder.rs:260/263/270/273) — a transient FS error cannot panic the worker
  (mirrors Python build_task try/except). Mapped to graph.py:472-474 / 500-502. CONFIRMED — narrowing is NOT
  back; the persisted terminal project state is observable via `Project::to_dict` (status/graph_id/error all
  serialized, project.rs to_dict) → served by GET /project/<id>.

### Additive-extend = U-015 NOT regressed — CONFIRMED
- `build_graph_async` KEEPS its 7-arg signature (graph_builder.rs:116-124) and delegates to
  `*_with_completion(.., None)` (graph_builder.rs:130-140).
- `build_graph_worker_inner` is UNCHANGED / project-agnostic: 7 params, no completion/project_id
  (graph_builder.rs:282-290). All completion mutations live in the `build_graph_worker` wrapper.
- `ProjectManager` is `#[derive(Clone)]` over a single `PathBuf` (project.rs:336-338) → additive,
  Send+Sync+'static, safe to move into tokio::spawn.
- NO production caller of the 7-arg `build_graph_async` exists (only test callers); the sole production
  caller of `*_with_completion` is the new `build_graph` handler (graph.rs:801) — nothing forced to change.
- U-015 tests green byte-unchanged: `test_build_graph_async_returns_task_id_immediately`,
  `test_build_graph_worker_inner_completes_with_result`, `build_graph_async_7arg_u015_non_regression`
  (all 1 passed).

### Guard sequence (verbatim, status + body) — ALL CONFIRMED
1. ZEP missing → 500 `{success:false,error:configError}` 2-key (NOT 3-key traceback). gate=`config.zep_api_key`
   empty (graph.rs:618-627 vs graph.py:286-295). Test `build_graph_500_zep_key_missing` asserts no traceback key. PASS.
2. missing/empty project_id → 400 requireProjectId (graph.rs:638-644 vs 302-306). Tests _400_missing/_empty. PASS.
3. project not found → 404 projectNotFound(id) (graph.rs:651-659 vs 309-314). Test _404. PASS.
4. status==Created → 400 ontologyNotGenerated (graph.rs:670-675 vs 319-323). Test _400_ontology_not_generated. PASS.
5. status==GraphBuilding && !force → 400 `{success:false,error:graphBuilding,task_id:<id|null>}` via
   `ApiError::client_with` (graph.rs:681-696 vs 325-330); task_id key present (Null if absent). Test
   _400_graph_building_without_force asserts task_id=="existing-task-id-123". PASS.
6. force && status∈[GraphBuilding,Failed,GraphCompleted] → reset OntologyGenerated + clear
   graph_id/graph_build_task_id/error (graph.rs:702-712 vs 333-337). Test _force_reset_path_proceeds. PASS.
7. get_extracted_text None **or empty** → 400 textNotFound (graph.rs:764-779 handles Some("")
   matching Python falsy `if not text`, vs 349-354). Test _400_text_not_found. PASS.
8. ontology None → 400 ontologyNotFound (graph.rs:784-792 vs 357-362). Test _400_ontology_not_found. PASS.

### Defaults / falsy-fallbacks — CONFIRMED
- graph_name = data||(project.name if non-empty else "MiroFish Graph") — `.filter(|s| !s.is_empty())` +
  empty-name branch (graph.rs:719-730 vs graph.py:340). literal "MiroFish Graph" kept.
- chunk_size/overlap = data||(project value if >0 else config default 500/50) — `if ps <= 0` replicates
  Python `or` falsy (0→default) (graph.rs:737-753 vs 341-342). config defaults 500/50 match (config.rs:256-257
  == config.py:44-45). project.chunk_size/overlap updated in-memory, saved at step 15 (graph.rs:758-759 vs 345-346).

### Response + reorder — CONFIRMED
- 200 `{success:true,data:{project_id,task_id,message:graphBuildStarted(taskId)}}` key order (graph.rs:826-833
  vs 515-522; i18n key graphBuildStarted en.json:336 exists). Test _200_happy_path asserts shape + UUID task_id.
- project.status=GraphBuilding + graph_build_task_id=task_id persisted after 200; graph_id=None until COMPLETED
  (graph.rs:818-820; test asserts saved.graph_id.is_none()). PASS.
- Reorder (task_id-first then save vs Python save-before-spawn): NON-OBSERVABLE — both visible to polls only
  after handler returns; worker touches project only at terminal (strictly after step-15 save). CONFIRMED.

### `[≠]` adjudications
- `[≠] U025-TASKNAME` (`构建图谱:{graph_name}` → `"graph_build"`): **LEGIT non-contractual.** Task name is a
  display label; graph_name preserved in task metadata (graph_builder.rs:171). Changing the token would
  reopen U-015's verified `task_type` shape. Not a distinct observable output. ACCEPTED.
- `[≠] U025-GRAPHID-TIMING` (graph_id set at COMPLETED not mid-build): **LEGIT non-contractual.** Independently
  refuted observability: frontend tracks build via `/task/<id>` (the /build response + graphBuildStarted msg
  point there); graph_id is consumed ONLY post-COMPLETED to address `/data/<graph_id>` & `/delete/<graph_id>`
  (graph.py:569-622, which need a *completed* graph to fetch). The two handles (Zep server id vs teri task id)
  are not byte-comparable. Sub-divergence checked: on FAILED, Python may leave a stale mid-build Zep graph_id
  while teri sets None — but a FAILED project's graph_id is not a consumed success-path observable (graph view
  gates on status==GraphCompleted), so non-contractual; teri's FAILED⇒None is consistent (status⇔graph_id).
  ACCEPTED.
- `[≠] U025-TRACEBACK` (carried): 3-key `{success,error,traceback}` envelope preserved (ApiError::server);
  only the value differs (Rust ctx vs Python stack). Non-contractual. ACCEPTED.
- Empty/missing JSON body: handler uses `Option<Json<Value>>` defaulting to `{}` (graph.rs:610,633) —
  replicates Python `request.get_json() or {}`; no 400/415 on empty body. CONFIRMED.

### No-downgrade of Y
- Full suite: **1289 passed, 6 ignored** (`cargo test -p teri`, 5 suites).
- clippy `-p teri --all-targets`: clean (no issues).
- (a)/(b)/(c)/(e) routes + /health: `build_route_non_regression_existing_routes_still_work` +
  `task_routes_non_regression...` green.

**VERDICT: PASS.** S-799 (`build_graph`, route 6) → `- [x]`; the `build_graph_async_with_completion`
symbol verified as part of it. U-025 stays `- [ ]` pending sub-cycle (f) (S-802 `/data`, S-803 `/delete`).
Test count: 1289 passed, 6 ignored.

---

## 2026-06-18 — U-025 sub-cycle (f): GET /data/:graph_id + DELETE /delete/:graph_id — VERDICT: FAIL

**Scope:** S-802 (`GET /data/<graph_id>` → `get_graph_data`), S-803 (`DELETE /delete/<graph_id>` → `delete_graph`).
Source: `MiroFish/backend/app/api/graph.py:569-622`, output shape `graph_builder.py:426-501`.
Rust: `src/api/graph.rs` `get_graph_data` (921-1074), `delete_graph` (1093-1119).

### Health gate (Y not downgraded)
- `cargo test -p teri`: **1296 passed, 6 ignored** (5 suites). (a)-(e) routes + /health intact.
- `cargo clippy -p teri --all-targets`: clean.

### Differential checks (passing)
- ZEP-missing → 500 2-key (no traceback) for BOTH routes: faithful (`get_graph_data_zep_missing_500`,
  `delete_graph_zep_missing_500`).
- graph-not-found → 500 server 3-key (traceback present): faithful (`get_graph_data_graph_not_found_500`).
  Python's `builder.get_graph_data` raise → `except` → 500 w/ traceback; teri's `ApiError::server`.
- Happy 200 envelope `{success,data:{graph_id,nodes[],edges[],node_count,edge_count}}`: faithful.
- Node shape: 6 keys exact `{uuid,name,labels,summary,attributes,created_at}`; teri-present uuid←id,
  name, labels←[kind]; summary=""/attributes={}/created_at=null. Verified by `assert_eq!(nobj.len(),6)`.
- Edge shape: 14 keys exact; counts recomputed from arrays; node_map source/target name resolution correct.
- valid_at/invalid_at: `Relation.valid_at: Option<(u64,Option<u64>)>` → start-string / end-string-or-null;
  faithful to Python `str(valid_at) if valid_at else None`.
- delete success → `{success:true,message:graphDeleted(id)}`, message contains id. i18n keys present.

### ADJUDICATION 1 — edge `uuid` random-per-request: **DOWNGRADE → FAIL** (the offending item)
graph.rs:1022 maps edge `uuid` ← `Uuid::new_v4()` **freshly per request per edge**. This is NOT a faithful
`[≠]` and NOT what the architect mandated (4b lists edge fields as `[≠] inexpressible` → which means
**default**, i.e. null, exactly as the porter did for `created_at`/`expired_at`/`episodes`). Random-per-call
is a porter-invented value that breaks the contract:

- **Python's `edge.uuid_` is the Zep server's STABLE edge identity** (graph.py:479; same `uuid_` used to key
  `node_map` and to fetch episodes `uuid_=ep_uuid` in graph_builder.py). It is reused across calls — a real
  identity a consumer references.
- **A consumer DOES rely on edge-uuid stability.** `frontend/src/components/GraphPanel.vue`:
  - `:116` `:key="loop.uuid || idx"` — Vue list-reconciliation key for self-loop edges.
  - `:118/:122/:126/:129` `expandedSelfLoops.has(loop.uuid||idx)` / `toggleSelfLoop(loop.uuid||idx)` — the
    self-loop expand/collapse UI state is a `Set` **keyed on edge `uuid`** (`expandedSelfLoops`, def `:255`,
    mutate `:274-282`). Self-loop edges carry the full edge incl. `uuid` (`...e` at `:378-382`), so
    `loop.uuid` IS the API edge uuid.
  - The `|| idx` fallback was written for Python's **null** case (`v-if="loop.uuid"` at `:130`), i.e. a
    *missing* uuid — NOT a *changing* one. With random-per-request uuids, every refetch changes the `:key`
    and orphans the `expandedSelfLoops` Set entries → observable UI-state break (expanded items collapse /
    wrong items stay flagged across `GET /data/:id` calls). Two GETs for the same edge return different
    uuids — a genuine contract break, not a non-contractual artifact.
- This is the same downgrade class as MiroFish→teri cycles 8-9: a portable observable behavior rationalized
  away. Random uuid produces a **distinct (and wrong) observable output** vs Python's stable id.

**Minimal fix (porter):** replace `Uuid::new_v4()` per request with the SAFER faithful value. Preferred:
**deterministic v5** — `Uuid::new_v5(&namespace, key)` where `key` = `format!("{src}|{tgt}|{kind}")` (stable
across calls, a real derived identity; survives refetch so the frontend Set/`:key` stays valid). Acceptable
alternative: **`null`** (honest "no Zep uuid", consistent with the other `[≠]` Zep defaults `created_at:null`
etc.; frontend's `|| idx` + `v-if="loop.uuid"` already handle null). Random-per-request is the one option that
is BOTH non-deterministic AND a fake identity — reject it. Update the graph.rs:1010-1022 comment (which
mis-claims "same observable semantics as Python") and add a test asserting two GETs return the SAME edge uuid
(or null).

### ADJUDICATION 2 — U025-GRAPHSTORE delete-no-op: **ACCEPTABLE `[!]`** (not a downgrade)
`delete_graph` returns the faithful success envelope; TaskManager has no remove API so the in-memory task
persists (subsequent `get_graph_data` still 200). This is recorded honestly (graph.rs:1106-1112,
architecture §4b/§7 U025-GRAPHSTORE), the porter did NOT fabricate a deletion, and Python's Zep delete is
itself fire-and-forget (no 404 for an already-absent graph). The *response contract* is byte-faithful; the
durable-delete side effect is a genuinely-deferred substrate (a future `GraphStore` unit), surfaced to the
owner — a recorded gap, not a silent drop. This `[!]` is sound. (It does not block S-803 on its own; S-803
fails only by association with the shared edge-uuid defect via the `/data` round-trip contract — but S-803's
own envelope is faithful. See verdict.)

### ADJUDICATION 3 — U025-ZEP-TEMPORAL value defaults: **ACCEPTABLE `[≠]`** (key-preserved, same class as U-015)
Every Python node/edge key is PRESENT (6/14 exact key sets verified by `nobj.len()==6`/`eobj.len()==14`);
only the Zep-server VALUES default (summary=""/attributes={}/created_at=null/fact=""/expired_at=null/
episodes=[]). This is the SAME class as U-015's Zep `[≠]` precedent (key preserved, value inexpressible) —
the Zep server is genuinely absent, so the bitemporal VALUES cannot be produced. NOT a key drop. Sound.
**Caveat:** edge `uuid` was incorrectly bundled into this `[≠]` by the porter — but uuid is NOT a Zep
inexpressible value; teri CAN produce a stable deterministic uuid from its own (src,tgt,kind). So uuid does
not belong in U025-ZEP-TEMPORAL; it is the Adjudication-1 downgrade.

### VERDICT: **FAIL.** S-802 and S-803 stay `- [~]` (unproven). U-025 stays `- [ ]` (9/10 routes verified;
sub-cycle (f) blocked on the edge-uuid fix). create_app S-024 remains partial regardless (U-026/U-027).
Route back to porter: single defect — edge `uuid` random-per-request → make it deterministic-v5 (preferred)
or null. All other (f) behavior is parity-faithful and will pass once the uuid is fixed. Test count: 1296.

---

## 2026-06-18 — U-025 sub-cycle (f) ROUND-2 RE-VERIFY (opus) — `GET /data/:id` + `DELETE /delete/:id`

**VERDICT: PASS.** Round-1 FAIL defect (per-request `new_v4` edge uuid nondeterminism — a real
contract break vs GraphPanel.vue's `:key` + expandedSelfLoops Set) is FIXED and verified. No new
divergence. U-025 is **route-complete 10/10** and the UNIT flips to `- [x]`. S-802/S-803 → `- [x]`.

### Check 1 — Determinism FIXED (the round-1 defect)
- `get_graph_data` edge uuid now `Uuid::new_v5(&EDGE_NS, edge_key)` where `EDGE_NS = NAMESPACE_OID`
  (fixed) and `edge_key = "{src_uuid}|{tgt_uuid}|{kind}|{valid_at_key}"` (graph.rs:1022/1037-1038).
  `valid_at_key` encodes the temporal window (`none` / `start` / `start-end`) — disambiguates parallel
  edges with different windows. Pure function of fixed inputs → deterministic by construction.
- New test `get_graph_data_edge_uuid_deterministic_across_requests` (graph.rs:3383) drives
  `GET /api/graph/data/:id` **TWICE** through the real HTTP app via `oneshot` and asserts
  `edges[].uuid` identical across the two responses (NOT just calling a fn twice). RAN + passed
  (0 ignored, confirmed via raw runner).
- Independent cross-check: computed the expected v5 offline for the seeded edge
  (src=1111…, tgt=2222…, kind `RelatedTo` (confirmed Display string at graph/mod.rs:72), valid_at None
  → key tail `none`): `uuid5(NAMESPACE_OID, "11111111-…|22222222-…|RelatedTo|none")` =
  `0c149838-c96f-5f70-80dc-c1e7123a93ed`. Deterministic + reproducible.

### Check 2 — No new divergence
- Node shape exactly 6 keys (uuid/name/labels/summary/attributes/created_at), edge exactly 14 keys
  (uuid/name/fact/fact_type/source_node_uuid/target_node_uuid/source_node_name/target_node_name/
  attributes/created_at/valid_at/invalid_at/expired_at/episodes) — asserted in
  `get_graph_data_happy_200_exact_key_shape` (`eobj.len()==14`, `nobj.len()==6`). node_count/edge_count
  == array lengths. teri-present fields + `[≠]` Zep defaults unchanged.
- uuid documented as synthesized-but-stable identity under `[≠] U025-ZEP-TEMPORAL` (graph.rs:1010-1019,
  1029-1031). The misleading "same observable semantics as Python" comment is gone; no leftover
  `new_v4` in the handler (only a regression note in the test comment). NAMESPACE_OID + key + collision
  behavior all documented.

### Check 3 — Parallel-edge handling
- Key includes `valid_at` to disambiguate same-(src,tgt,kind) edges with different windows. Truly
  identical edges (same src/tgt/kind/window) collapse to the same uuid — acceptable: semantically
  indistinguishable in teri's model (teri has no Zep server assigning distinct uuids). Documented, NOT
  silent data loss. `SerRelation.valid_at` is `Option<(u64,Option<u64>)>` with `#[serde(default)]`,
  matching `Relation.valid_at` (graph/mod.rs:93) — robust to graphs serialized before the field.

### Check 4 — Round-1 PASSes still hold
- get_graph_data: ZEP-missing 500 (2-key, no traceback), not-found 500 (3-key w/ traceback), happy 200
  shape — all still pass. delete_graph: ZEP-missing 500, success envelope {success,message:graphDeleted(id)},
  U025-GRAPHSTORE no-op (`delete_graph_task_persists_noop`) — all still pass. `[≠] U025-ZEP-TEMPORAL`
  value-defaults + `[!] U025-GRAPHSTORE` acceptable.

### Check 5 — Green
- `cargo test -p teri`: **1297 passed, 6 ignored** (was 1296; +1 = the new determinism test).
- The 7 graph (f) tests RAN + passed, 0 ignored (raw runner: get_graph_data_{zep_missing,graph_not_found,
  happy,edge_uuid_deterministic}, delete_graph_{zep_missing,success_envelope,task_persists_noop}).
- `cargo clippy -p teri --all-targets`: clean.
- `Cargo.toml` uuid features now `["v4","v5","serde"]` — additive (no other crate behavior change).
- (a)-(e) routes + /health non-regression test (`subcycle_f_non_regression_routes_ab_de_health`) passes.

### U-025 final `[!]`/`[≠]` ledger (all challenged + survive)
- `[≠] U025-ZEP-TEMPORAL` — Zep bitemporal node/edge fields teri's model cannot source; every Python
  key PRESENT, only Zep-only values defaulted (== Python's `or ""`/`or {}`/`None` fallbacks). Edge uuid:
  teri has no Zep server uuid → synthesized stable v5. **Non-contractual/inexpressible** — survives.
- `[!] U025-GRAPHSTORE` — no durable graph-by-id store (graph lives in task result); delete is a no-op.
  **Substrate-inexpressible** durable store; observable response output preserved. Survives `[!]`.
- `[≠] U025-TRACEBACK` — server 500 `traceback` value is a Rust string, not a Python stack; 3-key shape
  preserved. **Non-contractual** value — survives.
- `[≠] U025-TASKNAME` — Python `f"构建图谱: {graph_name}"` → stable `"graph_build"` token; graph_name in
  task metadata. **Non-observable** internal name — survives.
- `[≠] U025-GRAPHID-TIMING` — graph_id set once at COMPLETED (not at spawn). **Non-observable** to polls
  (visible only after handler returns either way). Survives.

**Note:** create_app S-024 stays `- [~]` (PARTIAL) — pending U-026/U-027 blueprint mounts. NOT flipped.

---

# U-026 sub-cycle (c) — 3 simulation routes + RunInstructions extension — 2026-06-19 (opus, parity-verifier)

**Verdict: FAIL** (2 PASS / 1 FAIL among routes; the extension FAILs). Worktree `port/mirofish`,
`/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri`. Differential read X(`MiroFish`)↔Y(teri),
plus `cargo test -p teri` = **1309 passed / 6 ignored / 0 failed**; all 8 sub-cycle-(c) tests pass
individually.

## Route 1 — POST /create (S-807) — PASS
Differential vs `simulation.py:165-237`. Every branch matches:
- empty body→`{}` (`body: Option<Json<Value>>` → `unwrap_or(json!({}))`, sim.rs:129) = `request.get_json() or {}`.
- missing/empty project_id→400 `api.requireProjectId` (sim.rs:134-139). `.as_str().unwrap_or("")` makes
  ""≡missing, matching Python's falsy `not project_id`.
- project-not-found→404 `api.projectNotFound` id-interpolated (sim.rs:144-150). i18n: teri en/zh strings
  byte-identical to MiroFish `locales/{en,zh}.json:322`; teri `t_args` `{id}`-replace (i18n/mod.rs:232-234)
  mirrors Python `value.replace('{id}',str(v))` (locale.py:59-61). Default locale zh both sides.
- graph_id = body.graph_id (non-empty) else project.graph_id; empty→400 `api.graphNotBuilt` (sim.rs:156-168).
  `.filter(|s| !s.is_empty())` correctly mirrors Python `body.graph_id or project.graph_id` empty-string semantics.
- enable_twitter/reddit default-true (sim.rs:171-174); body false honored.
- success `{success:true, data: state.to_dict()}` — `SimulationState::to_dict` (mgr.rs:300-335) is the
  17-key, declaration-order, status-as-`.value`-string, error-null port verified in U-023; route does not reshape.
- 500→`ApiError::server` 3-key `{success,error,traceback}` (`[≠] U025-TRACEBACK`, shape preserved).

## Route 3 — GET /list (S-811) — PASS
Differential vs `simulation.py:788-814`.
- `?project_id` filter → `list_simulations(Option<&str>)` (mgr.rs:1565-1597): None→all, Some→`project_id==pid`,
  skips `.`-prefixed + non-dir, empty when dir absent — faithful to `simulation.py:463-479`.
- body `{success,data:[to_dict...],count:len}`, key order data-then-count; `count==data.len()` (test-confirmed).
- **`?limit` refutation:** independently read `simulation.py:797` — Python reads ONLY
  `request.args.get('project_id')`. NO `?limit`. Porter's claim CONFIRMED; no narrowing.

## Route 2 — GET /:simulation_id (S-810) — **FAIL** (the carry-forward gate)
Differential vs `simulation.py:755-785`. Correct parts:
- not-found→404 `api.simulationNotFound` id-interpolated (sim.rs:220-231).
- success `{success,data:result}`, `result=state.to_dict()`.
- READY gate exact: `if sim_state.status == SimulationStatus::Ready` (sim.rs:238) = Python
  `state.status == SimulationStatus.READY`. `Ready.as_str()=="ready"` matches `to_dict` status serialization.
  Test `get_simulation_happy_path` proves a non-READY (created) sim has NO `run_instructions`;
  `get_simulation_ready_has_run_instructions` proves a patched-to-ready sim DOES.
- `run_instructions = RunInstructions::to_dict()` key order `simulation_dir, config_file,
  commands{twitter,reddit,parallel}, instructions, substrate_note` (mgr.rs:804-827); `scripts_dir` ABSENT —
  the ONE justified `[≠]` drop (teri has no `backend/scripts/` dir; survives the `[≠]` challenge:
  genuinely inexpressible). `commands`/`instructions` are NATIVE-EXPRESSED (correct in KIND — NOT a feature
  skip; the prose-only `substrate_note` downgrade the U-023 gate warned about was correctly avoided).

**DEFECT (no-downgrade / actionable-guidance):** the native guidance strings reference
`POST /api/simulation/{simulation_id}/start` — simulation_id in the URL **path** (mgr.rs:1696-1718,
both `commands.*` and `instructions`). But the authoritative start route is `POST /start` with
`simulation_id` carried in the **JSON body**:
  - Python: `@simulation_bp.route('/start', ...)` (`simulation.py:1451`), id read via
    `data.get('simulation_id')` (`simulation.py:1495`). There is NO `/<simulation_id>/start` route in source
    (grep-confirmed: exactly one start decorator).
  - teri ledger row **S-820** = `POST /start` (`simulation.py:1452`) → sub-cycle (g) will mount
    `POST /api/simulation/start`. Architecture route-table line 184 also lists `/start` (not `/:id/start`).
So the guidance points at a path (`/api/simulation/{id}/start`) that will NOT exist once (g) lands. The
prompt names this exact FAIL condition ("the guidance isn't pointing at a nonexistent path"). NATIVE-EXPRESSED
guidance must be CONCRETE & ACTIONABLE against the real endpoint; an id-in-path URL against a body-id route is
a non-actionable, wrong instruction → correctness defect, not a `[≠]`.

  - INPUT: GET a READY simulation `sim_abc`.
  - EXPECTED (faithful native): `commands.twitter` ≈
    `POST /api/simulation/start  body: {"simulation_id":"sim_abc","platform":"twitter"}`
    and `instructions` directing `POST /api/simulation/start` with `simulation_id` in the body.
  - ACTUAL: `commands.twitter` = `POST /api/simulation/sim_abc/start  body: {"platform":"twitter"}`
    (id in path, omitted from body) → unroutable against the real `POST /start`.

> Note: the design doc `u026-c-run-instructions.md` (L17) and this prompt both *assumed* `/:id/start`, but
> the authoritative source + ledger S-820 say `/start` (body id). The source is authoritative; the guidance
> must match it (or sub-cycle (g) must mount `/:id/start` AND adjust S-820 — but that would diverge from the
> faithful Python port, so the guidance is the correct thing to fix).

## RunInstructions / get_run_instructions extension (S-680) — **FAIL**
Additive change is well-formed: signature unchanged (`get_run_instructions(&self,&str)->Result<RunInstructions>`),
`[≠]` narrowed to `scripts_dir`+conda literals (folded into `substrate_note`), `to_dict` key order correct,
`scripts_dir` absent. The test `get_run_instructions_structural_fields` (mgr.rs:2130-2203) was EXTENDED, not
weakened — OLD asserts (`simulation_dir`/`config_file`/`substrate_note`/SimEngine) survive alongside the new
`commands`/`instructions` asserts; no coverage deleted. BUT the produced strings carry the same id-in-path
defect as Route 2. The existing tests pass only because they assert `cmd.contains("/start")` +
`cmd.contains(platform)` + `cmd.contains(sim_id)` — they DON'T assert the route SHAPE, so the wrong path
slips through. Extension is UNPROVEN until the path is fixed.

## Symbols (4 in scope): 2/4 covered
- [x] S-807 (POST /create) · [x] S-811 (GET /list) · [~] S-810 (GET /:id — FAIL) · S-680 extension FAIL.

## Required fix (route back to porter)
In `simulation_manager.rs::get_run_instructions` (mgr.rs:1696-1718), change `commands.{twitter,reddit,parallel}`
and `instructions` from `POST /api/simulation/{id}/start  body:{"platform":…}` to the body-id form:
`POST /api/simulation/start  body:{"simulation_id":"{id}","platform":"…"}`, mirroring the authoritative
`start_simulation` contract (`simulation.py:1451-1505`, ledger S-820). Then add a route-SHAPE assert to
`get_run_instructions_structural_fields` (and the route test) so the defect cannot recur. Re-verify Route 2 + S-680.

**Test count observed: 1309 passed, 6 ignored, 0 failed.**

---

## 2026-06-19 (opus) — U-026 sub-cycle (c) RE-VERIFY: S-810 + S-680 extension → PASS

Re-verification of the two items FAILed on 2026-06-19 (Routes 1 & 3 untouched, not re-litigated).
Worktree: `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch `port/mirofish`).
Source: `/home/drdave/Desktop/meta/MiroFish`.

### Prior FAIL (now fixed)
Native run-guidance referenced `POST /api/simulation/{simulation_id}/start` (id-in-PATH) — an
unrouteable route. Authoritative start route is `POST /start` with `simulation_id` + `platform` in
the JSON BODY (`MiroFish .../api/simulation.py:1451`, params at `1495-1505`; ledger S-820).

### (1) S-810 — `GET /:simulation_id` READY gate → **PASS**
- `get_run_instructions` (`simulation_manager.rs:1689-1731`): `endpoint = "POST /api/simulation/start"`
  (`:1698`); `mk(platform)` emits `body:{"simulation_id":"{id}","platform":"{platform}"}` (`:1699-1703`).
  All three `commands.{twitter,reddit,parallel}` and the `instructions` prose (`:1713-1723`) use the
  body-id form. NO id-in-path string in the function body (commands or prose).
- READY handler `api/simulation.rs:238-247`: `status == SimulationStatus::Ready` →
  inserts `run_instructions = get_run_instructions(...).to_dict()` into `result`.
- Full path routable: `server.rs:199` nests `simulation_router` under `/simulation`, under `/api`
  (`server.rs:207`) → `/api/simulation`; (g) adds `POST /start` → `POST /api/simulation/start`. ✓
- `to_dict()` (`simulation_manager.rs:804-827`) key order
  `simulation_dir, config_file, commands{twitter,reddit,parallel}, instructions, substrate_note`;
  `scripts_dir` absent. ✓
- `platform` named in each command; `instructions` mentions SimEngine + in-process path;
  `substrate_note` present. ✓
- Cross-check vs real start params (`simulation.py:1495-1505`): simulation_id (req), platform
  (default parallel), max_rounds, enable_graph_memory_update, force — instructions prose at
  `:1717-1720` describes all five faithfully. ✓

### (2) S-680 extension — `RunInstructions`/`get_run_instructions` + tests → **PASS**
- Additive: signature unchanged; U-023 structural asserts (simulation_dir/config_file/substrate_note)
  retained alongside the new ones (`simulation_manager.rs:2144-2160`).
- Regression guard `get_run_instructions_structural_fields` (`simulation_manager.rs:2186-2197`):
  asserts `cmd.contains("POST /api/simulation/start")`, `!cmd.contains("/simulation/{id}/start")`,
  and the body `"simulation_id":"{id}"`. NON-VACUOUS — a revert to id-in-path makes
  `contains("POST /api/simulation/start")` false AND `!contains(id-in-path)` false → test FAILS.
- Route guard `get_simulation_ready_has_run_instructions` (`api/simulation.rs:673-680`):
  asserts `cmd.contains("POST /api/simulation/start")` (the strong, non-vacuous guard) plus the
  id-in-path-absent check. Fails on revert. ✓

### Build / tests
`cargo test -p teri`: **1309 passed, 6 ignored, 0 failed.** Both named tests pass individually:
`get_run_instructions_structural_fields` ✓, `get_simulation_ready_has_run_instructions` ✓.

### Non-blocking note (doc-only; flag to porter, does NOT gate)
Stale id-in-path strings `POST /api/simulation/{id}/start` linger in DOC COMMENTS only —
`RunCommands`/`RunInstructions` rustdoc (`simulation_manager.rs:722-724, 756, 777`) and the
module DECISION note. These are not in any executable code path or serialized output (`to_dict()`
emits none of this prose), and the contract tests assert the correct shape. Recommend the porter
sync these comments to the body-id form for accuracy; not a parity/behavior divergence.

### VERDICT
- **S-810: PASS** — `symbol-map.md` → `[x]`.
- **S-680 extension: PASS** (resolved) — `symbol-map.md` row updated.

Sub-cycle (c) is clear to flip to `[x]` and commit.

---

## 2026-06-19 — U-026 sub-cycle (b): 3 entity-read routes (S-804/805/806) — PARITY PASS (opus)

**Verifier:** rust-port-parity-verifier (differential, fail-closed). Worktree `port/mirofish`.
**Source:** `MiroFish/backend/app/api/simulation.py:48-160` + `services/zep_entity_reader.py` + `utils/zep_paging.py`.
**Rust:** `src/api/simulation.rs` (3 handlers + `load_entity_reader_graph` helper, registered in `simulation_router`).
**Test count:** `cargo test -p teri` → **1327 passed, 6 ignored** (1323 baseline + 4 verifier-added). 21-test entity slice all green & executed (not skipped).

### Route 1 — GET /entities/:graph_id  vs get_graph_entities (py:48-90) — PASS
- ZEP guard: empty `config.zep_api_key` → 500 `api.zepApiKeyMissing` via `ApiError::client(INTERNAL_SERVER_ERROR,..)` → **2-key body, NO traceback** (`simulation.rs:146-151`; test `get_graph_entities_zep_guard_empty_500` asserts `traceback.is_none()`). Matches py:60-64.
- `entity_types` CSV: `simulation.rs:204-212` = `split(',').map(trim).filter(!empty)`, `if v.is_empty(){None}` — byte-for-byte the py:67 comprehension incl. the `if s else None` + all-empty→None. Test `..._empty_entity_types_csv_treated_as_none` (`?entity_types=,+,` → None → all). PASS.
- `enrich` parse: `simulation.rs:215` = `s.to_lowercase()=="true"` else default true — exact port of py:68 `.lower()=='true'` (NOT generic bool). **Verifier-added adversarial tests** prove it: `?enrich=1`→FALSE (no related_edges), `?enrich=yes`→FALSE, `?enrich=TRUE`→TRUE (edges populated), absent→TRUE, `?enrich=false`→FALSE. A generic bool-parse would fail `?enrich=1`; this REFUTED-and-survived. PASS.
- success body `{success:true, data: FilteredEntities::to_dict()}` — route does NOT reshape; `to_dict` (entity_reader.rs:214) emits exactly `entities, entity_types, total_count, filtered_count`. PASS.

### Route 2 — GET /entities/:graph_id/:entity_uuid  vs get_entity_detail (py:93-122) — PASS (with flagged [≠])
- ZEP guard as above (test `get_entity_detail_zep_guard_500`). PASS.
- reader `None` → 404 `t_args("api.entityNotFound",[("id",uuid)])` (`simulation.rs:254-257`). i18n key present en.json:341 / zh.json:341; `t_args` (`i18n/mod.rs:209`) does `result.replace("{id}", id)` — exact port of Python `t('api.entityNotFound', id=...)`. Test `get_entity_detail_not_found_404` asserts the uuid appears in the error string. PASS.
- `Some` → `{success:true, data: EntityNode::to_dict()}` (7-key). Test `get_entity_detail_found_200`. PASS.

### Route 3 — GET /entities/:graph_id/by-type/:entity_type  vs get_entities_by_type (py:126-156) — PASS
- ZEP guard + same `enrich` rule (shared parse; adversarial tests above cover Route 1's identical code; Route 3 reuses it `simulation.rs:290`).
- data key order EXACTLY `entity_type, count, entities` — built via explicit `serde_json::Map` insert order (`simulation.rs:302-305`). `count == entities.len()` enforced (`count = entities.len()`); tests `get_entities_by_type_happy_200` + `..._count_equals_len` assert `count == data.entities.len()` and `count==2`. PASS.

### Routing edge — NON-vacuous, PASS
`/entities/G/by-type/Person` (4 segs) resolves to get_entities_by_type, NOT get_entity_detail(uuid="by-type"). Test `route_by_type_not_captured_as_entity_uuid` asserts the RESPONSE SHAPE has `entity_type`+`count`+`entities` and NOT `uuid` — proving the right handler ran (axum segment-count disambiguation). Non-vacuous. PASS.

### GRAPH-LOAD-FAILURE MAPPING — explicit verdict: defensible [≠]/[!], NOT a downgrade
Challenge: does Python return EMPTY for an unknown graph_id, or propagate?
- **Routes 1 & 3** (`filter_defined_entities` → `get_all_nodes` → `fetch_all_nodes`): `zep_paging.py:44` retries ONLY `(ConnectionError,TimeoutError,OSError,InternalServerError)`; an unknown-graph 4xx propagates (no try/except in `filter_defined_entities`) → route `except Exception` → **500+traceback**. An *empty* graph → `[]` → empty FilteredEntities (200). So Python's unknown-graph contract for R1/R3 is **500**. teri's `load_entity_reader_graph` task-not-found → 500 (`server`, 3-key+traceback; test `get_graph_entities_graph_not_found_500` asserts traceback present). **MATCH** — and consistent with the U-025(f) `get_graph_data` precedent + DECISION-9.
- **Route 2** (`get_entity_with_context`, py:348-411): blanket `try/except → None` swallows ANY failure (incl. unknown graph_id) → route → **404**. teri Route 2 unknown-graph → **500** (verifier probe `probe_route2_unknown_graph_status` observed `500 Internal Server Error`). → **Status-code divergence (500 vs 404) on the unknown-graph_id input ONLY.**

**Classification: defensible `[≠]` (inexpressible substrate boundary), recorded — not FAIL.** Rationale:
  1. teri's reader borrows a locally-constructed `KnowledgeGraph` (graph_id==task_id, DECISION-9). For an absent graph there is NO task → no graph → the reader cannot be *constructed*; the load is a precondition that fails before the reader runs. Python's Zep client is always-constructable (network handle), so its blanket `except→None→404` is an artifact of that non-portable substrate, not a documented feature (the docstring describes only entity-not-found 404).
  2. No feature dropped: the PRIMARY, documented Route-2 contract — valid graph + missing *entity* → **404** — IS ported and faithful (test `get_entity_detail_not_found_404`). Only the secondary absent-graph case differs, and only in status code (both are `{success:false,error}` error bodies).
  3. Consistency: R1/R3 + `get_graph_data` all 500 on this same condition; Route-2 returning 404-on-absent-graph would be the *less* coherent choice under teri's load model.
  This is a narrowing-of-input-domain forced by the substrate, with the real contract preserved → defensible `[≠]`, NOT a portable feature skipped. **Flagged here explicitly** (the porter's uniform task→500 mapping did not call out the R2 404-vs-500 asymmetry; now recorded).

### Symbols
- **S-804 get_graph_entities → PASS → `[x]`**
- **S-805 get_entity_detail → PASS (with flagged [≠] on absent-graph status) → `[x]`**
- **S-806 get_entities_by_type → PASS → `[x]`**

### Verifier-added permanent regression guards (kept in tests)
`enrich_one_is_false_no_edges`, `enrich_yes_is_false_no_edges`, `enrich_uppercase_true_is_true_has_edges`, `probe_route2_unknown_graph_status`.

### OVERALL VERDICT: **PASS** (3/3 symbols). Observed test count: **1327 passed, 6 ignored**.

---

## 2026-06-19 (opus) — U-026 sub-cycle (e): profiles/config read routes ×5 — **FAIL**

**Scope:** S-813 `GET /<id>/profiles`, S-814 `/profiles/realtime`, S-815 `/config/realtime`, S-816 `/config`, S-817 `/config/download` + MTIME helper `python_isoformat_local_from` (project.rs:67) + `get_simulation_dir` `pub(crate)` (simulation_manager.rs:901). 45 api::simulation tests pass.

### VERDICT: FAIL — 2 downgrade-direction divergences (both UNTESTED). Symbols stay `- [ ]`.

**DIVERGENCE 1 (downgrade) — S-814 `profiles/realtime`, CSV (twitter) ragged/truncated rows.**
`api/simulation.rs:656` `csv::Reader::from_path` runs with the crate default `flexible(false)`: a row whose field-count ≠ header-count is a HARD csv error → caught by the handler's `try → []` → `profiles=[]`, `count=0`. Python `csv.DictReader` (simulation.py:1094-1095) is *lenient*: a SHORT row pads missing trailing fields with `None`→JSON `null`; a LONG row buckets extras under the `"null"` key — and **succeeds**. Differential (Python vs Rust, both through the full handler):
| mid-write input | Python | Rust |
|---|---|---|
| `H + "0,Alice,alice,bio he"` (truncated mid-field, no NL) | `[{...,"description":null}]` | `[]` |
| `H + "0,Alice"` (truncated after 2 fields) | `[{...3 nulls}]` | `[]` |
| `H + "0,A,a,b,s\n1,Bob\n"` (row1 OK, row2 short) | **2 rows** (row1 complete + row2 null-padded) | `[]` (loses the valid row1 too) |

This is NOT non-contractual: `/profiles/realtime` exists SPECIFICALLY to be polled DURING generation (its docstring + the `可能正在写入中` warning). The producer `oasis_profile_generator.py:1091-1117` writes the CSV row-by-row with `csv.writer`; a poll catching the file mid-`writerow` yields exactly these ragged/truncated states. So Python returns a non-empty `profiles` (and non-zero `count`) where Rust returns `[]`/`0` — strict information loss on the route's primary observable, on its designed use case. Fully EXPRESSIBLE in Rust (csv crate `.flexible(true)` + explicit zip-padding: short→pad with `null`, long→`"null"` key) → a portable feature silently narrowed under the shared `try/except`, NOT a defensible `[≠]`.
**Fix:** make the csv reader flexible and replicate DictReader's short/long padding (missing trailing fields → `Value::Null`; surplus fields → collected under key `"null"`), then keep the catch-all → `[]` only for genuinely-unparseable content (e.g. broken quoting).
Parity-preserved CSV facets (verified via Python `csv.DictReader` vs Rust csv-crate differential, 10 adversarial cases): header-ordered keys (teri `preserve_order` ON), all-string values, embedded-comma-quoted, embedded-escaped-quote `""`→`"`, empty field→`""`, CRLF, leading/trailing spaces preserved, trailing blank line dropped, no-data-rows→`[]`. ONLY ragged/truncated rows diverge.

**DIVERGENCE 2 (downgrade-direction) — S-815 `config/realtime`, empty-object config `{}` summary edge (porter-flagged item #4).**
`api/simulation.rs:848` `if config.is_object()` is TRUE for `{}` → `summary` appended. Python `if config:` (simulation.py:1232) is FALSE for `{}` (empty dict is falsy) → NO `summary`. Confirmed: Python emits NO `summary` key for a `{}` config; Rust emits `summary{total_agents:0, simulation_hours:null, ...}`. Observable divergence (extra key in the Rust body) on a reachable on-disk state (the route reads the file with no schema validation; `{}` is a valid placeholder/cleared/corrupt artifact). The handler's own comment (line 847, "non-null, non-empty object is truthy") contradicts the code — the intent was right, the predicate is wrong. NOTE: for non-object truthy/falsy JSON values (`[]`,`0`,`""`,`false`) Rust `is_object()` is FALSE = matches Python's "no summary" — the SOLE divergence is the empty object `{}`. Fully expressible → NOT a `[≠]`.
**Fix:** `if config.is_object() && !config.as_object().map(|o| o.is_empty()).unwrap_or(true)` (i.e. non-empty object only).

### Items 1–9 adjudication (per parity request)
1. **`get_profiles` error→status (S-813): PARITY-PRESERVED.** `TeriError::Sim`→404 (Python `ValueError`, simulation_manager.py:485 `raise`), all else→500. Missing profiles FILE→`Ok([])` (manager S-678, NOT Err) matches Python `if not exists: return []`. No input where 404↔500 flips. Manager calls `_get_simulation_dir` (creates dir) — consistent with Rust.
2. **`total_expected` raw clone: PARITY-PRESERVED.** simulation.rs:693-695 `state_data.get("entities_count").cloned()` → present(0)→`0`, absent→JSON `null`. Faithful to Python `.get("entities_count")` (stored int or None). Test `..._file_exists_mtime_and_state` pins `total_expected==50`.
3. **CSV: DIVERGES — see DIVERGENCE 1** (ragged/truncated only; all other DictReader facets match).
4. **`{}` summary edge: DIVERGES — see DIVERGENCE 2.**
5. **Key ORDER: PARITY-PRESERVED (all routes).** `serde_json` `preserve_order` ON (Cargo.toml:35) + explicit `Map::insert` sequence. Pinned by tests: profiles `[platform,count,profiles]`; profiles_realtime `[simulation_id,platform,count,total_expected,is_generating,file_exists,file_modified_at,profiles]`; config_realtime `[simulation_id,file_exists,file_modified_at,is_generating,generation_stage,config_generated,config]`; summary 8-key `[total_agents,simulation_hours,initial_posts_count,hot_topics_count,has_twitter_config,has_reddit_config,generated_at,llm_model]`. All match Python dict insertion order.
6. **`generation_stage` state machine: PARITY-PRESERVED (all 4 branches).** simulation.rs:818-831: preparing+profiles_generated→"generating_config"; preparing+!pg→"generating_profiles"; ready→"completed"; neither→`null`. Tests cover generating_profiles / generating_config / completed; the "neither"→null branch is exercised by `..._file_absent_shape` (no state.json → null).
7. **MTIME (`python_isoformat_local_from`): PARITY-PRESERVED.** project.rs:67-76 micros==0→`%Y-%m-%dT%H:%M:%S`, else→`%.6f`; local naive, no tz. Matches Python `datetime.fromtimestamp(st_mtime).isoformat()` (zero-frac omission + 6-digit micros, verified vs reference). File-absent→`Value::Null` (simulation.rs:635/773). Sub-µs rounding-vs-truncation is a non-contractual opaque-display artifact.
8. **`download_config` (S-817): PARITY-PRESERVED.** 200 = raw bytes + `Content-Type: application/json` + `Content-Disposition: attachment; filename="simulation_config.json"`; missing→404 `api.configFileNotFound`. Uses `sim_manager.get_simulation_dir` = Python `manager._get_simulation_dir` (S-671, both `makedirs`/`create_dir_all`) — **same path** as the realtime routes' direct `oasis_data_dir/<id>` join (both resolve to `{sim_data_dir}/{id}`). Test `download_config_happy_200_attachment` pins headers + byte-exact body.
9. **i18n: PARITY-PRESERVED.** profiles ValueError msg from manager (`{id}`); realtime sim-missing→404 `api.simulationNotFound` (`t_args id`, en/zh:344); config→404 `api.configNotFound` (351); download→404 `api.configFileNotFound` (352). Correct key + 404 status per route; client errors → 2-key body (no traceback), confirmed by `*_404` tests asserting `traceback.is_none()`.

### `[≠]` carried forward (legitimate, NOT part of this FAIL)
- `[≠] U025-TRACEBACK` (500 bodies carry Rust string, 3-key shape preserved) — unchanged, defensible.
- `[≠] U026-MTIME` — the helper itself is a faithful PORT (item 7), not a divergence.

### Route status after this verdict
- S-813 `profiles`: behavior MATCHES — but stays `- [ ]` (unit gate: a unit PASS requires ALL its symbols pass; the unit FAILs on S-814/S-815).
- S-814 `profiles/realtime`: **FAIL (DIVERGENCE 1)** → `- [ ]`.
- S-815 `config/realtime`: **FAIL (DIVERGENCE 2)** → `- [ ]`.
- S-816 `config`: behavior MATCHES — stays `- [ ]` (unit not yet PASS).
- S-817 `config/download`: behavior MATCHES — stays `- [ ]` (unit not yet PASS).

### Route back to porter (minimal fixes)
1. `api/simulation.rs:~656` — `csv::ReaderBuilder::new().flexible(true).from_path(...)`; replicate DictReader padding (zip header→record; missing trailing → `Value::Null`; surplus fields → push under key `"null"`). Add a ragged/truncated-CSV differential test (incl. the "row1 OK, row2 short → 2 rows" case).
2. `api/simulation.rs:848` — gate summary on non-empty object: `if config.as_object().is_some_and(|o| !o.is_empty())`. Add a `config={}` → no-summary test.

---

## 2026-06-19 — U-026 sub-cycle (g) `/start` + `/stop` — PARITY VERDICT: **PASS**

Verifier: rust-port-parity-verifier (fail-closed, default-skeptical). Source X = `MiroFish/backend/app/api/simulation.py`; port Y = `src/api/simulation.rs` + `src/services/simulation_runner.rs` + `src/services/simulation_manager.rs`. Architect landing = `.handoff/loop/findings/u026-g-architecture.md` (gap sanctioned). All 354 `simulation` tests green (`cargo test -p teri --lib 'simulation'` → 354 passed); 76 `api::simulation` tests green.

### g1 — `/stop` : FULL PARITY (no gap), PROVEN
- Body `or {}` tolerated; missing/empty `simulation_id` → 400 `requireSimulationId` (tests `stop_simulation_missing_id_400`, `stop_simulation_empty_body_400`).
- Error-class mapping verified: Python `stop_simulation` raises ONLY `ValueError` (runner.py:781 "模拟不存在", :784 "模拟未在运行") → `except ValueError→400`; teri runner returns `TeriError::Sim` for the SAME two cases → `map_runner_err` → 400 (test `stop_simulation_not_running_400`). Termination errors are swallowed internally on BOTH sides (Python 792-804 try/except; teri `terminate_handle` best-effort no-`?`), so a stop with termination trouble still 200s on both. The only 500 class is a `save_run_state` IO failure (non-`Sim` `TeriError` → 500) which mirrors Python `_save_run_state` raising → `except Exception→500`. **No teri-stop error 400s where Python 500s, or vice-versa.**
- Happy-path order matches Python EXACTLY: stop → get_simulation → `if Some` set Paused → `save_simulation_state`; a `None` state after a successful stop still returns 200 (the `if let Some` guards only the save). Body is exactly `run_state.to_dict()` under `{success,data}`. The stop primitive's STOPPED transition + `runs.remove` + `to_dict` shape is differentially exercised at the runner level (`stop_transitions_to_stopped`, drives a real RUNNING run). The HTTP 200-wrapper's Paused-persist is reachable only with a live RUNNING run (producer-gated at the HTTP layer); it reuses the independently-verified `save_simulation_state` path.

### g2 — `/start` : FULL boundary parity BEFORE the gap; honest-500 AT the gap, PROVEN
Every pre-gap path proven byte/status-faithful:
- id required→400 `requireSimulationId` (`start_simulation_missing_id_400`); platform default `parallel`; platform∉{twitter,reddit,parallel}→400 `invalidPlatform{platform}` (`start_simulation_bad_platform_400`, asserts the bad value is interpolated).
- `coerce_max_rounds` = Python `int()`: JSON int OK; `"5"`→5 accepted (`start_simulation_max_rounds_numeric_string_accepted` — passes validation, reaches 404); `≤0`→400 `maxRoundsPositive` (zero + negative tests); non-numeric `"abc"`→400 `maxRoundsInvalid`; null/absent→skip. String/number asymmetry confirmed: `"5.7"` rejected (parse i64 fails) matching Python `int("5.7")` raise.
- `get_simulation` None→404 `simulationNotFound{id}` (`start_simulation_not_found_404`).
- status≠Ready state machine: `check_simulation_prepared` gate; Running+live `runner_status==Running` → force?stop(swallow):400 `simRunningForceHint`; force→`cleanup_simulation_logs`+`force_restarted`; →Ready+save; not-prepared→400 `simNotReady{status}` (`start_simulation_not_ready_400`). Ready-status path skips the whole block (gate `if sim.status != Ready`, identical to Python `if state.status != READY`) — confirmed by the gap-500 test seeding a `ready` sim.
- graph_id resolution: `sim.graph_id` (empty→fallthrough via `!is_empty()`) else project.graph_id (`.filter(!is_empty)`) else 400 `graphIdRequiredForMemory`. BOTH empties fall through to the 400, matching Python `if not graph_id:` (true for `""`). Test `start_simulation_graph_id_required_for_memory_400`.

### THE GAP — adjudicated LEGITIMATE `[!]` (NOT a downgrade)
- (a) NO `MockLlm` / fabricated `run_state` / stubbed `RunInputs` anywhere in the success path. grep: the only `RunInputs`/`MockLlm`/`start_simulation`-call references in `src/api/simulation.rs` are in comments documenting the gap. No `todo!`/`unimplemented!`. The handler's terminal statement is `Err(ApiError::server(... GAP-U026-RUNINPUTS-BUILDER ...))`.
- (b) The gap is the FINAL statement; every 400/404 gate genuinely runs before it. PROVEN by `start_simulation_prepared_reaches_gap_500`: a prepared, valid, memory-disabled request reaches exactly this 500 (status 500, `success:false`, error contains `GAP-U026-RUNINPUTS-BUILDER` + "runtime not available"/"no RunInputs") — proving the whole boundary executed and emits a structured honest error, NOT a fabricated 200/400/404.
- (c) The deferred tail (status=RUNNING persist + 200 response `run_state.to_dict()` + `max_rounds_applied`/`graph_memory_update_enabled`/`force_restarted`/`graph_id`, the actual spawn) is the ONLY unported behavior; genuinely unreachable-until-producer, not silently dropped.
- Class: same producer frontier as `GAP-SOCIAL-WORLDSTATE`. `RunInputs<OpenAiAdapter>{engine,pool,..}` has NO production builder (verified independently: `SimConfig` is a thin tick-config — no `from_simulation_config`; zero code reads `*_profiles.*` into an `AgentPool` — the only pool-build is the test helper `run_inputs`). This is genuine INEXPRESSIBILITY-until-producer (U-028/029/030), a `[!]`, NOT a `[≠]`-disguised feature-skip. **Adjudicated legitimate.**

### `_check_simulation_prepared` — FULL port incl. side effect, PROVEN
- Missing dir / missing-files / read-err branches (`check_prepared_missing_dir`, `check_prepared_missing_files`). prepared_statuses set, config_generated gate (`check_prepared_config_not_generated_is_not_prepared`, `check_prepared_failed_config_generated_is_prepared`). profiles_count = len(reddit array else 0). Info-dict key order matches Python :337-346.
- AUTO-UPGRADE side effect PROVEN observable: `check_prepared_preparing_auto_upgrades_to_ready` seeds `status="preparing"`, calls, then RE-READS `state.json` from disk and asserts `status=="ready"` + a refreshed `updated_at`. Pretty-write (`to_string_pretty` = 2-space, non-ASCII preserved) matches `json.dump(ensure_ascii=False, indent=2)`. `updated_at = python_isoformat_local()` (`pub(crate)`, local-naive ISO) matches `datetime.now().isoformat()`.
- WRITE-FAILURE ORDERING edge VERIFIED by exact-code reasoning: Python sets local `status="ready"` as the LAST line of the try AFTER the dump (332), and the returned dict uses the LOCAL `status` (338) → on a write exception, `status` stays `"preparing"`. Teri's `effective_status` starts `"preparing"`, is set to `"ready"` ONLY in the write-success arm; on write-failure it stays `"preparing"`, and the returned info uses `effective_status` (1335). **Exact failure-mode match** (note: `state_data["status"]` is `"ready"` in-memory on both sides, but the *returned dict's* status is `"preparing"` on write-failure — identical).
- CJK reason strings byte-exact (verified via UTF-8 substring compare): 模拟目录不存在 / 缺少必要文件 / 读取状态文件失败:  / 状态不在已准备列表中或config_generated为false: status= .

### `cleanup_simulation_logs` — EXACT set, PROVEN
- Deletes exactly Python's set (runner.py:1136-1147): run_state.json, simulation.log, stdout.log, stderr.log, twitter_simulation.db, reddit_simulation.db, env_status.json, twitter/actions.jsonl, reddit/actions.jsonl. absent→skip; on remove-err→push to `errors`; success=errors.is_empty(). Does NOT touch config/profiles/state.json — PROVEN by `cleanup_simulation_logs_deletes_scoped_files_only` (asserts the 9 scoped files gone, config+profiles+state.json survive). sim_dir missing → success+message (`cleanup_simulation_logs_missing_dir_is_success`).
- `runs.remove(id)` in-memory cleanup is FAITHFUL, not invented: Python `del cls._run_states[simulation_id]` (runner.py:1171-1173, confirmed in source). `CleanupResult{success,cleaned_files,errors}` shape matches the dict the handler inspects via `.success`/`.errors`.

### Minor noted divergence (NOT a FAIL, non-contractual)
- `[~] U026-g-MAXROUNDS-FLOAT`: a JSON *number* `5.7` → teri `maxRoundsInvalid` (serde `as_i64` rejects non-integer floats), whereas Python `int(5.7)`=5 truncates. The string `"5.7"` matches (both reject). No known client sends a fractional float round-count (docstring + frontend show int/numeric-string only; no frontend `max_rounds` refs found). Non-contractual edge, same i18n key returned, NO capability dropped → not a `[≠]`-disguised skip, not a FAIL. Recorded for the trail; porter may optionally truncate floats to fully mirror `int()` if a client ever sends one.

### Ledger flags (consolidated, all legitimate)
- `[!] GAP-U026-RUNINPUTS-BUILDER` — `/start` 200-success path (spawn + response) → U-028/029/030. Producer frontier. ONLY unported behavior in (g). Verified honest, single-localized-swap.
- `[!] GRAPH-UPDATER-WIRING-PENDING` — `graph_for_updater` load+wrap, commented seam; graph_id 400-gate IS ported + tested. Sub-family of the above.
- `[!] SAVE-STATE-VISIBILITY` — `save_simulation_state` → `pub(crate)` (`simulation_manager.rs:923`). Cleared (visibility widened, used by /start + /stop).

### What IS proven vs deferred-to-producer
- PROVEN (full differential parity): /stop end-to-end (all error classes + order + None-guard); /start every 400/404/state-machine/graph-id path + the terminal honest-500 on a valid prepared request; `_check_simulation_prepared` incl. the observable auto-upgrade write + write-failure ordering; `cleanup_simulation_logs` exact deletion set + survivors + in-memory remove.
- DEFERRED-TO-PRODUCER (impossible until U-028/029/030, flagged not dropped): /start's 200 body (`run_state.to_dict()` + the 4 added keys), the RUNNING persist, the actual spawn; /stop's 200 HTTP-wrapper Paused-persist over a *live HTTP-started* run.

**VERDICT: PASS.** /stop = full parity. /start = full-boundary parity + the gap adjudicated a legitimate `[!]` (genuine inexpressibility-until-producer, no fabrication, gap is terminal). `_check_simulation_prepared` + `cleanup_simulation_logs` = faithful full ports incl. side effects. No disguised feature-skip; the one number-vs-string-float edge is a noted non-contractual `[~]`, not a downgrade.

---

## 2026-06-19 · U-026 sub-cycle (i) · world-state read routes · VERDICT: FAIL

**Unit:** U-026 (i) — `GET /<sim>/actions` (S-824), `/timeline` (S-825), `/agent-stats` (S-826).
**Port:** `.worktrees/mirofish-port/teri` `src/api/simulation.rs` handlers + `src/services/simulation_runner.rs` primitives.
**Source X:** `MiroFish/backend/app/api/simulation.py:1864-1982`, primitives `simulation_runner.py:955-1100`.

### VERDICT: FAIL — two observable-output downgrades in the timeline/agent-stats serialization

`serde_json` is built with `features=["preserve_order"]` (Cargo.toml:35) → **JSON key insertion order is the wire order and IS contractually observable.**

**[FAIL-1] timeline drops 2 fields (`first_action_time`, `last_action_time`).**
- Source `get_timeline` result dict (`simulation_runner.py:1045-1054`) emits **9 keys** ending in `"first_action_time"` and `"last_action_time"` (tracked at :1026-1027,:1039).
- Port `TimelineEntry` (simulation_runner.rs:4568-4577) has **no timestamp fields**; `TimelineRound` (:4528-4535) never tracks them — the porter left a bare comment "Track first/last timestamps ... we need to handle this" (:4021) and never did. `TimelineEntry::to_value` (:4581-4594) emits **7 keys**.
- Input: any sim with ≥1 action → `/timeline`. Expected entry keys (Python): round_num, twitter_actions, reddit_actions, total_actions, active_agents_count, active_agents, action_types, **first_action_time, last_action_time**. Actual (Rust): the same minus the last two. A distinct observable output (frontend timeline view consumes these). This is a dropped-field DOWNGRADE, not a `[≠]`.

**[FAIL-2] agent-stats key ORDER diverges (`action_types` misplaced).**
- Source `get_agent_stats` dict (`simulation_runner.py:1075-1083`) order: agent_id, agent_name, total_actions, twitter_actions, reddit_actions, **action_types**, first_action_time, last_action_time.
- Port `AgentStats::to_value` (simulation_runner.rs:4647-4654) order: agent_id, agent_name, total_actions, twitter_actions, reddit_actions, first_action_time, last_action_time, **action_types** (action_types emitted LAST).
- With preserve_order the serialized byte order differs → not byte-exact. All 8 keys present but ordered wrong.

### What PASSES (do not re-port)
- **actions wrapper (S-824):** byte-exact. `{success:true,data:{count:len,actions:[to_dict]}}`. `AgentAction::to_dict` (simulation_runner.rs:199-216) = 9 keys, identical order to Python (:61-72). count==len confirmed. PASS.
- **timeline/agent-stats envelope + count keys:** `rounds_count`/`agents_count` == list length, `success:true` envelope, nesting all correct. Only the inner entry shapes fail.
- **Flask type=int fallback (item 3):** `?limit=abc`→default 100 confirmed green by `get_actions_reads_tail_with_pagination_and_int_fallback`. agent_id/round_num None-default path correct (`.and_then(parse).ok()` → None on absent/unparseable). PASS.
- **platform empty-string (item 7):** `Some("")` treated as no_filter via `platform.is_none() || platform==Some("")` (simulation_runner.rs:3897) — consistent with the (h) fix. PASS.
- **500 path (item 6):** primitive `Result::Err` → `.map_err(ApiError::server)`; no traceback on 200. PASS.
- **Empty-log contract (item 5):** absent actions.jsonl → `count:0`/`[]` is FAITHFUL to Python on an absent log (Python reads tail → [] too). Not a stub. The `seed_actions` helper writes REAL jsonl so the populated tests genuinely exercise read+group+stats. The data-starved-in-prod gap is the legitimate `[!] U026-i-PRODUCER-PENDING` (producers land U-028/029/030).

### Why the green tests did NOT catch this
`get_timeline_empty_and_populated` (simulation.rs:4118-4122) asserts only `round_num` + `total_actions` present — written to the downgraded shape, so it passes blind to the 2 missing keys. `get_agent_stats_empty_and_populated` (:4172-4173) asserts only `agent_id`+`total_actions` present — never checks key order. Green build is necessary, not sufficient. Note: primitives S-621/S-622 are still `- [ ]` in symbol-map (NOT verified in U-022d as the prompt assumed) — consistent with the downgrade living in them.

### `[≠]` adjudication — item 4, negative `?limit`
`?limit=-5`: Python `int("-5")=-5` → slice `all[off:off-5]`; teri `parse::<usize>()` fails → default 100. **Adjudicated a defensible `[~]`/`[≠]`, NOT a blocking downgrade:** a negative limit is a non-contractual/nonsensical input (Python's own behavior on it — a backward slice yielding `[]` or fewer — is incidental, undocumented), and this is the SAME `parse::<usize>().unwrap_or(default)` precedent accepted for U-025 `?limit`. Consistent, non-material. Does NOT gate the unit. (The unit FAILs on FAIL-1/FAIL-2, not this.)

### Residual flags
- `[!] U026-i-PRODUCER-PENDING` — read path proven on real jsonl fixtures; data-starved in prod until SimEngine writes actions.jsonl (U-028/029/030). Faithful empty contract. Legitimate `[!]`.
- `[~]` negative-`?limit` int divergence — non-contractual, consistent with U-025 precedent. Non-blocking.

### Symbols
- S-824 `/actions` → **`- [x]` eligible** (wrapper + actions serialization byte-exact, fallback/filter/empty all proven). [Verified PASS.]
- S-825 `/timeline` → **stays `- [ ]`** (FAIL-1: 2 dropped fields).
- S-826 `/agent-stats` → **stays `- [ ]`** (FAIL-2: key order wrong).
- Unit U-026(i): **FAIL** (rollup: not all symbols `- [x]`/`- [≠]`).

### Minimal fix (route back to porter)
1. `TimelineRound`: add `first_action_time: String`, `last_action_time: String`. On first insert seed both = action.timestamp. Per action update `last_action_time = action.timestamp`. (Mind sort order: `get_all_actions` returns DESC/newest-first, so the FIRST action seen per round is the LATEST — Python iterates the same DESC list and assigns `first_action_time` from the first-seen + overwrites `last_action_time` each iter, so faithfully: seed both from first-seen, then set `last_action_time` from each subsequent. Match Python's exact assignment, do not assume ASC.) Add both fields to `TimelineEntry` and append them (in that order) at the END of `to_value`, after `action_types`.
2. `AgentStats::to_value`: move the `action_types` insert to BEFORE `first_action_time`/`last_action_time` so order = agent_id, agent_name, total_actions, twitter_actions, reddit_actions, action_types, first_action_time, last_action_time. (Verify the DESC-iteration first/last assignment matches Python :1082-1095 too.)
3. Strengthen the two populated tests to assert the full key SET and order (serialize to string + compare, or assert the missing keys explicitly) so the gap can't re-hide.

---

## 2026-06-19 · U-026 sub-cycle (j) · social-DB read routes · VERDICT: PASS

Gate: parity verifier (default-skeptical, fail-closed). Symbols S-827 (`GET /<id>/posts`),
S-828 (`GET /<id>/comments`) → both `- [x]`. 2/2 symbols covered.

Source X: MiroFish `simulation.py` `get_simulation_posts` (1987-2056), `get_simulation_comments`
(2061-2120). Port Y: `src/api/simulation.rs` handlers + `social_db_path` + feature-gated
`read_posts_response`/`read_comments_response`/`sqlite_row_to_object`.

Tests run (both pass):
- default build: `cargo test --lib -- get_posts get_comments` → 3 passed
  (`get_posts_missing_db_empty_contract`, `get_comments_missing_db_empty_contract`,
   `get_posts_db_exists_without_sqlite_feature_honest_500`).
- sqlite build: `cargo test --lib --features sqlite -- get_posts_populated get_comments_populated`
  → 2 passed (`get_posts_populated_from_sqlite`, `get_comments_populated_from_sqlite_with_post_id_filter`).

Checkable surfaces — all confirmed:
1. Missing-DB empty contracts byte-exact. posts → `{success:true, data:{platform, count:0,
   posts:[], message}}` — exactly 4 data keys, NO `total`; platform defaults "reddit", echoes
   `?platform` (test asserts both "reddit" and "twitter"). comments → `{success:true,
   data:{count:0, comments:[]}}` — exactly 2 data keys. Populated posts has `total` but NO
   `message` (verified in source + sqlite test). Key-set/values/envelope match.
2. sim_dir mapping — FAITHFUL, not a divergence. Python read routes use
   `dirname(app/api/simulation.py)/../../uploads/simulations/<id>` = `backend/uploads/simulations/<id>`.
   `Config.OASIS_SIMULATION_DATA_DIR` (config.py:49) = `dirname(app/config.py)/../uploads/simulations`
   = same dir. Producer/runner `RUN_STATE_DIR` (simulation_runner.py:208-211) =
   `dirname(app/services/...)/../../uploads/simulations` = same dir, and writes
   `{sim_dir}/{platform}_simulation.db` (runner:1751). ALL THREE Python paths resolve to the one
   physical directory; the read routes just hand-roll it relative to a deeper file instead of reading
   the config constant. teri consolidates to `oasis_simulation_data_dir/<id>` — the same dir its
   `cleanup_simulation_logs` deletes `{twitter,reddit}_simulation.db` from (simulation_runner.rs:1436,
   1457-1458) and where its producer will write. DB lands exactly where teri reads it. No divergence.
   `social_db_path` is a plain join — NO dir creation (mirrors Python read-only `os.path.join`).
3. Populated SELECT (sqlite) verified by test: `SELECT * FROM post ORDER BY created_at DESC LIMIT ?
   OFFSET ?`; row→object keyed by column name (`sqlite_row_to_object`, Python `dict(row)`);
   `COUNT(*)` total is UNPAGINATED (limit=1 → count 1, total 2); comments post_id filter
   (`WHERE post_id = ?`, post_id=10 → 2 of 3); OperationalError (missing table) → empty via
   `unwrap_or_else((Vec::new(), 0))` / `unwrap_or_default()`, NOT 500 — matches Python inner
   `except sqlite3.OperationalError`.
4. Honest-degradation: `#[cfg(not(feature="sqlite"))]` + DB-exists → 500 carrying
   `GAP-U026-SOCIALDB`, never a silent empty (test asserts 500 + `success:false` + error contains
   the gap id). ADJUDICATION: this is the CORRECT no-downgrade landing. Returning empty when a DB
   with data exists but teri can't read it would be a SILENT downgrade (data loss disguised as
   "no posts"). The honest 500 surfaces the missing capability instead of fabricating an empty
   result. Right call.
5. Flask `type=int` fallback: `params.get("limit").and_then(|s| s.parse::<usize>().ok())
   .unwrap_or(50)` (offset → 0). Bad/non-numeric → default, never 400. Matches Flask
   `request.args.get('limit', 50, type=int)`.
6. `[!] GAP-U026-SOCIALDB` legitimate. Populated branch is genuinely producer+feature-gated: the
   `*_simulation.db` producer is U-028 (twitter) / U-029 (reddit) / U-030 (parallel), all unported,
   and the `sqlite` cargo feature is OFF by default (Cargo.toml). Today the DB never exists → both
   routes return the missing-DB empty contract = the FAITHFUL current behavior (a sim that never ran
   has no DB), not a stub. The `#[cfg(feature="sqlite")]` SELECT path is fully implemented and
   test-verified against a hand-built real DB (data-starved in production only, not logic-starved).
7. 500 path: connection-open failure → `Connection::open(...).map_err(ApiError::server)` → 500;
   the two 200 branches (missing-DB empty, populated) carry no traceback-bearing error. `ApiError::server`
   emits the 3-key `{success:false, error, traceback}` shape (`[≠] U025-TRACEBACK` for the value only).

`[≠]` challenge: no NEW `[≠]` introduced by this unit. The only `[≠]` touched is the pre-existing
`U025-TRACEBACK` on `ApiError::server` (traceback VALUE is a Rust backtrace string, not a Python
stack) — non-contractual (frontend treats `traceback` as opaque debug text), 3-key CONTRACT
preserved. Survives the bar (non-contractual). Not a feature-skip.

Residual: `[!] GAP-U026-SOCIALDB` remains OPEN as a legitimate producer+feature frontier (clears
when U-028/029/030 land AND the sqlite feature is enabled) — NOT a parity failure for this unit; the
current-behavior branch is faithful and the deferred branch is implemented+tested. No `[~]`, no
disguised-skip `[≠]`.


---

## 2026-06-19 — U-026 sub-cycle (l): env-status (S-833) + close-env (S-834)

VERDICT: **FAIL** (unit not PASS) — S-833 PASS `[x]`, S-834 `[~]` (newly-found undocumented divergence).
Tests: `cargo test --quiet env_status` = 3 passed; `cargo test --quiet close_env` = 3 passed (6 total, 0 fail).

### S-833 `POST /env-status` — PASS `[x]` (pure read, fully portable+tested today)
Differential vs `simulation.py:2585-2647` + primitives `get_env_status_detail`/`check_env_alive`:
1. missing simulation_id → 400 requireSimulationId (env_status_missing_simulation_id_400). Empty/`{}` body path mirrors Python `request.get_json() or {}` + filter(!empty).
2. no env → 200, data = exactly 5 keys `{simulation_id, env_alive:false, twitter_available:false, reddit_available:false, message:envNotRunningShort}` (env_status_no_env_200_default_shape asserts len==5 + each value). Matches Python 200 body.
3. env_status.json with `twitter_available:true` surfaces; env_alive stays false (independent: env_alive←check_env_alive, available flags←file) (env_status_reads_env_status_json). Confirms the two sources are not conflated.
4. `env_status.get("x",False)` → teri `.get("x").and_then(Value::as_bool).unwrap_or(false)` — faithful default mapping.
5. default-on-error: Python catches JSONDecodeError/OSError → default dict; teri route `.unwrap_or_default()` → empty Map → false/false via the `.get().unwrap_or(false)` fallbacks. Read error → false/false, 200 not 500. FAITHFUL. (Primitive itself also returns Ok(default) on parse error, runner.rs:4195-4198 — belt-and-suspenders, still false/false.)
NOT IPC-gated — confirmed: pure file read, no run registration required. Fully portable + proven NOW.

### S-834 `POST /close-env` — `[~]` FAIL (one checkable surface faithful; success-path carries an UNFLAGGED divergence)
FAITHFUL + tested today:
- missing simulation_id → 400 requireSimulationId (close_env_missing_simulation_id_400 asserts error == requireSimulationId).
- valid id, no registered run → primitive Err(`Simulation not found: …`) = TeriError::Sim → map_runner_err → **400** (close_env_no_env_400 asserts 400 + no traceback). Python: no sim_dir → ValueError → 400. SAME status class. The 400 message is teri's English runner string vs Python CJK `模拟不存在` — this is the SAME translated-runner-message convention already accepted for the (g) start/stop map_runner_err precedent (S-833/834 siblings). NOT a new divergence class. Confirmed.
- CJK literal `环境关闭命令已发送` (close-sent message): byte-for-byte identical to Python primitive (verified via utf-8 byte compare). It is a hardcoded literal in the Python PRIMITIVE (not an i18n key) → correctly preserved verbatim in the route, not run through t().
- success-path shape (code-inspection, producer-pending): `{success: resp.status==CommandStatus::Completed, message:<CJK>, result: resp.result→Object|null, timestamp}` then outer `{success, data:result}`; IPCResponse.result is Option<Map> → map(Value::Object).unwrap_or(Null) matches Python `response.result` (dict) / absent. Correct shape.
- status→Completed update: `get_simulation → if Some → status=Completed + save_simulation_state` mirrors Python :2693-2697 (manager.get_simulation → if state → COMPLETED → _save_simulation_state). Reachable only on close success. Faithful.

`[!] U026-l-IPC-PRODUCER-PENDING`: CONFIRMED. The success path is genuinely IPC-gated — `close_simulation_env` requires a run in `self.runs` (a live IPC server, produced by U-028/029/030); none registered today → Err(no-run) → 400. env-status is NOT gated (above). Correct.

`[≠] U026-l-ALREADYCLOSED`: ADJUDICATED DEFENSIBLE (producer-pending). Python primitive calls `check_env_alive()` FIRST and on not-alive returns the 2-key `{success:True, message:"环境已经关闭"}`. teri's primitive (runner.rs:4151-4160) omits the pre-check and always `send_close_env`. Rationale survives the `[≠]` bar: teri's `check_env_alive` is an in-process AtomicBool substitute for Python's cross-process `env_status.json` read (S-483 `[≠]`), and the branch is only reachable WITH a live registered IPC run — which doesn't exist pre-producer. Non-contractual today (unreachable), substrate-shaped. Flagged in the route comment. Acceptable as producer-pending — but MUST be re-verified for observable shape when the producer lands.

**NEW FINDING (the FAIL): `[≠] U026-l-TIMEOUT` is UNFLAGGED.** Python's primitive has a THIRD branch the route comment does not mention: `except TimeoutError: return {success:True, message:"环境关闭命令已发送（等待响应超时，环境可能正在关闭）"}` (simulation_runner.py:1651-1656) → route returns **200, success:true**. teri's path: `send_command` elapsed → `Err(TeriError::Sim("…等待命令响应超时…"))` (simulation_ipc.rs:890/949) → route `.map_err(map_runner_err)?` → TeriError::Sim → **400, success:false**. So on an IPC close TIMEOUT: Python = 200/success (graceful "may be closing"), teri = 400/hard-error. This is a real producer-pending BEHAVIOR CHANGE (different status code AND success bool AND message) that is NOT recorded as a `[≠]`/`[!]` anywhere — only ALREADYCLOSED is flagged. Per fail-closed / "any divergence … leave `[~]`", an undocumented divergence on a ported surface cannot count as covered.

MINIMAL FIX (route back to porter — tracking/doc, not necessarily logic):
- EITHER add `[≠] U026-l-TIMEOUT` to the close_env_route comment + symbol-map S-834 row with rationale (Python's TimeoutError→200/success treats a timed-out close as "in progress"; teri's IPC timeout currently surfaces as 400 — adjudicate whether that is the intended port behavior or a branch to reproduce when the IPC producer lands),
- OR (preferred for true parity) reproduce Python's catch: in close_env_route, map the IPC-timeout error specifically to the 200 `{success:true, data:{success:true, message:"<CJK timeout literal>"}}` shape rather than 400. Both branches are equally producer-pending, so this is a code-inspection + one-comment fix today; it is the success-path contract that will go live with U-028/029/030, so flagging it now prevents a silent downgrade landing with the producer.

### Residual after this cycle
- S-833: `[x]` (PASS, no residual).
- S-834: `[~]` (FAIL) — blocked on flagging/handling `[≠] U026-l-TIMEOUT`.
- `[!] U026-l-IPC-PRODUCER-PENDING` (close-env success path): legitimate, clears with U-028/029/030.
- `[≠] U026-l-ALREADYCLOSED`: defensible producer-pending; re-verify observable shape when producer lands.
- `[≠] U026-l-TIMEOUT`: must be created (currently undocumented → the FAIL trigger).

Unit rollup: 1/2 symbols `[x]` (S-833). S-834 stays `[~]`. **U-026 sub-cycle (l) is NOT PASS** — do not commit the unit as parity-clean until S-834 is resolved.

---

## 2026-06-19 — U-026 sub-cycle (l) RE-VERIFY: `[≠] U026-l-TIMEOUT` documentation gate

VERDICT: **PASS** — the prior FAIL is resolved. S-834 → `[x]`/`[≠]` (covered, with `[≠] U026-l-TIMEOUT` as a tracked producer-pending residual). U-026 sub-cycle (l) is now PASS (S-833 `[x]` + S-834 `[≠]`).
Tests: `cargo test --quiet close_env` = 3 passed; `cargo test --quiet env_status` = 3 passed (6 total, 0 fail). `cargo build` green (cached — change is comment-only, route logic unchanged).

### What was under test
The prior gate FAILED S-834 on ONE issue: Python's `close_simulation_env` catches `TimeoutError`→**200** graceful `{success:true,message:"环境关闭命令已发送（等待响应超时，环境可能正在关闭）"}`, but teri maps the IPC close-timeout→**400** hard error, and this divergence was UNFLAGGED. The prior gate's minimal-fix option (a): "add `[≠] U026-l-TIMEOUT` to the close_env_route comment + S-834 row with rationale." That comment is now present (simulation.rs:2527-2535).

### Verification (confirm/refute the three asks)
1. **Divergence documented & accurate — CONFIRMED.** close_env_route header comment `[≠] U026-l-TIMEOUT` (simulation.rs:2527-2535) accurately states: Python 200-graceful vs teri 400 (status + success bool + message all diverge); root cause = `TeriError` has no `Timeout` variant; producer-pending/unreachable today; resolution plan ties to U-028/029/030 (add `TeriError::Timeout` → close-env 200-graceful / interview 504). The embedded CJK timeout literal is **byte-exact** vs Python source `simulation_runner.py:1655` (verified: `e78eaf…efbc89` 51 bytes, identical). Success-path CJK `环境关闭命令已发送` also byte-exact (route:2615 vs py:1647/2669).
   - Source-of-truth re-read: the 200-graceful branch lives in the PRIMITIVE `SimulationRunner.close_simulation_env` (simulation_runner.py:1651-1656), not the route — so the route's `except ValueError→400 / except Exception→500` never sees the TimeoutError; the primitive returns a dict and the route falls through to `jsonify(...)` = HTTP 200. Comment's "Python returns 200" is correct.
   - teri side re-read: `send_command` elapsed → `Err(TeriError::Sim("等待命令响应超时 (…秒)"))` (simulation_ipc.rs:946-957); `close_simulation_env` (simulation_runner.rs:4151-4161) calls `send_close_env`→`send_command`; route `.map_err(map_runner_err)?` (simulation.rs:2597) and `map_runner_err` maps `Sim→400` (simulation.rs:1492-1497). So elapsed close-timeout → 400/success:false. Comment's "teri 400" is correct.
   - `TeriError` (error.rs:4-55): NO `Timeout` variant — confirmed. The 200-graceful-on-timeout shape is genuinely inexpressible without one.

2. **Documenting `[≠]` vs porting a fragile string-match now — CORRECT CALL.** (a) Path is unreachable until the producer: `close_simulation_env` requires a registered run in `self.runs` (a live IPC server from U-028/029/030); none today → `Err("Simulation not found")` → 400 BEFORE any send/timeout (runner.rs:4156-4159). (b) teri cannot express 200-graceful-on-timeout without a `Timeout` error variant — adding one is a producer-coupled error-model change, not a local string hack; matching on the error *string* `"等待命令响应超时"` now would be fragile and would still need rework when the variant lands. (c) The identical `[≠] U026-k-TIMEOUT504` (interview IPC timeout → teri 400 vs Python 504, same no-`Timeout`-variant root cause, simulation.rs:2182-2187) was already adjudicated PASS as a producer-pending residual THIS session. Consistency demands the same verdict for the same class. This SURVIVES the `[≠]` bar (genuinely inexpressible in the destination substrate today) — it is NOT a portable-feature skip: there is no observable output being silently dropped on any reachable path (the divergent path cannot execute pre-producer), and the resolution is committed to the producer cycle, not waved away as "dest won't use it." Documenting it satisfies the fail-closed "no UNDOCUMENTED divergence" bar that triggered the original FAIL.

3. **Nothing else regressed — CONFIRMED.** The change is comment-only: `cargo build` is cached/green (route code unchanged). The faithful surfaces from the prior review are intact and tested: missing simulation_id → 400 `requireSimulationId` (close_env_missing_simulation_id_400, simulation.rs:5453-5462); no registered run → 400 + no traceback (close_env_no_env_400, simulation.rs:5467-5480); success-path shape `{success:status==completed, message:"环境关闭命令已发送", result|null, timestamp}` + `{success, data:result}` + status→Completed save (simulation.rs:2599-2619) unchanged; `[≠] U026-l-ALREADYCLOSED` still flagged (route:2609-2611). env-status (S-833) unchanged & PASS.

### Residual flags (carried into U-028/029/030 producer cycle)
- `[!] U026-l-IPC-PRODUCER-PENDING` (close-env success path): legitimate; clears when a live IPC run is registered.
- `[≠] U026-l-ALREADYCLOSED`: defensible producer-pending; re-verify observable shape when producer lands.
- `[≠] U026-l-TIMEOUT`: NOW DOCUMENTED + adjudicated PASS (was the FAIL trigger). MUST resolve WITH the producer: add `TeriError::Timeout` → route close-env timeout to 200-graceful (the byte-exact CJK literal) / interview to 504. Re-verify the live 200/success/message shape when U-028/029/030 lands.

### Verdict
- S-833: `[x]` (unchanged, PASS).
- S-834: `[≠]` (PASS — the documented `[≠] U026-l-TIMEOUT` resolves the prior FAIL; tracked residual, producer-pending).

Unit rollup: 2/2 symbols covered (S-833 `[x]`, S-834 `[≠]`). **U-026 sub-cycle (l) is PASS** — clears to commit. The two `[≠]` (TIMEOUT, ALREADYCLOSED) + one `[!]` (IPC-PRODUCER-PENDING) are tracked must-resolve-with-producer residuals, not downgrades.

---

## U-026 sub-cycle (m) — GET /history  (parity PASS round-1, 2026-06-19, 23rd→24th resume)

**Unit:** U-026 simulation routes, sub-cycle (m) — `GET /history` (history list with project enrichment).
**Symbol:** S-835 `get_simulation_history`.
**Source X:** `MiroFish/backend/app/api/simulation.py:876-987` + helper `_get_report_id_for_simulation:817-873`.
**Port Y:** `teri/src/api/simulation.rs::get_simulation_history` + 8 `history_*` tests.

### Method
Differential by reading BOTH sides (MiroFish runs need torch/creds → Python behavior verified by reading). Default-skeptical verifier (harness:rust-port-parity-verifier) ran `cargo test -p teri --lib api::simulation::tests::history` (8 passed) and traced every contract item to source.

### Contract proven (8 items)
1. **Key order (byte-observable, preserve_order):** 17 base `to_dict` keys, `current_round` UPDATED IN PLACE at pos 12 (IndexMap re-insert keeps position = Python `dict[k]=v`), then 8 appended: simulation_requirement, total_simulation_hours, runner_status, total_rounds, files, report_id, version, created_date. 25 keys total. `history_key_order_preserved` asserts the full order.
2. **config block:** requirement default ""; total_simulation_hours echoes RAW JSON Number; recommended_rounds = `int(tsh*60/max(mpr||60,1))` truncate-toward-zero (`.trunc()`). No-config → "",0,0.
3. **run_state block:** Some → current_round / runner_status.value (8 RunnerStatus values match) / total_rounds(>0 else recommended); None → 0/"idle"/recommended (get_run_state returns Ok(None), mirrors Python `if run_state:`).
4. **files:** project.files[:3] → [{"filename": f.get("filename","未知文件")}]; missing project/empty → [].
5. **report_id (FLAGGED item):** porter chose `list_reports(Some(id),1).first()` over loop_state-pointed `get_report_by_simulation`. **VERIFIER CONFIRMED MORE FAITHFUL, NOT a downgrade**: get_report_by_simulation (manager.rs:830) returns FIRST fs-match; Python returns NEWEST by created_at DESC; list_reports `sort_by(b.created_at.cmp(a.created_at))` stable = Python stable Timsort reverse → byte-faithful newest. Empty → null both sides.
6. **limit:** `?limit` default 20, usize-parse → neg/non-numeric → 20. `[~] U026-m-NEGLIMIT` (U-025 precedent; Python type=int negative `[:-n]` is non-contractual). `history_limit_caps_results` + `history_bad_limit_falls_back_to_default`.
7. **envelope:** {success,data,count} order; outer error → `ApiError::server` 500 {success,error,traceback} (`[≠] U025-TRACEBACK`).
8. **created_date:** created_at[:10] char-slice (ASCII ISO); empty if absent.

### Non-contractual edges (do NOT block — outside contract)
- Corrupt meta.json missing graph_id/simulation_requirement → Rust `get_report` `?`-rejects vs Python reads only sim_id/report_id. Well-formed reports (always from `report.to_dict()`) have all keys → contractual domain identical. `[~]`.
- Non-numeric minutes_per_round (string) → Python `max(str,1)` TypeError→500 vs Rust `.as_f64()→None→60`. Malformed config only. `[~]`.

### Residual flags
- `[!] U026-m-LIVEDATA` — run-state/config/report enrichment read real on-disk state; with no live producer they are the faithful empty-run snapshot (idle/0/recommended/None). Flips to richer values automatically when producers (U-028/029/030) land — same read path, no code change.

### Verdict
- S-835: `[x]` (PASS round-1). 9/9 symbols exercised at source.

Unit rollup: 1/1 symbol covered. **U-026 sub-cycle (m) is PASS** — clears to commit. With (m) landed, only (d) prepare(+status) remains for U-026 sub-cycles a-m.

---

## 2026-06-19 — U-026 sub-cycle (d): POST /prepare + POST /prepare/status + background prepare worker — VERDICT: PASS

Source: MiroFish `simulation.py` `prepare_simulation` L359-639 (incl. `run_prepare` L508-612,
`progress_callback` L522-581) and `get_prepare_status` L642-752.
Port: teri `src/api/simulation.rs` (`prepare_simulation_route` L786, `prepare_status_route` L953,
routes L127-128) + `src/services/simulation_manager.rs` (`spawn_prepare_simulation` L1840,
`prepare_worker` L1897, `prepare_progress_update` L1757).

Differential evidence (`cargo test -p teri --lib prepare`): 27 passed, 0 failed.
- 11 route tests (every /prepare + /prepare/status branch: 400 require-id, 404 sim-not-found,
  already-prepared short-circuit (no task_id), 400 project-missing-requirement, happy 200 preparing+task_id,
  status 400 neither, not_started, ready-by-sim, task-found to_dict, task-gone 404, B1-precedes-task_id).
- 3 progress-mapping unit tests (band math 45 / truncate 11 / unknown-stage(0,100); detailed_message
  with & without count segment; progress_detail 8 keys, total_stages=4, stage_index 1-based).
- All 10 api.* + 5 progress.* i18n keys confirmed present in BOTH en.json and zh.json AND match the
  MiroFish source `locales/{en,zh}.json` (1:1).

CONTRACT CRITICAL POINTS adjudicated:
- entity_types preview-vs-worker: NOT conflated. Response uses `preview_entity_types`
  (=preview.entity_types, simulation.rs:901/933 ↔ Python L484/623); worker receives body `entity_types`
  (simulation.rs:861/915 ↔ Python L587). Two distinct inputs preserved.
- overall-% i64 math `start + (end-start)*progress/100` == Python `int(start + (end-start)*progress/100)`
  for the non-negative domain (start integer ⇒ int(start+x)=start+floor(x); proven by tests 45 & 11).
- B1-precedes-task_id: B1 short-circuit runs first (returns ready/no-task_id); B3a double
  check_simulation_prepared call preserved. Faithful.
- to_simple_dict (9 keys) on complete; zero-entities → Ok(state, status=failed) → complete_task with
  result.status="failed" (faithful to Python L1261-1266 + L593-597, NOT fail_task).

DECISION-U026-d-1-REVISED (std::thread + current-thread runtime) — ADJUDICATED FAITHFUL, no downgrade:
- The architect's option (b) premise was REFUTED correctly by the porter: `prepare_simulation`'s future
  is genuinely !Send — the `*mut Option<&mut dyn FnMut(...)>` raw pointer (simulation_manager.rs:1321)
  and the `&mut dyn FnMut` are held LIVE across the `.await` at L1350 (generate_profiles_from_entities).
  A tokio::spawn worker awaiting that future inherits !Send. The design-doc claim "the &mut dyn never
  crosses an await" was wrong; the std::thread + Builder::new_current_thread().block_on(with_locale(...))
  is the correct realization (current-thread RT drives !Send futures on one thread).
- Observable parity vs threading.Thread(daemon=True): (1) task_id returned immediately (spawn returns
  synchronously); (2) background progress lands via global OnceLock TaskManager (Send+Sync mutex);
  (3) terminal complete/fail; (4) locale captured pre-spawn + re-applied via with_locale; (5) runtime-build
  failure → fail_task (observable, strict-superset safety); (6) panic-isolated detached thread (≈ daemon).
  At least as faithful as option (b), arguably more (real OS thread = Python's thread).
- Blast radius = 0 on the U-023 surface: `prepare_simulation` signature (L1180-1192) and the raw-pointer
  trick (L1321) are UNCHANGED — git -L confirms the region was touched only by U-023 (a94658a) + a
  whitespace fmt commit (1afbe0c), by no sub-cycle (d) commit. Option (a) correctly rejected.

Risk flags adjudicated:
- `[~] U026-d-STAGE4` (copying_scripts band dead in teri): CONFIRMED not a downgrade. teri's pipeline
  emits only stages 1-3; the (90,100) band + total_stages=4 are kept for index/total fidelity; emitted
  overall %s for stages 1-3 are byte-identical to Python. Status [~] justified.
- `[!] U026-d-GRAPHREQ` (prepare requires &KnowledgeGraph; graph-resolve failure → empty graph → 0
  entities → Ok(FAILED) → complete_task with status=failed, observable via /prepare/status, NOT a route
  500): CONFIRMED faithful degradation, not a downgrade — matches Python's empty-Zep → failed-prepare.
  Inexpressible-otherwise (teri has no live Zep client); status [!] justified.
- `[≠] U026-ZEPKEY`, `[≠] U025-TRACEBACK`: inherited pre-approved precedents, not newly introduced.

Build precondition: `cargo build -p teri --lib` clean.

VERDICT: PASS — all contract behaviors match; std::thread spawn is a faithful, no-downgrade realization;
U-023 surface untouched (blast radius 0). Symbols verified: prepare_simulation_route,
prepare_status_route, spawn_prepare_simulation, prepare_worker, prepare_progress_update,
to_simple_dict(reuse) — all exercised.

---

## PARITY VERDICT — U-027 sub-cycle (b): report log-read routes — 2026-06-19 (opus)

**Verdict: PASS** · symbols 4/4 verified (S-847, S-848, S-849, S-850 → `- [x]`)
**Tests:** `cargo test -p teri --lib api::report` → 21 passed, 0 failed (7 new sub-cycle (b) tests).

### Differential shape diff (MiroFish report.py jsonify  vs  teri serde_json::json!)

| Route | Source (report.py) | teri (src/api/report.rs) | Result |
|---|---|---|---|
| `GET /:id/agent-log` (758-814) | `{success:True, data: get_agent_log(id, from_line)}` = `data:{logs,total_lines,from_line,has_more}` | `:248` `{success:true, data: Map{logs,total_lines,from_line,has_more}}` straight passthrough | key-for-key MATCH |
| `GET /:id/agent-log/stream` (817-848) | `{success:True, data:{logs, count:len(logs)}}` plain `jsonify` | `:265` `{success:true, data:{logs, count:logs.len()}}` plain JSON | MATCH; NOT SSE on either side |
| `GET /:id/console-log` (853-896) | `{success:True, data: get_console_log(id, from_line)}` | `:281` `{success:true, data: Map{...}}` passthrough | MATCH |
| `GET /:id/console-log/stream` (899-930) | `{success:True, data:{logs, count:len(logs)}}` | `:298` `{success:true, data:{logs, count}}` | MATCH |

### REFUTATION TARGET (critical): is any source `/stream` route actually SSE? — REFUTED, port is faithful
- `grep -nE 'text/event-stream|stream_with_context|EventSource|Response\(' report.py` → **0 hits**. Both `/stream` handlers (report.py:817-848, 899-930) are bare `return jsonify({...})` one-shot full-dumps. The "stream" name means "fetch the whole stream at once"; incremental tailing is the `from_line` param on the NON-`/stream` routes. No frontend `EventSource` consumes them either.
- The lone "SSE log streams" phrase in symbol-map S-1057 (U-048 frontend contract) is a loose label for the polling mechanism, NOT a backend `text/event-stream` route. It does not contradict the source: backend behavior is JSON.
- ∴ teri's JSON `/stream` handlers MATCH the source. **NOT a downgrade-from-SSE.** Decision (ii) confirmed.

### Contract details verified
- **`from_line` parse**: source `request.args.get('from_line',0,type=int)` (Flask `type=int` → default 0 on non-coercible); teri `parse::<usize>().ok().unwrap_or(0)` (non-numeric/neg → 0). Faithful. Behavioral evidence: `?from_line=1` skips line 0 (agent_log); `?from_line=2` leaves 1 line (console_log).
- **passthrough**: agent-log/console-log re-wrap the manager's `Map{logs,total_lines,from_line,has_more}` verbatim under `data` — no re-ordering, no re-keying.
- **count**: `count == logs.len()` on both `/stream` routes.
- **routing**: 2-seg `/agent-log` vs 3-seg `/agent-log/stream` resolve distinctly (test `agent_log_stream_vs_nonstream_distinct`: non-stream has `total_lines`, stream has `count` + NO `total_lines`); same for console-log; both under `/:report_id` capture, registered after seg-0 statics (`[!] U027-ROUTE-ORDER`).
- **substrate (reuse-Y, U-024)**: `get_agent_log_route` calls `ReportManager::get_agent_log`; `_stream_route` calls `get_agent_log_stream`; console pair calls `get_console_log`/`get_console_log_stream`. Correct CALLs confirmed against manager.rs:146/200/222/278 (internals already U-024-verified; the Python managers at report_agent.py:1958/2005/2019/2067 return matching `{logs,total_lines,from_line,has_more}` / `List` shapes).

### Markers
- **`[~] U027-SSE-SEAM-DORMANT`** — CORRECT to leave dormant. There is NO live-SSE report route in the SOURCE (refutation above), so not building one drops zero source behavior. S-763 h4 note confirms `NullSink covers parity`, SseSink adapter is "optional polish". Leaving the U-024 sink.rs SseSink seam reserved is faithful, NOT a downgrade.
- **`[≠] U025-TRACEBACK`** — inherited correctly. 500 envelope `ApiError::server` → `{success:false,error,traceback}` (3-key shape == source `jsonify({success:False,error,traceback:format_exc()})`); only the traceback VALUE differs (Rust backtrace string vs Python format_exc), which is opaque non-contractual debug text. Valid `[≠]`, not a feature-skip. (Note: these 4 log routes have no throwing path in practice — missing/malformed files yield empty/skipped lines, not 500 — so the 500 envelope is effectively unreachable here but shape-correct if hit.)

### `[≠]` challenge
No `[≠]` rows are claimed for the 4 sub-cycle (b) symbols themselves — all 4 are full `- [x]` behavioral matches. The two inherited markers above are unit-level: `U027-SSE-SEAM-DORMANT` survives as "genuinely no source SSE route exists" (not a portable feature being skipped); `U025-TRACEBACK` survives as "identical shape, non-contractual value." Neither is a disguised feature-skip.

**Conclusion:** All 4 contract behaviors match source; all 4 symbols `- [x]`; refutation target (hidden SSE) actively REFUTED. Sub-cycle (b) is parity-clean. (The U-027 UNIT ledger row stays `- [ ]` — sub-cycles c–f remain unported; only (b) is gated here.)

---

## PARITY VERDICT — U-027 sub-cycle (c): sections + download routes + GAP-A/B wrappers — 2026-06-19 (opus)

**Verdict: PASS** · symbols 5/5 verified (3 routes + 2 ReportManager pub wrappers)
**Tests:** `cargo test -p teri --lib api::report` → 29 passed, 0 failed (8 new sub-cycle (c) tests:
download_not_found_404, download_found_serves_markdown_attachment, sections_empty_no_files,
sections_with_files_and_is_complete, sections_generating_report_not_complete, single_section_found,
single_section_not_found_404, single_section_non_integer_404).
**Build precondition:** `cargo build -p teri --lib` clean (tests compile+run).

### Differential shape diff (MiroFish report.py vs teri src/api/report.rs + manager.rs)

| Behavior | Source (report.py) | teri | Result |
|---|---|---|---|
| download 404 | get_report None → 404 `{success:false, error:reportNotFound{id}}` (L408-412) | report.rs:334-340 same | MATCH |
| download 200 | send_file(full_report.md OR temp from markdown_content) as_attachment, download_name=`{id}.md` (L414-433) | report.rs:344-358 reads on-disk md (GAP-A) `.unwrap_or(markdown_content)`; CD=`attachment; filename="{id}.md"`; body=content bytes | MATCH (bytes+filename); CT adjudicated below |
| download bytes-equality | both branches same bytes (save_report writes full_report.md = markdown_content, report_agent.py:2441 + temp branch writes report.markdown_content) | teri save_report manager.rs:755-760 writes full_report.md = markdown_content; fallback uses report.markdown_content | SAME BYTES on both branches, both repos |
| sections | get_generated_sections(id) + is_complete=report&&COMPLETED → `{report_id, sections, total_sections:len, is_complete}` (L636-649) | report.rs:370-385 same 4-key data | MATCH |
| section valid+present | `{filename:section_{NN:02}.md, section_index, content}` (L687-693) | report.rs:426-433 same 3-key | MATCH |
| section valid+missing | os.path.exists False → 404 sectionNotFound index=`{n:02d}` (L678-682) | report.rs:417-424 → 404 sectionNotFound idx2=`{n:02}` | MATCH (test: "05" for idx 5) |
| section non-integer | Flask `<int:section_index>` no-match → Werkzeug default 404 (STATUS) | report.rs:407-414 String-capture + manual parse → 404 | STATUS MATCH (`[≠]` on body, adjudicated below) |

### GAP-A/B wrapper audit (manager.rs:526-555) — thin, correct, private helpers UNCHANGED
- `git diff --stat HEAD src/report/manager.rs` = **+31 insertions, 0 deletions** → the two new pub
  wrappers ONLY; the private `get_report_markdown_path` (manager.rs:86-88) and `get_section_path`
  (manager.rs:101-104) are byte-unchanged.
- `read_report_markdown` (GAP-A, L534-537): `path = get_report_markdown_path(id); if exists → Some(read), else None`.
  Wraps the private helper, returns on-disk full_report.md content or None → route falls back to
  `report.markdown_content`. Both branches yield identical `.md` bytes. CORRECT, thin.
- `get_single_section` (GAP-B, L544-555): `path = get_section_path(id, idx); !exists → None; else Some((format!("section_{:02}.md",idx), read))`.
  Mirrors Python report.py:676-694 (path → exists-check → read → filename `{idx:02d}.md`). CORRECT, thin.

### THE ADJUDICATION — `[≠] U027-c-SECTIONIDX-404BODY` is the RIGHT classification (survives challenge)
1. **STATUS correction is faithful & sound.** Flask `<int:section_index>` is a route *converter*: a
   non-integer segment does NOT match the route, so Werkzeug raises a default 404 (no JSON 404
   errorhandler — CONFIRMED: `grep errorhandler\|404 app/__init__.py` → 0 hits). The architect's
   original design claim ("axum Path<usize> would 404 like <int:>") is WRONG and the porter correctly
   refuted it: axum has NO type-matching route converter (context7/axum routing docs, verbatim: *"It is
   not possible to create segments that only match some types like numbers or regular expression. You
   must handle that manually in your handlers."*). A typed `Path<usize>` that fails to deserialize
   returns a `PathRejection` → **400**, a STATUS divergence from Flask's 404. The porter captured the
   segment as `Path<(String,String)>` and parses manually (report.rs:404-415) → non-integer now yields
   **404**, restoring STATUS parity. Test `single_section_non_integer_404` (`/section/abc` → 404)
   proves it. The correction is SOUND.
2. **The body divergence is genuinely non-contractual** (the `[≠]` survives the challenge):
   - It is NOT inexpressible (teri *can* return a body) — so not `[!]`.
   - It is NON-CONTRACTUAL: Flask emits its **framework-default HTML 404 error page** (there is no JSON
     404 handler); teri emits its JSON `{success:false, error:sectionNotFound...}`. Neither body is a
     designed/observed API contract — it is a malformed-URL artifact. The frontend constructs
     `/section/{int}` (S-1057 streaming contract uses integer section indices), so a non-integer segment
     is **never sent** by any real consumer. No consumer parses the 404 body of a malformed URL.
   - This is NOT a portable-feature-skip: there is no source *feature* (export format, file sink, CLI
     flag, distinct render path) being dropped — only a framework-default error *page* on an
     impossible-in-practice URL. The contractual surface (404 STATUS) IS ported faithfully.
   - ∴ `[≠] U027-c-SECTIONIDX-404BODY` is correct, NOT `[~]`. (A `[~]` would imply the STATUS itself is
     unproven; it is proven 404-faithful. Only the non-contractual body shape differs, which is exactly
     what `[≠]` records.)

### Content-Type adjudication (download) — non-divergent in contract
- teri sets `text/markdown; charset=utf-8` explicitly (report.rs:351). Flask `send_file(download_name="x.md")`
  infers `text/markdown` from the `.md` extension. Even if charset suffix differs trivially, the
  **download contract is**: attachment (Content-Disposition: attachment; filename="{id}.md") + the
  markdown bytes. Both are byte-identical (test `download_found_serves_markdown_attachment` asserts
  CT starts_with "text/markdown", CD == `attachment; filename="report_dl.md"`, body == markdown_content).
  Content-Type for a file *attachment* is non-contractual (the browser saves by filename, not MIME).
  No divergence of substance.

### i18n (en+zh, both with interpolation) — CONFIRMED byte-identical to source
- teri `src/i18n/locales/{en,zh}.json:371,375` == MiroFish `locales/{en,zh}.json:371,375` (byte-for-byte,
  same line numbers): `reportNotFound:"...{id}"` / `"...报告不存在: {id}"`;
  `sectionNotFound:"Section not found: section_{index}.md"` / `"章节不存在: section_{index}.md"`. Both keys
  present in BOTH locales; `sectionNotFound` carries `{index}` interpolation, `reportNotFound` `{id}`.
  `t_args` interpolates placeholders (existing passing tests t_args_interpolation_*; here proven by
  single_section_not_found_404 asserting the rendered error contains "05" for index 5).

### Inherited markers
- `[≠] U025-TRACEBACK` — inherited; 500 envelope shape `{success:false,error,traceback}` matches source,
  only the opaque traceback VALUE differs (non-contractual debug text). Valid `[≠]`, not a feature-skip.
  (In sub-cycle (c) the throwing paths are: download HeaderValue build error / fs read errors →
  ApiError::server → 3-key shape if hit; effectively unreachable for well-formed reports.)
- `[!] U027-ROUTE-ORDER` — /download, /sections, /section/:idx registered under the /:report_id capture
  after seg-0 statics; distinct full-path tails resolve correctly (existing routing tests + the 8 new
  tests address real paths without collision).

### `[≠]` challenge summary
- `U027-c-SECTIONIDX-404BODY`: CHALLENGED → SURVIVES as non-contractual framework-default-error-page on
  a URL no consumer sends. NOT a disguised portable-feature-skip (no export/sink/flag/render dropped;
  the 404 STATUS is ported faithfully). Classification CONFIRMED.
- `U025-TRACEBACK`: inherited precedent, identical shape / non-contractual value. Not a skip.

**Conclusion:** All sub-cycle (c) contract behaviors match source; download serves the identical
markdown bytes as a `.md` attachment with the correct filename; sections/section data shapes are
key-for-key; the section-index 404 STATUS correction is faithful and sound (axum has no `<int:>`
converter — verified); GAP-A/B wrappers are thin/correct and leave the private helpers byte-unchanged;
i18n is byte-identical en+zh with interpolation. 5/5 symbols `- [x]`/`- [≠]`. Sub-cycle (c) parity-clean.
(The U-027 UNIT ledger row stays open — sub-cycles d–f remain unported; only (c) is gated here.)

---

## U-027 sub-cycle (d) — tools debug routes (POST /tools/search, /tools/statistics) — PASS

**Gate verdict: PASS (2026-06-19, 26th resume). 2/2 symbols `- [x]` (S-852f, S-852g).**

Differential re-verification in teri's context against `report.py:935-1020` + `zep_tools.py`.
Tried to REFUTE on 7 axes; all hold:

1. **Validation + order** — `not graph_id or not query` (search) / `not graph_id` (statistics) →
   400 with the right i18n key, BEFORE the ZEP guard. teri `unwrap_or("")` + `is_empty()` reproduces
   Python falsy (absent ≡ empty-string → 400). The 400 returns before the single `.await`.
2. **Success envelope / data shape** — search `data` = SearchResult::to_dict 5-key
   {facts,edges,nodes,query,total_count}; statistics `data` = the 5-key dict DIRECTLY (Python result is
   already a plain dict, NOT double-wrapped via to_dict). `query`/`graph_id` echoed. preserve_order →
   byte-faithful key order.
3. **scope/limit defaults** — source omits scope (Python default "edges") → teri `Some("edges")`;
   `data.get('limit', 10)` → `as_i64().unwrap_or(10)`.
4. **`[≠] U026-ZEPKEY`** — guard KEPT (empty zep_api_key → 500 zepApiKeyMissing), consistent with
   U-025/026; proven to fire AFTER validation. Preserved behavior, not a skip. LEGITIMATE.
5. **`[≠] U026-R2-ABSENTGRAPH`** — unresolvable graph_id (no graph_build task) → teri 500 vs Python
   blanket-except → empty. Substrate-forced input-domain narrowing (teri can't synthesize a reader over
   a graph with no local task); the PRIMARY resolved-graph→to_dict contract is faithful and is the live
   path. SAME divergence already accepted for the entities routes (sim sub-cycle b). LEGITIMATE.
6. **Send-safety** — only `.await` (load_entity_reader_graph) yields an owned KnowledgeGraph; build_llm
   + ReportTools::new + search_graph/get_graph_statistics are sync on owned values → no borrow across
   await → handler futures Send (compiles as axum post handler).
7. **`[!] U027-GRAPHREQ`** — resolved-but-empty graph → empty result set, faithful to source on an empty
   graph. Data is producer-supplied. Runs end-to-end today.

**Structural reuse:** `load_entity_reader_graph` (sim:205) promoted `async fn` → `pub(crate) async fn`
so report.rs reuses the ONE graph-resolution path / ONE ZEP guard rather than duplicating it
(no-downgrade: reuse-not-duplicate; blast-radius = visibility only). `[≠] U025-TRACEBACK` inherited.

**Conclusion:** Sub-cycle (d) parity-clean. +7 tests (1465→1472 broader / lib 1450→1457). clippy
--all-targets + --all-features clean. Y-not-regressed. Atomic gate PASS. (U-027 UNIT row stays open —
sub-cycles e,f remain.)

---

## U-027 sub-cycle (e) — chat route (POST /chat) — PASS

**Gate verdict: PASS (2026-06-19, 26th resume). 2/2 symbols `- [x]` (S-852h chat_route, S-852i parse_chat_history).**

Differential re-verification against `report.py:472-564` + `report_agent.py:1766-1881`.
Tried to REFUTE on 9 axes; all hold:

1. **Validation + order** — simulation_id 400 (requireSimulationId) BEFORE message 400
   (requireMessage), both before resolution. Python falsy via unwrap_or("")+is_empty().
2. **Resolution order + 404s** — sim THEN project; simulationNotFound{id} then
   projectNotFound{**project_id**} (interpolates the PROJECT id, faithful to report.py:532).
3. **graph_id fallback** — `state.graph_id or project.graph_id` reproduced by
   `if !sim.graph_id.is_empty() {sim} else {project.graph_id.unwrap_or_default()}`; both
   branches + the both-empty 400 missingGraphId tested.
4. **simulation_requirement** — `.unwrap_or_default()` == Python `or ""` (Some("")/None→"").
5. **`[~] U027-e-CHATROLE-NARROW`** — Python appends role-dicts verbatim (arbitrary roles);
   teri narrows to closed ChatRole (system/assistant mapped, else→user). Frontend only sends
   user/assistant (docstring contract) → non-contractual narrowing, NOT a downgrade. Verifier
   confirmed NO double-windowing: handler passes the FULL array, `chat` does `[-10:]` internally
   (mod.rs:2190). LEGITIMATE `[~]`.
6. **Success envelope** — `{success:true, data: result.to_dict()}`; BOTH Python return paths
   (report_agent.py:1841 early + 1877 post-loop) emit exactly {response,tool_calls,sources} —
   matches ChatResponse::to_dict 3-key set + order.
7. **`[≠] U026-ZEPKEY`** — empty zep key → 500, fires only AFTER full resolution. The two
   ZEP-500 tests double as full-resolution proof. LEGITIMATE inherited.
8. **Send-safety / no OS-thread** — `ReportAgent::chat` is a PLAIN async fn (NO RefCell across
   await, unlike generate_report); graph owned after the only await, &tools/&llm/&manager are
   shared refs over Sync types → future Send. Compiled as `post(chat_route)` (axum rejects
   non-Send). The (f) keystone — NOT (e) — needs the OS-thread.
9. **`[!] U027-e-LLM-GATED`** — the 200 path drives a live LLM; ALL pre-LLM paths are tested
   (2×400, 2×404, graph_id fallback both branches, ZEP-500); success wiring correct by
   inspection (new_react/chat arg order, to_dict). The chat substrate is already U-024
   parity-verified with mock adapters. No coverage hole — same producer-gating convention as
   the (g)/(k) success paths.

**Conclusion:** Sub-cycle (e) parity-clean. +9 tests (1472→1481). clippy --all-targets +
--all-features clean. Y-not-regressed. Atomic gate PASS. (U-027 UNIT row stays open — only the
(f) async-generate keystone remains; after (f), create_app S-024 flips [x].)

---

## U-027 sub-cycle (f) — async-generate KEYSTONE (POST /generate, /generate/status) — PASS → U-027 COMPLETE

**Gate verdict: PASS (2026-06-19, 26th resume). 5 symbols `- [x]` (S-852j-n). U-027 COMPLETE (all 6 sub-cycles a-f).**

Highest-risk sub-cycle (async background task, OS-thread spawn, task lifecycle).
Differential re-verification against `report.py:25-272`. Verifier refuted on 8 axes; all hold:

1. **Decision (i) OS-thread** — generate_report IS `!Send` (RefCell<&mut dyn ReportSink>
   borrowed through Fn closures across the section .awaits, mod:1672) → tokio::spawn won't
   compile → spawn_report_generation mirrors spawn_prepare_simulation VERBATIM (std::thread +
   current_thread runtime, locale capture+with_locale, rt-build-fail→fail_task). Faithful port
   of Python threading.Thread(daemon=True). Blast-radius 0. No Send/borrow bug.
2. **/generate validation order** — sim_id 400 → get_simulation 404 → `!force_regenerate &&
   COMPLETED` short-circuit → project 404 → graph_id 400 (missingGraphIdEnsure, DISTINCT source
   key from chat's missingGraphId — both confirmed in en/zh) → requirement 400. Exact order.
3. **Eager report_id + task** — report_{uuid_hex[:12]}, create_task("report_generate",
   {sim,graph,report_id}), immediate 200 6-key {simulation_id,report_id,task_id,status:generating,
   message,already_generated:false}.
4. **Worker fidelity** — PROCESSING/0/initReportAgent → generate_report(...,sink,Some(report_id))
   → save_report[Err→fail_task=Python outer except] → COMPLETED→complete_task else
   fail_task(error or reportGenerateFailed empty→default).
5. **TaskUpdateSink** — ReportEvent→update_task(progress as i64, `[{stage}] {message}`),
   stage=to_status_str() lowercase. Holds only String → Send.
6. **`[≠] U027-f-GRAPHRESOLVE-EAGER`** — route resolves graph (load_entity_reader_graph, ZEP
   guard) + passes owned graph to worker, vs Python lazy-in-thread. Graph-error → sync 500 vs
   Python generating+failed-task. Consistent with spawn_prepare_simulation precedent; primary
   contract (valid graph → generating+bg-gen) faithful. LEGITIMATE.
7. **/generate/status** — simulation_id-truthy(!is_empty)+COMPLETED short-circuit (200
   already_completed:true progress:100 reportGenerated) → not task_id 400 requireTaskOrSimId →
   get_task None 404 taskNotFound{id} → 200 task.to_dict. No reachable 500 (infallible substrate)
   → source no-traceback outer-except moot.
8. **`[!] U027-f-LLM-GATED` test adequacy** — 200 returns pre-LLM; ALL pre-spawn paths tested
   (7 generate + 4 status) + worker-to-terminal-state. generate_report ALWAYS returns terminal
   Report (Completed/Failed, never Pending — graceful LLM-fallback Completes even w/o LLM; this
   is the substrate's real behavior, confirmed NOT a test-mask). No gap, no over-permissive assert.

**Conclusion:** Sub-cycle (f) parity-clean → **U-027 COMPLETE** (18 handlers / 17 paths, all
parity-verified). +12 tests (1481→1493). clippy --all-targets + --all-features clean.
Y-not-regressed. Atomic gate PASS. create_app S-024 now has all 3 blueprints nested (U-025✓/026✓/
027✓); only register_cleanup (U-023/U-049) remains before S-024 itself flips.

---

## 2026-06-19 — U-028 Cycle 1 parity verdict (adversarial re-verification)

**Scope:** Two sub-deliverables of U-028 cycle 1. Verified in worktree against
`MiroFish/backend`. Ran `cargo test -p teri` → **1502 passed / 6 ignored / 0 failed**;
`cargo clippy -p teri --all-targets --all-features` → **clean** (0 warnings after forced rebuild).

### Deliverable (a) — `SimConfig::from_simulation_config` — **PASS**
- Internal-consistency claim CONFIRMED: `sim/mod.rs:338-372` is formula-identical to the landed
  `simulation_runner.rs:1091-1118` (same `((h*60.0)/mpr) as i64` truncate, same `mpr==0→0` guard,
  same `max_rounds>0` `min`). ONE truncation impl holds.
- DIFFERENTIAL vs Python: the *script* source (`run_twitter_simulation.py:550`,
  `run_reddit_simulation.py:539`) is `(h*60)//mpr` (**floor-div**), while the docstring claims
  `int(h*60/mpr)`. The teri impl matches the *service* primitive form (`simulation_runner.py:353`,
  `int(.../.)`, truncate-toward-zero), NOT the script's `//`. PROVED these two Python forms are
  observably IDENTICAL over the entire reachable domain (`total_hours ≥ 0`, `mpr > 0`): 0 mismatches
  / 200K random non-neg cases; f64 path exact within u32 domain. They diverge ONLY for
  `total_hours < 0`, which is UNREACHABLE (config_generator constrains 24-168; defaults 72/0; never
  negative). 5 unit tests cover default(144), floor table incl. 10h/7min=85 + 1h/7min=8 (truncate),
  max_rounds {50→50, 1000→144, 0→144, -5→144}, missing-keys→72/30, zero-cadence→0. All faithful.
  NOTE for cartographer: docstring/comment says "matches `int(...)`" referencing the script line that
  is actually `//`; technically imprecise but observably equivalent — recommend wording fix, not a FAIL.

### Deliverable (b) — `TeriError::Timeout` → HTTP status mapping — **PASS with 2 recorded divergences**
- `error.rs:21-22` Timeout variant; `simulation_ipc.rs:946-957` elapsed→`Timeout` (was `Sim`);
  test `send_command_times_out_when_server_not_draining` (ipc:1531) asserts `Timeout`. CONFIRMED.
- Regression check (Sim→Timeout): the ONLY `send_command` callers are `send_interview`/
  `send_batch_interview`/`send_close_env` + the test. `stop_simulation` does NOT call send_command
  (uses `terminate_handle`) → a `Timeout` can never reach `map_runner_err` via stop's `:2034` call
  site → the new `Timeout→504` arm is inert there. NO regression. CONFIRMED.
- `map_runner_err` (`:1985`) Timeout→504 raw, BEFORE Sim→400/catch-all→500. `map_interview_err`
  (`:2001`) Timeout→504 i18n-wrapped, else defers. Three routes use DISTINCT keys
  `api.interviewTimeout`/`api.batchInterviewTimeout`/`api.globalInterviewTimeout` (`:2826/2909/2951`)
  matching Python `:2256/2394/2497`. Keys exist in teri en/zh with `{error}` placeholder; `t_args`
  faithfully mirrors `locale.py` ({name}-replace, zh-fallback, key-passthrough). 4 mapper tests
  (504 status, success:false, raw vs i18n, per-route EN+ZH keys, non-timeout deferral). FAITHFUL.
- close-env graceful body BYTE-faithful: route timeout arm (`:3110-3118`) emits
  `{success:true, data:{success:true, message:"环境关闭命令已发送（等待响应超时，环境可能正在关闭）"}}` —
  exact CJK literal + exact `{success, data:{...}}` envelope from Python route(`:2698-2701`)×primitive
  (`simulation_runner.py:1655`). Structural relocation (Python swallows in primitive, teri at route)
  is observably faithful on body+status.

**DIVERGENCE B-1 (real, unflagged, producer-pending) — status-update side effect on close-env timeout.**
Python close-env route swallows TimeoutError IN THE PRIMITIVE → route exception NOT raised → route
*unconditionally* runs the status block: `state.status=COMPLETED; _save_simulation_state` (`simulation.py:2691-2696`).
teri's `close_env_route` Timeout arm `return`s EARLY (`:3110-3118`) **before** the status-update block
(`:3122-3128`) → teri does NOT persist status=Completed on the close-env timeout path. Same JSON body
+ 200, but a DIFFERENT persisted side effect. This is NOT an inexpressible/non-contractual/superset
`[≠]` — it is a dropped side effect. UNREACHABLE today (close-env IPC producer-pending until cycle 3),
so latent, but the porter MUST fix before cycle 3 makes it reachable: on the timeout arm, set
status=Completed + save THEN return the graceful 200. Recorded as a porter follow-up, not a cycle-1
blocker (the directly-claimed surface — mappers + IPC production — is faithful; the route handler is
explicitly producer-pending honest-gap).

**DIVERGENCE B-2 (cosmetic, in-message) — int-vs-float timeout render in 504 error text.**
Python interview routes default `data.get('timeout', 60)` → **int 60** → `send_command` →
`f"等待命令响应超时 ({60}秒)"` → `"60秒"`. teri `parse_timeout(_, 60.0)` → f64 → `{:?}` → `"60.0秒"`.
So the i18n-wrapped 504 body diverges (`...60秒` vs `...60.0秒`) whenever the timeout is integer-valued
(default, or a client int). The test `map_interview_err_*` bakes in teri's `60.0秒` rather than catching
this. Low severity (numeric formatting inside an error string, producer-pending path). Recommend the
porter coerce integer-valued timeouts to match Python's int repr, or record an explicit `[≠]` cosmetic.

### Honest-gap audit — CONFIRMED not over-claimed
All timeout paths are UNREACHABLE end-to-end today (no live IPC env until U-028/029/030 cycle 3).
The cycle correctly claims ONLY: (1) the pure config table; (2) IPC `send_command` elapsed→Timeout
production; (3) the two mappers. The route-level 504/graceful-200 through a full live handler is
producer-pending and NOT claimed as tested. The cycle does NOT over-claim.

### Symbol-map rollup
No whole U-028 symbol is fully exercised by cycle 1 (S-877 `run`, S-865/866 interview handlers,
S-868 poll-loop remain producer-pending). The cycle delivers the config→tick mapping (a fragment of
S-877's contract) and the timeout-mapping infra (cross-cutting, not a single U-028 symbol). U-028
symbols S-853..S-879 stay `- [ ]`/`- [~]` (NOT flipped to `- [x]`) — their full contracts are unproven.

**VERDICT:** (a) **PASS** · (b) **PASS** (mappers + IPC production faithful; 2 divergences recorded in
producer-pending route surface — B-1 a real dropped side effect to fix before cycle 3, B-2 cosmetic).
Cycle-1 claimed surface is parity-clean. 1502 passed / clippy clean. No cycle-1 blocker.

---

## U-028 Cycle 2 — `load_agent_pool` profile→`AgentPool` reader — PARITY VERDICT (2026-06-19)

**Verifier:** rust-port-parity-verifier (adversarial, default-skeptical, fail-closed).
**Unit:** U-028 (twitter producer), Cycle 2 deliverable only — the deterministic profile→pool
round-trip. **Scope guard honored:** activation policy + RunInputs wiring + actions.jsonl producer
are CYCLE 3 and were NOT verified here (correctly out of scope).
**Landed:** `src/services/oasis_profile_export.rs` — `pub fn load_agent_pool` + 4 private helpers
(`load_twitter_csv_into`, `load_reddit_json_into`, `social_profile_base`, `push_profile_agent`) +
4 round-trip tests. **Build:** `cargo test -p teri` = **1506 passed / 6 ignored**;
`cargo clippy -p teri --all-targets --all-features` = **clean**.

### Differential method
Source contract is the **round-trip** (writer→reader recovers what OASIS would feed its agent graph).
The authoritative source-of-truth is NOT just the MiroFish writer — it is **what the OASIS library's
`generate_twitter_agent_graph` / `generate_reddit_agent_graph` actually CONSUME** from the files. I
read the real OASIS library source the subprocess imported
(`backend/.venv/.../oasis/social_agent/agents_generator.py`) to settle the `[≠]` challenges, ran the 4
landed round-trip tests, and ran 5 ADVERSARIAL PROBE tests against uncovered edges (header-only CSV,
ragged row, bad `user_id` cell, empty/malformed reddit JSON, parallel-missing-file) directly through
`load_agent_pool` (added, run, reverted — suite green after revert).

### Refute target findings

**RT1 — twitter round-trip fidelity (col0=row-index, user_char→persona): CONFIRMED FAITHFUL.**
The OASIS fn MiroFish calls is `generate_twitter_agent_graph` at `agents_generator.py:614-650` (NOT
the follow-graph variant at :40). It reads **exactly 3 columns**: `user_char`→`other_info.user_profile`
(the LLM system-prompt personality), `username`→`UserInfo.name`, `description`→`UserInfo.description`
(bio). `agent_id` = the `range(len(agent_info))` loop index — it does **NOT** read the CSV `user_id`
column at all. teri reads col0→`SocialProfile.user_id` = the writer's enumerate row index (proven by
`load_agent_pool_twitter_roundtrip`: row 0 → user_id 0, row 1 → user_id 1), col3 (`user_char`)→
`social.persona`, col4 (`description`)→`bio`. The whole `user_char` blob → `social.persona` is the
faithful recovery of OASIS's `user_profile` system-prompt personality. PASS.

**RT2 — reddit demographics + conditionals: CONFIRMED FAITHFUL.** OASIS reddit
(`agents_generator.py:567-610`) reads `persona, mbti, gender, age, country` (`other_info`) +
`username` (name) + `bio` (description). teri recovers all of these PLUS karma/created_at/profession/
interested_topics. `load_agent_pool_reddit_roundtrip_*` proves: present-conditionals recovered
(profession="engineer", topics=[ai,music]); ABSENT-conditionals on a minimal profile read back as
`None`/`[]` (NOT fabricated), while the writer's forced demographic defaults (age=30/gender=other/
mbti=ISTJ/country=中国) are recovered. PASS.

**RT3 — the two `[≠]` flags: BOTH SURVIVE THE `[≠]` CHALLENGE (legitimate, not disguised skips).**
- `[≠] U028-CSV-LOSSY` — **legitimate (non-contractual / strict-superset).** The decisive evidence:
  OASIS's twitter generator consumes only `user_char`/`username`/`description` (3 of the 5 columns);
  it reads NO karma/demographics from the CSV. The MiroFish writer `_save_twitter_csv`
  (`oasis_profile_generator.py:1070-1119`) writes exactly those 5 columns and drops karma/demographics
  — and OASIS never reads them. So the loss is the OASIS contract itself, and teri actually recovers a
  SUPERSET (all 5 cols + base counters), not a downgrade. (Aside corroborating the "lossy is faithful":
  the *follow-graph* OASIS variant at :40 DOES read `following_agentid_list`/`previous_tweets`, but the
  MiroFish CSV never writes those columns either — so that variant is unusable with MiroFish's own
  files; the actually-called :614 variant is the contract.) NOT a feature skip.
- `[≠] U028-PERSONA-CORE-FROM-PROFILE` — **legitimate (dest-superset fill).** `Persona.background/
  traits/role` have no OASIS-profile counterpart (OASIS profiles are bio/persona/demographics only);
  filled `background=bio, traits=[], role="agent"`. No source behavior dropped; every field OASIS
  produces is read. NOT a feature skip.

**RT4 — error/edge: PASS, with one Cycle-3 watch item (NOT a Cycle-2 defect).** Empirically probed:
missing file → `TeriError::Sim` error (not silent empty pool) ✓; unknown platform → error ✓;
parallel with one file missing → error (no partial pool leaks) ✓; malformed reddit JSON → `Json`
error (not silent empty) ✓; empty header-only CSV / `[]` reddit → empty pool (correct, not error) ✓.
Ragged CSV row (`flexible(true)`): the agent is KEPT with missing cells empty + bad `user_id`→0
fallback (no panic, no corruption, no silent drop). Since the writer always emits well-formed 5-col
rows, ragged input is not a real-world contract path, and teri's tolerance exceeds OASIS's
(`pd.read_csv` would NaN/raise). Not a downgrade.

**RT5 — field completeness + base counters: PASS.** `social_profile_base` sets ALL ~19 `SocialProfile`
fields; the OASIS-default base counters (karma 1000 / friend 100 / follower 150 / following 100 /
statuses 500) match the writer's `save_reddit_json` defaults and the `SocialProfile::default_*` impls
(`agent/mod.rs:79-90`). Parsed values override the base where present.

### Cycle-3 WATCH ITEM (informational, not a Cycle-2 gate failure)
teri's decision template (`agent/mod.rs:1701-1711`) injects `agent_background` (= the reader's
`bio`-fill) and `agent_traits`, NOT `social.persona`. OASIS feeds `user_char`→`user_profile` as the
agent's system-prompt personality. The reader correctly STORES `user_char` in `social.persona` (data
not lost), but Cycle 3's decision wiring must route `social.persona` (the OASIS personality) into the
prompt — otherwise the recovered personality is shadowed by the bio-fill. Flagged for Cycle 3; the
Cycle-2 round-trip recovery is faithful and complete.

### Symbol coverage (this cycle's deliverable)
The Cycle-2 symbols (`load_agent_pool` + 4 helpers) are the substrate mapping of the OASIS
profile-load step inside `TwitterSimulationRunner.run` (S-877) / `RedditSimulationRunner.run` (S-904),
via `_get_profile_path` (S-873 twitter / S-900 reddit) → `generate_*_agent_graph` (OASIS-library, no
S-row). These platform-runner S-rows remain `- [ ]` (their FULL contract — run loop, env.step, IPC —
is producer-pending in Cycle 3); ONLY the deterministic profile→pool reader landed and is verified.
Both `[≠]`s are recorded + challenge-passed; no new S-row flips this cycle (correct — same discipline
as Cycle 1).

**VERDICT: PASS.** The deterministic profile→pool round-trip is parity-clean. Twitter (3-col OASIS
consume, superset recovery) + reddit (lossless demographics + honest conditional None/empty) +
parallel union + error/edge paths all verified differentially against the real OASIS library contract.
Both `[≠]`s are legitimate (non-contractual lossy-collapse / dest-superset fill), NOT disguised
feature-skips. 1506 passed / clippy clean. No downgrade. The `pool`-field half of
`GAP-U026-RUNINPUTS-BUILDER` is now satisfied; Cycle 3 (activation + RunInputs + actions.jsonl) may
proceed, with the one persona-routing watch item above.

---

## 2026-06-19 — U-028 Cycle 3a · `TimeActivationPolicy` (port of `_get_active_agents_for_round`) — VERDICT: **PASS**

**Verifier:** rust-port-parity-verifier (adversarial, fail-closed).
**Unit/symbol:** U-028 · **S-876** `TwitterSimulationRunner._get_active_agents_for_round` (`run_twitter_simulation.py:462-529`) + round→hour math (`:635-636`). Contributes to U-029 **S-903** (reddit mirror, byte-identical — see below).
**Rust:** `src/sim/activation.rs` — `TimeActivationPolicy::{from_config, simulated_hour, active_agents}`, `select_multiplier`, `AgentActivation`. `pub mod activation;` in `src/sim/mod.rs:2`. Dep `rand = "0.8"` (`Cargo.toml:50`; `Cargo.lock` rand 0.8.6).
**Gates:** `cargo test -p teri` = **1515 passed / 6 ignored** (baseline held); `cargo clippy -p teri --all-targets --all-features` = **clean**. 9 module tests + 3 gate-added adversarial tests (500-seed fuzz, min==max, truncation) all PASS, adversarial tests reverted (clean `git diff`).

### Differential parity — source (Python) vs Rust, line-by-line
| Behavior | Python (twitter) | Rust | Verdict |
|---|---|---|---|
| `simulated_hour = (round*mpr//60)%24` | `:635-636` | `simulated_hour` `:165-168` | MATCH (table test: r0→0, r2→1, r48→0 day-wrap, r50→1) |
| multiplier select: peak→`peak_mult` `elif` off-peak→`off_mult` else `1.0`; peak precedence | `:490-495` | `select_multiplier` `:76-90` | MATCH (incl. hour-in-both → peak wins, the `if`/`elif` order) |
| `target_count = int(random.uniform(min,max)*mult)` | `:497` | `min+(max-min)*gen::<f64>()` then `*mult as i64` `:189-191` | MATCH. `uniform(a,b)=a+(b-a)*random()` byte-equiv; `as i64` truncates toward zero = `int()` (5.9→5 verified); `min==max` returns `a`, no panic (`gen_range(a..a)` WOULD panic — manual form avoids it, 500-seed fuzz confirms) |
| active_hours gating: `hour ∉ active_hours → skip` | `:507-508` | `:196-198` | MATCH (deterministic regardless of seed — verified over 20 seeds) |
| activity threshold `random.random()<activity_level` (0.0 never, 1.0 always) | `:511-512` | `gen::<f64>()<activity_level` `:199-201` | MATCH (boundary tests both directions) |
| sample cap `random.sample(cands, min(target,len))` without replacement | `:515-518` | `choose_multiple(rng, clamp(0,len))` `:211-214` | MATCH. `choose_multiple` = without-replacement (uniqueness verified 500 seeds, 0 dups, 0 out-of-candidate); `clamp` lower-0 guards only unreachable negative k (target is structurally ≥0) — faithful guard, not a behavior change |
| `if candidates else []` | `:518` | `candidates.is_empty() → Vec::new()` `:205-207` | MATCH |
| id→agent resolution (`env.agent_graph.get_agent`) | `:520-529` | DEFERRED to 3b run loop (per scope) | correctly out of scope |

### `.get` default fidelity (the key parity subtlety) — CONFIRMED byte-correct
`from_config` (`:110-141`) mirrors the SCRIPT's `.get` defaults exactly, NOT the U-019 dataclass:
`peak_hours [9,10,11,14,15,20,21,22]` (`:113` vs Py `:487`), `off_peak_hours [0,1,2,3,4,5]` (`:114` vs `:488`), `peak_mult 1.5` (`:115` vs `:491`), `off_peak_mult 0.3` (`:116` vs `:493`) — NOT the dataclass `0.05`, agents_per_hour min/max `5`/`20` (`:111-112` vs `:483-484`), per-agent `active_hours range(8,23)` = `(8..23)` (`:119` vs `:503`), `activity_level 0.5` (`:137` vs `:504`), `agent_id 0` (`:126` vs `:502`), `minutes_per_round 30` (`:110`). Mirroring the script (not the dataclass) is CORRECT: `_get_active_agents_for_round` IS the ported function; it reads the raw dict with its own fallbacks. In practice the generator always writes the keys so the defaults never fire — but for byte-parity on a key-absent artifact the literals must match the script, and they do.

### reddit "structurally identical" claim — VERIFIED
`run_reddit_simulation.py:469-521` (`_get_active_agents_for_round`, S-903) diffed against twitter `:462-529`: SAME defaults (`:481-485,488,490,498-500`), SAME formula (`:494` `int(uniform*mult)`), SAME gating (`:502-505`), SAME sample cap (`:508-511`), SAME round→hour (`:627-628`). No divergence. The single port covers both; reddit is not a separate algorithm.

### `[≠]` challenge
- **`[≠] U028-RNG-SEQUENCE`** (exact selected multiset): SURVIVES the bar. Python is **unseeded** (verified: no `random.seed` in either script) → the exact sequence is non-reproducible **in Python itself**, so there is no stable source sequence to match — genuinely NON-CONTRACTUAL. teri preserves the verifiable STRUCTURE (gating, multiplier, count cap, uniqueness, candidate membership); only the draw order differs. NOT a disguised feature-skip — there is no observable export, format, or branch being dropped. Legal non-contractual-sequence `[≠]`.
- The seedable `StdRng` itself is `[≠]`-neutral: a strict testability superset (Python had no seed); production path uses `from_entropy` matching Python's run-to-run non-determinism.

### Scope (3a only) — CONFIRMED
NOT wired into `SimEngine::run`: no `SimConfig.activation` field (`mod.rs:339` is only a forward-looking comment), no run-loop consult (0 refs to the policy in `mod.rs`), no production caller of `active_agents` (only the module's own tests). Integration (`build_run_inputs` + actions.jsonl producer + `/start` 200) correctly deferred to 3b. The id→agent resolution (Py `:520-529`) is correctly deferred to 3b's run loop.

### Test honesty — CONFIRMED non-tautological
The 9 tests exercise the contract, not the happy path: seed-independent gating (20-seed loops), activity 0.0/1.0 boundaries, target_count cap + no-duplicate, reproducibility under fixed seed, peak-precedence, simulated_hour day-wrap, empty-config. Gate-added adversarial fuzz (500 seeds: selected ⊆ candidates ∧ unique ∧ |selected| ≤ |candidates|), min==max no-panic, and truncation-toward-zero all PASS — none cherry-picked, all reverted.

**VERDICT: PASS.** Symbols verified 1/1 for this cycle (S-876; reddit S-903 confirmed identical, marked when U-029 cycle lands). No downgrade. The one `[≠]` is legal (non-contractual unseeded sequence). Mark S-876 `- [x]`.

---

## 2026-06-20 — U-028 Cycle 3b-i · actions.jsonl PRODUCER + activation-gate wiring into `SimEngine::run` — VERDICT: **PASS**

**Verifier:** rust-port-parity-verifier (adversarial, fail-closed). Read the REAL Python source + REAL teri code; ran the tests.
**Unit/cycle:** U-028 · cycle 3b-i (producer half of `S-877 TwitterSimulationRunner.run`). Authoritative producer source = `run_parallel_simulation.py:run_twitter_simulation` (`:1101-1290`) — the ONLY MiroFish script that writes `actions.jsonl` (single-platform scripts do not; teri's landed monitor tails exactly this file).
**Rust verified:** `src/sim/mod.rs` — `SocialAction::oasis_action_type()` (`:108-125`), `SocialAction::oasis_action_args()` (`:133-153`), `trait ActivationPolicy` (`:525-530`), `struct RunProducer` + `minutes_per_round()` (`:545-563`), `SimEngine` fields `activation`/`producer` (`:614/:617`) + builders `with_activation`/`with_producer` (`:649/:659`), `SimEngine::run` rewrite (`:734-947`); `src/sim/activation.rs` — `impl ActivationPolicy for TimeActivationPolicy` (`:225-229`). Logger record bodies (`src/sim/action_logger.rs:115-201`) re-confirmed byte-faithful (landed S-072..S-075).
**Gates:** `cargo test --lib` = **1505 passed** (producer_tests = 5 pass); `cargo clippy --lib --all-features` = **clean**. No pre-existing `run` caller changed (additive None-default seams).

### Differential parity — Python `run_twitter_simulation` vs teri `SimEngine::run`, line-by-line
| Contract behavior | Python (`run_parallel_simulation.py`) | teri | Verdict |
|---|---|---|---|
| `ACTION_TYPE_MAP` values | `:614-629` (13 social keys + `interview`) | `oasis_action_type` `:108-125` | MATCH — all 13 social arms incl. `Comment→"CREATE_COMMENT"` (not "COMMENT"), `Like`/`Dislike` split POST/COMMENT via `TargetKind` → 4 LIKE/DISLIKE entries, `Trend→"TREND"`, `DoNothing→"DO_NOTHING"`. (`interview`/`refresh`/`sign_up` are OASIS-DB-internal, not the SocialAction taxonomy — see `[≠]` below.) |
| `simulation_start` BEFORE loop | `:1163-1164` `log_simulation_start(config)` | `:765-770` (emitted pre-loop) | MATCH |
| `log_round_start(round+1, hour)` ALWAYS, even empty | `:1244-1245` then `if not active: log_round_end(round+1,0); continue` | `:812-821` round_start every round; empty set → 0 actions, round_end count 0 `:899-906` | MATCH (1-based round; empty round still start+end) |
| `simulated_hour = (round*mpr//60)%24`, mpr default 30 | `:1234-1236`,`:1216` | `:816` `(tick*mpr/60)%24`; `minutes_per_round` default 30 `:556-562` | MATCH |
| per committed social action → `log_action` | `:1265-1273` | `:874-895` only `Action::Social(_)` | MATCH |
| `log_action` fields: round=round+1, agent_id, agent_name, action_type, action_args | `action_logger.py:43-66` (8-key, result=None, success=True) | `:884-893` round(1-based), agent_id=`user_id as i64`, name=`Persona.name`, type via map, native args, result=None, success=true | MATCH field-for-field; logger body `action_logger.rs:115-136` byte-order faithful (preserve_order on) |
| `log_round_end(round+1, count)` | `:1274-1275` | `:899-906` | MATCH |
| `log_simulation_end(total_rounds, total_actions)` AFTER loop | `:1284` — `total_rounds` = config-derived count (NOT executed), `total_actions` = Σ log_action | `:928-932` passes `max_ticks`(==config total_rounds) + accumulated Social count | MATCH — early-shutdown still passes full config `max_ticks` (mirrors Python passing untruncated-by-break `total_rounds`) |
| only Social actions logged (DB holds only OASIS social rows) | DB fetch `:1267` | generic Speak/Move/etc never logged `:874` | MATCH (faithful) |
| activation gate: `if not active_agents: continue` | `:1247-1251` | empty `active_indices` → no prepare/commit/log `:793-810` | MATCH |
| id→agent resolution by numeric id | `env.agent_graph.get_agent` `:520-529` | `SocialProfile.user_id`-match onto pool `:797-808` | MATCH — agents w/o a social profile are excluded under a policy (faithful: Python's `agent_configs` carry numeric ids; an un-profiled agent has no OASIS id and is not in agent_graph for this purpose) |

### Adversarial refutation attempts (all FAILED to refute)
1. **`simulation_start.total_rounds` (logger's `hours*2`) vs `simulation_end.total_rounds` (loop's `hours*60//mpr`) — divergence teri must MIRROR.** CONFIRMED mirrored: `action_logger.rs:173` uses `total_simulation_hours*2`; `SimConfig::from_simulation_config` (`mod.rs:415-419`) uses `int(hours*60/mpr)`; `simulation_end` passes `max_ticks`. For the canonical 30-min round they coincide (72h→144 both); for any other mpr they DIVERGE exactly as Python's two independent formulas do (e.g. 72h/60min → sim_start 144 vs sim_end 72). teri reproduces BOTH Python quirks independently — not an accidental change. NOT refuted.
2. **Off-by-one in round numbering / round-0 silently dropped.** Round 1 IS the first MAIN-loop round (`tick_idx 0 → round 1`, `:814`). Python's round-0 initial_posts phase (`:1175-1211`) is genuinely DEFERRED, not emitted — see deferral honesty below. NOT refuted.
3. **Empty-round teri snapshot/inject_fn vs Python `continue` → observable JSONL divergence?** NO. On an empty round teri still builds/broadcasts a WorldState snapshot + runs inject_fn (`:908-921`), which Python's `continue` skips — BUT the snapshot stream is a teri-native SSE/history channel with NO Python counterpart (Python has no per-tick world snapshot; it relies on the OASIS DB). It is never written to actions.jsonl, and the monitor (`simulation_runner.rs:457`) keys completion on the `simulation_end` JSONL record only. Orthogonal to the JSONL contract. NOT refuted.
4. **Mid-stream write error → truncated actions.jsonl the monitor misreads?** A failed `log_*` write returns `Err` (`log_err` → `TeriError::Sim`, `:568-570`) and aborts `run()`. Faithful to Python's UNGUARDED `action_logger.*` calls (a write exception propagates out of the coroutine — Python never recovers a half-written line either). A torn line is the monitor's parse-tolerance concern in BOTH worlds; no record-completeness guarantee is dropped. NOT refuted.
5. **agent_id fallback 0 for an un-profiled agent (`:880`).** Unreachable under a policy (the gate excludes profile-less agents). Reachable only on the `None`-activation path with a social action from a profile-less agent — a teri-native combination with no Python analogue (Python only reaches `log_action` for agents resolved from agent_graph by numeric id). The `unwrap_or(0)` is a non-panicking guard for an out-of-contract input, not a behavior divergence. NOT refuted.

### `[≠]` ledger — challenged, all legitimate
- **`[≠] U028-OASIS-INTERNALS`** (`oasis_action_args` emits teri-native keys content/target_id/post_id/user_id/query, NOT the OASIS-DB enrichment keys post_content/author_name/quote_content/follow_id/like_id from `_enrich_action_context` `:749-855`): **LEGITIMATE — genuinely INEXPRESSIBLE.** The enrichment keys are read from the OASIS SQLite trace DB (`fetch_new_actions_from_db` `:657-746`) which teri does not and will not have (teri runs the sim in-process, no OASIS env, no trace DB). There is no teri data source for `post_content`/`author_name`. teri instead emits its OWN native args keyed exactly as `Agent::parse_social_action` parses them, so a record round-trips through teri's parser — the structural fields (content/target_id/post_id/…) ARE emitted faithfully. This is a substrate-absence inexpressibility, NOT a portable feature being skipped (there is no teri-side artifact, format, or branch that could carry the OASIS DB context). Survives the bar.
- The OASIS map entries `interview`/`refresh`/`sign_up` (`:614-629`,`FILTERED_ACTIONS :611`) have no `SocialAction` variant: same boundary — these are OASIS-DB-internal action rows (interview is a separate IPC phase; refresh/sign_up are filtered out by Python ITSELF before logging). Not part of the modeled social taxonomy. Consistent with U028-OASIS-INTERNALS.
- **`[≠] U028-RNG-SEQUENCE`** (inherited from c3a S-876, unchanged this cycle): still legal (Python unseeded → non-contractual draw sequence).

### Symbol coverage — honest, no premature flip
- **S-876** already `- [x]` (c3a); this cycle WIRES it in (`active_agent_ids(tick)=active_agents(simulated_hour(tick))`, `activation.rs:226-228`) — wiring verified, status unchanged.
- **S-877** (`TwitterSimulationRunner.run`) remains **`- [~]`** — its PRODUCER half (actions.jsonl stream + activation gate) is now parity-proven, but its FULL contract (wait-for-commands mode after rounds, IPCHandler, OASIS `env.step`, signal-driven graceful shutdown, `env_status.json`) is NOT ported. Flipping S-877 would overstate coverage. Correct to leave `- [~]` with the producer-half proven and the run-loop contract pending. No S-row is fraudulently flipped (same discipline as c1/c2).
- The teri-native producer symbols (`oasis_action_type`/`oasis_action_args`/`RunProducer`/`ActivationPolicy`/`SimEngine::run` rewrite) are substrate symbols realizing the producer half of the parallel-script's `run_twitter_simulation` (U-030 territory) — verified here; they flip no source S-row on their own.

### Deferred items — HONESTLY flagged, NOT silently dropped
- **Round-0 initial_posts phase** (Python `:1175-1211`: `log_round_start(0,0)`, per-post CREATE_POST `log_action(round_num=0,...)`, `log_round_end(0,count)`): teri emits NO round-0 records — the MAIN loop starts at logged round 1. This is correct DEFERRAL to **c3b-ii**, NOT a drop: the engine's loop has no round-0 phase yet because `build_run_inputs`/initial-events wiring is the next half. The full-stream golden test asserts the stream STARTS at round 1 (no round-0 record), making the deferral explicit and test-pinned. **MUST be ported in c3b-ii** before U-028 can close.
- **`build_run_inputs` / `/start` 200 / monitor→COMPLETED / round-0 initial_posts** → c3b-ii/iii (out of scope here, per the cycle plan).
- **U-030 dual-sink** (parallel twitter+reddit, `fetch_new_actions_from_db`/`_enrich_action_context`) → U-030 (`parity-ledger.md:129`, still `- [ ]`).
- U-028 ledger row (`parity-ledger.md:125`) correctly remains `- [ ]` (full single-platform-runner contract: IPC, signals, wait-mode, setup_oasis_logging all unported).

### Test honesty — non-tautological
The 5 producer_tests exercise the contract: full-stream 10-record golden (exact structure, 1-based rounds, simulated_hour formula, agent_id from user_id, action_type map, total_actions=4), empty-activation round (start+end(0), zero action records, sim_end total_actions=0), user_id subset gate (only matched agent acts, correct agent_id/name/args), no-producer→no-file (additive seam proven byte-unaffected), TimeActivationPolicy integration (real policy → monitor-terminating sim_end). Not happy-path-only.

**VERDICT: PASS.** The actions.jsonl producer stream + activation gate are parity-clean against `run_twitter_simulation` over the whole producer contract (start/round_start-always/log_action/round_end/end, 1-based rounds, simulated_hour, both independent total_rounds formulas, Social-only logging, empty-round, user_id gate, early-shutdown end). The one `[≠]` (OASIS-INTERNALS) is genuinely inexpressible (no OASIS trace DB in teri), NOT a disguised feature-skip. All 5 adversarial refutation attempts failed. The round-0 initial_posts phase and the c3b-ii/iii + U-030 halves are honestly flagged pending — nothing silently dropped. No premature S-row flip (S-877 stays `- [~]`, producer-half proven). 1505 lib pass / clippy clean / no caller changed. No downgrade.

---

## 2026-06-20 — U-028 Cycle 3b-ii · `build_run_inputs` + `/start` honest-500→200 swap (single-platform) — VERDICT: **PASS**

**Verifier:** rust-port-parity-verifier (adversarial, fail-closed). Read the REAL Python source + REAL teri code; ran the tests + clippy.
**Unit/cycle:** U-028 · c3b-ii (API/runner half of GAP-U026-RUNINPUTS-BUILDER for SINGLE-platform twitter/reddit). Source = `simulation.py:start_simulation` success path (`:1604-1641`); runner `start_simulation` (`simulation_runner.py:313-347`); `RunState.to_dict` (`:160-186`).
**Rust verified:** `src/api/simulation.rs` — `build_run_inputs` (`:2094-2162`), `start_simulation` route (`:2169-2357`), `map_runner_err` (`:1985-1993`), `coerce_max_rounds` (`:1784-1824`); `src/services/simulation_runner.rs` — `start_simulation` (`:1069-1234`), `SimulationRunState::to_dict` (`:572-650`); `src/sim/mod.rs` — `SimConfig::from_simulation_config` (`:398-432`); `src/api/mod.rs` — `ApiError::server` (`:217-227`).
**Gates (re-run by verifier):** `cargo test --lib` = **1507 passed** (was 1505, +2). `cargo clippy --lib --all-targets --all-features` = **No issues found**. All 3 cited tests independently re-run green: `start_simulation_twitter_prepared_returns_200`, `start_simulation_prepared_reaches_gap_500`, `producer_run_reaches_completed_via_monitor`.

### Differential parity — Python `/start` success path vs teri, line-by-line
| Behavior | Python | teri | Verdict |
|---|---|---|---|
| `run_state = runner.start_simulation(id, platform, max_rounds, mem, graph_id)` | `:1604-1610` | `state.sim_runner.start_simulation(...)` `:2320-2332` | MATCH |
| `state.status = RUNNING; manager._save(state)` AFTER runner returns | `:1613-1614` | `sim.status=Running; save_simulation_state` AFTER `:2335-2336` | MATCH — ordering identical (save after runner returns) |
| `response_data = run_state.to_dict()` (RUNNING snapshot) | `:1616` | `run_state.to_dict()` `:2339` — runner returns `state` w/ `runner_status=Running` (`runner.rs:1194/1233`) | MATCH — RUNNING-state snapshot, 24 base keys byte-identical + identical insertion order (preserve_order) |
| `if max_rounds: response['max_rounds_applied']=max_rounds` | `:1617-1618` | `if let Some(mr) {...}` `:2340-2343` | MATCH — `coerce_max_rounds` guarantees any Some is `>0` (≤0 already 400'd), so `Some(mr)` ≡ Python truthy `if max_rounds:`. The `max_rounds=0` truthiness edge is UNREACHABLE (0 → maxRoundsPositive 400). |
| `response['graph_memory_update_enabled']=mem` ALWAYS | `:1619` | unconditional insert `:2344-2345` | MATCH (always present) |
| `response['force_restarted']=force_restarted` ALWAYS | `:1620` | unconditional insert `:2346` | MATCH (always present) |
| `if mem: response['graph_id']=graph_id` | `:1621-1622` | `if mem && Some(gid) {...}` `:2347-2351` | MATCH (only when memory enabled) |
| `return {success:true, data:response_data}` | `:1624-1627` | `{success:true, data:Object(response_data)}` `:2353-2356` | MATCH — key names byte-identical |
| `except ValueError → 400 {success:false,error}` | `:1629-1633` | `map_runner_err(Sim→400)` `:1990` on runner Err | MATCH — runner ValueErrors (already-running `:337`, missing-config `:344`, missing graph_id `:375`, missing script `:402`) are all `TeriError::Sim` → 400. Faithful. |
| `except Exception → 500 + traceback` | `:1635-1641` | `ApiError::server` 3-key `{success:false,error,traceback}` `mod.rs:217-227` | MATCH (shape; `[≠] U025-TRACEBACK` value-only, pre-recorded) |

### Adversarial refutation attempts (all FAILED to refute)
1. **Two config reads diverge (engine total_rounds via `from_simulation_config` vs runner total_rounds).** Both use the SAME `get_simulation_config` artifact and the IDENTICAL formula: defaults 72/30, `int(total_hours*60/mpr)` float-truncate, zero-divisor→0 guard, `min(max_rounds)` only-when-`>0`. (`mod.rs:403-426` ≡ `runner.rs:1091-1118`.) `max_ticks`(engine) and `total_rounds`(run-state) coincide by construction. NOT refuted — verified by the 200 test (1h/30min → 2 rounds, `data["total_rounds"]==2`).
2. **200 returned while spawned run later fails (LLM unreachable).** Faithful: Python's `/start` is an async subprocess (`Popen`) — the 200 reports STARTED, not COMPLETED; a later LLM failure flips the run-state to FAILED out-of-band in BOTH worlds. The `to_dict()` is the start-time RUNNING snapshot in both. NOT refuted.
3. **`max_rounds_applied` truthiness gap (Python `if max_rounds:` falsy on 0).** Impossible: `coerce_max_rounds` (`:1817-1822`) 400s any `n<=0` (incl. float-truncated 0.5→0) BEFORE the response, so `max_rounds` reaching the response is always `Some(>0)` ≡ Python truthy. NOT refuted.
4. **Parallel honest-500 is a disguised downgrade / single-logger could "work" for parallel.** Single-platform path is fully wired (200, verified); parallel returns `ApiError::server` (true 500, success=false, traceback, describes the dual-sink gap + U-030). A single-logger producer MUST NOT serve parallel: reddit agents' actions would misroute to `twitter/actions.jsonl` and the monitor's dual-platform `simulation_end` gate (S-615) would never fire. The deferral is CORRECT (substrate-honest), not lazy. Gap test asserts the honest-500 shape, NOT a fabricated 200. NOT refuted.
5. **`build_run_inputs` loses behavior — graph clone for updater vs engine; parallelism=8.** Engine's `_graph` is reserved (no-op currently, pre-recorded); the updater (U-021) writes the `Arc<Mutex<clone>>` — no behavior lost vs Python (graph-memory updater is the only graph consumer). parallelism=8 is a `[≠]` perf knob (OASIS semaphore=30 — non-contractual concurrency budget, architect §2 default), NOT an observable-contract change. activation seeded `None` = entropy (production, matches Python unseeded — `[≠] U028-RNG-SEQUENCE` pre-recorded). NOT refuted.

### `[≠]`/`[!]` ledger — challenged, all legitimate
- **`[!] U028-PARALLEL-DUALSINK`** (default platform=parallel → honest 500): LEGITIMATE honest gap. Narrowed from the old all-platform GAP-U026-RUNINPUTS-BUILDER 500 to ONLY parallel; twitter/reddit now 200. No fabrication, no single-sink misroute, true server-error shape w/ traceback. Deferred to U-030/c3b-iii (needs multi-logger producer + dual-platform monitor gate). This is a `[!]` (substrate not yet built), correctly NOT a `[≠]` disguising a skip — the feature WILL be ported.
- **`[≠]` eager-vs-lazy seam (U-022 RunInputs subprocess-boundary):** teri reads config+profiles SYNCHRONOUSLY in `build_run_inputs` before spawning; Python's `/start` returns immediately and the subprocess reads them later — so a missing profile/config → teri `/start` errors vs Python `/start` 200-then-empty-run. LEGITIMATE: unreachable in practice (the READY-state gate via `check_simulation_prepared` ⟹ `/prepare` already wrote both artifacts), honestly documented in the `build_run_inputs` doc-comment (`:2089-2093`). Non-contractual divergence on an unreachable path, NOT a feature skip.
- **`[≠] U028-RNG-SEQUENCE`** (inherited, unchanged): still legal (Python unseeded → non-contractual draw sequence).
- **`[≠] U025-TRACEBACK`** (inherited, the 500 traceback value is Rust text): still legal (3-key contract preserved, value non-contractual).

### Symbol coverage — honest, no premature flip
- **S-820** (`POST /start`) STAYS **`- [~]`** — single-platform twitter/reddit success path is now parity-proven (200 byte-faithful, ValueError→400, RUNNING-snapshot, ordering), but the FULL contract requires the `parallel` dual-sink which is honestly gapped (`[!] U028-PARALLEL-DUALSINK`). Flipping to `- [x]` would overstate coverage (per the no-over-flip rule). Flips `- [x]` when U-030 lands parallel. Annotation updated to record c3b-ii progress. CORRECT status.
- **S-877** (`TwitterSimulationRunner.run`) unchanged `- [~]` (producer + now run-assembly proven; full IPC/wait-mode/env.step contract still pending — U-030 territory).
- teri-native `build_run_inputs` is a substrate symbol realizing the RunInputs-builder half; it flips no source S-row on its own.

### Test honesty — non-tautological
The 200 test asserts the conditional-field truthiness exactly (memory off ⟹ NO `graph_id`, NO `max_rounds_applied`; `force_restarted`/`graph_memory_update_enabled` ALWAYS present; `total_rounds==2` from the real 1h/30min config formula). The gap test asserts the HONEST 500 (success=false + traceback key + error describes "parallel dual-sink"/"U-030"), refuting any fabricated-200. The monitor test is the end-to-end gap-closure proof (producer writes actions.jsonl → monitor → COMPLETED). Not happy-path-only.

**VERDICT: PASS.** (a) The 200 response shape is byte-faithful — all 24 `to_dict()` base keys in identical order + the 4 add-on fields with exactly-matching truthiness/conditionality vs Python `:1616-1622`; key NAMES byte-match. (b) The `parallel` gap is a legitimate honest `[!]` (narrowed, true-500, no single-sink misroute, no fabrication), NOT a downgrade — a single-logger producer would misroute reddit actions and never satisfy the dual-platform completion gate. (c) The eager-vs-lazy `[≠]` is honestly recorded (doc-comment `:2089-2093`) and unreachable past the READY gate. (d) S-820 correctly STAYS `- [~]` (single-platform proven, parallel gapped) — NOT flipped to `- [x]`. ValueError→400 mapping faithful, status=RUNNING-save ordering matches, both config reads agree. 1507 lib pass (+2) / clippy --all-targets --all-features clean. No downgrade. All 5 adversarial refutations failed.

---

## CYCLE 52 (29th resume) — U-030 cycle A: RunProducer generalization + run() fan-out engine

**Scope:** generalize the single-platform `actions.jsonl` producer to per-platform routing WITHOUT changing single-platform output, so the unified `SimEngine::run` can emit twitter/ AND reddit/actions.jsonl for a parallel run. Engine-only (no API change; parallel `/start` stays honest-500 until cycle B). Architected first → `findings/u030-architecture.md` (DECISION-U030-1/2/3, A/B/C split).

**Changes:** `RunProducer.logger: Arc<PlatformActionLogger>` → `RunProducer.loggers: PlatformLoggerSet` (internal `Vec<(Platform,Arc<…>)>`; `::single`/`::parallel`/`get(platform)`); NEW `PerPlatform<i64>` per-platform accumulators; `SimEngine::run` producer wiring rewrite (boundary records fan out to ALL loggers; each committed `Action::Social` routes to `producer.loggers.get(agent.persona.social.platform)`; per-platform `round_end`/`simulation_end` counts). `with_producer` sig UNCHANGED. 7 construction sites migrated (`build_run_inputs` single + 4 producer_tests + `run_inputs_with_producer` helper).

- **S-820** (`POST /start`) STAYS `- [~]` — engine generalization alone does not flip it; the parallel `/start` 200 lands in cycle B (then `- [x]`).
- **S-877** (producer half) — generalized to dual-platform routing; full parallel run-loop contract proven in cycle B's e2e dual-gate test. STAYS `- [~]`.

**VERDICT: PASS** (rust-port-parity-verifier, 5/5 adversarial refutations FAILED):
(1) **single-platform BYTE-IDENTICAL** — a one-entry `PlatformLoggerSet` makes every fan-out resolve to the single logger and every `log_action` route to it; the conditional-vs-old-unconditional count increment is non-divergent (single-platform route always hits); the 4 pre-existing producer tests retain IDENTICAL assertions and pass.
(2) **parallel routing faithful** to Python's two `asyncio.gather`'d coroutines (`run_parallel_simulation.py:1101-1290`/`1293-1489`/`1585-1588`) — boundary fan-out each platform-stamped with shared `total_rounds`/`agents_count`/`simulated_hour`; `log_action` routed by `social.platform` (reddit CANNOT land in twitter's file); per-platform `round_end`/`simulation_end` counts via `PerPlatform`.
(3) **`[≠]U030-UNIFIED-LOOP`** is a legit substrate gap, NOT a disguised downgrade — only the shared activation draw + native-vs-DB counts diverge, both rooted in landed `[≠]U028-RNG-SEQUENCE`/`[≠]U028-OASIS-INTERNALS`; record SCHEMA / round-numbering / round_start-always / round_end(r,0)-on-empty / `simulation_end total_rounds==max_ticks` ALL byte-faithful; BOTH streams fully emitted (not a feature-skip).
(4) **route-miss fail-closed**, not silent-drop — unreachable under the pool/logger-set invariant; single-platform no regression vs U-028's unconditional log; only prevents the parallel misroute.

**Test:** +1 `run_parallel_routes_actions_to_platform_loggers` (2 twitter + 1 reddit, 2 rounds → twitter file len 10 / reddit len 8, routed agent_ids, per-platform round_end `[2,2]`/`[1,1]`, sim_end totals 4/2, platform-stamped sim_start). 1509→1510 lib pass. clippy `--all-targets` + `--all-features` clean. Y-not-regressed (develop=7c354a5). No downgrade.

---

## U-030 cycle B — `build_run_inputs` parallel dual-logger + `/start` honest-500→200 swap — PARITY PASS (2026-06-20, opus)

**Verdict: PASS.** Parallel `/start` is faithfully wired (real 200, not fabricated); the 200 envelope matches Python's platform-agnostic shape; the monitor's dual-gate genuinely requires BOTH platforms; no orphaned honest-500 reference, no misroute. **S-820 flips `- [x]`.**

**Scope:** UNCOMMITTED worktree `port/mirofish` on top of cycle A `8f1df6e`. `git diff` = cycle B only (162 lines `api/simulation.rs`, 68 lines `services/simulation_runner.rs`).

### Claim 1 — `build_run_inputs` parallel branch (PASS)
- (a) **Single-platform path BYTE-IDENTICAL** to cycle A: `PlatformLoggerSet::single(platform_enum, make_logger(platform)?)` over the same sim_dir; the only change is the logger ctor extracted into a `make_logger` closure (`api/simulation.rs:2136-2152`). Same enum keying (`reddit→Reddit` else `Twitter`).
- (b) **Parallel path** builds TWO loggers over the SAME sim_dir (`make_logger("twitter")?` + `make_logger("reddit")?`) → `PlatformLoggerSet::parallel(...)`; pool = `load_agent_pool(&sim_dir,"parallel")` which unions twitter CSV (`Platform::Twitter`) + reddit JSON (`Platform::Reddit`) (`oasis_profile_export.rs:462-465`), each agent carrying its own `social.platform`. Routing at `sim/mod.rs:980-981` (`producer.loggers.get(s.platform)`) sends each action to its platform file; route-miss = fail-closed no-op (unreachable under the §3 invariant).
- (c) **Activation policy UNCHANGED**: `engine.with_activation(TimeActivationPolicy::from_config(&config,None))` at `api/simulation.rs:2133` (pre-existing line, untouched); gates the unioned pool by `social.user_id` (`sim/mod.rs:882-897`).

### Claim 2 — honest-500 GONE, real 200 (PASS, no fabrication)
- The `if platform=="parallel" { return Err(ApiError::server(...)) }` block at `/start` is DELETED. `grep` for `U028-PARALLEL-DUALSINK` / `dual-sink simulation not yet` / `reaches_gap` over `src/` → ZERO hits (only an unrelated `c3b-iii` comment in `agent/mod.rs:2440` about persona recovery). Doc-comments on `build_run_inputs` (2181-2186) + `start_simulation` updated; the old in-handler gap comment replaced by the all-platforms-closed comment.
- The 200 is a REAL run: `start_simulation` calls `build_run_inputs` → `state.sim_runner.start_simulation(...inputs...)` (spawns run + monitor) → builds response from the spawned `run_state.to_dict()` + conditionals (`api/simulation.rs:2348-2365`). Envelope is **platform-agnostic and identical to Python :1616-1627** (`run_state.to_dict()` + `max_rounds_applied` only-when-truthy + `graph_memory_update_enabled`/`force_restarted` always + `graph_id` only-when-memory). Parallel produces NO different shape from twitter/reddit (same proven path U-028 c3b-ii verified).
- `start_simulation_parallel_prepared_returns_200`: seeds READY state + config (1h/30min) + BOTH `twitter_profiles.csv` AND `reddit_profiles.json`, posts `platform:"parallel"`, asserts 200 + `success:true`, no traceback, simulation_id, `total_rounds==2`, `graph_memory_update_enabled:false`, `force_restarted:false`, no `max_rounds_applied`, no `graph_id`. Shape matches Python. PASS.

### Claim 3 — e2e dual-gate fires (PASS, gap-closure proof)
- (a) **Genuinely requires BOTH.** `apply_log_record` simulation_end branch (`simulation_runner.rs:1881-1906`) sets `twitter_completed`/`reddit_completed` STRICTLY keyed by the file's platform; monitor reads twitter file as "twitter" / reddit as "reddit" (`1711`/`1715`). `check_all_platforms_completed` (`2014-2033`, faithful port of Python L709-718): if both files exist, `reddit_enabled && !reddit_completed → false`. A one-file completion CANNOT set both flags. Load-bearing & non-vacuous — directly proven by `check_completed_dual_requires_both` ("one platform done must NOT complete") + `simulation_end_dual_one_platform_not_completed`.
- (b) **Empty-pool completion is legitimate.** All boundary records fan out over `producer.loggers.iter()` independent of pool/actions: `simulation_start` (`sim/mod.rs:851`), `round_start` (`908`), `round_end` (`1007`), `simulation_end` (`1037-1042`). With an empty pool both twitter + reddit files still get the full `simulation_start→round_start/round_end×2→simulation_end` stream → both `*_completed` → gate fires. Matches Python's coroutines (cite `run_parallel_simulation.py:1244-1245`/1284) which emit the full boundary stream regardless of actions. `parallel_producer_run_reaches_completed` polls COMPLETED (only after both sim_end) and asserts both files contain `simulation_end`. PASS.

### Python differential
- `run_parallel_simulation.py:1583-1589` else-branch = `asyncio.gather(run_twitter_simulation(...,twitter_logger,...), run_reddit_simulation(...,reddit_logger,...))` — both platforms concurrent, each own logger over same simulation_dir. Rust models the OBSERVABLE output (both platform files, full boundary streams, per-platform routing) via one unified engine over a unioned pool. Output parity holds; structural mechanism (unified engine vs 2 coroutines) is a faithful `[≠]`-class idiom already accepted in cycle A.
- `simulation.py:1604-1627` `/start` response = `run_state.to_dict()` + conditionals — platform-agnostic; Rust matches key-for-key.

### Tests
`cargo test -p teri --lib` → **1511 passed** (expected). `parallel` filter → 5 passed (incl. both new tests). `prepared_returns_200` → 2 passed (parallel + twitter). Old `start_simulation_prepared_reaches_gap_500` cleanly removed.

### S-820 decision — FLIP `- [x]`
S-820's `[~]` reason was exactly ONE residual: `[!] U028-PARALLEL-DUALSINK` (parallel honest-500), per the row's own "flips `[x]` when U-030 lands parallel." Cycle B closes it. The eager-vs-lazy `[≠]` seam is a recorded non-contractual substrate boundary, unreachable past the READY gate (survives the `[≠]` bar — genuinely inexpressible eager/lazy timing, already accepted). The round-0 `initial_posts` item lives on **S-877** (U-028 `TwitterSimulationRunner.run`) and concerns actions.jsonl STREAM CONTENT during an LLM run — ORTHOGONAL to `/start`'s response/run-spawn contract; it does NOT gate S-820. Full S-820 contract (validation + state-machine + check_prepared + cleanup + graph_id + RunInputs-builder-for-ALL-platforms + real-200 + ValueError→400) now holds → **`- [x]`**.

**Symbols verified this cycle: S-820 (1/1 for the unit's `/start` contract).** No downgrade. Y-not-regressed (no merge in scope; this is a port-only cycle in `port/mirofish`).

---

## U-030 cycle C — round-0 `initial_posts` injection in `SimEngine::run` — PARITY PASS (2026-06-20, opus)

**Verdict: PASS.** Round-0 emission is byte-faithful to Python `run_parallel_simulation.py:1171-1211` (twitter) / `1364-1410` (reddit, confirmed structurally identical for logging); the single-platform output change is a genuine parity FIX (Python's single-platform coroutines DO emit round-0); routing + skip are correct; the `env.step` world-injection gap is a legitimate `[≠]U028-OASIS-INTERNALS`. 1512 lib pass (+1). **No symbol flips to `- [x]`** (cycle C is a sub-portion of S-938/S-939's coroutines); S-877's `[~]` note updated to mark round-0 ported.

**Scope:** UNCOMMITTED worktree `port/mirofish` on top of cycle B `4bfb0fe`. `git diff` = cycle C only (192/-32, all `src/sim/mod.rs`: the round-0 block in `run()` L861-924, 4 updated producer-test assertions, +1 new test).

### Claim 1 — round-0 emission byte-faithful to Python L1171-1211 (PASS)
Record-by-record, both sides:
- `round_start(0,0)` — teri L874-876 fans out to ALL loggers; Python L1176-1177 emits unconditionally inside `if action_logger`. Record shape (`action_logger.rs:140-148` vs `action_logger.py:68-78`): `{round:0, timestamp, event_type:"round_start", simulated_hour:0}` — identical keys.
- per `initial_posts` entry → `log_action(0, poster_id, agent.persona.name, "CREATE_POST", {"content":content}, None, true)` — teri L904-914 vs Python L1192-1199. Record shape (`action_logger.rs:115-136` vs `action_logger.py:43-66`): `{round:0, timestamp, agent_id, agent_name, action_type:"CREATE_POST", action_args:{"content":…}, result:null, success:true}` — identical keys, exact `action_args` key `"content"`, `result:None→null`, `success:true`.
- `round_end(0, count)` — teri L919-923 vs Python L1210-1211. `{round:0, timestamp, event_type:"round_end", actions_count:count}` — identical.
- **Count semantics MATCH.** Python `initial_action_count` increments once per SUCCESSFULLY-resolved post (inside the `try`, only after `get_agent` succeeds, L1201); teri `round0_counts.add(platform,1)` only on `routed` (L915). Both = number of successfully-routed posts. The `round_end` count is that number per platform.
- **`total_actions` increment MATCHES.** Python `total_actions += 1` per post (L1200, before the main loop); teri accumulates per-platform then adds once at L922 (`total_actions.add(platform,count)`) before the main loop. Equivalent sum; flows into `simulation_end` (`sim/mod.rs:1105` `total_actions.get(*platform)`).
- **Empty/absent `initial_posts` STILL emits round-0 start/end** — teri gates ONLY on `if let Some(ref producer)` (L873), NOT on `initial_posts` presence (the `if let Some(posts)` only wraps the per-post loop). Python L1176-1177 / L1210-1211 are likewise outside the `if initial_posts:` block. Proven by `run_emits_full_actions_jsonl_stream` (config has NO `event_config` → round-0 trio with count 0) and `run_empty_activation_round_logs_start_end_only`.
- **agent_name source consistent.** Python round-0 uses `agent_names.get(agent_id)` = `entity_name` from `agent_configs` (L645-652); teri uses `agent.persona.name`. This is the IDENTICAL name source already verified for the main-loop CREATE_POST records in cycle A (`sim/mod.rs:1054`), so round-0 is consistent with the proven main-loop path. `agent_id` = matched `s.user_id` = same as main-loop's `social.user_id`.

### Claim 2 — single-platform output change is a parity FIX (PASS)
- (a) **Genuinely matches Python.** BOTH coroutines emit round-0: `run_twitter_simulation` L1176-1211 AND `run_reddit_simulation` L1367-1410 (read both — reddit's logging block is byte-identical to twitter's per-post; the only reddit-specific delta is multi-post-per-agent dict accumulation for `env.step`, which does NOT change the `log_action`/count/`round_end` emission). U-028 single-platform OMITTED round-0; cycle C ADDS it → now faithful to the single-platform coroutine.
- (b) **Test assertions STRENGTHENED, not weakened.** `run_emits_full_actions_jsonl_stream` 10→12 (round-0 trio inserted at recs[1]=round_start(0)/recs[2]=round_end(0,0); main rounds shifted to recs[3+]; UNCHANGED `simulation_end total_actions==4`, agent names still asserted at shifted recs[4]/[5]). `run_empty_activation_round_logs_start_end_only` 4→6 (round-0 + round-1 both asserted). Parallel `run_parallel_routes_actions_to_platform_loggers` 10/8→12/10 with `round_end` `[0,2,2]`/`[0,1,1]` (round-0 prepended). Every assertion still pins the FULL stream length + each record's shape; the +2 records assert the new round-0 trio. No assertion DELETED to make tests pass — the new counts are the Python-faithful counts. All 7 producer_tests pass.

### Claim 3 — routing of round-0 CREATE_POST by platform is correct (PASS)
- Resolution `pool.agents.iter().find_map(|a| a.persona.social … (s.user_id as i64 == poster_id).then(|| loggers.get(s.platform).map(…)).flatten())` (L893-901): finds the pool agent with matching `user_id`, routes to ITS `social.platform` logger. A twitter agent's post → `twitter/actions.jsonl`, reddit → `reddit/actions.jsonl` (per-platform routing, identical mechanism to the cycle-A main-loop route at `sim/mod.rs:1045-1046`).
- **Skip is faithful.** An unresolvable `poster_agent_id` (no pool agent with that user_id, OR resolved platform has no logger) → `find_map` yields `None` → no record, no count. Python L1202-1203 `except Exception: pass` fires when `result.env.agent_graph.get_agent(agent_id)` raises (agent not in THAT platform's graph) — same observable: a post whose agent doesn't resolve in this platform is NOT logged. teri does not silently drop a post Python WOULD log: Python only logs after a successful `get_agent`, exactly mirroring teri's "only log if resolved."
- **New test meaningful.** `run_round0_initial_posts_route_by_platform`: 2 platform agents (10=twitter, 20=reddit) + ghost 99; `max_ticks==0` isolates round-0. Asserts twitter file = [sim_start, round_start(0), CREATE_POST(agent 10, "tw hello"), round_end(0,1), sim_end(total 1)] (len 5); reddit file = same shape for agent 20 (len 5); `all_ids==[10,20]` (ghost 99 nowhere, each post in its OWN file only); per-platform `total_actions==1` each. Assertions are correct and non-vacuous (the cross-file `all_ids` check refutes any misroute; the ghost-skip is directly proven).

### Claim 4 — `env.step` world-injection gap is a legitimate `[≠]` (PASS)
- Python L1206 `await result.env.step(initial_actions)` injects the posts into the OASIS world so later agents react. teri's `WorldState` has no OASIS post-graph/DB → this side-effect is inexpressible on the existing substrate. This is the SAME substrate gap already recorded as `[≠]U028-OASIS-INTERNALS` (no new tag; the cycle-A/B verdicts and S-877's row already carry it). Genuinely INEXPRESSIBLE, not a portable-feature skip.
- **Only the world-injection side-effect is dropped, NOT the actions.jsonl records** — which ARE fully emitted (round_start + per-post CREATE_POST + round_end, all routed/counted). The observable actions.jsonl output (the monitored artifact) is byte-faithful; only the unobservable internal OASIS-world mutation is gapped. Survives the `[≠]` bar (inexpressible substrate internal, distinct from the observable record stream which IS ported). Not a disguised feature-skip.

### Symbol status
- **S-938** (`run_twitter_simulation`) / **S-939** (`run_reddit_simulation`) STAY `- [ ]` — these are whole coroutines (IPC poll, DB-fetch/enrich, create_model, main loop, wait-for-commands) ported incrementally; cycle C covers ONLY their round-0 initial-events sub-block. No premature flip.
- **S-877** (`TwitterSimulationRunner.run`) STAYS `- [~]` — round-0 `initial_posts` (the c3b-ii-deferred item) is now ported & proven, but wait-for-commands/IPC/env.step/signal-shutdown remain UNPORTED. Note updated to mark round-0 done.
- **No symbol flips to `- [x]` this cycle.** Symbols verified: 0 flipped; round-0 sub-contract of S-877/S-938/S-939 PASS.

### Claim 5 — does cycle C complete U-028? (status recommendation)
**U-028's actions.jsonl PRODUCER contract is now complete** (round-0 + main-loop stream, single + parallel, all faithful): `simulation_start` → `round_start(0,0)` → round-0 routed CREATE_POSTs → `round_end(0,n)` → per-tick `round_start`/`log_action`/`round_end` → `simulation_end`, per-platform routed, both single and parallel, byte-faithful. The S-820 `/start` contract (cycle B) and producer half (cycles A/B/C) are done.

**But U-028 as a UNIT does NOT flip `- [x]` on S-877 yet.** S-877 (`TwitterSimulationRunner.run`) is the run-loop owner and still has UNPORTED, non-round-0 contract: wait-for-commands mode (IPC `poll_command`/`send_response`/interview handling), `env.step` real action execution, signal-based cooperative shutdown beyond the existing flag. Those are U-030 territory (the `ParallelIPCHandler` symbols S-913–S-924, the coroutines S-938/S-939, `main` S-940, signal handlers S-941 — all `- [ ]`). The `[≠]U028-OASIS-INTERNALS` (enrichment / world-injection) and `[≠]U028-RNG-SEQUENCE` survive the bar and are not blockers, but the IPC/wait-mode/env.step run-loop pieces are genuine unported behavior.

**Recommendation:** Commit cycle C (round-0 producer sub-contract PASS). Do NOT flip S-877 / U-028 unit to `- [x]`. What remains to close the U-028 unit: port the run-loop's wait-for-commands/IPC + env.step execution + signal shutdown (S-938/S-939 main-loop + S-913–S-924 IPC + S-941) — these are the U-030 verify/port-forward items. Round-0 is the last of S-877's c3b-deferred producer items; the residual is the orchestration/IPC layer (U-030), not the producer.

**Tests:** `cargo test -p teri --lib` → 1512 passed (expected). `round0` filter → 1 passed. `producer_tests` → 7 passed. No downgrade. Y-not-regressed (port-only cycle in `port/mirofish`, no merge in scope).

---

## CYCLE 55 (2026-06-20, 30th resume) — SCOPE + VERIFY-ONLY FLIPS (S-903, S-935)

**Scoping (rust-port-cartographer → findings/u030-orchestration-residual.md):** classified all 89
`[ ]`/`[~]` orchestration symbols across U-028/029/030 → 17 `[x]`-substrate-satisfied / ~50
`[≠]`-substrate-gap (NEW tags U028-LOGGING, U028-SUBPROCESS-IPC, U028-SUBPROCESS-RUNNER + existing
U028-OASIS-INTERNALS) / ~10 `[ ]`-genuinely-unported + S-934 dual-LLM (bucket-3 to-port). The ~10
genuinely-unported all collapse to ONE ROOT GATE: `run_sim_body` (simulation_runner.rs:1542) lacks
the post-sim IPC command-service loop (poll_commands/send_response exist but are never driven inside
the sim task) — also why the U-026 interview/env surface is still `[!] IPC-PRODUCER-PENDING`.

**VERIFY-ONLY FLIPS (rust-port-parity-verifier, 2/2 PASS):**
- **S-903** (`RedditSimulationRunner._get_active_agents_for_round`, run_reddit_simulation.py:469-521)
  → `[x]`. Byte-structurally identical to the already-`[x]` S-876 (same 9 time_config keys+defaults,
  same uniform×multiplier→int target_count, same peak/off/1.0 multiplier precedence, same
  active_hours+activity_level per-agent gating, same random.sample(min(target,len)) cap). Fully
  covered by the landed `TimeActivationPolicy` (src/sim/activation.rs). `[≠]U028-RNG-SEQUENCE`
  carries unchanged; NO new divergence.
- **S-935** (`get_active_agents_for_round` free fn, run_parallel_simulation.py:1040-1090) → `[x]`.
  Byte-identical body; SOLE material diff = `config` direct param vs `self.config`, observably
  equivalent (`from_config(&Value,…)` already consumes config as a plain arg). Same coverage + same
  `[≠]`.

Verifier adversarial checks (extra/different config key? different branch order/rounding/sampling
cap? per-agent field not modeled? free-fn observable change? platform-specific leak?) ALL came back
clean. No fresh port — pure mirrors of S-876. 9 `sim::activation` tests pass; 1512 lib unchanged
(verify-only, no code change). Y-not-regressed (develop=7c354a5).

**Units STAY `[ ]`** — the IPC command-service loop gate (CYCLE 56 keystone) is unported; no over-flip.

---

## CYCLE 56 (2026-06-20, 30th resume) — KEYSTONE: IPC command-service loop + native interview execution

**Ported** (simulation_runner.rs + simulation_ipc.rs): the post-simulation **wait-for-commands loop**
into `run_sim_body` (after `engine.run()`: poll `ipc_server.try_poll()` → dispatch
CloseEnv/Interview/BatchInterview; exit on close_env / shutdown flag / all-clients-dropped) + native
`execute_interview` / `execute_batch_interview` (resolve pool agent by `social.user_id` → `llm.complete
(build_interview_prompt(...))` → `{agent_id, response, timestamp}`). New `CommandPoll`/`try_poll`
(distinguish Empty vs Disconnected). `spawn_sim_task` threads the run's `shutdown` flag into the body.

**PARITY PASS (rust-port-parity-verifier, all 7 refutation targets confirmed faithful):**
1. `dispatch_command` ≡ `process_commands` (py:343-384); CloseEnv→success`{message:"环境即将关闭"}`+exit;
   the py:380 unknown-else is unreachable (3-variant CommandType + from_dict rejects unknown at deser).
2. `execute_interview` shape ≡ `_get_interview_result` `{agent_id,response,timestamp}` (py:303-308);
   OASIS `env.step(INTERVIEW)`+trace-DB read = `[≠]U028-OASIS-INTERNALS` (LLM output returned inline);
   unknown agent_id→error (≡ `get_agent` raising); arg defaults `agent_id`/`prompt` matched.
3. `execute_batch_interview`: per-item resolve, skip+warn unresolvable, empty→`"没有有效的Agent"`,
   `{interviews_count, results}` keyed by agent_id string. Defensible `[≠]` sub-divergence: per-agent
   `llm.complete` (no OASIS batched env.step) drops an LLM-failing agent — rides on OASIS-INTERNALS,
   `[!]`-LLM-gated; contractually-portable behavior faithful.
4. **NO-DOWNGRADE-OF-Y confirmed.** Lingering after `engine.run()` is FAITHFUL: wait_for_commands=True
   is the MiroFish default (`__init__` py:398) and the Flask launcher (app/services/simulation_runner.py
   :399-440) NEVER passes `--no-wait` → API runs ALWAYS linger (`poll() is None`→running). The 2 modified
   cleanup tests (`cleanup_all_preserves_finished_run_state`, `..._stops_running_but_skips_finished`) now
   send close_env to finish the run before asserting — a REQUIRED faithful-behavior update that PRESERVES
   the FAIL-2 invariant (finished run keeps COMPLETED+no shutdown error+completed_at; gate discriminates
   finished vs running). COMPLETED is NOT delayed by lingering (monitor marks it from actions.jsonl
   `simulation_end` via `subscribe_completion` fired when `engine.run` returned —
   `producer_run_reaches_completed_via_monitor` reaches COMPLETED while the task still lingers).
5. 50ms poll cadence vs Python 0.5s = non-contractual, strictly more responsive; `Disconnected` exit is a
   teri-substrate lifecycle nicety (Python relied on OS process kill), no observable divergence.
6. **RESOLVES `[!] IPC-PRODUCER-PENDING`** for S-829/830/831/832 (interview) + S-833/834 (env/close) —
   full route→client→mpsc→wait-loop→execute→oneshot-reply closed (proven by
   `wait_for_commands_services_interview_then_close`: live interview→COMPLETED, unknown→Failed,
   close_env→COMPLETED+loop exits+env not-alive).

**FLIPS:** S-865/866/868 (twitter) + S-892/893/895 (reddit mirrors) → `[x]`. +7 tests 1512→1519,
clippy `--all-targets`+`--all-features` clean, fmt clean, Y-not-regressed (develop=7c354a5).

**NO OVER-FLIP (verifier-enforced):** S-920/921/922/924 (PARALLEL `ParallelIPCHandler`) STAY `[ ]` —
GENUINELY UNPORTED with a RICHER contract (per-platform `platform` key, dual-platform integration shape
`{platforms:{twitter,reddit}}`, `success_count`, `没有可用的模拟环境` error, platform routing/split at
run_parallel_simulation.py:317-414) NOT covered by the single-env `dispatch_command`. S-877 stays `[~]`
(env.step world-injection `[≠]` + signal pieces remain). S-904/938/939 stay `[ ]`. S-934 dual-LLM `[ ]`.
**U-028/U-029/U-030 UNITS STAY `[ ]`** — each still carries OASIS env/SQLite/logging/signal-handler
contracts (+ U-030 parallel handler + DB-enrichment); this port closed the IPC-consumer gate, not the units.
