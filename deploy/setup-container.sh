#!/usr/bin/env bash
# setup-container.sh — Bootstrap CCC agent in a container environment
# Use this instead of setup-node.sh when running inside Kasm, Docker, or any
# container where systemd and crontab are unavailable.
#
# Usage:
#   bash deploy/setup-container.sh
#   (run from inside the cloned repo, or after cloning to ~/.ccc/workspace)
#
# What it does:
#   1. Detects that we're actually in a container (exits if not)
#   2. Symlinks ~/.ccc/workspace → this repo (if not already set up)
#   3. Creates ~/.ccc/ccc-pull-loop.sh (a while-true pull loop)
#   4. Registers ccc-pull-loop with supervisord (or falls back to nohup)
#   5. Registers ccc-exec-listener (SquirrelBus remote exec) with supervisord
#   6. Starts a 'claude-main' tmux session with Claude Code
#   7. Pre-creates the log file
#   8. Prints a summary
#
# Idempotent: safe to run more than once.

set -euo pipefail

CCC_DIR="$HOME/.ccc"
WORKSPACE="$CCC_DIR/workspace"
PULL_LOOP="$CCC_DIR/ccc-pull-loop.sh"
EXEC_LISTENER="$CCC_DIR/ccc-exec-listener.sh"
OPENCLAW_WRAPPER="$CCC_DIR/ccc-openclaw-gateway.sh"
MEMORY_SYNC="$CCC_DIR/ccc-memory-sync.sh"
LOG_DIR="$CCC_DIR/logs"
LOG_FILE="$LOG_DIR/pull.log"
EXEC_LOG="$LOG_DIR/exec-listener.log"
OPENCLAW_LOG="$LOG_DIR/openclaw-gateway.log"
SUPERVISORD_CONF="/etc/supervisord.conf"
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Colors ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${BLUE}[container-setup]${NC} $1"; }
success() { echo -e "${GREEN}[container-setup]${NC} ✓ $1"; }
warn()    { echo -e "${YELLOW}[container-setup]${NC} ⚠ $1"; }
error()   { echo -e "${RED}[container-setup]${NC} ✗ $1" >&2; exit 1; }

echo ""
echo "🐿️  CCC Agent — Container Setup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── Step 1: Container detection ─────────────────────────────────────────────
# PID 1 in a container is typically supervisord, docker-init, tini, or a shell.
# On a real host with systemd it will be /sbin/init or /lib/systemd/systemd.
info "Detecting environment..."

PID1_COMM=""
if [ -r /proc/1/comm ]; then
  PID1_COMM="$(cat /proc/1/comm)"
fi

PID1_EXE=""
if [ -L /proc/1/exe ]; then
  PID1_EXE="$(readlink -f /proc/1/exe 2>/dev/null || true)"
fi

IS_CONTAINER=false

# Check for explicit container markers
if [ -f /.dockerenv ] || [ -f /run/.containerenv ]; then
  IS_CONTAINER=true
  info "Container marker found (/.dockerenv or /run/.containerenv)"
fi

# Check PID 1 — if it's not systemd or init it's almost certainly a container
case "$PID1_COMM" in
  supervisord|supervisord-*|docker-init|tini|dumb-init|s6-svscan|runit|openrc-init|bash|sh)
    IS_CONTAINER=true
    info "PID 1 is '$PID1_COMM' — container confirmed"
    ;;
  systemd|init)
    # Could be a systemd container, but probably a real host
    if [ "$IS_CONTAINER" = false ]; then
      info "PID 1 is '$PID1_COMM' — looks like a regular host"
    fi
    ;;
  *)
    info "PID 1 comm: '${PID1_COMM:-unknown}' (exe: ${PID1_EXE:-unknown})"
    ;;
esac

