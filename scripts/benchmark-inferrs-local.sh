#!/usr/bin/env bash
# Run fresh local inferrs CUDA benchmarks for Teri-supported local models.
set -euo pipefail

TERI_ROOT=${TERI_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
if [[ -z "${META_ROOT:-}" ]]; then
  probe="$TERI_ROOT"
  META_ROOT=""
  while [[ "$probe" != "/" ]]; do
    if [[ -d "$probe/inferrs" ]]; then META_ROOT="$probe"; break; fi
    probe=$(dirname "$probe")
  done
  if [[ -z "$META_ROOT" ]]; then echo "could not infer META_ROOT; set META_ROOT=/path/to/meta" >&2; exit 1; fi
fi
INFERRS_ROOT=${INFERRS_ROOT:-$META_ROOT/inferrs}
INFERRS_BIN=${INFERRS_BIN:-$INFERRS_ROOT/target/release/inferrs}
CUDA_HOME=${CUDA_HOME:-/usr/local/cuda-13.3}
BENCH_OUT_DIR=${BENCH_OUT_DIR:-/tmp/teri-inferrs-benchmarks/$(date -u +%Y%m%dT%H%M%SZ)}
BENCH_RUNS=${BENCH_RUNS:-3}
BENCH_WARMUP=${BENCH_WARMUP:-1}
BENCH_PROMPT_LEN=${BENCH_PROMPT_LEN:-128}
BENCH_MAX_TOKENS=${BENCH_MAX_TOKENS:-64}
BENCH_PORT=${BENCH_PORT:-11436}
# Space-separated list; override to benchmark every locally cached inferrs model you care about.
BENCH_MODELS=${BENCH_MODELS:-"Qwen/Qwen3-4B Qwen/Qwen2.5-0.5B-Instruct Qwen/Qwen2-0.5B"}

export CUDA_HOME CUDA_COMPUTE_CAP=${CUDA_COMPUTE_CAP:-120}
export PATH="/usr/bin:/bin:$CUDA_HOME/bin:$CUDA_HOME/nvvm/bin:$PATH"
export LD_LIBRARY_PATH="/usr/lib/x86_64-linux-gnu:$CUDA_HOME/lib64:${LD_LIBRARY_PATH:-}"

need() { command -v "$1" >/dev/null || { echo "missing command: $1" >&2; exit 1; }; }
safe_name() { printf '%s' "$1" | tr '/: ' '___'; }

need git
need nvidia-smi
need nvcc
need python3
test -x "$INFERRS_BIN" || { echo "missing executable inferrs binary: $INFERRS_BIN" >&2; exit 1; }
mkdir -p "$BENCH_OUT_DIR"

summary="$BENCH_OUT_DIR/summary.md"
tsv="$BENCH_OUT_DIR/results.tsv"
{
  echo "# Teri local inferrs benchmark run"
  echo
  echo "- Timestamp UTC: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- Teri commit: $(git -C "$TERI_ROOT" rev-parse --short HEAD)"
  echo "- inferrs commit: $(git -C "$INFERRS_ROOT" rev-parse --short HEAD)"
  echo "- inferrs binary: $INFERRS_BIN"
  echo "- CUDA_HOME: $CUDA_HOME"
  echo "- Runs: $BENCH_RUNS timed, $BENCH_WARMUP warm-up, prompt_len=$BENCH_PROMPT_LEN, max_tokens=$BENCH_MAX_TOKENS"
  echo "- GPU(s): $(nvidia-smi --query-gpu=driver_version,name,memory.total --format=csv,noheader | paste -sd ';' -)"
  echo "- CUDA toolkit: $(nvcc --version | awk '/release/ {print $0}')"
  echo
  echo "| Model | Prefill tok/s | Decode tok/s | TTFT ms | E2E avg ms | Log |"
  echo "|---|---:|---:|---:|---:|---|"
} >"$summary"
printf 'model\tprefill_tok_s\tdecode_tok_s\tttft_ms\te2e_avg_ms\tlog\n' >"$tsv"

for model in $BENCH_MODELS; do
  name=$(safe_name "$model")
  log="$BENCH_OUT_DIR/$name.log"
  echo "== Benchmarking $model =="
  "$INFERRS_BIN" bench \
    --device cuda \
    --host 127.0.0.1 \
    --port "$BENCH_PORT" \
    --runs "$BENCH_RUNS" \
    --warmup "$BENCH_WARMUP" \
    --prompt-len "$BENCH_PROMPT_LEN" \
    --max-tokens "$BENCH_MAX_TOKENS" \
    "$model" 2>&1 | tee "$log"
  python3 - "$model" "$log" "$summary" "$tsv" <<'PY'
import re, sys
model, log_path, summary_path, tsv_path = sys.argv[1:]
text = open(log_path, encoding='utf-8').read()
def grab(label):
    m = re.search(label + r"\s*:\s*([0-9.]+)", text)
    if not m:
        raise SystemExit(f"could not parse {label} from {log_path}")
    return m.group(1)
prefill = grab(r"Prefill throughput")
decode = grab(r"Decode  throughput")
ttft = grab(r"Time to first token")
e2e = grab(r"End-to-end latency \(avg\)")
with open(tsv_path, "a", encoding="utf-8") as f:
    f.write(f"{model}\t{prefill}\t{decode}\t{ttft}\t{e2e}\t{log_path}\n")
with open(summary_path, "a", encoding="utf-8") as f:
    f.write(f"| `{model}` | {prefill} | {decode} | {ttft} | {e2e} | `{log_path}` |\n")
PY
done

echo
echo "Benchmark summary: $summary"
echo "Machine-readable TSV: $tsv"
cat "$summary"
