# MiroFish → teri — full-feature port parity PROOF (2026-06-21)

Card: KBTASK-MIROFISH-TERI-PORT-PARITY-PROOF. Method: 4 parallel adversarial
`rust-port-parity-verifier` agents over the `MIROFISH-PORT-PLAN.md` matrix vs CURRENT
teri source + tests (code-truth, differential, file:line evidence). Baseline:
`cargo test --lib` = **1,614 passed / 0 failed** (vs the plan's 140 — 11.5× expansion).

## Headline verdict

The port is **module-complete and heavily tested**, BUT the **full-feature port is NOT
end-to-end runnable today.** All five MiroFish stages exist as real, tested Rust modules —
most of the plan's W4 "missing/placeholder/stub" flags are now STALE — yet the pipeline is
**not composed into a runnable `teri run`**, and the LLM layer carries three real downgrades.

## CONFIRMED ported (real logic + tests) — plan flags now STALE

| Stage | Capability | Evidence |
|---|---|---|
| 1 Graph | ingestion (pdf/md/txt/**json/url** — exceeds upstream), 500/50 chunk, LLM ontology, **entity/relation extraction keystone** (`graph/mod.rs:617 extract_and_merge_into` — was the placeholder), petgraph store | 232 tests |
| 2/3 Sim | persona OASIS attrs, sim-config (bias/reaction/influence), two-phase sim loop + God-events, **14-action OASIS set incl DO_NOTHING**, **temporal memory write-back WIRED** (sim→`actions.jsonl`→monitor→`GraphMemoryUpdater`, e2e test `graph_fire_enabled_forwards_actions_to_updater`) | sim 118 / agent 113 / memory 100 / runner 106 |
| 4/5 Report | **ReACT loop** (outline→per-section) over **all 4 graph tools** (insight_forge/panorama/quick/interview), report `/chat`, **axum server REAL** (`serve` wired), interview endpoints, SSE/streaming | report 221 / server 17 / api 142+ |
| Infra | embeddings + **real cosine hybrid over redb** (plan "stubbed" = STALE), OpenAI adapter, **backend honesty guard** (fail-closed) | memory 100 / embedding 7 / preflight 17 |

## GAPS — the proof the port is NOT complete

| # | Gap | Evidence | Severity |
|---|---|---|---|
| G1 | **CLI spine unwired** — `teri run` bails; pipeline never composed. Capabilities reachable only via a 3–4 POST dance against `teri serve`. P1 "wire the spine" milestone UNMET. | `main.rs:83 Err("Pipeline not yet implemented")` | **P1 keystone** |
| G2 | **`verdict.json` absent** — no `teri run --out`; CLI-fork parity not started. | no `--out` flag / no writer anywhere | P2 |
| G3 | **Provider selection fake** — always `OpenAiAdapter`; Anthropic/Gemini dead on every run path; Anthropic base_url configurable only `#[cfg(test)]`. | `api/mod.rs:248 build_llm`, `llm.rs:592,601` | P1 |
| G4 | **`max_tokens` not sent** on `complete()`/`complete_json()` (persona + extraction hot paths) → 256-token truncation vs shimmy default. | `llm.rs:374–430`, `agent/mod.rs:1438`, `graph_builder.rs:547` | P1 |
| G5 | **Anthropic/Gemini `stream()` latent bug** — parse OpenAI SSE framing; broken vs real endpoints (green only because tests mock OpenAI-shaped SSE). | `llm.rs:743,1007` | P2 |
| G6 | (divergent, acceptable) per-tick graph write-back is async/batched not in-loop; report reads graph not sim timeline; live interview success path runtime-pending on unported OASIS producer. | `sim/mod.rs:862`, `graph_memory.rs:1246` | note |

## Fix plan ("teri must be fixed") — see follow-up cards
- **FIX-1 (keystone):** compose `run_cmd` = seed → graph build (real LLM extraction) → persona →
  sim (write-back) → report, reusing the service layer the HTTP handlers already call
  (`graph_builder`, `simulation_runner`, `report::manager`); add `--out verdict.json`. (G1,G2)
- **FIX-2:** `build_llm` selects adapter by `config.llm.provider`. (G3)
- **FIX-3:** send `max_tokens` on `complete()`/`complete_json()`. (G4)
- **FIX-4:** fix or explicitly gate Anthropic/Gemini `stream()` SSE framing. (G5)
- Verification: composition unit/integration tests with an injected mock LLM (no live GGUF);
  full live e2e stays gated on a running shimmy+GGUF backend (honesty guard enforces it).
