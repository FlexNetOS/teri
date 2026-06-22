//! Regression tests for the repo-local Codex harness surface.
//!
//! These tests keep `.codex` from becoming a hand-checked config island. The harness is part of
//! the repository contract: it must emit valid hook JSON, flag stale simulation docs, and preserve
//! the bounded "simulate/predict anything" counterargument in prompts.

use serde_json::Value;
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_repo(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).expect(path)
}

fn host_rtk_available() -> bool {
    Command::new("rtk").arg("--version").output().is_ok()
}

fn rtk_path_env() -> Option<(tempfile::TempDir, OsString)> {
    if host_rtk_available() {
        return None;
    }

    let tmp = tempfile::tempdir().expect("rtk shim tempdir");
    let shim = tmp.path().join("rtk");
    std::fs::write(&shim, "#!/usr/bin/env bash\nexec \"$@\"\n").expect("write rtk shim");
    let mut perms = std::fs::metadata(&shim).expect("rtk shim metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&shim, perms).expect("chmod rtk shim");

    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![tmp.path().to_path_buf()];
    paths.extend(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).expect("join PATH with rtk shim");
    Some((tmp, path))
}

fn run_truth_hook(root: &Path) -> Value {
    let rtk_env = rtk_path_env();
    let mut command = Command::new("rtk");
    command.arg("bash").arg(".codex/hooks/teri-context-session-start.sh");
    if let Some((_tmp, path)) = &rtk_env {
        command.env("PATH", path);
    }

    let output = command.current_dir(root).output().expect("run teri truth hook");

    assert!(
        output.status.success(),
        "hook failed: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("hook stdout must be valid JSON")
}

#[test]
fn hooks_json_wires_truth_hook_for_session_and_compaction() {
    let hooks: Value = serde_json::from_str(&read_repo(".codex/hooks.json"))
        .expect(".codex/hooks.json must parse as JSON");

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
            "{event_name} should run the teri truth hook, got {command:?}"
        );
        assert!(
            command.starts_with("rtk bash -lc "),
            "{event_name} should route through rtk bash, got {command:?}"
        );
        assert!(
            command.contains("rtk git rev-parse --show-toplevel"),
            "{event_name} should resolve the repo through rtk git, got {command:?}"
        );
        assert!(
            !command.contains("/home/"),
            "hook command should be repo-relative, got {command:?}"
        );
    }
}

#[test]
fn codex_runtime_files_route_commands_through_rtk() {
    let hooks_script = read_repo(".codex/hooks/teri-context-session-start.sh");
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
fn truth_hook_emits_valid_session_context_for_current_repo() {
    let root = repo_root();
    let json = run_truth_hook(&root);
    let output = &json["hookSpecificOutput"];

    assert_eq!(output["hookEventName"], "SessionStart");

    let context = output["additionalContext"].as_str().expect("additional context");
    for phrase in [
        "Teri truth guardrails:",
        "cannot literally simulate or predict anything",
        "not a calibrated probability",
        "Source truth: teri run is wired",
        "serve/API still builds an OpenAiAdapter-backed ApiState",
        "No known stale teri-run/provider/test-count phrases found",
    ] {
        assert!(context.contains(phrase), "missing hook context phrase: {phrase}");
    }

    let watch_paths: HashSet<&str> = output["watchPaths"]
        .as_array()
        .expect("watchPaths array")
        .iter()
        .map(|v| v.as_str().expect("watch path string"))
        .collect();

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
        assert!(watch_paths.contains(expected), "missing watch path: {expected}");
    }
}

#[test]
fn truth_hook_flags_stale_docs_in_a_minimal_repo_fixture() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join(".codex/hooks")).unwrap();
    std::fs::create_dir_all(root.join("src/api")).unwrap();

    std::fs::copy(
        repo_root().join(".codex/hooks/teri-context-session-start.sh"),
        root.join(".codex/hooks/teri-context-session-start.sh"),
    )
    .unwrap();

    std::fs::write(root.join("README.md"), "Pipeline not yet implemented\n").unwrap();
    std::fs::write(root.join("RUNBOOK.md"), "preflights, then bails\n").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "1629 tests\n").unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() { build_provider_llm(); run_pipeline(); }\n",
    )
    .unwrap();
    std::fs::write(root.join("src/pipeline.rs"), "pub async fn run_pipeline() {}\n").unwrap();
    std::fs::write(
        root.join("src/api/mod.rs"),
        "pub(crate) fn build_llm() -> OpenAiAdapter {} struct OpenAiAdapter; type X = SimulationRunner<OpenAiAdapter>;\n",
    )
    .unwrap();

    let json = run_truth_hook(root);
    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("additional context");

    assert!(context.contains("Stale doc markers found"), "context was: {context}");
    assert!(context.contains("README.md: contains stale phrase"));
    assert!(context.contains("RUNBOOK.md: contains stale phrase"));
    assert!(context.contains("CLAUDE.md: contains stale phrase"));
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
