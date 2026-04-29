# Agent Command Center (ACC)

Distributed AI agent coordination system. Connects a fleet of heterogeneous machines — cloud VMs, Mac laptops, GPU boxes — through a durable task plane, message bus, and hub-and-spoke API that allows all agents to coordinate.

**Hub API + Dashboard:** port 8789 (Rust/Axum) — open `http://your-server:8789/` in a browser

---

## Quick Start

### Hub node

```bash
cp deploy/.env.server.template ~/.ccc/.env   # fill in CCC_PORT, CCC_AUTH_TOKENS, CCC_ADMIN_TOKEN
make docker-up                                # starts acc-api (8789)
```

### Agent node

```bash
cp deploy/.env.template ~/.ccc/.env           # fill in CCC_URL + CCC_AGENT_TOKEN
make register                                 # POST capabilities to the hub
```

Full walkthrough: [GETTING_STARTED.md](GETTING_STARTED.md)

---

## Components

| Component | Role | Port |
|-----------|------|------|
| `acc-server` | Rust/Axum REST API + web dashboard — `/api/tasks`, agent registry, secrets | 8789 |
| `ccc-queue-worker` | Legacy queue compatibility worker | — |
| `ccc-bus-listener` | AgentBus SSE receiver | — |
| `ccc-exec-listen` | Remote exec handler (sandboxed) | — |
| `hermes gateway` | Channel gateway (Slack, Telegram) — per-agent | — |
| TokenHub | LLM routing proxy (OpenAI-compatible) | 8090 |
| MinIO | S3-compatible object storage | 9000 |
| Qdrant | Vector database | 6333 |

---

## Agent Runtime

The minimal agent runtime is **acc-agent supervise**, which starts durable task
work and the bus listener by default:

```bash
acc-agent tasks               # durable work from /api/tasks
acc-agent bus                 # AgentBus listener and operator exec handling
```

Optional processes are explicit opt-ins: `ACC_ENABLE_LEGACY_QUEUE=true` for the
legacy queue worker, `ACC_ENABLE_HERMES_POLL=true` for Hermes durable polling,
Slack/Telegram credentials for Hermes gateways, and NVIDIA/proxy variables for
local model routing.

For coding tasks, persistent CLI sessions run in tmux panes alongside the agent:

```bash
tmux new-session -d -s codex:acc
tmux send-keys -t codex:acc 'codex' Enter
```

Durable tasks with `preferred_executor` / `required_executors` are routed against live heartbeat `executors`, `sessions`, and capacity telemetry. The legacy queue worker remains for old ingress only; new durable coding behavior belongs on `/api/tasks`.

---

## Durable Work Plane

`/api/tasks` is the authoritative durable orchestration plane. Durable coding work, reviews, gap tasks, join gates, and final commit tasks are represented as tasks with `outcome_id` and `workflow_role`.

`/api/queue` remains as compatibility ingress for older workers. New workflow semantics must be added to `/api/tasks`, not queue-only fields. `/api/exec` is for operator-issued remote commands, not normal durable scheduling.

Legacy `/api/queue` and `/api/exec` calls emit `acc.compat` warnings. Keep these
routes only while mixed-version agents still use them; removal is safe after all
agents run the minimal runtime and the operator sees no compatibility warnings
for a full migration window.

Finalization is single-owner: commit role tasks are claimable only by the persisted `finisher_agent`. See [docs/workflow-runbook.md](docs/workflow-runbook.md).

Slack and Telegram provenance is captured as durable conversation chains linking messages, reactions, participants, entities, outcomes, and spawned tasks. See [docs/conversation-chains.md](docs/conversation-chains.md).

CLI-first rollout and operator remediation steps live in
[docs/cli-first-migration-runbook.md](docs/cli-first-migration-runbook.md).

GitHub issues can be synced into beads and promoted to fleet tasks with the
`github_sync` tool. The linked metadata schema, mirror command, migration, and
timer setup are in [docs/github-beads-sync-runbook.md](docs/github-beads-sync-runbook.md).

## Executor Routing

Each task may carry executor and affinity fields:

| Field | Meaning |
|-------|---------|
| `preferred_executor` | Soft executor preference, such as `claude_cli`, `codex_cli`, or `hermes` |
| `required_executors` | Hard executor compatibility filter |
| `preferred_agent` | Soft agent affinity |
| `assigned_agent` | Hard assignment once routed or claimed |
| `assigned_session` | Optional session affinity |

Persistent CLI sessions are advertised through heartbeat `executors`, `sessions`, and capacity fields. API-backed providers remain useful for coordination and light reasoning, but long coding tasks should prefer ready CLI sessions.

