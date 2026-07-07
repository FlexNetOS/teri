//! Regression tests for the Codex guidance surface.
//!
//! FlexNetOS owns lifecycle hook enforcement at the workspace root, so this repo must not
//! reactivate repo-local Codex lifecycle hooks. Teri still keeps the previous hook payload archived
//! as evidence, and these tests make sure the archive remains parseable and useful.

use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn archive_dir() -> PathBuf {
    repo_root().join(".codex/archive/lifecycle-hooks-20260703T024950Z")
}

fn read_repo(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).expect(path)
}

fn read_archive(path: &str) -> String {
    std::fs::read_to_string(archive_dir().join(path)).expect(path)
}

#[test]
fn repo_local_lifecycle_hooks_are_archived_not_active() {
    assert!(
        !repo_root().join(".codex/hooks.json").exists(),
        "repo-local Codex lifecycle hooks must stay inactive; root workspace hooks own enforcement"
    );
    assert!(
        !repo_root().join(".codex/hooks").exists(),
        "repo-local Codex hook scripts must stay inactive; archived evidence lives under .codex/archive"
    );
    assert!(
        archive_dir().join("hooks.zip").exists(),
        "archive must preserve the removed lifecycle hook payload"
    );

    let hooks: Value = serde_json::from_str(&read_archive("hooks.json.md"))
        .expect("archived hooks.json must parse as JSON");
    let top_level_keys: HashSet<&str> = hooks
        .as_object()
        .expect("archived hooks.json top-level object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        top_level_keys,
        HashSet::from(["hooks"]),
        "archived hooks.json should preserve the schema that used to be active"
    );

    let hook_table = hooks["hooks"].as_object().expect("hooks object");
    for event_name in ["SessionStart", "PreCompact"] {
        let entries = hook_table[event_name].as_array().expect("event hook array");
        let command = entries
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .find_map(|hook| hook["command"].as_str())
            .expect("event must declare a command hook");

        assert!(
            command.contains(".codex/hooks/teri-context-session-start.sh"),
            "{event_name} archive should preserve the teri truth hook command, got {command:?}"
        );
        assert!(
            command.starts_with("rtk bash -lc "),
            "{event_name} archive should preserve rtk bash routing, got {command:?}"
        );
        assert!(
            command.contains("rtk git rev-parse --show-toplevel"),
            "{event_name} archive should preserve rtk git repo resolution, got {command:?}"
        );
        assert!(
            !command.contains("/home/"),
            "archived hook command should remain repo-relative, got {command:?}"
        );
    }
}

#[test]
fn archived_runtime_files_route_commands_through_rtk() {
    let hooks_script = read_archive("hooks/teri-context-session-start.sh.md");
    assert!(hooks_script.contains("rtk git rev-parse --show-toplevel"));
    assert!(hooks_script.contains("rtk python3 - <<'PY'"));
    assert!(!hooks_script.contains("$(git rev-parse"));
    assert!(!hooks_script.contains("\npython3 -"));

    for path in [".codex/prompts/teri-docs-sync.md", ".codex/prompts/teri-simulation-truth.md"] {
        let prompt = read_repo(path);
        assert!(prompt.contains("Codex command rule: run repo commands through `rtk`"));
        assert!(!prompt.contains("\nrg -n "), "{path} should use rtk rg");
        assert!(!prompt.contains("`cargo "), "{path} should use rtk cargo");
        assert!(!prompt.contains(" plus `cargo "), "{path} should use rtk cargo");
    }
}

#[test]
fn archived_truth_hook_preserves_session_context_contract() {
    let hooks_script = read_archive("hooks/teri-context-session-start.sh.md");
    for phrase in [
        "Teri truth guardrails:",
        "cannot literally simulate or predict anything",
        "not a calibrated probability",
        "Source truth: teri run is wired to provider-selected run_pipeline",
        "serve/API builds a provider-polymorphic ApiState (SimulationRunner<ProviderAdapter>); run uses provider selection via build_provider_llm.",
        "No known stale teri-run/provider/test-count phrases found",
    ] {
        assert!(hooks_script.contains(phrase), "archived hook missing context phrase: {phrase}");
    }

    for expected in [
        "src/main.rs",
        "src/pipeline.rs",
        "src/api/mod.rs",
        "src/sim/mod.rs",
        "src/preflight.rs",
        "README.md",
        "RUNBOOK.md",
        "CLAUDE.md",
    ] {
        assert!(hooks_script.contains(expected), "archived hook missing watch path: {expected}");
    }
}

#[test]
fn archived_truth_hook_retains_stale_doc_detection_logic() {
    let hooks_script = read_archive("hooks/teri-context-session-start.sh.md");

    for phrase in [
        "Pipeline not yet implemented",
        "preflights, then bails",
        "Skeleton with real organs",
        "hardcoded OpenAI",
        "1629 tests",
        "Stale doc markers found",
        "contains stale phrase",
    ] {
        assert!(
            hooks_script.contains(phrase),
            "archived hook missing stale-doc detection phrase: {phrase}"
        );
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
