#!/usr/bin/env bash
# restart-agent.sh — rebuild acc-agent on THIS node and restart it.
#
# Verifies daemon manager (launchd/systemd) is configured and up-to-date
# BEFORE killing acc-agent, so the agent is never left dead due to a missing
# or stale daemon config.
#
# macOS launchd notes
# -------------------
# • launchctl load/unload are deprecated on macOS 10.11+ and exhibit two
#   races we must avoid:
#
#   1. "Already loaded" silent failure: calling `launchctl load` on a plist
#      whose service is already registered is a no-op (old launchctl) or
#      an error (new launchctl). We must never call load on a running service.
#
#   2. Stale-update gap: the old sequence
#         unload → write-new-plist → load
#      makes launchd respawn the agent immediately after `load` — with the
#      OLD binary still on disk. The binary swap that happens later then
#      leaves a stale process running, and the subsequent pkill triggers
#      yet another respawn, burning time and risking a window where no
#      agent is alive.
#
# Fix: split the macOS work into two phases.
#   Phase 1 (before build): write the plist to disk only; never touch the
#            live launchd service.  For a brand-new install the service is
#            not yet registered so there is nothing to race against.
#   Phase 2 (after binary swap): perform the actual launchd registration/
#            reload via bootout+bootstrap (10.11+) or unload+load (fallback).
#            At this point the new binary is already in place, so launchd
#            respawns directly into the new version — no gap.
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

AGENT_DEST="${ACC_DIR}/bin/acc-agent"
mkdir -p "$LOG_DIR" "${ACC_DIR}/bin"

# ── Phase 1: write daemon config to disk (before build/binary-swap) ──────────
#
# On macOS we ONLY write the plist file here — we never touch the live launchd
# service at this stage.  The actual bootout/bootstrap (or unload/load) is
# deferred to reload_launchd_after_binary_swap(), called after the new binary
# is atomically in place.  This eliminates both races described in the header:
#
#  • "Already loaded" failure — we never call `load` on a running service.
#  • Stale-update gap — launchd never sees the new plist until the new binary
#    is already sitting at $AGENT_DEST.
#
# A global flag tells the later phase what action (if any) is required.
# Values: "none" | "register" | "reload"
_LAUNCHD_ACTION="none"

ensure_daemon_configured() {
    if [[ "$(uname)" == "Darwin" ]]; then
        local PLIST_SRC="${WORKSPACE}/deploy/launchd/com.acc.agent.plist"
        local PLIST_DST="${HOME}/Library/LaunchAgents/com.acc.agent.plist"
        local rendered
        rendered=$(sed "s|AGENT_HOME|${HOME}|g" "${PLIST_SRC}")

        if [ ! -f "${PLIST_DST}" ]; then
            # Brand-new install: write plist now; register with launchd after
            # the binary is in place (phase 2).
            echo "[restart-agent] LaunchAgent not installed — writing plist (will register after binary swap)"
            echo "${rendered}" > "${PLIST_DST}"
            _LAUNCHD_ACTION="register"
        else
            local installed
            installed=$(cat "${PLIST_DST}")
            if [ "${rendered}" != "${installed}" ]; then
                # Plist is stale: write the new content now; bootout+bootstrap
                # (or unload+load) will be done after the binary is in place.
                echo "[restart-agent] LaunchAgent is stale — updating plist (will reload after binary swap)"
                echo "${rendered}" > "${PLIST_DST}"
                _LAUNCHD_ACTION="reload"
            else
                # Plist is current; no launchd config change needed.
                # Phase 2 will simply kill the running process and let launchd
                # KeepAlive respawn it with the new binary — no load/unload.
                echo "[restart-agent] LaunchAgent is current"
                _LAUNCHD_ACTION="none"
            fi
        fi

    elif command -v systemctl &>/dev/null; then
        local SERVICE_SRC="${WORKSPACE}/deploy/systemd/acc-agent.service"
        local SERVICE_DST="/etc/systemd/system/acc-agent.service"
        local rendered
        rendered=$(sed "s|AGENT_USER|$(whoami)|g; s|AGENT_HOME|${HOME}|g" "${SERVICE_SRC}")

        if [ ! -f "${SERVICE_DST}" ]; then
            echo "[restart-agent] systemd unit not installed — installing now"
            if command -v sudo &>/dev/null; then
                echo "${rendered}" | sudo tee "${SERVICE_DST}" > /dev/null
                sudo systemctl daemon-reload
                sudo systemctl enable acc-agent
                echo "[restart-agent] ✓ systemd unit installed and enabled"
            else
                echo "[restart-agent] ERROR: no sudo access — cannot install systemd unit" >&2
                echo "    Run: sudo systemctl enable --now acc-agent" >&2
                exit 1
            fi
        else
            local installed
            installed=$(cat "${SERVICE_DST}")
            if [ "${rendered}" != "${installed}" ]; then
                echo "[restart-agent] systemd unit is stale — updating"
                if command -v sudo &>/dev/null; then
                    echo "${rendered}" | sudo tee "${SERVICE_DST}" > /dev/null
                    sudo systemctl daemon-reload
                    echo "[restart-agent] ✓ systemd unit updated"
                else
                    echo "[restart-agent] WARNING: no sudo access — cannot update systemd unit (continuing anyway)" >&2
                fi
            else
                echo "[restart-agent] systemd unit is current"
            fi
        fi

        # Verify the unit is enabled (will restart on exit)
        if ! systemctl is-enabled acc-agent &>/dev/null; then
            echo "[restart-agent] WARNING: acc-agent.service is not enabled — enabling now"
            sudo systemctl enable acc-agent 2>/dev/null || \
                echo "[restart-agent] WARNING: could not enable — agent may not restart after reboot" >&2
        fi
    else
        echo "[restart-agent] WARNING: no daemon manager detected (not macOS launchd, not systemd)" >&2
        echo "    The agent will NOT automatically restart after this script kills it." >&2
        echo "    Start manually after: nohup ${AGENT_DEST} supervise &" >&2
    fi
}

