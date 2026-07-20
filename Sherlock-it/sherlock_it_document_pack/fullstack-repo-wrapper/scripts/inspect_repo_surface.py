#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


def run_git(repo: Path, args: list[str]) -> str | None:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=repo,
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip()
    except Exception:
        return None


def find_files(repo: Path, names: set[str], limit: int = 200) -> list[str]:
    matches: list[str] = []
    for root, dirs, files in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in {".git", "node_modules", "target", ".next", "dist", "build"}]
        for file_name in files:
            if file_name in names:
                rel = str(Path(root, file_name).relative_to(repo))
                matches.append(rel)
                if len(matches) >= limit:
                    return sorted(matches)
    return sorted(matches)


def find_dirs(repo: Path, names: set[str], limit: int = 200) -> list[str]:
    matches: list[str] = []
    for root, dirs, _files in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in {".git", "node_modules", "target", ".next", "dist", "build"}]
        for dir_name in dirs:
            if dir_name in names:
                rel = str(Path(root, dir_name).relative_to(repo))
                matches.append(rel)
                if len(matches) >= limit:
                    return sorted(matches)
    return sorted(matches)


def keyword_paths(repo: Path, keywords: tuple[str, ...], limit: int = 200) -> list[str]:
    matches: list[str] = []
    for root, dirs, files in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in {".git", "node_modules", "target", ".next", "dist", "build"}]
        for name in [*dirs, *files]:
            lower = name.lower()
            if any(keyword in lower for keyword in keywords):
                rel = str(Path(root, name).relative_to(repo))
                matches.append(rel)
                if len(matches) >= limit:
                    return sorted(set(matches))
    return sorted(set(matches))


def read_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as fh:
            data = tomllib.load(fh)
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def cargo_workspace_members(repo: Path) -> list[str]:
    cargo_toml = repo / "Cargo.toml"
    if not cargo_toml.exists():
        return []
    data = read_toml(cargo_toml)
    workspace = data.get("workspace", {})
    members = workspace.get("members", []) if isinstance(workspace, dict) else []
    return members if isinstance(members, list) else []


def root_package_name(repo: Path) -> str | None:
    cargo_toml = repo / "Cargo.toml"
    if cargo_toml.exists():
        data = read_toml(cargo_toml)
        package = data.get("package", {})
        if isinstance(package, dict):
            name = package.get("name")
            if isinstance(name, str) and name.strip():
                return name.strip()
    package_json = repo / "package.json"
    if package_json.exists():
        try:
            payload = json.loads(package_json.read_text())
            name = payload.get("name")
            if isinstance(name, str) and name.strip():
                return name.strip()
        except Exception:
            pass
    return None


def recommend_wrapper(data: dict[str, Any]) -> str:
    has_desktop = bool(data["desktop_indicators"])
    has_web = bool(data["web_indicators"])
    has_mobile = bool(data["mobile_indicators"])
    has_wasm = bool(data["wasm_indicators"])
    has_rust = bool(data["cargo_toml_files"])
    has_node = bool(data["package_json_files"])

    if has_desktop:
        return "reuse-native-desktop"
    if has_web and (has_rust or has_node):
        return "tauri"
    if has_web or has_wasm:
        return "pwa"
    if has_mobile:
        return "reuse-native-mobile"
    return "analysis-required"


def main() -> int:
    parser = argparse.ArgumentParser(description="Inspect a repository and recommend a wrapper strategy.")
    parser.add_argument("repo_path", help="Path to the local repository clone")
    parser.add_argument("--json-out", help="Optional path to save the JSON result")
    args = parser.parse_args()

    repo = Path(args.repo_path).resolve()
    if not repo.exists() or not repo.is_dir():
        print(json.dumps({"error": f"Repository path not found: {repo}"}, indent=2))
        return 1

    data: dict[str, Any] = {
        "repo_path": str(repo),
        "resolved_remote_url": run_git(repo, ["remote", "get-url", "origin"]),
        "default_branch_guess": run_git(repo, ["rev-parse", "--abbrev-ref", "HEAD"]),
        "head_commit": run_git(repo, ["rev-parse", "HEAD"]),
        "root_package_name": root_package_name(repo),
        "cargo_workspace_members": cargo_workspace_members(repo),
        "cargo_toml_files": find_files(repo, {"Cargo.toml"}),
        "package_json_files": find_files(repo, {"package.json"}),
        "lockfiles": find_files(repo, {"pnpm-lock.yaml", "package-lock.json", "yarn.lock", "bun.lockb", "Cargo.lock"}),
        "script_directories": find_dirs(repo, {"scripts"}),
        "web_indicators": find_dirs(repo, {"web", "www", "frontend", "ui", "site", "app", "apps"}) + find_files(repo, {"vite.config.ts", "vite.config.js", "next.config.js", "next.config.mjs", "index.html"}),
        "desktop_indicators": find_dirs(repo, {"src-tauri", "electron", "desktop"}) + find_files(repo, {"tauri.conf.json", "tauri.conf.toml", "electron-builder.yml"}),
        "mobile_indicators": find_dirs(repo, {"android", "ios"}) + find_files(repo, {"capacitor.config.ts", "capacitor.config.json", "app.json"}),
        "wasm_indicators": keyword_paths(repo, ("wasm", "wasi")),
        "container_indicators": find_files(repo, {"Dockerfile", "docker-compose.yml", "docker-compose.yaml", "Containerfile"}),
        "ci_indicators": find_dirs(repo, {".github"}) + find_files(repo, {"Makefile", "justfile"}),
    }
    data["recommended_wrapper"] = recommend_wrapper(data)

    output = json.dumps(data, indent=2)
    print(output)

    if args.json_out:
        json_path = Path(args.json_out).resolve()
        json_path.parent.mkdir(parents=True, exist_ok=True)
        json_path.write_text(output + "\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