# Final verdict
if [ "$IS_CONTAINER" = false ]; then
  echo ""
  echo "  This does not appear to be a container environment."
  echo "  PID 1: ${PID1_COMM:-unknown} (${PID1_EXE:-unknown})"
  echo ""
  echo "  For regular Linux hosts, use setup-node.sh instead:"
  echo "    bash $REPO_DIR/deploy/setup-node.sh"
  echo ""
  echo "  To force container setup anyway, re-run with:"
  echo "    FORCE_CONTAINER=1 bash deploy/setup-container.sh"
  echo ""
  if [ "${FORCE_CONTAINER:-0}" != "1" ]; then
    exit 0
  fi
  warn "FORCE_CONTAINER=1 set — proceeding anyway"
fi

success "Environment: container"

# ── Step 2: Symlink workspace ────────────────────────────────────────────────
info "Checking workspace symlink..."

mkdir -p "$CCC_DIR"

if [ -L "$WORKSPACE" ]; then
  CURRENT_TARGET="$(readlink -f "$WORKSPACE")"
  if [ "$CURRENT_TARGET" = "$REPO_DIR" ]; then
    success "Workspace already symlinked: $WORKSPACE -> $REPO_DIR"
  else
    warn "Workspace symlink exists but points elsewhere: $WORKSPACE -> $CURRENT_TARGET"
    warn "Expected: $REPO_DIR"
    warn "Skipping — update manually if needed: ln -sfn $REPO_DIR $WORKSPACE"
  fi
elif [ -d "$WORKSPACE" ]; then
  warn "$WORKSPACE exists as a real directory, not a symlink"
  warn "Skipping symlink creation. If this is wrong, remove it and re-run."
else
  ln -s "$REPO_DIR" "$WORKSPACE"
  success "Workspace symlinked: $WORKSPACE -> $REPO_DIR"
fi

# ── Step 3: Create pull loop script ─────────────────────────────────────────
info "Creating pull loop script..."

cat > "$PULL_LOOP" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
while true; do
  bash "$SCRIPT_DIR/workspace/deploy/agent-pull.sh" || true
  sleep 600
done
EOF

chmod +x "$PULL_LOOP"
success "Pull loop script: $PULL_LOOP"

# ── Step 4: Log file pre-creation ───────────────────────────────────────────
info "Pre-creating log files..."
mkdir -p "$LOG_DIR"
touch "$LOG_FILE" "$EXEC_LOG"
chown "$USER:$USER" "$LOG_FILE" "$EXEC_LOG" 2>/dev/null || true
success "Log files: $LOG_FILE, $EXEC_LOG"

# ── Step 5a: Create exec-listener wrapper script ─────────────────────────────
info "Creating exec-listener wrapper script..."

cat > "$EXEC_LISTENER" << 'EOF'
#!/usr/bin/env bash
# ccc-exec-listener.sh — starts agent-listener.mjs with env from ~/.ccc/.env
set -euo pipefail
ENV_FILE="$HOME/.ccc/.env"
if [ -f "$ENV_FILE" ]; then
  set -a; source "$ENV_FILE"; set +a
fi
WORKSPACE="$HOME/.ccc/workspace"
exec node "$WORKSPACE.ccc/exec/agent-listener.mjs"
EOF

chmod +x "$EXEC_LISTENER"
success "Exec listener wrapper: $EXEC_LISTENER"

# ── Step 5b: Supervisord integration (or nohup fallback) ─────────────────────
PULL_REGISTERED=false
EXEC_REGISTERED=false

if [ -f "$SUPERVISORD_CONF" ]; then
  info "supervisord.conf found — registering programs..."

  # Register ccc-agent-pull
  if grep -q "\[program:ccc-agent-pull\]" "$SUPERVISORD_CONF" 2>/dev/null; then
    success "ccc-agent-pull already in supervisord.conf — skipping"
    PULL_REGISTERED=true
  else
    sudo tee -a "$SUPERVISORD_CONF" > /dev/null << EOF

