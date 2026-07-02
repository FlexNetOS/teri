#!/usr/bin/env bash
# Strict no-downgrade proof for Teri's inferrs backend upgrade.
# Checks current source defaults, Rust/CUDA/cuda-oxide/inferrs truth sources, and focused tests.
set -euo pipefail

TERI_ROOT=${TERI_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
if [[ -z "${META_ROOT:-}" ]]; then
  probe="$TERI_ROOT"
  META_ROOT=""
  while [[ "$probe" != "/" ]]; do
    if [[ -d "$probe/inferrs" && -d "$probe/cuda-oxide" ]]; then
      META_ROOT="$probe"
      break
    fi
    probe=$(dirname "$probe")
  done
  if [[ -z "$META_ROOT" ]]; then
    echo "could not infer META_ROOT; set META_ROOT=/path/to/meta" >&2
    exit 1
  fi
fi
INFERRS_ROOT=${INFERRS_ROOT:-$META_ROOT/inferrs}
CUDA_OXIDE_ROOT=${CUDA_OXIDE_ROOT:-$META_ROOT/cuda-oxide}
CUDA_HOME=${CUDA_HOME:-/usr/local/cuda-13.3}
LLVM21_BIN=${LLVM21_BIN:-/usr/bin}

export CUDA_HOME CUDA_COMPUTE_CAP=${CUDA_COMPUTE_CAP:-120}
export PATH="$LLVM21_BIN:$CUDA_HOME/bin:$CUDA_HOME/nvvm/bin:$PATH"
export LD_LIBRARY_PATH="/usr/lib/x86_64-linux-gnu:$CUDA_HOME/lib64:${LD_LIBRARY_PATH:-}"
export CUDA_OXIDE_LLC=${CUDA_OXIDE_LLC:-$LLVM21_BIN/llc-21}

need() { command -v "$1" >/dev/null || { echo "missing command: $1" >&2; exit 1; }; }
section() { printf '\n== %s ==\n' "$*"; }
require_grep() {
  local pattern=$1 path=$2 why=$3
  if ! grep -Eq "$pattern" "$path"; then
    echo "missing invariant in $path: $why" >&2
    exit 1
  fi
  echo "ok: $why"
}

section "required commands"
for cmd in cargo rustc git rg nvidia-smi nvcc clang llc-21; do need "$cmd"; done

section "Teri git/source identity"
git -C "$TERI_ROOT" rev-parse --show-toplevel
git -C "$TERI_ROOT" log --oneline -1
git -C "$TERI_ROOT" diff --check

section "nightly-only Rust toolchain"
rustup show active-toolchain
rustc -Vv | sed -n '1,8p'
require_grep 'channel *= *"nightly"' "$TERI_ROOT/rust-toolchain.toml" "Teri toolchain channel is nightly"
if grep -Eq 'channel *= *"stable"|channel *= *"[0-9]+\.[0-9]+' "$TERI_ROOT/rust-toolchain.toml"; then
  echo "downgrade detected: rust-toolchain.toml contains a stable/date-pinned channel" >&2
  exit 1
fi

section "Teri inferrs defaults"
require_grep 'Qwen/Qwen3-4B' "$TERI_ROOT/src/config.rs" "default model remains Qwen/Qwen3-4B"
require_grep 'http://127\.0\.0\.1:11435/v1' "$TERI_ROOT/src/config.rs" "default base URL remains local inferrs"
require_grep 'unwrap_or\(300\)' "$TERI_ROOT/src/config.rs" "default LLM timeout remains 300s"
require_grep 'unwrap_or\(2048\)' "$TERI_ROOT/src/config.rs" "default LLM max tokens remains 2048"
require_grep 'LLM_MAX_CONCURRENT_REQUESTS' "$TERI_ROOT/src/llm.rs" "local inferrs concurrency limiter remains present"
if grep -Eq 'ollama|shimmy|ruvllm' "$TERI_ROOT/src/config.rs"; then
  echo "warning: config.rs still mentions legacy-compatible backend names in comments/provider aliases; defaults above are authoritative"
fi
if grep -Eq '(^|[[:space:]])cudarc([[:space:]]|=)' "$TERI_ROOT/Cargo.toml" "$TERI_ROOT/Cargo.lock" 2>/dev/null; then
  echo "downgrade detected: Teri itself depends on cudarc; cuda-oxide should stay the Teri GPU-codegen path" >&2
  exit 1
fi

grep -R "LLM_BASE_URL.*11434\|LLM_MODEL_NAME.*ollama\|LLM_MODEL.*ollama" "$TERI_ROOT/src" "$TERI_ROOT/README.md" "$TERI_ROOT/RUNBOOK.md" && {
  echo "downgrade detected: source/docs default back to Ollama port/model" >&2
  exit 1
} || true

section "GPU + CUDA driver/toolkit"
nvidia-smi --query-gpu=driver_version,name,compute_cap --format=csv,noheader | tee /tmp/teri-no-downgrade-nvidia.txt
grep -q '^610\.' /tmp/teri-no-downgrade-nvidia.txt || { echo "expected NVIDIA 610.x driver" >&2; exit 1; }
nvcc --version | tee /tmp/teri-no-downgrade-nvcc.txt
grep -q 'release 13\.3' /tmp/teri-no-downgrade-nvcc.txt || { echo "expected CUDA toolkit 13.3" >&2; exit 1; }

section "cuda-oxide identity + doctor"
test -d "$CUDA_OXIDE_ROOT/.git" || { echo "missing cuda-oxide git repo: $CUDA_OXIDE_ROOT" >&2; exit 1; }
git -C "$CUDA_OXIDE_ROOT" remote -v | tee /tmp/teri-no-downgrade-cuda-oxide-remote.txt
grep -E 'github.com[:/]NVlabs/cuda-oxide(\.git)?' /tmp/teri-no-downgrade-cuda-oxide-remote.txt >/dev/null || {
  echo "cuda-oxide remote is not NVlabs/cuda-oxide" >&2
  exit 1
}
(
  cd "$CUDA_OXIDE_ROOT"
  cargo oxide doctor
)

section "inferrs identity + CUDA build"
test -d "$INFERRS_ROOT/.git" || { echo "missing inferrs git repo: $INFERRS_ROOT" >&2; exit 1; }
git -C "$INFERRS_ROOT" remote -v | tee /tmp/teri-no-downgrade-inferrs-remote.txt
grep -E 'github.com[:/]FlexNetOS/inferrs(\.git)?' /tmp/teri-no-downgrade-inferrs-remote.txt >/dev/null || {
  echo "inferrs remote is not FlexNetOS/inferrs" >&2
  exit 1
}
git -C "$INFERRS_ROOT" log --oneline -1
(
  cd "$INFERRS_ROOT"
  cargo build --release -p inferrs --features cuda
)

section "focused Teri tests"
(
  cd "$TERI_ROOT"
  cargo test test_default_model_is_qwen3_inferrs_when_no_env --all-features
  cargo test test_default_base_url_is_inferrs_when_no_env --all-features
  cargo test test_default_llm_timeout_allows_local_inferrs_latency --all-features
  cargo test test_default_llm_max_tokens_preserves_ontology_budget --all-features
  cargo test test_openai_request_limit_defaults_for_local_inferrs_only --all-features
  cargo test build_llm_selects_provider_from_config --all-features
  cargo test preflight --all-features
)

section "no-downgrade proof complete"
echo "inferrs remains the default local backend; nightly + NVlabs/cuda-oxide + FlexNetOS/inferrs CUDA stack verified."
