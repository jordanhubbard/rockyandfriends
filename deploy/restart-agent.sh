#!/usr/bin/env bash
# restart-agent.sh — rebuild acc-agent on THIS node and restart it.
#
# Run on an agent node. The acc-agent supervisor relaunches the binary
# automatically after `pkill -x acc-agent`, so no daemon-manager needed.
set -euo pipefail

ACC_DIR="${ACC_DIR:-$HOME/.acc}"
LOG_DIR="${LOG_DIR:-$ACC_DIR/logs}"
WORKSPACE="${WORKSPACE:-$(git rev-parse --show-toplevel 2>/dev/null || true)}"

# Non-interactive SSH sessions don't source ~/.bashrc, so rustup-installed
# cargo isn't on PATH. Pull it in explicitly if it's there.
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

if [ -z "$WORKSPACE" ] || [ ! -f "${WORKSPACE}/Cargo.toml" ]; then
    echo "[restart-agent] ERROR: cannot locate workspace (run from inside the repo, or set WORKSPACE=)" >&2
    exit 1
fi

AGENT_BIN="${WORKSPACE}/target/release/acc-agent"
AGENT_DEST="${ACC_DIR}/bin/acc-agent"
mkdir -p "$LOG_DIR" "${ACC_DIR}/bin"

echo "[restart-agent] Building acc-agent (release) from $WORKSPACE"
cargo build --release --manifest-path "${WORKSPACE}/Cargo.toml" -p acc-agent

if [ ! -x "$AGENT_BIN" ]; then
    echo "[restart-agent] ERROR: build produced no binary at $AGENT_BIN" >&2
    exit 1
fi

echo "[restart-agent] Installing → $AGENT_DEST"
tmp="${AGENT_DEST}.new.$$"
cp "$AGENT_BIN" "$tmp"
chmod +x "$tmp"
mv "$tmp" "$AGENT_DEST"

echo "[restart-agent] Stopping running acc-agent so the daemon manager respawns with the new binary"
# Capture old supervise PID before kill so we can confirm a NEW one comes up
old_sup=$(pgrep -f "acc-agent supervise" | head -1 || true)
pkill -x acc-agent 2>/dev/null || true

# launchd (macOS, KeepAlive=true) and systemd typically relaunch within
# 2-8 seconds. Poll up to ~30s for a NEW supervise process whose PID
# differs from the one we killed.
new_sup=""
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    sleep 2
    candidate=$(pgrep -f "acc-agent supervise" | head -1 || true)
    if [ -n "$candidate" ] && [ "$candidate" != "$old_sup" ]; then
        new_sup="$candidate"
        break
    fi
done

if [ -n "$new_sup" ]; then
    echo "[restart-agent] ✓ acc-agent supervise relaunched (pid=$new_sup, was pid=${old_sup:-none})"
else
    echo "[restart-agent] ✗ acc-agent supervise did NOT relaunch within 30s" >&2
    echo "    Likely cause: launchd/systemd unit not installed or not configured to restart on exit." >&2
    if [[ "$(uname)" == "Darwin" ]]; then
        echo "    Try: launchctl load ~/Library/LaunchAgents/com.acc.agent.plist" >&2
    else
        echo "    Try: re-run deploy/setup-node.sh, or manually exec: nohup $AGENT_DEST supervise &" >&2
    fi
    exit 1
fi