[program:ccc-agent-pull]
command=$PULL_LOOP
autostart=true
autorestart=true
startsecs=5
startretries=3
stdout_logfile=$LOG_FILE
stderr_logfile=$LOG_FILE
stdout_logfile_maxbytes=1MB
stdout_logfile_backups=1
user=$USER
EOF
    success "ccc-agent-pull block appended to $SUPERVISORD_CONF"
    PULL_REGISTERED=true
  fi

  # Register ccc-exec-listener
  if grep -q "\[program:ccc-exec-listener\]" "$SUPERVISORD_CONF" 2>/dev/null; then
    success "ccc-exec-listener already in supervisord.conf — skipping"
    EXEC_REGISTERED=true
  else
    sudo tee -a "$SUPERVISORD_CONF" > /dev/null << EOF

[program:ccc-exec-listener]
command=$EXEC_LISTENER
autostart=true
autorestart=true
startsecs=5
startretries=10
stdout_logfile=$EXEC_LOG
stderr_logfile=$EXEC_LOG
stdout_logfile_maxbytes=2MB
stdout_logfile_backups=2
user=$USER
EOF
    success "ccc-exec-listener block appended to $SUPERVISORD_CONF"
    EXEC_REGISTERED=true
  fi

  # Reload supervisord once for both changes
  info "Reloading supervisord..."
  sudo supervisorctl -c "$SUPERVISORD_CONF" reread && \
  sudo supervisorctl -c "$SUPERVISORD_CONF" update && \
  success "supervisord reloaded — ccc-agent-pull + ccc-exec-listener running" || \
  warn "supervisorctl update failed — you may need to restart supervisord manually"

else
  info "supervisord.conf not found — falling back to nohup background processes"

  # Pull loop
  if pgrep -f "ccc-pull-loop.sh" > /dev/null 2>&1; then
    success "ccc-pull-loop.sh already running (PID: $(pgrep -f ccc-pull-loop.sh | head -1))"
    PULL_REGISTERED=true
  else
    nohup bash "$PULL_LOOP" >> "$LOG_FILE" 2>&1 &
    PULL_PID=$!
    sleep 1
    if kill -0 "$PULL_PID" 2>/dev/null; then
      success "Pull loop started via nohup (PID: $PULL_PID)"
      echo "$PULL_PID" > "$CCC_DIR/pull-loop.pid"
    else
      warn "Pull loop may have exited immediately — check $LOG_FILE"
    fi
    PULL_REGISTERED=true
  fi

  # Exec listener
  if pgrep -f "agent-listener.mjs" > /dev/null 2>&1; then
    success "agent-listener.mjs already running (PID: $(pgrep -f agent-listener.mjs | head -1))"
    EXEC_REGISTERED=true
  else
    nohup bash "$EXEC_LISTENER" >> "$EXEC_LOG" 2>&1 &
    EXEC_PID=$!
    sleep 1
    if kill -0 "$EXEC_PID" 2>/dev/null; then
      success "Exec listener started via nohup (PID: $EXEC_PID)"
      echo "$EXEC_PID" > "$CCC_DIR/exec-listener.pid"
    else
      warn "Exec listener may have exited — check $EXEC_LOG (SQUIRRELBUS_TOKEN required in .env)"
    fi
    EXEC_REGISTERED=true
  fi
fi

# ── Step 6: SSH reverse tunnel to do-host1 ──────────────────────────────────
# This opens an SSH reverse tunnel so Rocky can SSH back into this container
# even though horde-dgxc.nvidia.com is unreachable from do-host1/puck.
# The tunnel exposes this container's SSH port (22) as a port on do-host1 localhost.
# Rocky then has an always-available shell door: ssh -p <port> localhost (on do-host1).
info "Setting up SSH reverse shell tunnel to do-host1..."

# Load .env to get AGENT_NAME and any tunnel config
ENV_FILE="$HOME/.ccc/.env"
if [[ -f "$ENV_FILE" ]]; then
  set -a; source "$ENV_FILE"; set +a 2>/dev/null || true
