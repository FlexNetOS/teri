# Browser Mobile Inference Wire

## FACT

- `FlexNetOS/cellm` is a mobile-native LLM serving research project with paged KV cache, multi-session scheduling, WASM support, benchmark docs, and mobile bindings.
- `cryscan/web-rwkv` is a pure WebGPU inference engine with WASM support and browser demos.
- `web-rwkv` explicitly states that it does not provide an OpenAI API or APIs of any kind.

## INFERENCE

- `cellm` is the stronger fit for future Teri mobile/offline planning because it already frames serving, scheduling, cache, and platform constraints in first-party Rust terms.
- `web-rwkv` is best treated as a browser feasibility and backend-research wire, not as a drop-in Teri backend.

## POLICY

- No mobile or browser inference dependency is added in this issue.
- Browser/mobile references stay in `L1`/`L2` until a future issue proves a bounded experiment.
- Do not present browser inference as equivalent to Teri's current server/backend defaults.

## QUESTION

- Which Teri surface would consume a browser or mobile backend first: frontend interaction, report assist, or a separate offline agent path?
- What minimum benchmark envelope should a mobile/browser experiment satisfy before it is more than a research lane?
