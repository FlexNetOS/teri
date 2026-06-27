#!/usr/bin/env bash
# Verify Teri can keep local inferrs and cloud/Codex paths available together.
set -euo pipefail

TERI_ROOT=${TERI_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
OUT_DIR=${OUT_DIR:-/tmp/teri-cloud-codex-coexistence/$(date -u +%Y%m%dT%H%M%SZ)}
LOCAL_INFERRS_SMOKE=${LOCAL_INFERRS_SMOKE:-1}
CODEX_SMOKE=${CODEX_SMOKE:-1}
mkdir -p "$OUT_DIR"

section() { printf '\n== %s ==\n' "$*"; }

section "source-level provider matrix"
(
  cd "$TERI_ROOT"
  cargo test build_llm_selects_provider_from_config --all-features
  cargo test api_state_constructs_under_anthropic_and_gemini --all-features
  cargo test test_provider_adapter_selection --all-features
  cargo test test_llm_provider_from_env_str --all-features
  cargo test test_openai_request_limit_defaults_for_local_inferrs_only --all-features
)

section "local inferrs config remains default while cloud providers stay selectable"
(
  cd "$TERI_ROOT"
  env -u LLM_BASE_URL -u LLM_MODEL -u LLM_MODEL_NAME LLM_API_KEY=sk-local \
    cargo test test_default_model_is_qwen3_inferrs_when_no_env --all-features
  env -u LLM_BASE_URL -u LLM_MODEL -u LLM_MODEL_NAME LLM_API_KEY=sk-local \
    cargo test test_default_base_url_is_inferrs_when_no_env --all-features
  env LLM_API_KEY=sk-local LLM_PROVIDER=anthropic cargo test build_llm_selects_provider_from_config --all-features
  env LLM_API_KEY=sk-local LLM_PROVIDER=gemini cargo test build_llm_selects_provider_from_config --all-features
)

section "live local inferrs/Teri runtime smoke"
if [[ "$LOCAL_INFERRS_SMOKE" != "1" ]]; then
  echo "LOCAL_INFERRS_SMOKE=$LOCAL_INFERRS_SMOKE; skipping live local inferrs smoke."
else
  "$TERI_ROOT/scripts/verify-inferrs-runtime.sh" | tee "$OUT_DIR/local-inferrs-runtime.log"
fi

section "Codex cloud CLI smoke"
if [[ "$CODEX_SMOKE" != "1" ]]; then
  echo "CODEX_SMOKE=$CODEX_SMOKE; skipping live Codex cloud smoke. Set CODEX_SMOKE=1 to run it."
  exit 0
fi
if ! command -v codex >/dev/null; then
  echo "codex CLI is not installed" >&2
  exit 1
fi
codex_out="$OUT_DIR/codex-last-message.txt"
stdout="$OUT_DIR/codex.stdout"
stderr="$OUT_DIR/codex.stderr"
(
  cd "$TERI_ROOT"
  timeout "${CODEX_TIMEOUT_SECS:-180}" codex exec --ephemeral --ignore-user-config --ignore-rules --sandbox read-only --cd "$TERI_ROOT" \
    --output-last-message "$codex_out" 'Return exactly: codex-cloud-ok' >"$stdout" 2>"$stderr"
)
cat "$codex_out"
echo
grep -qx 'codex-cloud-ok' "$codex_out" || { echo "Codex smoke returned unexpected output" >&2; exit 1; }
if grep -Eiq "warning:|error:|failed to parse hooks config|database error" "$stderr"; then
  echo "Codex smoke emitted warnings/errors" >&2
  cat "$stderr" >&2
  exit 1
fi

echo "Codex cloud smoke passed; artifacts in $OUT_DIR"