fi

TUNNEL_HOST="${TUNNEL_HOST:-146.190.134.110}"
TUNNEL_IDENTITY="$HOME/.ssh/ccc-tunnel-key"
TUNNEL_LOG="$LOG_DIR/ssh-tunnel.log"
touch "$TUNNEL_LOG" 2>/dev/null || true
SHELL_TUNNEL_PORT=""

# Generate a per-agent SSH key if we don't already have one
if [[ ! -f "$TUNNEL_IDENTITY" ]]; then
  info "Generating SSH tunnel key..."
  ssh-keygen -t ed25519 -f "$TUNNEL_IDENTITY" -N "" -C "${AGENT_NAME:-unknown}-shell-tunnel" 2>/dev/null
  success "SSH tunnel key generated: $TUNNEL_IDENTITY"
fi

# Register with CCC — get port assignment + auto-authorize the key
# Uses /api/tunnel/request which allocates a port and writes to authorized_keys
PUBKEY=$(cat "${TUNNEL_IDENTITY}.pub" 2>/dev/null || echo "")
if [[ -n "$PUBKEY" ]]; then
  info "Registering shell tunnel key with CCC..."
  TUNNEL_RESP=$(curl -sf -X POST "${CCC_URL:-http://146.190.134.110:8789}/api/tunnel/shell" \
    -H "Authorization: Bearer ${CCC_AGENT_TOKEN:-}" \
    -H "Content-Type: application/json" \
    -d "{\"pubkey\":\"${PUBKEY}\",\"agent\":\"${AGENT_NAME:-unknown}\",\"label\":\"${AGENT_NAME:-unknown}-shell-tunnel\"}" 2>/dev/null || echo "")
  
  if [[ -n "$TUNNEL_RESP" ]]; then
    SHELL_TUNNEL_PORT=$(echo "$TUNNEL_RESP" | node -e "process.stdin.setEncoding('utf8');let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{try{const p=JSON.parse(d);console.log(p.port||'')}catch(e){}}" 2>/dev/null || echo "")
    KEY_WRITTEN=$(echo "$TUNNEL_RESP" | node -e "process.stdin.setEncoding('utf8');let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{try{const p=JSON.parse(d);console.log(p.keyWritten?'yes':'no')}catch(e){console.log('no')}}" 2>/dev/null || echo "no")
    TUNNEL_USER_REMOTE=$(echo "$TUNNEL_RESP" | node -e "process.stdin.setEncoding('utf8');let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{try{const p=JSON.parse(d);console.log(p.user||'tunnel')}catch(e){console.log('tunnel')}}" 2>/dev/null || echo "tunnel")
    
    if [[ -n "$SHELL_TUNNEL_PORT" ]]; then
      success "Shell tunnel port assigned: ${SHELL_TUNNEL_PORT} (key authorized: ${KEY_WRITTEN})"
    else
      warn "CCC response missing port — will retry on next setup run. Response: ${TUNNEL_RESP:0:100}"
    fi
  else
    warn "Could not reach CCC tunnel API — tunnel will be configured but may not connect until key is authorized"
    TUNNEL_USER_REMOTE="tunnel"
  fi
else
  warn "Could not read tunnel public key — skipping CCC registration"
  TUNNEL_USER_REMOTE="tunnel"
fi

# If CCC didn't give us a port, use a deterministic fallback based on agent name
if [[ -z "$SHELL_TUNNEL_PORT" ]]; then
  case "${AGENT_NAME:-}" in
    peabody) SHELL_TUNNEL_PORT=19080 ;;
    snidely) SHELL_TUNNEL_PORT=19081 ;;
    sherman) SHELL_TUNNEL_PORT=19082 ;;
    dudley)  SHELL_TUNNEL_PORT=19083 ;;
    boris)   SHELL_TUNNEL_PORT=19084 ;;
    *)       SHELL_TUNNEL_PORT=19099 ;;
  esac
  warn "Using fallback shell tunnel port: ${SHELL_TUNNEL_PORT}"
