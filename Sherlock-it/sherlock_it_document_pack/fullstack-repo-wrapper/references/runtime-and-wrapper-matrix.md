# Runtime and Wrapper Matrix

Use this reference when deciding how to turn a repository into a wrapper application without overstating support.

## Repository Signal Checklist

Scan for these signals before choosing a shell.

| Signal | Examples |
| --- | --- |
| Rust workspace | `Cargo.toml`, workspace members, `crates/`, `src-tauri/` |
| Node.js frontend or tooling | `package.json`, `pnpm-lock.yaml`, `vite.config.*`, `next.config.*` |
| WASM or WASI target | crates or folders named `wasm`, `wasi`, `wasm32-unknown-unknown`, generated `.wasm` artifacts |
| Browser UI | `apps/web`, `frontend`, `site`, `ui`, `index.html`, `src/App.*` |
| Mobile indicators | `capacitor.config.*`, `android/`, `ios/`, Expo, React Native, Ionic |
| Existing packaging | Tauri, Electron, native installers, container bundles, release scripts |

## Default Wrapper Choices

| Verified repo state | Recommended wrapper | Notes |
| --- | --- | --- |
| Rust backend plus web frontend | **Tauri/Vite** | Strong fit for desktop shells with a web UI |
| Browser-first app with no proven native shell | **PWA shell** | Honest path for browser and mobile-browser delivery |
| Existing native desktop packaging already works | **Reuse native packaging** | Do not replace a healthy native path just to add a wrapper |
| WASM package loads in browser | **PWA shell or direct web bundle** | Keep the delivery surface close to the actual runtime |
| Mobile app already exists and builds | **Use repo-native mobile path** | Do not downgrade to a wrapper if native support is real |

## Truth Matrix Rules

Always classify each target with one of these labels.

| Label | Meaning |
| --- | --- |
| `native` | The repo already produces and validates a platform-native artifact |
| `wrapped` | The platform is delivered through a wrapper shell around a verified web or service surface |
| `browser-delivered` | The platform is reached through a browser or PWA, not a native shell |
| `unsupported` | No verified path exists |

Never rename `wrapped` or `browser-delivered` output as `native`.

## Tauri/Vite Wrapper Guidance

Choose the bundled Tauri template when all of the following are true:

1. A desktop surface is genuinely requested.
2. The repo has a verified UI or service endpoint that the shell can expose.
3. A Rust-based shell is compatible with the repo's delivery model.

Use the Tauri wrapper to:

- point to built web assets,
- point to a local runtime endpoint,
- present launch and health information,
- preserve a Rust plus Node.js toolchain story,
- package honest desktop deliverables.

Do **not** use it to disguise a backend that does not start.

## PWA Shell Guidance

Choose the bundled PWA shell when:

1. The browser surface already works or can be made to work truthfully.
2. Mobile compatibility is requested but native mobile support is unproven.
3. The strongest realistic outcome is browser or PWA delivery.

Use the PWA shell to:

- host an iframe or routed frontend surface,
- provide installability on supported devices,
- centralize environment variables and runtime checks,
- create a mobile-compatible wrapper without false native claims.

## WASM-Specific Notes

| Situation | Handling |
| --- | --- |
| Repo produces `.wasm` plus JS glue | Serve it through the web or wrapper frontend and verify load timing |
| Repo targets `wasm32-unknown-unknown` only | Treat browser delivery as primary unless another shell is proven |
| Repo mentions WASI | Validate the actual runtime separately before claiming support |
| Multiple WASM crates exist | Map each crate to its role before choosing what the wrapper exposes |

## Minimal Decision Procedure

1. Prove the repo can build.
2. Prove the repo can run or serve.
3. Prove the user-facing surface exists.
4. Choose the smallest honest wrapper.
5. Package only what is actually verified.

If any of the first three steps fail, stop calling the wrapper a release path and report the blocker clearly.
