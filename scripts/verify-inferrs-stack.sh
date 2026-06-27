#!/usr/bin/env bash
# Verify Teri's local inferrs/CUDA upgrade stack.
# This is a non-destructive health gate: it checks toolchain, GPU, docs-default backend,
# cuda-oxide, inferrs CUDA build, and Teri build/tests. It does not start long-lived servers.
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
CUDA_COMPUTE_CAP=${CUDA_COMPUTE_CAP:-120}
LLVM21_BIN=${LLVM21_BIN:-/usr/bin}

export CUDA_HOME CUDA_COMPUTE_CAP
export PATH="$LLVM21_BIN:$CUDA_HOME/bin:$CUDA_HOME/nvvm/bin:$PATH"
export LD_LIBRARY_PATH="/usr/lib/x86_64-linux-gnu:$CUDA_HOME/lib64:${LD_LIBRARY_PATH:-}"
export CUDA_OXIDE_LLC=${CUDA_OXIDE_LLC:-$LLVM21_BIN/llc-21}
# cuda-oxide doctor shells out to `clang -print-resource-dir`; prefer system LLVM-21 because
# meta-local clang shims may point at an incomplete resource dir without include/stddef.h.

need() { command -v "$1" >/dev/null || { echo "missing command: $1" >&2; exit 1; }; }
section() { printf '\n== %s ==\n' "$*"; }

section "required commands"
need cargo
need rustc
need nvidia-smi
need nvcc
need clang
need llc-21

section "GPU + CUDA"
nvidia-smi --query-gpu=driver_version,name,compute_cap --format=csv,noheader | tee /tmp/teri-inferrs-nvidia.txt
grep -q '^610\.' /tmp/teri-inferrs-nvidia.txt || { echo "expected NVIDIA 610.x driver" >&2; exit 1; }
nvcc --version | tee /tmp/teri-inferrs-nvcc.txt
grep -q 'release 13\.3' /tmp/teri-inferrs-nvcc.txt || { echo "expected CUDA toolkit 13.3" >&2; exit 1; }

section "clang resource dir"
RESOURCE_DIR=$(clang -print-resource-dir)
echo "$RESOURCE_DIR"
test -f "$RESOURCE_DIR/include/stddef.h" || { echo "clang resource dir lacks include/stddef.h" >&2; exit 1; }

section "cuda-oxide doctor"
test -d "$CUDA_OXIDE_ROOT" || { echo "missing cuda-oxide repo: $CUDA_OXIDE_ROOT" >&2; exit 1; }
(
  cd "$CUDA_OXIDE_ROOT"
  cargo oxide doctor
)

section "inferrs CUDA build"
test -d "$INFERRS_ROOT" || { echo "missing inferrs repo: $INFERRS_ROOT" >&2; exit 1; }
(
  cd "$INFERRS_ROOT"
  cargo build --release -p inferrs --features cuda
)

section "Teri build + focused config/preflight tests"
(
  cd "$TERI_ROOT"
  cargo build --workspace --all-features
  cargo test test_default_model_is_qwen3_inferrs_when_no_env --all-features
  cargo test test_default_base_url_is_inferrs_when_no_env --all-features
  cargo test test_default_llm_timeout_allows_local_inferrs_latency --all-features
  cargo test preflight --all-features
)

section "health complete"
echo "Teri inferrs upgrade stack is healthy."