fi

TUNNEL_SCRIPT="$HOME/.ccc/ccc-ssh-tunnel.sh"
cat > "$TUNNEL_SCRIPT" << TUNEOF
#!/usr/bin/env bash
# ccc-ssh-tunnel.sh — Maintains reverse SSH shell tunnel to do-host1
# Exposes this container's sshd as do-host1 localhost:${SHELL_TUNNEL_PORT}
# Rocky can then SSH in: ssh -p ${SHELL_TUNNEL_PORT} ${USER:-horde}@localhost (from do-host1)

IDENTITY="${TUNNEL_IDENTITY}"
REMOTE="${TUNNEL_USER_REMOTE:-tunnel}@${TUNNEL_HOST}"
PORT="${SHELL_TUNNEL_PORT}"
LOG="${TUNNEL_LOG}"

echo "\$(date -u) [ssh-tunnel] Starting reverse shell tunnel :${SHELL_TUNNEL_PORT} → localhost:22" >> "\$LOG"

while true; do
  ssh -o StrictHostKeyChecking=no \
      -o ServerAliveInterval=30 \
      -o ServerAliveCountMax=3 \
      -o ExitOnForwardFailure=yes \
      -o BatchMode=yes \
      -i "\$IDENTITY" \
      -N \
      -R "${SHELL_TUNNEL_PORT}:localhost:22" \
      "\$REMOTE" >> "\$LOG" 2>&1
  EXIT_CODE=\$?
  echo "\$(date -u) [ssh-tunnel] Tunnel exited (code \$EXIT_CODE) — reconnecting in 15s" >> "\$LOG"
  sleep 15
done
TUNEOF
chmod +x "$TUNNEL_SCRIPT"
success "SSH tunnel script written: $TUNNEL_SCRIPT (port ${SHELL_TUNNEL_PORT})"

# Register tunnel with supervisord if available, else nohup
if [[ -f "$SUPERVISORD_CONF" ]]; then
  if grep -q "\[program:ccc-ssh-tunnel\]" "$SUPERVISORD_CONF" 2>/dev/null; then
    success "ccc-ssh-tunnel already in supervisord.conf — skipping"
  else
    sudo tee -a "$SUPERVISORD_CONF" > /dev/null << SUPEOF

[program:ccc-ssh-tunnel]
command=${TUNNEL_SCRIPT}
autostart=true
autorestart=true
startsecs=10
startretries=999
stdout_logfile=${TUNNEL_LOG}
stderr_logfile=${TUNNEL_LOG}
stdout_logfile_maxbytes=1MB
stdout_logfile_backups=1
user=${USER}
SUPEOF
    success "ccc-ssh-tunnel registered with supervisord"
    sudo supervisorctl -c "$SUPERVISORD_CONF" reread 2>/dev/null && \
    sudo supervisorctl -c "$SUPERVISORD_CONF" update 2>/dev/null || \
    warn "supervisorctl reload failed — restart supervisord manually"
  fi
else
  # nohup fallback
  if pgrep -f "ccc-ssh-tunnel.sh" > /dev/null 2>&1; then
    success "ccc-ssh-tunnel.sh already running"
  else
    nohup bash "$TUNNEL_SCRIPT" >> "$TUNNEL_LOG" 2>&1 &
    SSH_PID=$!
    sleep 2
    if kill -0 "$SSH_PID" 2>/dev/null; then
      success "SSH tunnel started via nohup (PID: $SSH_PID)"
      echo "$SSH_PID" > "$CCC_DIR/ssh-tunnel.pid"
    else
      warn "SSH tunnel may not have started — check $TUNNEL_LOG"
      warn "Likely cause: tunnel key not yet authorized on do-host1"
    fi
  fi
fi

