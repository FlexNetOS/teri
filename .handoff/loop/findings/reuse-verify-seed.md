# Reuse-Y Differential Verification — MiroFish→teri seed units (U-009, U-013)

Date: 2026-06-14
Verifier: rust-port-parity-verifier (reuse-Y mode, default-skeptical)
Method: differential — read SOURCE contract + DEST impl, compare shapes, run both sides where feasible.
DEST: `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (src/seed/mod.rs, GREEN baseline).
SOURCE: MiroFish `backend/app/utils/file_parser.py`, `backend/app/services/text_processor.py`.

Reuse is NEVER trusted: any divergence → RECLASSIFY reuse-Y → extend-Y / port-fresh with exact missing behavior.

---

## U-009 — file parsing (claim: reuse-Y via `SeedIngestor::from_file`)

### Contract table (per-symbol)

| Sym | MiroFish behavior (file:line) | teri covers? | Evidence (teri file:line) | Verdict |
|-----|-------------------------------|--------------|----------------------------|---------|
| S-061 `FileParser` | static dispatch class | YES (as `SeedIngestor`) | `src/seed/mod.rs:16-47` | covered |
| S-064 `extract_text` dispatch | suffix→pdf/md/txt; `ValueError` on unsupported; `FileNotFoundError` on missing | PARTIAL | dispatch `mod.rs:24-29`; missing-file → `Err` (`mod.rs:89-91`, test `mod.rs:436`) | covered w/ divergence (see Δ1, Δ2) |
| S-065 `_extract_from_pdf` PyMuPDF, **silently skips failed pages** | per-page extract, skip empty/failed | YES — `Err(_) => continue` per page | `mod.rs:115-123` (pdfium_render) | **covered** (page-error resilience present) |
| S-066 `_extract_from_md` (.md/.markdown) | markdown via text-fallback reader | **NO** — no `.md`/`.markdown` arm | dispatch has only txt/pdf/json; `.md` hits `_` fallback `mod.rs:28` | **GAP Δ1** |
| S-067 `_extract_from_txt` | txt via text-fallback reader | PARTIAL — reads txt but strict UTF-8 only | `read_plain_text` `mod.rs:88-96` | covered w/ Δ2 |
| S-060 `_read_text_with_fallback` | multi-level encoding: utf-8 → charset_normalizer → chardet → utf-8/replace | **NO** — `tokio::fs::read_to_string` = strict UTF-8, errors on non-UTF8 | `mod.rs:89` (txt), `mod.rs:131` (json) | **GAP Δ2** |
| S-062 `SUPPORTED_EXTENSIONS` {.pdf,.md,.markdown,.txt} | allow-set | **NO equivalent** — teri has no allow-set; unknown ext silently → plain text | `_ => read_plain_text` `mod.rs:28` | divergence Δ3 (behavioral: teri accepts ALL, never rejects) |
| S-063 `is_supported(filename)` | bool vs allow-set | **NO** — no such method | — | **GAP Δ4** |
| S-068 `extract_from_multiple(paths)` | concat N files with `=== 文档 i: name ===` headers; per-file try/except → keeps going on failure | **NO** — teri `from_file` is single-file; no multi-file concat | only `from_file` single path | **GAP Δ5** |
| S-069 `split_text_into_chunks` | (lives in file_parser.py but is U-013's chunking) | NO — see U-013 | — | distributed to U-013 |

### Divergences (differential evidence, both sides)

- **Δ1 — `.md`/`.markdown` not dispatched.** SOURCE: `extract_text` routes `.md`/`.markdown` to `_extract_from_md` (file_parser.py:103-104, 128). teri: dispatch arms are only `txt`/`pdf`/`json`; `.md` falls into `_ => read_plain_text` (mod.rs:28). *Observable difference:* a `.md` file is tagged `file_format=md` and read as plain text — functionally it reads the bytes (MiroFish's `_extract_from_md` is ALSO just the text reader), so **content parity holds for ASCII/UTF-8 .md**, BUT (a) it shares Δ2's encoding gap, and (b) `.markdown` files behave identically (fallback). Low-severity for content; the real teeth are Δ2.
- **Δ2 — encoding fallback absent (RUNNABLE divergence).** SOURCE `_read_text_with_fallback` (file_parser.py:11-58) decodes non-UTF-8 (Latin-1, GBK, …) by detection. teri `read_to_string` requires valid UTF-8. *Differential run:* `"Café résumé".encode('latin-1')` and `"中文测试".encode('gbk')` both FAIL `bytes.decode('utf-8')` → MiroFish falls back and **succeeds**; Rust `String::from_utf8([…,0xe9])` returns **Err** (probed: `/tmp/rust_enc_probe` → "from_utf8 ERR on latin-1 byte 0xe9"). So a non-UTF-8 `.txt`/`.md` that MiroFish ingests, teri **rejects with `TeriError::Seed`**. Real behavior loss.
- **Δ3 — no supported-extension gate.** SOURCE rejects unknown formats (`ValueError`, file_parser.py:98-99). teri accepts ANY extension as plain text (`_` arm, mod.rs:28; test `test_unknown_file_format_defaults_to_plain_text` mod.rs:398 asserts this is intentional). Behavioral divergence — teri is permissive by design, MiroFish strict. Note as intentional-divergence candidate, but it pairs with Δ4.
- **Δ4 — `is_supported()` has no teri equivalent.** SOURCE classmethod (file_parser.py:67). No teri symbol exposes "is this extension supported". GAP.
- **Δ5 — multi-file concat absent.** SOURCE `extract_from_multiple` (file_parser.py:138) concatenates N docs with index/filename headers and per-file failure-tolerance. teri offers only single-file `from_file`. GAP — this is the batch ingestion contract.

### U-009 VERDICT: **extend-Y**
teri's `from_file` genuinely covers PDF (with page-skip resilience ✓), txt, json — the per-file happy path is real reuse. But it is **NOT full parity**: it is missing encoding-fallback (Δ2, runnable loss), `.md`/`.markdown` explicit handling (Δ1), the supported-ext gate + `is_supported` (Δ3/Δ4), and multi-file concat (Δ5). Reclassify **reuse-Y → extend-Y**.

Symbols flipping: PDF-page-resilience portion of **S-065 → `- [x]`** (genuinely verified equivalent). All others (S-060, S-063, S-066, S-068) stay `- [~]` (unproven / gap). S-061/S-062/S-064/S-067 are `- [≠]`-or-`- [~]` partial — leave `- [~]` pending the extend.

---

## U-013 — chunk / preprocess (claim: reuse-Y "seed chunk/preprocess") — resolves GAP-U015-1

### Critical question: does teri have text chunking? **NO.**

Searched all of `src/**.rs` for `split_text | chunk_size | chunk_overlap | preprocess | get_text_stats | overlap`. The only `chunk` hits are **streaming-byte chunks** (HTTP/report streaming), NOT text chunking:
- `src/report/mod.rs:137-151,401-483` — streaming report chunks (`stream.next()`, byte buffer).
- `src/llm.rs:187-189,379-381,565-567,673` — `byte_stream.next()` LLM response chunks.
- `src/agent/mod.rs`, `src/memory/mod.rs` — incidental `chunk` word, no chunking contract.

No `split_text`-equivalent, no char-count windowing, no overlap logic anywhere. Confirmed by symbol-map: S-167–S-171 (U-013) all `- [ ]`; S-190 (U-015) explicitly states "split_text/chunking distributed to U-013 (`- [!]` GAP-U015-1)".

### Contract table

| Sym | MiroFish behavior (file:line) | teri covers? | Verdict |
|-----|-------------------------------|--------------|---------|
| S-167 `TextProcessor` | static text-processing class | **NO** | port-fresh |
| S-169 `split_text(text,chunk_size=500,overlap=50)` | char-count split + overlap; sentence-boundary backtrack (。！？.\n etc.); `[]` for empty/whitespace; `[text]` if `len<=chunk_size` | **NO** (`split_text_into_chunks` file_parser.py:161-202) | **port-fresh** |
| S-170 `preprocess_text(text)` | CRLF→LF, collapse `\n{3,}`→`\n\n`, strip each line, strip ends | **NO** (text_processor.py:37-61) | port-fresh |
| S-171 `get_text_stats(text)` | dict {total_chars=len, total_lines=count('\n')+1, total_words=len(split())} | **NO** (text_processor.py:64-70) | port-fresh |
| S-168 `extract_from_files(paths)` | delegates to `FileParser.extract_from_multiple` | **NO** (= U-009 Δ5) | port-fresh (pairs with Δ5) |

### U-013 VERDICT: **extend-Y / port-fresh**
teri has **NO chunking, NO preprocess, NO text-stats capability**. This is a from-scratch port, not reuse.

**Target (where it should live):** new module `src/seed/text_processor.rs` (or `src/services/text_processor.rs`) re-exported in `lib.rs`. Recommend `src/seed/` (cohesive with `SeedIngestor`; the chunker feeds seed ingestion before graph build).

**Exact contracts to add (idiomatic Rust):**
1. `pub fn split_text(text: &str, chunk_size: usize /*=500*/, overlap: usize /*=50*/) -> Vec<String>` — char-count windows (use `char_indices`, NOT byte slicing — UTF-8 safety, unlike Python's char semantics which Rust must replicate via chars); sentence-boundary backtrack over `['。','！','？',".\n","!\n","?\n","\n\n",". ","! ","? "]` only when the separator is past `chunk_size*0.3`; trim each chunk; skip empty; `len<=chunk_size` → `vec![text]` (or `[]` if whitespace-only). Mirror file_parser.py:161-202 exactly.
2. `pub fn preprocess_text(text: &str) -> String` — `\r\n`/`\r`→`\n`; regex collapse `\n{3,}`→`\n\n`; trim each line; trim ends. Mirror text_processor.py:37-61.
3. `pub fn get_text_stats(text: &str) -> TextStats { total_chars, total_lines, total_words }` — chars = `text.chars().count()`, lines = `matches('\n').count()+1`, words = `split_whitespace().count()`. Mirror text_processor.py:64-70.

**This unblocks GAP-U015-1:** once `split_text` lands, extend `KnowledgeGraph::build()` (src/graph/mod.rs:237) to split→extract-per-chunk→merge(dedup) for large docs. Confirmed in parity.md:50.

Symbols S-167–S-171 stay `- [~]`/`- [ ]` (port-fresh, not yet written).

---

## Summary

| Unit | Claim | VERDICT | Why |
|------|-------|---------|-----|
| U-009 | reuse-Y | **extend-Y** | PDF/txt/json + page-resilience reuse-confirmed; missing encoding-fallback (Δ2 runnable), .md dispatch (Δ1), ext-gate/is_supported (Δ3/Δ4), multi-file concat (Δ5) |
| U-013 | reuse-Y | **port-fresh** | teri has NO text chunking/preprocess/stats at all; `chunk` hits are byte-streaming only |

teri has **NO text chunking** → GAP-U015-1 requires a port-fresh `split_text` (decided).
