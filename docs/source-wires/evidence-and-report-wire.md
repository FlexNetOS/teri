# Evidence And Report Wire

## FACT

- `brain-in-the-fish` states "Score any document. Prove every claim." and explicitly verifies source quotes against the original document.
- Its README describes score, ontology, and verdict layers (`CONFIRMED`, `FLAGGED`, `REJECTED`).
- `mirofish-guide/references/report-audit.md` treats the report as a summary layer whose important claims must be traced back to artifacts and tool logs.

## INFERENCE

- Teri's report layer is the natural future home for BITF-style quote verification and claim/evidence vocabulary.
- A lighter-weight seed-quality gate may also be useful, but issue 86 does not prove a concrete runtime seam for it yet.

## POLICY

- No BITF code import or runtime dependency is introduced in this issue.
- Future evidence scoring must remain fail-closed: unsupported quotes should lower confidence or block upgrade claims, not silently pass.
- Report claims should be downgraded when evidence is missing, even if the prose is polished.

## Future acceptance gate

| Level | Gate |
| --- | --- |
| `L2` | registry entry, docs crosswalk, validation, and explicit evidence paths |
| `L3` | optional offline fixture proving quote-verification logic against local sample inputs |
| `L4` | integration test behind an explicit feature/env gate, with no default network dependency |
| `L5` | only after repeated Teri-side validation proves it strengthens reports without regressions |

## QUESTION

- Which Teri artifact should carry verified source-quote spans if BITF-style checking is added later?
- Should report confidence stay synthesized until claim verification exists, or should later work split `confidence` from `evidence score`?
