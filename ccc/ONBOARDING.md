# CCC Onboarding Guide — Claw Command Center

CCC is a lightweight, self-hosted coordination layer for multi-agent teams. It provides a shared work queue, agent heartbeat registry, lessons ledger, ClawBus messaging, and ClawChat — a Slack-compatible team chat. Any agent team can run their own CCC without Rocky, do-host1, or any specific agent topology.

---

## Prerequisites

| Requirement | Min Version | Notes |
|-------------|------------|-------|
| Rust toolchain | 1.75+ | Install via `rustup.rs`; includes `cargo` and `rustc` |
| Git | any | For repo sync |
| curl | any | For heartbeats + API calls |
| Tailscale | any | Mesh networking — all agents must join the tailnet |
| MinIO (optional) | any | For durable storage / ClawFS; runs without it |
| wasm32 target (optional) | — | For building ClawChat WASM: `rustup target add wasm32-unknown-unknown` |
| trunk (optional) | any | For building ClawChat WASM: `cargo install trunk` |

Node.js is **not required** — CCC runs as a single Rust binary (`ccc-server`).

---

## Quick Start

### 1. Clone the repo

```bash
git clone git@github.com:<your-org>/CCC.git ~/Src/CCC
```

### 2. Run setup

**Full VM (systemd):**
```bash
bash ~/Src/CCC/deploy/setup-node.sh
```

**Container (supervisord — Kasm, Docker, DGX Cloud, etc.):**
```bash
bash ~/Src/CCC/deploy/setup-container.sh
```

These scripts:
- Detect your platform (Linux/macOS) or container environment
- Create `~/.ccc/` directory structure and symlink workspace → repo
- Copy `.env.template` → `~/.ccc/.env` (first run only)
- Install pull cron / supervisord programs / macOS LaunchAgent
- Set up Tailscale (userspace networking in containers, kernel TUN on VMs)

### 3. Configure OpenClaw/Hermes gateway mode

Before starting, set local mode:

```bash
openclaw config set gateway.mode local
# or for Hermes:
hermes config set gateway.mode local
```

Required for agent routing. Automated by `/api/onboard` for managed setups.

### 4. Configure your `.env`

Edit `~/.ccc/.env`:

```bash
nano ~/.ccc/.env
```

**Required fields:**

```env
AGENT_NAME=myagent           # short lowercase name (becomes your identity)
AGENT_HOST=my-host.example.com

CCC_URL=http://your-ccc-host:8789
CCC_AGENT_TOKEN=             # filled in by register-agent.sh
CCC_AUTH_TOKENS=token1,token2

PRIMARY_AGENT=myagent
```

**Optional:**
```env
NVIDIA_API_KEY=              # for LLM inference
MINIO_ENDPOINT=http://...    # for durable storage
GITHUB_TOKEN=                # for scout (repo watching)
TELEGRAM_TOKEN=              # for Telegram alerts
TS_AUTHKEY=                  # Tailscale pre-auth key for unattended setup
```

### 5. Register this agent with CCC

```bash
bash ~/Src/CCC/deploy/register-agent.sh
```

### 6. Build and start ccc-server (hub node only)

```bash
cd ~/Src/CCC/ccc/dashboard
cargo build --release -p ccc-server

CCC_AUTH_TOKENS=your-secret-token-here \
./target/release/ccc-server
```

Or via systemd (installed by `setup-node.sh` on Linux):

```bash
sudo systemctl enable --now ccc-server
```

### 7. (Optional) Build and serve ClawChat

```bash
cd ~/Src/CCC/ccc/clawchat
trunk build --release          # outputs to dist/

# Serve from ccc-server:
DASHBOARD_DIST=dist ./target/release/ccc-server
```

---

## Configuration

### `~/.ccc/ccc.json` — Primary config

```json
{
  "port": 8789,
  "data_dir": "~/.ccc/data",
  "auth_tokens": ["token1", "token2"],
  "minio": {
    "endpoint": "http://localhost:9000",
    "bucket": "agents",
    "access_key": "minioadmin",
    "secret_key": "minioadmin"
  },
  "tokenhub": {
    "url": "http://127.0.0.1:8090"
  },
  "qdrant": {
    "url": "http://localhost:6333"
  }
}
```

### Environment Variables

All env vars override `ccc.json`.

**Required (or set in ccc.json):**
```env
CCC_AUTH_TOKENS=token1,token2    # Comma-separated valid Bearer tokens
CCC_PORT=8789                    # Server port (default 8789)
```

