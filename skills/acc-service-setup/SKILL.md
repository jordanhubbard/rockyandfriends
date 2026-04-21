---
name: acc-service-setup
description: Install and manage ACC agent services (acc-bus-listener, acc-queue-worker, acc-nvidia-proxy) on any init system — systemd (system or user), launchd, or supervisord. Use when setting up a new agent or fixing broken services.
version: 1.0.0
platforms: [linux, macos]
metadata:
  hermes:
    tags: [acc, services, systemd, launchd, supervisord]
    category: infrastructure
---

# ACC Service Setup

## Determine the init system first

Before installing anything, identify what init system the agent runs:

```bash
ps -p 1 -o comm=
```

| Output | Init system | Notes |
|---|---|---|
| `systemd` | systemd | Check if agent uses system or user systemd (see below) |
| `supervisord` | supervisord | Common in containers (Boris) |
| `launchd` | launchd | macOS (Bullwinkle) |

### System vs user systemd

```bash
# Does the agent have passwordless sudo?
sudo -n true 2>/dev/null && echo "has sudo" || echo "no sudo"

# If no sudo, they must use user systemd
systemctl --user status 2>/dev/null | head -3
```

Agents **without passwordless sudo** (e.g. natasha) must use `systemctl --user`.
User units live in `~/.config/systemd/user/`, not `/etc/systemd/system/`.

---

## Service unit templates

### Variables used below

```
ACC_BIN   = ~/.acc/bin/acc-agent
ACC_DIR   = ~/.acc
ACC_ENV   = ~/.acc/.env
USER_NAME = the agent's login user (e.g. jkh, horde)
```

---

## A. systemd — system-wide (has sudo)

```bash
sudo tee /etc/systemd/system/acc-bus-listener.service > /dev/null << 'EOF'
[Unit]
Description=ACC Bus Listener (AgentBus SSE daemon)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=USER_NAME
EnvironmentFile=-/home/USER_NAME/.acc/.env
Environment=PATH=/home/USER_NAME/.local/bin:/home/USER_NAME/.acc/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=/home/USER_NAME/.acc/bin/acc-agent bus
Restart=always
RestartSec=5s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

sudo tee /etc/systemd/system/acc-queue-worker.service > /dev/null << 'EOF'
[Unit]
Description=ACC Queue Worker (autonomous task executor)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=USER_NAME
EnvironmentFile=-/home/USER_NAME/.acc/.env
Environment=PATH=/home/USER_NAME/.local/bin:/home/USER_NAME/.acc/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=/home/USER_NAME/.acc/bin/acc-agent queue
Restart=always
RestartSec=10s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now acc-bus-listener acc-queue-worker
```

---

## B. systemd — user mode (no sudo)

**Critical differences from system mode:**
- Unit files go in `~/.config/systemd/user/`
- **No `User=` directive** (you are already that user)
- `WantedBy=default.target` not `multi-user.target`
- All commands use `systemctl --user`
- Use `loginctl enable-linger` so units survive logout

```bash
mkdir -p ~/.config/systemd/user

cat > ~/.config/systemd/user/acc-bus-listener.service << 'EOF'
[Unit]
Description=ACC Bus Listener (AgentBus SSE daemon)
After=network-online.target

[Service]
Type=simple
EnvironmentFile=-%h/.acc/.env
Environment=PATH=%h/.local/bin:%h/.acc/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=%h/.acc/bin/acc-agent bus
Restart=always
RestartSec=5s

[Install]
WantedBy=default.target
EOF

cat > ~/.config/systemd/user/acc-queue-worker.service << 'EOF'
[Unit]
Description=ACC Queue Worker (autonomous task executor)
After=network-online.target

[Service]
Type=simple
EnvironmentFile=-%h/.acc/.env
Environment=PATH=%h/.local/bin:%h/.acc/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=%h/.acc/bin/acc-agent queue
Restart=always
RestartSec=10s

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now acc-bus-listener acc-queue-worker
loginctl enable-linger "$USER"
```

`%h` expands to the home directory in user unit files.

---

## C. supervisord (containers, Boris)

Supervisor runs as root; services declare `user=` inside the conf.

