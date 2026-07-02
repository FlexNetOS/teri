# Adoption Gates

## Levels

```text
L0 indexed only
L1 documented crosswalk
L2 registry + validation
L3 optional offline adapter / fixture
L4 integration test behind explicit feature/env gate
L5 production default candidate
```

## Rules

- All new issue-86 wires start at `L0`, `L1`, or `L2` only.
- No source advances to `L4` or `L5` without a later issue/PR and explicit validation evidence.
- License facts must be recorded before any code adoption.
- Research-only wires stay bounded by explicit non-goals.

## Blocking conditions for advancement

- missing evidence paths
- missing or ambiguous license status
- no clear Teri target surface
- no local test or fixture seam for `L3+`
- any change that would weaken the backend honesty guard