**Storage:**
```env
CCC_DATA_DIR=./data              # Data directory (default ./data)
CCC_DB_PATH=./data/ccc.db        # Optional SQLite path (uses JSON files if unset)
QUEUE_PATH=./data/queue.json
AGENTS_PATH=./data/agents.json
SECRETS_PATH=./data/secrets.json
BUS_LOG_PATH=./data/bus.jsonl
PROJECTS_PATH=./data/projects.json
```

**Integrations:**
```env
TOKENHUB_URL=http://127.0.0.1:8090
QDRANT_FLEET_URL=http://...
QDRANT_FLEET_KEY=...
MINIO_ENDPOINT=http://localhost:9000
MINIO_BUCKET=agents
MINIO_ACCESS_KEY=...
MINIO_SECRET_KEY=...
```

**Dashboard SPA:**
```env
DASHBOARD_DIST=/path/to/dist
CCC_CORS_ORIGINS=https://yourdomain
```

**Process supervision:**
```env
SUPERVISOR_ENABLED=true
TOKENHUB_BIN=/path/to/tokenhub
```

**vLLM (GPU nodes):**
```env
VLLM_ENABLED=true
VLLM_MODEL=google/gemma-4-31B-it
VLLM_SERVED_NAME=gemma
VLLM_PORT=8080
```

---

## Networking — Tailscale First

**All inter-agent communication uses Tailscale.** Every agent node joins the same tailnet and is reachable by its Tailscale IP. This replaces SSH tunnels for vLLM, tokenhub, and agent-to-agent traffic.

### Why Tailscale, not tunnels

SSH reverse tunnels were fragile: they die silently, ports stay bound after restart, each new GPU node needed a unique port allocation on the hub. Tailscale gives every node a stable IP — vLLM on `sherman` at `100.65.161.47:8080` is reachable from any fleet node directly.

### Container setup (DGX Cloud, Kasm, etc.)

Containers lack `CAP_NET_ADMIN`, so Tailscale runs in userspace networking mode:

```bash
tailscaled --tun=userspace-networking --socket=$HOME/.tailscale/tailscaled.sock --statedir=$HOME/.tailscale
```

`setup-container.sh` handles this automatically.

### Headscale coordination server

The fleet uses Headscale (`vpn.mass-hysteria.org`) as the coordination server. `TS_LOGIN_SERVER` is set in the supervisord conf:

```ini
environment=HOME="/home/horde",TS_LOGIN_SERVER="https://vpn.mass-hysteria.org"
```

### Rules

- **Agent services** (vLLM, tokenhub, CCC API, ClawBus) → Tailscale/localhost only
- **Human-facing services** (dashboard, web UIs) → public via Caddy
- Rule of thumb: **human=public, agent=Tailscale**

---

## vLLM — Local GPU Inference

GPU nodes run vLLM to serve models locally. Fleet standard: **gemma-4-31B-it** on 4× L40 GPUs, port **8080**.

### supervisord conf

```ini
[program:vllm]
command=/bin/bash -c 'exec /home/horde/.vllm-venv/bin/vllm serve /path/to/model \
  --kv-cache-dtype fp8 --tensor-parallel-size 4 --trust-remote-code \
  --served-model-name gemma --enable-auto-tool-choice \
  --tool-call-parser gemma4 --reasoning-parser gemma4 \
  --port 8080 --max-model-len 16384 --gpu-memory-utilization 0.90 \
  >> /tmp/vllm.log 2>&1'
user=horde
environment=HOME="/home/horde",XDG_CACHE_HOME="/tmp/xdg-cache",NCCL_IB_DISABLE="1",NCCL_P2P_DISABLE="1"
autostart=true
autorestart=true
startsecs=30
priority=50
```

### Verifying vLLM

```bash
# Local
curl -s http://127.0.0.1:8080/v1/models | python3 -m json.tool

# Remote via Tailscale
curl -s http://<tailscale-ip>:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gemma","messages":[{"role":"user","content":"hello"}],"max_tokens":20}'
```

The old `vllm-tunnel` supervisord program (SSH reverse tunnel) is **deprecated** — stop and disable it. Use the node's Tailscale IP directly.

---

## TokenHub — LLM Routing Proxy

TokenHub runs on the hub node (port 8090) and provides a unified OpenAI-compatible API routing to NVIDIA inference, vLLM on GPU nodes, Anthropic, etc.

### Registering a vLLM provider

Add to `~/.tokenhub/credentials` on the hub:

```json
{
  "id": "mynode-gemma",
  "type": "vllm",
  "base_url": "http://<tailscale-ip>:8080",
  "api_key": "",
  "models": [{ "id": "gemma-mynode", "upstream_id": "gemma" }]
}
```

`base_url` uses the **Tailscale IP**. After editing: `sudo systemctl restart tokenhub`.

All agents route through tokenhub (`http://127.0.0.1:8090/v1/...`) — **never call vLLM directly** from application code.

---

## Data Files

CCC stores data as JSON files by default. Paths are relative to `CCC_DATA_DIR`:

| File | What it stores |
|------|---------------|
| `data/queue.json` | Work queue (pending + completed items) |
| `data/agents.json` | Agent registry and capabilities |
| `data/secrets.json` | Key-value secrets store (chmod 600) |
| `data/projects.json` | Project registry |
| `data/bus.jsonl` | ClawBus message log (append-only JSONL) |

**SQLite mode** (optional): Set `CCC_DB_PATH` to a `.db` path. ccc-server will create the database and migrate existing JSON data on first start.

---

## Agent Registry (`data/agents.json`)

Agents are auto-registered via `POST /api/heartbeat/:name`. To manually add one:

```json
{
  "myagent": {
    "name": "myagent",
    "host": "my-host.example.com",
    "type": "full",
    "capabilities": {
      "claude_cli": true,
      "claude_cli_model": "claude-sonnet-4-6",
      "gpu": false
    },
    "token": "wq-<generated-token>"
  }
}
```

---

## Watched Repos (`data/projects.json`)

```bash
curl -X POST http://localhost:8789/api/repos \
  -H "Authorization: Bearer $CCC_AUTH_TOKENS" \
  -H "Content-Type: application/json" \
  -d '{"full_name": "yourorg/yourrepo"}'
```

---

## Registering a New Agent

1. Run `setup-node.sh` (VM) or `setup-container.sh` (container) on the new machine
2. Point `CCC_URL` at the hub's API
3. Run `register-agent.sh` with the admin token
4. The hub adds the agent to `data/agents.json` and issues a token
5. The new agent uses that token for all API calls

**GPU nodes — extra steps:**

6. Verify vLLM: `curl -s http://127.0.0.1:8080/v1/models`
7. Set `VLLM_ENABLED=true`, `VLLM_MODEL`, `VLLM_PORT=8080` in `~/.ccc/.env`
8. Add tokenhub provider entry using the node's Tailscale IP
9. Restart tokenhub on the hub: `sudo systemctl restart tokenhub`

---

## Remote Exec (ClawBus RCE)

Agents can be commanded remotely via ClawBus exec — no inbound SSH required. This is how Rocky manages GPU containers (peabody, sherman).

**Run the agent-listener daemon** on any node:

```bash
CLAWBUS_TOKEN=<token> \
CLAWBUS_URL=https://ccc.example.com \
CCC_URL=https://ccc.example.com \
CCC_AUTH_TOKEN=<agent-token> \
AGENT_NAME=mynode \
ALLOW_SHELL_EXEC=true \
node /opt/ccc/exec/agent-listener.mjs

# Or as a supervisord program / systemd service (see deploy/)
```

**Send a command:**

```bash
# JS mode (sandboxed vm):
curl -s -X POST https://ccc.example.com/api/exec \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"targets":["mynode"],"code":"Object.keys(process.env).length"}'

# Shell mode (pre-approved commands only):
curl -s -X POST https://ccc.example.com/api/exec \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"targets":["mynode"],"mode":"shell","code":"nvidia-smi --query-gpu=name,memory.used --format=csv,noheader"}'
```

See `ccc/docs/remote-exec.md` for full details, security model, and shell allowlist.

---

## Connecting to ClawBus

ClawBus is integrated into ccc-server (port 8789).

**Poll for messages:**
```bash
curl http://your-hub:8789/bus/messages?to=myagent&since=2026-01-01T00:00:00Z \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN"
```

**Send a message:**
```bash
curl -X POST http://your-hub:8789/bus/send \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"from":"myagent","to":"all","type":"text","subject":"#ops","body":"Hello!"}'
```

**Subscribe to SSE stream:**
```bash
curl http://your-hub:8789/bus/stream?token=$CCC_AUTH_TOKEN
```

See `clawbus/SPEC.md` for the full protocol.

---

## ClawChat

ClawChat is a Slack-compatible team chat UI built in Leptos/WASM. Connects to ClawBus for messaging, threads, reactions, and DMs.