---

## Configuration

Hub: `cp deploy/.env.server.template ~/.ccc/.env`
Agent: `cp deploy/.env.template ~/.ccc/.env`

### Required (all agents)

| Variable | Purpose |
|----------|---------|
| `CCC_URL` | Hub API URL |
| `CCC_AGENT_TOKEN` | Bearer token for this agent |
| `AGENT_NAME` | Short identifier (used in queue, heartbeats, logs) |
| `AGENT_HOST` | Human-readable hostname (dashboard display) |

### Hub-only

| Variable | Purpose |
|----------|---------|
| `CCC_PORT` | API port (default `8789`) |
| `CCC_AUTH_TOKENS` | Comma-separated valid bearer tokens |
| `CCC_ADMIN_TOKEN` | Operator-only — never distribute |
| `MINIO_ENDPOINT` / `MINIO_ACCESS_KEY` / `MINIO_SECRET_KEY` | MinIO credentials |
| `MINIO_BUCKET` | Default bucket (default `agents`) |
| `SLACK_TOKEN` / `SLACK_DEFAULT_CHANNEL` | Slack hub integration |
| `MATTERMOST_TOKEN` / `MATTERMOST_URL` | Mattermost integration |
| `GITHUB_TOKEN` / `WATCH_CHANNEL` | Issue scanner |

### Agent capabilities

| Variable | Default | Purpose |
|----------|---------|---------|
| `AGENT_CLAUDE_CLI` | `false` | Claude Code tmux session available |
| `AGENT_CLAUDE_MODEL` | `claude-sonnet-4-6` | Model used for CLI tasks |
| `AGENT_HAS_GPU` | `false` | GPU hardware present |
| `AGENT_GPU_MODEL` | — | GPU model (e.g. L40, Blackwell) |
| `AGENT_GPU_COUNT` | `0` | Number of GPUs |
| `AGENT_GPU_VRAM_GB` | `0` | Total VRAM |

### vLLM (GPU nodes)

| Variable | Default | Purpose |
|----------|---------|---------|
| `VLLM_ENABLED` | `false` | Enable local vLLM service |
| `VLLM_MODEL` | `google/gemma-4-31B-it` | Model to serve |
| `VLLM_SERVED_NAME` | `gemma` | Routing alias |
| `VLLM_PORT` | `8000` | Local port |
| `VLLM_MODEL_PATH` | — | Model cache directory |

### Inference

| Variable | Default | Purpose |
|----------|---------|---------|
| `TOKENHUB_URL` | `http://localhost:8090` | LLM routing proxy |
| `TOKENHUB_API_KEY` | — | Agent key for TokenHub |
| `TOKENHUB_ADMIN_TOKEN` | — | Admin key (hub only) |
| `NVIDIA_API_BASE` | `https://inference-api.nvidia.com/v1` | Cloud inference endpoint |
| `NVIDIA_API_KEY` | — | NVIDIA metered API key |

### Messaging

| Variable | Purpose |
|----------|---------|
| `SLACK_TOKEN` | Slack bot token (`xoxb-...`) |
| `SLACK_SIGNING_SECRET` | Slack signing secret |
| `TELEGRAM_TOKEN` | Telegram bot token |

### Other

| Variable | Purpose |
|----------|---------|
| `AGENTBUS_TOKEN` | HMAC-SHA256 secret for AgentBus exec payloads |
| `QDRANT_URL` | Qdrant endpoint (default `http://localhost:6333`) |
| `QDRANT_API_KEY` | Qdrant API key (if remote) |
| `CCC_TAILSCALE_URL` | Tailscale fallback URL for hub |
| `CCC_MINIO_URL` | MinIO endpoint for shared file sync |
| `TS_AUTHKEY` | Tailscale pre-auth key |

When an install path brings up Tailscale, run
`tailscale set --accept-dns=false` before `tailscale up` so agent DNS stays on
the host resolver.

Full reference: `deploy/.env.template` (agents) · `deploy/.env.server.template` (hub)

---

## Services

### Linux (systemd) — `deploy/systemd/`