# ── Step 6b: Tailscale (userspace networking — containers only) ──────────────
# Standard Tailscale requires CAP_NET_ADMIN + a kernel TUN device — both are
# unavailable in most containers.  tailscaled --tun=userspace-networking works
# anywhere because it implements the WireGuard/TUN stack entirely in userspace.
#
# Detection: IS_CONTAINER=true was established above (PID 1 is supervisord /
# docker-init / tini, or /.dockerenv / /run/.containerenv was found).  We never
# rely on knowing the host name; the same logic fires for any outbound-only
# container regardless of where it lives.
#
# Full VMs (systemd PID 1, no containerenv) use setup-node.sh instead, which
# installs Tailscale with normal kernel TUN via the standard Tailscale package.
info "Setting up Tailscale (userspace networking for container)..."

TS_DIR="$HOME/.tailscale"
TS_SOCK="$TS_DIR/tailscaled.sock"
TS_LOG="$LOG_DIR/tailscaled.log"
TS_LOGIN_SERVER="${TS_LOGIN_SERVER:-https://vpn.mass-hysteria.org}"
TS_AUTHKEY="${TS_AUTHKEY:-}"

mkdir -p "$TS_DIR"
touch "$TS_LOG" 2>/dev/null || true

# Install tailscale/tailscaled binaries if missing
if ! command -v tailscale &>/dev/null || ! command -v tailscaled &>/dev/null; then
  info "Installing Tailscale..."
  if command -v curl &>/dev/null; then
    curl -fsSL https://tailscale.com/install.sh | sh 2>/dev/null && \
      success "Tailscale installed" || \
      warn "Tailscale auto-install failed — install manually: https://tailscale.com/download/linux"
  else
    warn "curl not found — install Tailscale manually: https://tailscale.com/download/linux"
  fi
else
  success "Tailscale binaries present ($(tailscale version 2>/dev/null | head -1 || echo 'version unknown'))"
fi

# Locate supervisord drop-in directory (preferred) or fall back to main conf
SUPERVISORD_CONFD=""
for _dir in "/etc/supervisor/conf.d" "/etc/supervisord.d"; do
  if [ -d "$_dir" ]; then
    SUPERVISORD_CONFD="$_dir"
    break
  fi
done

TS_REGISTERED=false

if [ -n "$SUPERVISORD_CONFD" ]; then
  TS_CONF_FILE="$SUPERVISORD_CONFD/tailscaled.conf"
  if [ -f "$TS_CONF_FILE" ]; then
    success "tailscaled supervisord conf already exists: $TS_CONF_FILE — skipping"
    TS_REGISTERED=true
  else
    sudo tee "$TS_CONF_FILE" > /dev/null << TSEOF
[program:tailscaled]
command=tailscaled --tun=userspace-networking --socket=${TS_SOCK} --statedir=${TS_DIR}
user=${USER}
environment=HOME="${HOME}",TS_LOGIN_SERVER="${TS_LOGIN_SERVER}"
stdout_logfile=${TS_LOG}
stdout_logfile_maxbytes=5MB
stdout_logfile_backups=0
redirect_stderr=true
autostart=true
autorestart=true
priority=5
TSEOF
    success "tailscaled conf written: $TS_CONF_FILE"
    TS_REGISTERED=true
    sudo supervisorctl -c "$SUPERVISORD_CONF" reread 2>/dev/null && \
    sudo supervisorctl -c "$SUPERVISORD_CONF" update 2>/dev/null || \
    warn "supervisorctl reload failed — restart supervisord manually to start tailscaled"
  fi
elif [ -f "$SUPERVISORD_CONF" ]; then
  if grep -q "\[program:tailscaled\]" "$SUPERVISORD_CONF" 2>/dev/null; then
    success "tailscaled already in $SUPERVISORD_CONF — skipping"
    TS_REGISTERED=true
  else
    sudo tee -a "$SUPERVISORD_CONF" > /dev/null << TSEOF