**Build:**
```bash
cd ccc/clawchat && trunk build --release
```

**Serve via ccc-server:**
```bash
DASHBOARD_DIST=ccc/clawchat/dist ./target/release/ccc-server
```

Open `http://localhost:8789` — log in with any agent token.

---

## Lessons Ledger

**Record:**
```bash
curl -X POST http://localhost:8789/api/lessons \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"domain":"myapp","tags":["error"],"symptom":"what broke","fix":"what fixed it","agent":"myagent"}'
```

**Query (prepend to agent context):**
```bash
curl "http://localhost:8789/api/lessons?domain=myapp&q=my+query&format=context" \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN"
```

---

## Heartbeats

```bash
curl -X POST http://localhost:8789/api/heartbeat/myagent \
  -H "Authorization: Bearer $CCC_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"host":"my-host.example.com","status":"online"}'
```

`deploy/agent-pull.sh` does this automatically on each pull cycle.

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `{"error":"Unauthorized"}` | Check `CCC_AUTH_TOKENS` env var and your Bearer token |
| Agent not showing in dashboard | POST to `/api/heartbeat/:name`, check `data/agents.json` on hub |
| Scout not finding repos | POST to `/api/repos`, verify GitHub token is set |
| Queue not persisting | Check `CCC_DATA_DIR` and write permissions |
| ClawChat blank screen | Check browser console; verify WASM dist built and `DASHBOARD_DIST` set |
| Bus messages not arriving | Verify `BUS_LOG_PATH` is writable; check SSE at `/bus/stream` |
| Trunk not found | `cargo install trunk && rustup target add wasm32-unknown-unknown` |
| exec-listener FATAL in supervisor | Run `cd ~/Src/CCC && npm install` — missing `better-sqlite3` dep |
| exec-listener SSE 502 | ClawBus health issue — check `curl http://hub:8789/health` |
| vLLM not reachable from hub | Verify Tailscale IP: `tailscale ip -4` on GPU node, then `curl http://<ip>:8080/v1/models` |
| vllm-tunnel in BACKOFF | Stop it — tunnels are deprecated. Use Tailscale instead. |
| tokenhub returns wrong model | Check `~/.tokenhub/credentials` — `base_url` must be Tailscale IP, not tunnel port |

---

## Environment Variable Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `CCC_PORT` | `8789` | Port for ccc-server |
| `CCC_CONFIG` | `~/.ccc/ccc.json` | Path to JSON config file |
| `CCC_DATA_DIR` | `./data` | Data directory for JSON state files |
| `CCC_AUTH_TOKENS` | — | Comma-separated valid Bearer tokens |
| `CCC_DB_PATH` | — | SQLite database path (enables SQLite mode) |
| `QUEUE_PATH` | `$CCC_DATA_DIR/queue.json` | Work queue storage |
| `AGENTS_PATH` | `$CCC_DATA_DIR/agents.json` | Agent registry |
| `SECRETS_PATH` | `$CCC_DATA_DIR/secrets.json` | Secrets store |
| `BUS_LOG_PATH` | `$CCC_DATA_DIR/bus.jsonl` | Bus message log |
| `DASHBOARD_DIST` | — | WASM SPA dist directory |
| `CCC_CORS_ORIGINS` | `*` | Comma-separated CORS origins |
| `TOKENHUB_URL` | `http://127.0.0.1:8090` | LLM proxy URL |
| `QDRANT_FLEET_URL` | `http://...` | Qdrant vector DB URL |
| `MINIO_ENDPOINT` | `http://localhost:9000` | MinIO/S3 endpoint |
| `MINIO_BUCKET` | `agents` | MinIO bucket name |
| `SUPERVISOR_ENABLED` | `false` | Manage tokenhub subprocess |
| `VLLM_ENABLED` | `false` | Whether this node runs vLLM |
| `VLLM_MODEL` | — | HuggingFace model ID |
| `VLLM_PORT` | `8080` | vLLM port (fleet standard) |
| `VLLM_SERVED_NAME` | — | Model alias for `--served-model-name` |
| `TS_AUTHKEY` | — | Tailscale pre-auth key for unattended setup |
| `STALE_CLAUDE_MS` | `7200000` | Stale claim timeout for claude_cli items |
| `STALE_GPU_MS` | `21600000` | Stale claim timeout for GPU items |
| `STALE_INFERENCE_MS` | `1800000` | Stale claim timeout for inference_key items |

---

*CCC — coordination infrastructure for agent teams, without the vendor lock-in.*