| Unit | Purpose |
|------|---------|
| `acc-server.service` | API server + dashboard (port 8789) |
| `acc-agent.service` | Minimal agent supervisor (`tasks` + `bus` by default) |
| `acc-github-sync.timer` | Optional GitHub Issues ↔ beads ↔ fleet task sync |
| `ccc-queue-worker.service` | Legacy queue compatibility worker (optional) |
| `ccc-bus-listener.service` | Legacy AgentBus SSE receiver (optional on old nodes) |
| `ccc-exec-listen.service` | Remote exec handler |
| `ccc-agent.service` + `ccc-agent.timer` | Periodic `agent-pull.sh` + heartbeat |
| `consul.service` | Service discovery + DNS |
| `heartbeat-local.service` | Agent health reporting |
| `ollama-keepalive.service` | GPU: keep local model warm |
| `whisper-natasha.service` | GPU: speech-to-text |
| `sparky-reverse-tunnel.service` | Firewalled agents: reverse SSH tunnel to hub |

### macOS (launchd) — `deploy/launchd/`

| Plist | Purpose |
|-------|---------|
| `com.ccc.agent.plist` | Agent pull (every 600s) |
| `com.ccc.queue-worker.plist` | Queue processor |
| `com.ccc.bus-listener.plist` | AgentBus receiver |
| `com.ccc.exec-listen.plist` | Remote exec handler |
| `com.ccc.claude-main.plist` | Claude Code tmux session |
| `com.ccc.consul.plist` | Consul |

### Makefile targets

| Target | Purpose |
|--------|---------|
| `make deps` | Install operator tools (mc, gh, jq) |
| `make env` | Create/verify `~/.ccc/.env` |
| `make init` | Interactive onboarding wizard |
| `make register` | Register this agent with the hub |
| `make sync` | Push update + broadcast to all agents |
| `make build` | Build the Rust API server |
| `make test` | Run tests |
| `make docker-up/down/logs` | Docker Compose stack |
| `make release` | Bump version, update CHANGELOG, tag |

---

## Firewalled Agents

Nodes with no inbound connectivity connect to the hub via reverse SSH tunnel:

1. Agent registers its SSH pubkey: `POST /api/tunnel/request`
2. Hub appends key to `tunnel` user's `authorized_keys`
3. Agent opens: `ssh -N -R <port>:localhost:8080 tunnel@<hub>`
4. Hub has `localhost:<port>` → agent's local service

Port assignment: `GET /api/agents/:name/tunnel-port`. See `deploy/systemd/sparky-reverse-tunnel.service`.

---

## AgentBus (P2P Messaging)

Direct agent-to-agent messages, HMAC-SHA256 signed with `AGENTBUS_TOKEN`. The hub fans messages out to registered peers via SSE.

Remote execution flows over AgentBus:
```
POST /api/exec  →  AgentBus (ccc.exec)  →  ccc-exec-listen  →  POST /api/exec/:id/result
```

See [agentbus/SPEC.md](agentbus/SPEC.md).

---

## Migrations

Sequential idempotent scripts in `deploy/migrations/` (0001–0015). Each checks whether it's already been applied before making changes.

Use the `/add-migration` skill whenever you add, remove, or change files in `deploy/systemd/`, `deploy/launchd/`, or `deploy/crontab-acc.txt`.

---

## Secrets

Secrets flow through the hub store (`POST /api/secrets`) and are pushed to agent `.env` files by `deploy/secrets-sync.sh`. Never scatter credentials manually. Populate TokenHub before starting any service that needs inference.

---

## Data Store

State is stored as JSON files in `~/.ccc/data/` on the hub:

| File | Purpose |
|------|---------|
| `queue.json` | Work queue — all pending, in-progress, completed items |
| `agents.json` | Agent registry + last heartbeat |
| `secrets.json` | Key-value secret store |
| `bus.jsonl` | AgentBus message log (append-only) |
| `exec.jsonl` | Remote execution log |
| `lessons.jsonl` | Shared fleet lessons |

---

## Related Repos

| Repo | Location | Purpose |
|------|----------|---------|
| `tokenhub` | `~/Src/tokenhub` | LLM routing gateway — must run before agents do inference |

Minimal setup: CCC + tokenhub only.

---

## Further Reading

- [GETTING_STARTED.md](GETTING_STARTED.md) — step-by-step for operators, agent deployers, developers
- [ARCHITECTURE.md](ARCHITECTURE.md) — component descriptions, agent topology, capability model
- [SPEC.md](SPEC.md) — complete system specification
- [AGENTS.md](AGENTS.md) — fleet cast (Rocky, Bullwinkle, Natasha, Boris)
- [deploy/README.md](deploy/README.md) — service management and node setup
- [workqueue/WORKQUEUE_AGENT.md](workqueue/WORKQUEUE_AGENT.md) — queue schema and lifecycle

---

## Generated Assets

Generated files (images, PDFs, charts, etc.) go under `assets/<project-name>/`. Never dump to the repo root.
