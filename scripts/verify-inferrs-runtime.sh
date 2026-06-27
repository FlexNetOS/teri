#!/usr/bin/env bash
# Runtime smoke test for Teri's inferrs CUDA backend.
# Starts inferrs on a clean port, proves CUDA chat, then starts Teri and proves it binds only after
# the backend honesty guard accepts inferrs. All long-lived processes are killed by PID on exit.
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
CUDA_HOME=${CUDA_HOME:-/usr/local/cuda-13.3}
MODEL=${LLM_MODEL_NAME:-Qwen/Qwen3-4B}
INFERRS_PORT=${INFERRS_PORT:-11435}
TERI_PORT=${TERI_PORT:-5001}
INFERRS_LOG=${INFERRS_LOG:-/tmp/teri-inferrs-runtime-inferrs.log}
TERI_LOG=${TERI_LOG:-/tmp/teri-inferrs-runtime-teri.log}

export CUDA_HOME CUDA_COMPUTE_CAP=${CUDA_COMPUTE_CAP:-120}
export PATH="/usr/bin:/bin:$CUDA_HOME/bin:$CUDA_HOME/nvvm/bin:$PATH"
export LD_LIBRARY_PATH="/usr/lib/x86_64-linux-gnu:$CUDA_HOME/lib64:${LD_LIBRARY_PATH:-}"

inferrs_pid=""
teri_pid=""
cleanup() {
  if [[ -n "$teri_pid" ]] && kill -0 "$teri_pid" 2>/dev/null; then kill "$teri_pid" 2>/dev/null || true; fi
  if [[ -n "$inferrs_pid" ]] && kill -0 "$inferrs_pid" 2>/dev/null; then kill "$inferrs_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT

wait_http() {
  local url=$1
  local tries=${2:-120}
  for _ in $(seq 1 "$tries"); do
    if curl -fsS --max-time 2 "$url" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  return 1
}

post_chat_ok() {
  local base=$1
  curl -fsS --max-time 60 "$base/v1/chat/completions" \
    -H 'Content-Type: application/json' \
    -H 'Authorization: Bearer sk-local' \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply exactly ok\"}],\"max_tokens\":2,\"temperature\":0}" \
    | tee /tmp/teri-inferrs-runtime-chat.json >/dev/null
  python3 - <<'PYJSON'
import json, sys
p = '/tmp/teri-inferrs-runtime-chat.json'
body = json.load(open(p))
content = body.get('choices', [{}])[0].get('message', {}).get('content', '')
if not content.strip():
    print('empty completion content', file=sys.stderr)
    sys.exit(1)
low = content.lower()
if 'mock mode' in low or 'stub' in low or 'placeholder' in low:
    print(f'stub-like completion content: {content!r}', file=sys.stderr)
    sys.exit(1)
PYJSON
}

if ss -ltn | grep -q ":$INFERRS_PORT "; then
  echo "port $INFERRS_PORT already in use; stop the existing backend or set INFERRS_PORT" >&2
  exit 1
fi
if ss -ltn | grep -q ":$TERI_PORT "; then
  echo "port $TERI_PORT already in use; stop the existing Teri server or set TERI_PORT" >&2
  exit 1
fi

cd "$INFERRS_ROOT"
: >"$INFERRS_LOG"
"$INFERRS_ROOT/target/release/inferrs" serve \
  --device cuda --host 127.0.0.1 --port "$INFERRS_PORT" --max-tokens 64 "$MODEL" \
  >"$INFERRS_LOG" 2>&1 &
inferrs_pid=$!

wait_http "http://127.0.0.1:$INFERRS_PORT/v1/models" 120 || {
  echo "inferrs did not expose /v1/models" >&2
  tail -80 "$INFERRS_LOG" >&2 || true
  exit 1
}
# Wait for the selected model to be marked loaded, not merely listed from cache.
for _ in $(seq 1 180); do
  if curl -fsS --max-time 2 "http://127.0.0.1:$INFERRS_PORT/v1/models" | grep -q '"owned_by":"inferrs (loaded)"\|"owned_by": "inferrs (loaded)"'; then
    break
  fi
  sleep 1
done
curl -fsS --max-time 5 "http://127.0.0.1:$INFERRS_PORT/v1/models" | grep -q '"owned_by":"inferrs (loaded)"\|"owned_by": "inferrs (loaded)"' || {
  echo "inferrs did not mark $MODEL as loaded" >&2
  tail -120 "$INFERRS_LOG" >&2 || true
  exit 1
}
post_chat_ok "http://127.0.0.1:$INFERRS_PORT" || {
  echo "inferrs CUDA chat smoke failed" >&2
  tail -120 "$INFERRS_LOG" >&2 || true
  cat /tmp/teri-inferrs-runtime-chat.json >&2 || true
  exit 1
}

cd "$TERI_ROOT"
: >"$TERI_LOG"
env LLM_API_KEY=sk-local LLM_BASE_URL="http://127.0.0.1:$INFERRS_PORT/v1" LLM_MODEL_NAME="$MODEL" \
  "$TERI_ROOT/target/debug/teri" serve --addr "127.0.0.1:$TERI_PORT" \
  >"$TERI_LOG" 2>&1 &
teri_pid=$!
wait_http "http://127.0.0.1:$TERI_PORT/health" 120 || {
  echo "teri did not boot against inferrs" >&2
  tail -120 "$TERI_LOG" >&2 || true
  exit 1
}
grep -q 'Backend honesty guard passed' "$TERI_LOG" || {
  echo "teri health responded but guard-pass log missing" >&2
  tail -120 "$TERI_LOG" >&2 || true
  exit 1
}

echo "Teri runtime is healthy against inferrs CUDA ($MODEL)."
