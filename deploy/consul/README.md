# Consul Service Discovery — CCC Fleet

CCC uses Consul for all internal service mesh so that no IP addresses or personal
domain names ever appear in source code. All required services register themselves
with Consul and are reachable via `<name>.service.consul`.

## Fleet Topology (Tailscale)

| Agent name  | Host     | Role           | Platform        |
|-------------|----------|----------------|-----------------|
| rocky       | do-host1 | Consul server  | Linux (DO)      |
| natasha     | sparky   | Consul client  | Linux (GPU)     |
| bullwinkle  | puck     | Consul client  | macOS           |

## Service Ecosystem

All services required by CCC — every one is registered in Consul and started
by the CCC migration system (so removing CCC removes all of them):

### rocky (hub) — service-defs/rocky.hcl
| Service      | Port  | Consul DNS name                 | Managed by            |
|--------------|-------|---------------------------------|-----------------------|
| ccc-server   | 8789  | ccc-server.service.consul       | systemd (migration 0005) |
| tokenhub     | 8090  | tokenhub.service.consul         | ccc-server supervisor |
| qdrant       | 6333  | qdrant.service.consul           | Docker compose        |
| minio        | 9000  | minio.service.consul            | systemd               |
| searxng      | 8888  | searxng.service.consul          | Docker compose        |
| prometheus   | 9090  | prometheus.service.consul       | Docker compose        |
| grafana      | 3000  | grafana.service.consul          | Docker compose        |

### natasha (GPU worker) — service-defs/natasha.hcl
| Service      | Port  | Consul DNS name                 | Managed by            |
|--------------|-------|---------------------------------|-----------------------|
| whisper-api  | 8792  | whisper-api.service.consul      | systemd               |
| clawfs       | 8791  | clawfs.service.consul           | systemd               |
| ollama       | 11434 | ollama.service.consul           | systemd               |

### bullwinkle (macOS dev) — service-defs/bullwinkle.hcl
| Service      | Port  | Consul DNS name                 | Managed by            |
|--------------|-------|---------------------------------|-----------------------|
| clawfs       | 8791  | clawfs.service.consul           | launchd               |

## Deployment

Consul is installed via the CCC migration system — no manual steps needed:

```bash
# On any fleet node after git pull:
bash deploy/run-migrations.sh

# This runs 0009_install_consul.sh and 0010_configure_consul_dns.sh automatically.
```

### Agent nodes require one env var in ~/.ccc/.env:
```bash
CONSUL_SERVER_ADDR=<rocky-tailscale-ip>   # e.g. 100.89.199.14
```

## Verifying

```bash
# Check Consul is running
consul members                               # all fleet nodes visible

# Look up a service
dig ccc-server.service.consul @127.0.0.1 -p 8600
dig tokenhub.service.consul @127.0.0.1 -p 8600

# After DNS migration 0010, no port needed:
dig tokenhub.service.consul

# Consul UI (hub only)
open http://127.0.0.1:8500
```

## Adding a New Service

1. Add a `service {}` block to `service-defs/<agent-name>.hcl`
2. Re-run migration: `bash deploy/run-migrations.sh --force=0009`
3. Verify: `consul catalog services`

## Directory Layout

```
deploy/consul/
├── README.md                      # This file
├── consul.hcl.tmpl                # Unified config template (rendered by migration 0009)
└── service-defs/
    ├── rocky.hcl                  # Hub: ccc-server, tokenhub, qdrant, minio, etc.
    ├── natasha.hcl                # GPU: whisper-api, clawfs, ollama
    └── bullwinkle.hcl             # macOS: clawfs
```

The old `server/` and `client/` directories are superseded by the migration system.