# ── Phase 2: reload launchd AFTER the new binary is atomically in place ───────
#
# Uses bootout/bootstrap (macOS 10.11+) with a graceful fallback to the
# deprecated unload/load pair.  At this point $AGENT_DEST already holds the
# new binary, so launchd respawns directly into the new version — no gap.
#
# For the "none" action (plist was already current) we skip this entirely;
# the pkill below is sufficient — launchd's KeepAlive will respawn with the
# new binary automatically.
reload_launchd_after_binary_swap() {
    [[ "$(uname)" != "Darwin" ]] && return 0
    [[ "${_LAUNCHD_ACTION}" == "none" ]] && return 0

    local PLIST_DST="${HOME}/Library/LaunchAgents/com.acc.agent.plist"
    # GUI session domain target for the current user (uid/bootstrap token).
    local GUI_TARGET="gui/$(id -u)"

    # Prefer bootout/bootstrap (10.11+, not deprecated).
    if launchctl bootout "${GUI_TARGET}" "${PLIST_DST}" 2>/dev/null; then
        echo "[restart-agent] launchctl bootout succeeded"
        # Small pause so launchd finishes tearing down before we re-register.
        sleep 1
        if launchctl bootstrap "${GUI_TARGET}" "${PLIST_DST}" 2>/dev/null; then
            echo "[restart-agent] ✓ LaunchAgent bootstrapped (new plist + new binary)"
        else
            echo "[restart-agent] WARNING: launchctl bootstrap failed; attempting legacy load" >&2
            launchctl load "${PLIST_DST}" 2>/dev/null || true
        fi
    else
        # Fallback for systems where bootout is unavailable or the service
        # was not yet loaded (brand-new install after a failed first boot).
        echo "[restart-agent] launchctl bootout unavailable or service not loaded — using legacy load"
        launchctl unload "${PLIST_DST}" 2>/dev/null || true
        launchctl load  "${PLIST_DST}" 2>/dev/null || true
    fi

    # Verify launchd will keep it alive
    if ! launchctl list 2>/dev/null | grep -q "com.acc.agent"; then
        echo "[restart-agent] WARNING: com.acc.agent not visible in launchctl list — may not auto-restart" >&2
    else
        echo "[restart-agent] ✓ com.acc.agent confirmed in launchctl list"
    fi
}

echo "[restart-agent] Checking daemon manager configuration (phase 1: plist/unit write)..."
ensure_daemon_configured

# ── Build new acc-agent binary ─────────────────────────────────────────────
AGENT_BIN="${WORKSPACE}/target/release/acc-agent"

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

# ── Phase 2: reload launchd now that the new binary is in place ───────────
# (On Linux/systemd this is a no-op; systemd picks up daemon-reload changes
#  applied earlier and Restart=on-failure handles the respawn after pkill.)
echo "[restart-agent] Reloading daemon manager with new binary in place (phase 2)..."
reload_launchd_after_binary_swap

# ── Kill and wait for respawn ──────────────────────────────────────────────
echo "[restart-agent] Stopping running acc-agent so the daemon manager respawns with the new binary"
# Capture old supervise PID before kill so we can confirm a NEW one comes up
old_sup=$(pgrep -f "acc-agent supervise" | head -1 || true)
pkill -x acc-agent 2>/dev/null || true

# launchd (macOS, KeepAlive=true) and systemd (Restart=on-failure) typically
# relaunch within 2-8 seconds. Poll up to ~30s for a NEW supervise process
# whose PID differs from the one we killed.
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
    echo "    Daemon config was verified above — check daemon manager logs:" >&2
    if [[ "$(uname)" == "Darwin" ]]; then
        echo "    log show --predicate 'subsystem == \"com.apple.launchd\"' --last 2m" >&2
    else
        echo "    journalctl -u acc-agent -n 50" >&2
    fi
    echo "    Manual recovery: nohup ${AGENT_DEST} supervise &" >&2
    exit 1
fi
