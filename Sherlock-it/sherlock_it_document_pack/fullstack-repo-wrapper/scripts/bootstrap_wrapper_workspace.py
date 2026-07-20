#!/usr/bin/env python3
import argparse
import json
import shutil
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_ROOT = SCRIPT_DIR.parent
TEMPLATES_DIR = SKILL_ROOT / "templates"


def replace_tokens(text: str, variables: dict[str, str]) -> str:
    for key, value in variables.items():
        text = text.replace(f"__{key}__", value)
    return text


def copy_template_tree(src: Path, dst: Path, variables: dict[str, str]) -> None:
    for item in src.rglob("*"):
        relative = item.relative_to(src)
        target = dst / relative
        if item.is_dir():
            target.mkdir(parents=True, exist_ok=True)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        content = item.read_text()
        target.write_text(replace_tokens(content, variables))


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text.rstrip() + "\n")


def operator_brief_text(custom_prompt_file: Path | None) -> str:
    if custom_prompt_file and custom_prompt_file.exists():
        return custom_prompt_file.read_text().rstrip() + "\n"
    return (
        "# Operator Brief\n\n"
        "Replace this file with the user-provided mission prompt. Preserve repo-specific build commands, phase gates, and packaging truth constraints.\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Create a reusable workspace for a repository wrapper mission.")
    parser.add_argument("--repo-url", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--product-name", required=True)
    parser.add_argument("--app-id", required=True, help="Reverse-DNS style id, for example com.example.product")
    parser.add_argument("--mode", choices=["tauri", "pwa", "hybrid"], default="hybrid")
    parser.add_argument("--repo-path", help="Optional local clone path")
    parser.add_argument("--custom-prompt-file", help="Optional path to an operator brief markdown file")
    parser.add_argument("--web-target-url", default="http://localhost:3000")
    args = parser.parse_args()

    output_dir = Path(args.output_dir).resolve()
    if output_dir.exists() and any(output_dir.iterdir()):
        raise SystemExit(f"Output directory is not empty: {output_dir}")

    output_dir.mkdir(parents=True, exist_ok=True)
    variables = {
        "PRODUCT_NAME": args.product_name,
        "APP_ID": args.app_id,
        "REPO_URL": args.repo_url,
        "WEB_TARGET_URL": args.web_target_url,
    }

    for relative in [
        "artifacts",
        "coordination",
        "evidence/logs",
        "evidence/screens",
        "reports",
        "wrapper",
    ]:
        (output_dir / relative).mkdir(parents=True, exist_ok=True)

    repo_path = str(Path(args.repo_path).resolve()) if args.repo_path else ""
    write_json(output_dir / "coordination" / "mission.json", {
        "repo_url": args.repo_url,
        "repo_path": repo_path,
        "product_name": args.product_name,
        "app_id": args.app_id,
        "mode": args.mode,
        "web_target_url": args.web_target_url,
    })

    write_text(output_dir / "coordination" / "teams.yaml", f"""teams:
  - name: orchestrator
    responsibility: sequencing, gates, final recommendation
  - name: repo-analysis
    responsibility: identity, inventory, topology
  - name: build
    responsibility: toolchains and compilation
  - name: integration
    responsibility: runtime wiring and wrapper bridge
  - name: qa-e2e
    responsibility: integrated behavior validation
  - name: packaging-release
    responsibility: packageable outputs and instructions
  - name: audit
    responsibility: evidence, manifest, checksums
""")

    write_text(output_dir / "coordination" / "subagents.yaml", f"""subagents:
  - name: orchestrator-agent
    team: orchestrator
  - name: repo-analysis-agent
    team: repo-analysis
  - name: build-agent
    team: build
  - name: integration-agent
    team: integration
  - name: qa-e2e-agent
    team: qa-e2e
  - name: packaging-release-agent
    team: packaging-release
  - name: audit-agent
    team: audit
""")

    write_text(output_dir / "coordination" / "hooks.yaml", f"""hooks:
  - name: preflight_start
    when: before clone or mutation
  - name: post_clone_inventory
    when: after clone and surface inspection
  - name: pre_build_gate
    when: before required builds
  - name: post_build_gate
    when: after required builds
  - name: pre_integration_gate
    when: before wrapper wiring
  - name: post_e2e_gate
    when: after end-to-end validation
  - name: pre_package_gate
    when: before packaging
  - name: release_candidate_gate
    when: before final recommendation
  - name: failure_triage
    when: after any failed gate
""")

    write_text(output_dir / "reports" / "checkpoints.md", """# Checkpoints

| Phase | Status | Owner | Evidence | Notes |
| --- | --- | --- | --- | --- |
| 0 | TODO | Orchestrator | | |
| 1 | TODO | Build | | |
| 2 | TODO | Repo Analysis | | |
| 3 | TODO | Build | | |
| 4 | TODO | Orchestrator | | |
| 5 | TODO | Integration | | |
| 6 | TODO | QA/E2E | | |
| 7 | TODO | Packaging/Release | | |
| 8 | TODO | Audit | | |
| 9 | TODO | Orchestrator | | |
""")

    write_text(output_dir / "reports" / "fix-ledger.md", """# Fix Ledger

| File | Reason | Diff summary | Risk | Evidence |
| --- | --- | --- | --- | --- |
""")

    write_text(output_dir / "reports" / "artifact-manifest.md", """# Artifact Manifest

| Path | Type | Size | Checksum | Notes |
| --- | --- | --- | --- | --- |
""")

    custom_prompt = Path(args.custom_prompt_file).resolve() if args.custom_prompt_file else None
    write_text(output_dir / "operator-brief.md", operator_brief_text(custom_prompt))

    if args.mode in {"tauri", "hybrid"}:
        copy_template_tree(TEMPLATES_DIR / "tauri-vite-wrapper", output_dir / "wrapper" / "desktop", variables)
    if args.mode in {"pwa", "hybrid"}:
        copy_template_tree(TEMPLATES_DIR / "pwa-shell", output_dir / "wrapper" / "web", variables)

    print(json.dumps({
        "workspace": str(output_dir),
        "mode": args.mode,
        "repo_url": args.repo_url,
        "repo_path": repo_path,
        "product_name": args.product_name,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