[program:tailscaled]
command=tailscaled --tun=userspace-networking --socket=${TS_SOCK} --statedir=${TS_DIR}
user=${USER}
environment=HOME="${HOME}",TS_LOGIN_SERVER="${TS_LOGIN_SERVER}"
stdout_logfile=${TS_LOG}
stdout_logfile_maxbytes=5MB
stdout_logfile_backups=0
redirect_stderr=true
autostart=true
autorestart=true
priority=5
TSEOF
    success "tailscaled appended to $SUPERVISORD_CONF"
    TS_REGISTERED=true
    sudo supervisorctl -c "$SUPERVISORD_CONF" reread 2>/dev/null && \
    sudo supervisorctl -c "$SUPERVISORD_CONF" update 2>/dev/null || \
    warn "supervisorctl reload failed — restart supervisord manually"
  fi
fi

# Fallback: no supervisord config found at all — nohup
if [ "$TS_REGISTERED" = false ]; then
  warn "No supervisord config found — starting tailscaled via nohup"
  if pgrep -f "tailscaled" > /dev/null 2>&1; then
    success "tailscaled already running"
  else
    nohup tailscaled --tun=userspace-networking --socket="$TS_SOCK" --statedir="$TS_DIR" \
      >> "$TS_LOG" 2>&1 &
    sleep 2
    success "tailscaled started via nohup"
  fi
fi

# Wait up to 10s for tailscaled to become responsive, then run tailscale up
_ts_wait=0
while [ "$_ts_wait" -lt 10 ]; do
  tailscale --socket="$TS_SOCK" status &>/dev/null 2>&1 && break
  sleep 1
  _ts_wait=$((_ts_wait + 1))
done

if tailscale --socket="$TS_SOCK" status &>/dev/null 2>&1; then
  TS_BACKEND=$(tailscale --socket="$TS_SOCK" status --json 2>/dev/null \
    | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('BackendState',''))" \
    2>/dev/null || echo "")
  if [ "$TS_BACKEND" = "Running" ]; then
    TS_IP=$(tailscale --socket="$TS_SOCK" ip -4 2>/dev/null || echo "unknown")
    success "Tailscale connected (IP: $TS_IP, login server: $TS_LOGIN_SERVER)"
  else
    info "Running tailscale up (login server: $TS_LOGIN_SERVER)..."
    TS_UP_CMD="tailscale --socket=${TS_SOCK} up --login-server=${TS_LOGIN_SERVER}"
    [ -n "$TS_AUTHKEY" ] && TS_UP_CMD="$TS_UP_CMD --authkey=${TS_AUTHKEY}"
    $TS_UP_CMD 2>&1 | tee -a "$TS_LOG" || \
      warn "tailscale up incomplete — if an auth URL was printed, visit it to authorize"
    warn "  Or run manually: tailscale --socket=${TS_SOCK} up --login-server=${TS_LOGIN_SERVER}"
    [ -z "$TS_AUTHKEY" ] && \
      warn "  For unattended setup, set TS_AUTHKEY in ~/.ccc/.env and re-run this script"
  fi
else
  warn "tailscaled not yet responding — supervisord will start it on next boot"
  warn "  Then run: tailscale --socket=${TS_SOCK} up --login-server=${TS_LOGIN_SERVER}"
fi

# ── Step 7: Claude tmux session ─────────────────────────────────────────────
info "Checking claude-main tmux session..."

if ! command -v tmux &>/dev/null; then
  warn "tmux not found — skipping claude-main session"
  warn "Install with: sudo apt-get install -y tmux"
