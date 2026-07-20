//! Regression tests for the Codex guidance surface.
//!
//! FlexNetOS owns lifecycle hook enforcement at the workspace root, so this repo must not
//! reactivate repo-local Codex lifecycle hooks or retain a legacy hook payload.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_repo(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).expect(path)
}

#[test]
fn retired_lifecycle_hook_payloads_remain_absent() {
    for path in [
        ".codex/hooks.json",
        ".codex/hooks",
        ".codex/archive/lifecycle-hooks-20260703T024950Z",
        ".codex/archive/hooks-lifecycle.zip",
    ] {
        assert!(
            !repo_root().join(path).exists(),
            "retired repo-local lifecycle-hook payload must remain absent: {path}"
        );
    }
}

#[test]
fn active_prompts_route_commands_through_rtk() {
    for path in [".codex/prompts/teri-docs-sync.md", ".codex/prompts/teri-simulation-truth.md"] {
        let prompt = read_repo(path);
        assert!(prompt.contains("Codex command rule: run repo commands through `rtk`"));
        assert!(!prompt.contains("\nrg -n "), "{path} should use rtk rg");
        assert!(!prompt.contains("`cargo "), "{path} should use rtk cargo");
        assert!(!prompt.contains(" plus `cargo "), "{path} should use rtk cargo");
    }
}

#[test]
fn docs_do_not_reintroduce_known_stale_harness_claims() {
    let stale_patterns = [
        "Pipeline not yet implemented",
        "preflights, then bails",
        "Skeleton with real organs",
        "hardcoded OpenAI",
        "1629 tests",
        "preflight-only",
        "wiring pending",
        "implementation pending",
        "server wiring = serve phase",
    ];

    for path in ["README.md", "RUNBOOK.md", "CLAUDE.md"] {
        let text = read_repo(path);
        for pattern in stale_patterns {
            assert!(
                !text.contains(pattern),
                "{path} reintroduced stale harness/doc phrase {pattern:?}"
            );
        }
    }
}

#[test]
fn prompts_preserve_simulation_truth_counterargument() {
    let simulation_truth = read_repo(".codex/prompts/teri-simulation-truth.md");
    let docs_sync = read_repo(".codex/prompts/teri-docs-sync.md");
    let combined = format!("{simulation_truth}\n{docs_sync}");

    for phrase in [
        "Do not call Teri an oracle",
        "no causal proof",
        "missing calibrated probabilities",
        "finite action",
        "specialized solvers",
        "physics",
        "epidemiology",
        "markets",
        "weather",
        "supply chains",
        "adversarial security",
        "build_provider_llm",
        "OpenAiAdapter",
    ] {
        assert!(combined.contains(phrase), "prompt missing phrase: {phrase}");
    }
}
