#!/usr/bin/env python3
"""
Cross-platform pre-commit helper scripts for Pebesen
"""

import os
import sys
import subprocess
import pathlib

def run_command(cmd, cwd=None):
    """Run a command and return the result"""
    try:
        result = subprocess.run(
            cmd,
            shell=True,
            cwd=cwd,
            capture_output=True,
            text=True
        )
        return result.returncode == 0, result.stdout, result.stderr
    except Exception as e:
        return False, "", str(e)

def eslint_frontend():
    """Run ESLint on frontend code"""
    frontend_dir = pathlib.Path(__file__).parent.parent / "frontend"
    if not frontend_dir.exists():
        print("Frontend directory not found")
        return False

    success, stdout, stderr = run_command("pnpm lint", cwd=str(frontend_dir))
    if not success:
        print(f"ESLint failed: {stderr}")
        return False
    return True

def svelte_check():
    """Run svelte-check on frontend code"""
    frontend_dir = pathlib.Path(__file__).parent.parent / "frontend"
    if not frontend_dir.exists():
        print("Frontend directory not found")
        return False

    success, stdout, stderr = run_command("pnpm check", cwd=str(frontend_dir))
    if not success:
        print(f"svelte-check failed: {stderr}")
        return False
    return True

def prettier_frontend():
    """Run prettier on frontend code"""
    frontend_dir = pathlib.Path(__file__).parent.parent / "frontend"
    if not frontend_dir.exists():
        print("Frontend directory not found")
        return False

    success, stdout, stderr = run_command("pnpm prettier --write .", cwd=str(frontend_dir))
    if not success:
        print(f"Prettier failed: {stderr}")
        return False
    return True

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: pre-commit-hooks.py <command>")
        print("Commands: eslint, svelte-check, prettier")
        sys.exit(1)

    command = sys.argv[1]

    if command == "eslint":
        success = eslint_frontend()
    elif command == "svelte-check":
        success = svelte_check()
    elif command == "prettier":
        success = prettier_frontend()
    else:
        print(f"Unknown command: {command}")
        sys.exit(1)

    sys.exit(0 if success else 1)
