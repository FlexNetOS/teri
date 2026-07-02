# Fish Lineage Wire

## FACT

- Teri's current README and RUNBOOK describe Teri as a Rust-native rewrite and upgrade of MiroFish, with parity tracked by spec and evidence rather than code copy.
- `MOZARTINOS/mirofish-guide` is an operator playbook, not the engine itself. Its inspected references cover workflow, debugging, report audit, evaluation, and model/proxy guidance.
- `666ghj/MiroFish#325` is an open upstream delta. Its inspected PR metadata claims Vertex AI integration, CSV/XLSX ingestion, tabular-to-narrative conversion, English localization, and two bug fixes. It was not merged truth when inspected.
- `666ghj/BettaFish` predates MiroFish and exposes a broader multi-agent analysis stack with Query, Media, Insight, Report, and Forum engines.

## INFERENCE

- `mirofish-guide` is the strongest crosswalk source for Teri's five-stage pipeline because it explains the operator practices around graph quality, runtime forensics, and report audit without requiring code adoption.
- `MiroFish#325` is most useful as an adoption-question source, not as implementation truth.
- `BettaFish` matters more for lineage and coordination patterns than for direct parity.

## POLICY

- Treat `MiroFish#325` as `UPSTREAM_DELTA`, never as merged truth.
- Treat `mirofish-guide` as operator guidance, not as proof of current Teri behavior.
- Treat `BettaFish` as a lineage and question source only unless a later Teri change proves a directly matching seam.

## Stage Crosswalk

| Teri stage | Primary source | Why it matters |
| --- | --- | --- |
| `seed` | `mirofish-guide/references/workflow.md` | source-material quality and focused scenario guidance |
| `graph` | `mirofish-guide/references/debugging.md`, `MiroFish#325` | sparse ontology troubleshooting and tabular-ingestion questions |
| `agent` | `mirofish-guide/references/workflow.md`, `BettaFish` | persona relevance and richer analysis context |
| `sim` | runtime-forensics guidance and BettaFish lineage | runtime health and action diversity heuristics |
| `report` | `mirofish-guide/references/report-audit.md`, `BettaFish` | audit expectations and summary-vs-evidence discipline |

## QUESTION

- Should Teri seed intake grow CSV/XLSX support directly, or begin with an explicit preprocessing seam?
- Does Teri need Vertex-style provider auth, or only a cleaner provider-routing abstraction?
- Which bug-class parity checks from PR #325 deserve targeted Teri tests later?
