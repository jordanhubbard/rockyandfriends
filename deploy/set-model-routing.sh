#!/usr/bin/env bash
# set-model-routing.sh — write fleet-wide model/CLI routing into ACC secrets.
#
# Examples:
#   bash deploy/set-model-routing.sh --model azure/openai/gpt-5.4 \
#     --provider openai --base-url https://inference-api.nvidia.com/v1 \
#     --cli-order cursor_cli,codex_cli,claude_cli --restart
#
#   OPENAI_API_KEY=sk-... bash deploy/set-model-routing.sh --model gpt-5.5 \
#     --provider openai --base-url https://api.openai.com/v1 \
#     --api-key-env OPENAI_API_KEY --restart
set -euo pipefail

if [[ -f "$HOME/.acc/.env" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "$HOME/.acc/.env"
  set +a
fi

HUB_URL="${ACC_URL:-${CCC_URL:-http://localhost:8789}}"
TOKEN="${ACC_AGENT_TOKEN:-${ACC_TOKEN:-${CCC_AGENT_TOKEN:-}}}"
MODEL=""
PROVIDER="openai"
BASE_URL=""
API_KEY_ENV=""
API_KEY_FILE=""
CLI_ORDER=""
RESTART=false

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
  cat <<'EOF'

Options:
  --model <name>          Model ID to use for Hermes/API-backed agents.
  --provider <name>       Provider type: openai or anthropic. Default: openai.
  --base-url <url>        Provider base URL, e.g. https://api.openai.com/v1.
  --api-key-env <var>     Store API key from environment variable <var>.
  --api-key-file <path>   Store API key read from file.
  --cli-order <list>      Default CLI order, e.g. codex_cli,cursor_cli,claude_cli.
  --restart              Run deploy/restart-fleet.sh after writing secrets.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model) MODEL="${2:-}"; shift 2 ;;
    --provider) PROVIDER="${2:-}"; shift 2 ;;
    --base-url) BASE_URL="${2:-}"; shift 2 ;;
    --api-key-env) API_KEY_ENV="${2:-}"; shift 2 ;;
    --api-key-file) API_KEY_FILE="${2:-}"; shift 2 ;;
    --cli-order) CLI_ORDER="${2:-}"; shift 2 ;;
    --restart) RESTART=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *)
      if [[ -z "$MODEL" ]]; then
        MODEL="$1"
        shift
      else
        echo "[set-model-routing] unknown argument: $1" >&2
        usage >&2
        exit 2
      fi
      ;;
  esac
done

if [[ -z "$TOKEN" ]]; then
  echo "[set-model-routing] ERROR: ACC_AGENT_TOKEN/CCC_AGENT_TOKEN is not set" >&2
  exit 1
fi

json_body() {
  python3 -c 'import json,sys; print(json.dumps({"value": sys.argv[1]}))' "$1"
}

set_secret() {
  local key="$1" value="$2"
  curl -sf -X POST "${HUB_URL%/}/api/secrets/${key}" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$(json_body "$value")" >/dev/null
  echo "[set-model-routing] set ${key}"
}

if [[ -n "$MODEL" ]]; then
  set_secret "hermes/provider" "$PROVIDER"
  set_secret "hermes/model" "$MODEL"
  case "$PROVIDER" in
    openai|openai-compat)
      set_secret "openai/model" "$MODEL"
      [[ -n "$BASE_URL" ]] && set_secret "openai/base_url" "$BASE_URL"
      ;;
    anthropic)
      set_secret "anthropic/model" "$MODEL"
      [[ -n "$BASE_URL" ]] && set_secret "anthropic/base_url" "$BASE_URL"
      ;;
    *)
      echo "[set-model-routing] WARNING: provider '$PROVIDER' will be passed through as-is" >&2
      ;;
  esac
fi

if [[ -n "$API_KEY_ENV" ]]; then
  value="${!API_KEY_ENV:-}"
  if [[ -z "$value" ]]; then
    echo "[set-model-routing] ERROR: environment variable $API_KEY_ENV is empty" >&2
    exit 1
  fi
  case "$PROVIDER" in
    anthropic) set_secret "anthropic/api_key" "$value" ;;
    *)         set_secret "openai/api_key" "$value" ;;
  esac
fi

if [[ -n "$API_KEY_FILE" ]]; then
  if [[ ! -f "$API_KEY_FILE" ]]; then
    echo "[set-model-routing] ERROR: API key file not found: $API_KEY_FILE" >&2
    exit 1
  fi
  value="$(tr -d '\r\n' < "$API_KEY_FILE")"
  case "$PROVIDER" in
    anthropic) set_secret "anthropic/api_key" "$value" ;;
    *)         set_secret "openai/api_key" "$value" ;;
  esac
fi

if [[ -n "$CLI_ORDER" ]]; then
  set_secret "cli/executor_order" "$CLI_ORDER"
fi

if $RESTART; then
  bash "$(dirname "$0")/restart-fleet.sh"
fi