```bash
sudo tee /etc/supervisor/conf.d/acc-agent.conf > /dev/null << 'EOF'
[program:acc-bus-listener]
command=/home/USER_NAME/.acc/bin/acc-agent bus
directory=/home/USER_NAME/.acc/workspace
user=USER_NAME
autostart=true
autorestart=true
startsecs=5
stopwaitsecs=10
stdout_logfile=/home/USER_NAME/.acc/logs/acc-bus-listener.log
stderr_logfile=/home/USER_NAME/.acc/logs/acc-bus-listener.log
environment=HOME="/home/USER_NAME",PATH="/home/USER_NAME/.local/bin:/home/USER_NAME/.acc/bin:/usr/local/bin:/usr/bin:/bin"

[program:acc-queue-worker]
command=/home/USER_NAME/.acc/bin/acc-agent queue
directory=/home/USER_NAME/.acc/workspace
user=USER_NAME
autostart=true
autorestart=true
startsecs=10
stopwaitsecs=10
stdout_logfile=/home/USER_NAME/.acc/logs/acc-queue-worker.log
stderr_logfile=/home/USER_NAME/.acc/logs/acc-queue-worker.log
environment=HOME="/home/USER_NAME",PATH="/home/USER_NAME/.local/bin:/home/USER_NAME/.acc/bin:/usr/local/bin:/usr/bin:/bin"
EOF

sudo supervisorctl reread && sudo supervisorctl update
sudo supervisorctl status acc-bus-listener acc-queue-worker
```

---

## D. launchd (macOS, Bullwinkle)

```bash
mkdir -p ~/Library/LaunchAgents

cat > ~/Library/LaunchAgents/com.acc.bus-listener.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>             <string>com.acc.bus-listener</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/USER_NAME/.acc/bin/acc-agent</string>
    <string>bus</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>  <string>/Users/USER_NAME</string>
    <key>PATH</key>  <string>/Users/USER_NAME/.local/bin:/Users/USER_NAME/.acc/bin:/usr/local/bin:/usr/bin:/bin</string>
  </dict>
  <key>WorkingDirectory</key>  <string>/Users/USER_NAME/.acc/workspace</string>
  <key>KeepAlive</key>         <true/>
  <key>StandardOutPath</key>   <string>/Users/USER_NAME/.acc/logs/acc-bus-listener.log</string>
  <key>StandardErrorPath</key> <string>/Users/USER_NAME/.acc/logs/acc-bus-listener.log</string>
</dict>
</plist>
EOF

cat > ~/Library/LaunchAgents/com.acc.queue-worker.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>             <string>com.acc.queue-worker</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/USER_NAME/.acc/bin/acc-agent</string>
    <string>queue</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>  <string>/Users/USER_NAME</string>
    <key>PATH</key>  <string>/Users/USER_NAME/.local/bin:/Users/USER_NAME/.acc/bin:/usr/local/bin:/usr/bin:/bin</string>
  </dict>
  <key>WorkingDirectory</key>  <string>/Users/USER_NAME/.acc/workspace</string>
  <key>KeepAlive</key>         <true/>
  <key>StandardOutPath</key>   <string>/Users/USER_NAME/.acc/logs/acc-queue-worker.log</string>
  <key>StandardErrorPath</key> <string>/Users/USER_NAME/.acc/logs/acc-queue-worker.log</string>
</dict>
</plist>
EOF

launchctl load ~/Library/LaunchAgents/com.acc.bus-listener.plist
launchctl load ~/Library/LaunchAgents/com.acc.queue-worker.plist
```

---

## Verify services are running

```bash
# systemd (system)
sudo systemctl status acc-bus-listener acc-queue-worker

# systemd (user)
systemctl --user status acc-bus-listener acc-queue-worker

# supervisord
sudo supervisorctl status acc-bus-listener acc-queue-worker

# launchd
launchctl list | grep com.acc
```

Then confirm the agent appears online in the fleet registry:

```bash
source ~/.acc/.env 2>/dev/null || source ~/.ccc/.env 2>/dev/null
curl -sf -H "Authorization: Bearer $ACC_AGENT_TOKEN" \
  "${ACC_URL}/api/agents/$AGENT_NAME" | python3 -m json.tool | grep -E "online|lastSeen"
```

## Common mistakes

- **`User=` in user-mode systemd**: Invalid directive in `~/.config/systemd/user/` — remove it entirely.
- **`WantedBy=multi-user.target` in user-mode systemd**: Units won't be enabled. Use `WantedBy=default.target`.
- **`acc-agent listen`**: Not a valid subcommand. Use `acc-agent bus` and `acc-agent queue` as separate programs.
- **Missing `loginctl enable-linger`**: User units stop when the user session ends. Always run this on user-mode installs.
- **`environment=` in supervisord not loading `.env`**: Supervisord `environment=` is a comma-separated list of `KEY="val"` pairs, not a shell source. The `.env` file must be explicitly parsed or the vars inlined.
