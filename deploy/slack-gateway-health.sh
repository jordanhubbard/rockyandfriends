#!/usr/bin/env bash
# Check native Hermes Slack gateway health across the registered online fleet.
#
# This is intentionally read-only: it verifies processes, DNS, and recent logs
# without sending Slack messages. Use an explicit manual smoke test for posting.
set -euo pipefail

if [ -f "$HOME/.acc/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$HOME/.acc/.env"
  set +a
fi

HUB_URL="${ACC_URL:-${CCC_URL:-http://localhost:8789}}"
TOKEN="${ACC_AGENT_TOKEN:-${ACC_TOKEN:-${CCC_AGENT_TOKEN:-}}}"
PREFER_TAILSCALE="${PREFER_TAILSCALE:-true}"

if [ -z "$TOKEN" ]; then
  echo "[slack-gateway-health] ERROR: ACC_AGENT_TOKEN/CCC_AGENT_TOKEN is not set" >&2
  exit 1
fi

fetch_agents() {
  local url="$1"
  curl -sf -H "Authorization: Bearer $TOKEN" "$url/api/agents?online=true"
}

if ! AGENTS_JSON="$(fetch_agents "$HUB_URL" 2>/dev/null)"; then
  if [ -n "${ACC_TAILSCALE_URL:-}" ] && AGENTS_JSON="$(fetch_agents "$ACC_TAILSCALE_URL" 2>/dev/null)"; then
    HUB_URL="$ACC_TAILSCALE_URL"
  else
    echo "[slack-gateway-health] ERROR: cannot query $HUB_URL/api/agents?online=true" >&2
    exit 1
  fi
fi

mapfile -t TARGETS < <(printf '%s\n' "$AGENTS_JSON" | jq -r '
  (.agents // .)[] |
  select(.ssh_user != null and .ssh_user != "" and .ssh_host != null and .ssh_host != "") |
  [
    (.name // "?"),
    .ssh_user,
    .ssh_host,
    (.ssh_port // 22),
    (.tailscale_ip // ""),
    (.gateway_health // {} | @json)
  ] | @tsv')

if [ "${#TARGETS[@]}" -eq 0 ]; then
  echo "[slack-gateway-health] ERROR: no online agents with SSH coordinates" >&2
  exit 1
fi

remote_check='
set +e
if [ -f "$HOME/.acc/.env" ]; then
  set -a
  . "$HOME/.acc/.env"
  set +a
fi

fail=0
expected_default=0
expected_offtera=0

case "${SLACK_APP_TOKEN:-}" in
  xapp-*) expected_default=1 ;;
esac
if [ -n "${TELEGRAM_BOT_TOKEN:-}" ]; then
  expected_default=1
fi
offtera_token="${SLACK_APP_TOKEN_OFFTERA:-${SLACK_APP_TOKEN_OFTERRA:-}}"
case "$offtera_token" in
  xapp-*) expected_offtera=1 ;;
esac

if ps -axo pid=,command= >/dev/null 2>&1; then
  procs="$(ps -axo pid=,command= | grep -E "acc-agent hermes --gateway|(^|/)hermes gateway" | grep -v grep)"
else
  procs="$(ps -eo pid=,args= | grep -E "acc-agent hermes --gateway|(^|/)hermes gateway" | grep -v grep)"
fi

default_count="$(printf "%s\n" "$procs" | grep "acc-agent hermes --gateway" | grep -v -- "--workspace offtera" | grep -c .)"
offtera_count="$(printf "%s\n" "$procs" | grep "acc-agent hermes --gateway" | grep -- "--workspace offtera" | grep -c .)"
legacy_count="$(printf "%s\n" "$procs" | grep -E "(^|[ /])hermes gateway" | grep -c .)"

if [ "$expected_default" -eq 1 ]; then
  if [ "$default_count" -eq 1 ]; then
    echo "OK default_gateway process_count=1"
  else
    echo "ERROR default_gateway expected=1 actual=$default_count"
    fail=1
  fi
else
  echo "OK default_gateway not_expected actual=$default_count"
fi

if [ "$expected_offtera" -eq 1 ]; then
  if [ "$offtera_count" -eq 1 ]; then
    echo "OK offtera_gateway process_count=1"
  else
    echo "ERROR offtera_gateway expected=1 actual=$offtera_count"
    fail=1
  fi
else
  echo "OK offtera_gateway not_expected actual=$offtera_count"
fi

if [ "$legacy_count" -gt 0 ]; then
  echo "ERROR legacy_hermes_gateway_processes=$legacy_count"
  fail=1
fi

if [ "$expected_default" -eq 1 ] || [ "$expected_offtera" -eq 1 ]; then
  dns_ok=0
  if command -v getent >/dev/null 2>&1; then
    getent hosts slack.com >/dev/null 2>&1 && getent hosts api.slack.com >/dev/null 2>&1 && dns_ok=1
  elif command -v dscacheutil >/dev/null 2>&1; then
    dscacheutil -q host -a name slack.com >/dev/null 2>&1 && dscacheutil -q host -a name api.slack.com >/dev/null 2>&1 && dns_ok=1
  fi
  if [ "$dns_ok" -eq 1 ]; then
    echo "OK slack_dns"
  else
    echo "ERROR slack_dns_failed"
    fail=1
  fi
fi

recent_logs=""
if command -v journalctl >/dev/null 2>&1; then
  recent_logs="$(journalctl -u acc-agent.service --since "15 minutes ago" --no-pager 2>/dev/null; journalctl --user -u acc-agent.service --since "15 minutes ago" --no-pager 2>/dev/null)"
fi
if printf "%s\n" "$recent_logs" | grep -qE "auth.test failed|no platforms configured"; then
  printf "%s\n" "$recent_logs" | grep -E "auth.test failed|no platforms configured" | tail -n 5 | sed "s/^/ERROR recent_log /"
  fail=1
elif printf "%s\n" "$recent_logs" | grep -qE "Slack adapter started|socket mode connected"; then
  printf "%s\n" "$recent_logs" | grep -E "Slack adapter started|socket mode connected" | tail -n 3 | sed "s/^/OK recent_log /"
else
  echo "WARN recent_gateway_log_unavailable"
fi

exit "$fail"
'

failed=0
echo "[slack-gateway-health] hub=$HUB_URL targets=${#TARGETS[@]}"
for row in "${TARGETS[@]}"; do
  IFS=$'\t' read -r name user host port tailscale_ip gateway_health <<< "$row"
  ssh_host="$host"
  if [ "$PREFER_TAILSCALE" = "true" ] && [ -n "$tailscale_ip" ]; then
    ssh_host="$tailscale_ip"
  fi

  registry_state="$(printf '%s\n' "$gateway_health" | jq -r '
    if (.children? // {}) == {} then "registry_gateway_health=unreported"
    else
      (.children | to_entries | map("\(.key):\(.value.status // "?")/running=\(.value.running // false)") | join(","))
    end')"
  echo "== $name ($user@$ssh_host:$port) $registry_state =="

  if ssh -o ConnectTimeout=8 -o BatchMode=yes -o StrictHostKeyChecking=accept-new -p "$port" "$user@$ssh_host" "$remote_check" 2>&1 | sed "s/^/  /"; then
    :
  else
    failed=$((failed + 1))
  fi
done

if [ "$failed" -ne 0 ]; then
  echo "[slack-gateway-health] FAILED agents=$failed" >&2
  exit 1
fi

echo "[slack-gateway-health] OK"