else
  if tmux has-session -t claude-main 2>/dev/null; then
    success "tmux session 'claude-main' already exists — skipping"
  else
    info "Creating tmux session 'claude-main'..."
    tmux new-session -d -s claude-main

    sleep 1

    # Start Claude Code with permissions bypass
    tmux send-keys -t claude-main "claude --dangerously-skip-permissions" ""

    sleep 1

    # NOTE: The Down arrow must be sent as a separate tmux send-keys call.
    # Combining it with the command string doesn't work — tmux only processes
    # key names (like "Down") when they are the sole argument to send-keys.
    tmux send-keys -t claude-main "Down" ""

    sleep 0.5

    tmux send-keys -t claude-main "" Enter

    success "claude-main tmux session started"
    echo ""
    echo "  Note: The Down arrow key is sent as a separate tmux call."
    echo "  This is intentional — tmux requires key names to be sent alone."
    echo "  If Claude Code doesn't start correctly, run:"
    echo "    tmux attach -t claude-main"
  fi
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "🐿️  ${GREEN}Container setup complete!${NC}"
echo ""
echo "  Workspace:      $WORKSPACE"
echo "  Pull loop:      $PULL_LOOP"
echo "  Exec listener:  $EXEC_LISTENER"
echo "  SSH tunnel:     $TUNNEL_SCRIPT (port ${SHELL_TUNNEL_PORT:-?} → do-host1)"
echo "  Tailscale:      $TS_DIR  (login: $TS_LOGIN_SERVER)"
echo "  Pull log:       $LOG_FILE"
echo "  Exec log:       $EXEC_LOG"
echo "  Tunnel log:     $TUNNEL_LOG"
echo "  Tailscale log:  $TS_LOG"
echo ""

if [ -f "$SUPERVISORD_CONF" ] || [ -n "$SUPERVISORD_CONFD" ]; then
  echo "  Process manager: supervisord"
  echo "  Programs:        ccc-agent-pull, ccc-exec-listener, ccc-ssh-tunnel, tailscaled"
  echo "  Check:           sudo supervisorctl -c $SUPERVISORD_CONF status"
else
  echo "  Process manager: nohup background"
  echo "  Check pull:      pgrep -fa ccc-pull-loop"
  echo "  Check exec:      pgrep -fa agent-listener"
  echo "  Check tunnel:    pgrep -fa ccc-ssh-tunnel"
  echo "  Check tailscale: pgrep -fa tailscaled"
  echo "  PID files:       $CCC_DIR/pull-loop.pid, $CCC_DIR/exec-listener.pid, $CCC_DIR/ssh-tunnel.pid"
fi

echo ""
echo "  Verify these manually:"
echo "  1. ~/.ccc/.env exists with AGENT_NAME, CCC_URL, CCC_AGENT_TOKEN, SQUIRRELBUS_TOKEN"
echo "  2. Pull loop running:      tail -f $LOG_FILE"
echo "  3. Exec listener running:  tail -f $EXEC_LOG"
echo "  4. SSH tunnel:             tail -f $TUNNEL_LOG (needs key authorized on do-host1)"
echo "  5. Tailscale status:       tailscale --socket=${TS_SOCK} status"
echo "  6. Claude session:         tmux attach -t claude-main"
echo ""
echo "  From do-host1, once tunnel key is authorized:"
echo "    ssh -p ${SHELL_TUNNEL_PORT:-?} ${USER:-horde}@localhost"
echo ""
echo "  If .env is missing:"
echo "    cp $REPO_DIR/deploy/.env.template ~/.ccc/.env"
echo "    nano ~/.ccc/.env"
echo ""
echo "  Required .env keys for exec listener:"
echo "    SQUIRRELBUS_TOKEN   — shared bus secret (get from Rocky/CCC)"
echo "    SQUIRRELBUS_URL     — http://146.190.134.110:8788"
echo "    CCC_URL             — http://146.190.134.110:8789"
echo "    CCC_AGENT_TOKEN     — your agent bearer token"
echo "    AGENT_NAME          — your agent name (peabody, sherman, etc.)"
echo ""
echo "  Optional .env keys for Tailscale:"
echo "    TS_LOGIN_SERVER     — coordination server (default: https://vpn.mass-hysteria.org)"
echo "    TS_AUTHKEY          — pre-auth key for unattended join (generate in Headscale admin)"
echo ""
