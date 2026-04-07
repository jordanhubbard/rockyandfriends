# CCC Tech Debt Audit — 2026-04-06

**Auditor:** Rocky (principal engineer review)
**Scope:** Full codebase — dead code, duplication, architectural violations, language policy compliance
**Directive:** jkh — "NO NODE.JS after this. Everything Rust/WASM." (2026-04-02)

---

## Executive Summary

CCC has **54,442 LOC** across two languages: **38,769 LOC Node.js** (124 .mjs files) and **15,673 LOC Rust** (62 .rs files). The project declared a Rust-only policy on April 2nd, but **71% of the codebase is still JavaScript**. Worse, significant chunks of that JS are dead, duplicated, or referencing decommissioned infrastructure (Milvus). The repo has **24 top-level directories** — roughly double what a project this size should have.

The Rust rewrite (`ccc-server`) is 6,809 LOC and covers the core API routes, but **nothing deploys it**. Production still runs `node.ccc/api/index.mjs`. The new thing is written and tested; the old thing is still running. That's the single biggest issue.

**Bottom line:** ~8,000-10,000 LOC can be deleted today with zero risk. Another ~5,000 LOC is stale Milvus references that need updating. The remaining Node.js (~20K LOC) needs a phased migration plan to Rust, prioritized by what's actually running in production.

---

## 🔴 Critical — Delete Now (Zero Risk)

### 1. `ccc-hub/` — Dead fork
11 files, own package.json, zero imports from anywhere, not running on any host. This is a stale fork of .ccc/` from before the workspace reorganization. **Delete the entire directory.**

### 2. .ccc/wasm-dashboard/` — Superseded
3 Rust files that diverged from .ccc/dashboard/dashboard-ui/`. The canonical dashboard-ui ships; this doesn't. **Delete.**

### 3. `clawchat-mobile/` — Abandoned skeleton
React Native project with a package.json but no builds, no CI, no app store target. Never got past scaffolding. **Delete.**

### 4. `shanghai_output/` — Debug artifacts in repo
6 PNG test images (~800KB). These are debug output that got committed. Binary artifacts don't belong in the repo. **Delete.**

### 5. `offline-heartbeat/` — Triple duplicate
This is the same heartbeat server that exists in two other places:
- .ccc/heartbeat-local/server.mjs` (238L) — the one with its own package.json
- .ccc/agent/offline-heartbeat.mjs` (94L) — compact inline version

Three copies. **Delete `offline-heartbeat/` entirely** (it's the middle child — neither the canonical one nor the inline one).

### 6. Milvus Ghost References — Migration incomplete
The Milvus→Qdrant migration landed in commit `015cd97` but left ghosts everywhere:
- Root `package.json` still lists `@zilliz/milvus2-sdk-node` as a dependency
- 17+ files still import or reference Milvus
- 18 of 28 workqueue scripts reference Milvus/MinIO
- .ccc/vector/index.mjs` (680L) is the canonical Milvus client — now dead code

**Grep for `milvus|Milvus|MILVUS` and delete or replace every reference.** The Qdrant equivalents already exist.

### 7. Trust System Duplication
Two files, same domain:
- .ccc/trust/adaptive-trust.mjs` (193L) — canonical implementation
- .ccc/guardrails/adaptive-trust.mjs` (65L) — shim that re-exports from trust/

The shim exists because something imported from the wrong path. **Delete the shim, update the one import.**

### 8. Heartbeat System Duplication
Three implementations:
- .ccc/heartbeat-local/server.mjs` (238L) — standalone server with own package.json
- `offline-heartbeat/server.mjs` (217L) — near-identical copy at top level
- .ccc/agent/offline-heartbeat.mjs` (94L) — lightweight inline version

**Pick .ccc/heartbeat-local/` as canonical. Delete the other two.** (Or pick the 94L inline version if the full server is overkill now that Hermes handles heartbeats.)

### 9. Ollama Watchdog × 4
Four copies of the same watchdog:
- `ollama-watchdog.mjs` (top-level) — original
- .ccc/scripts/ollama-watchdog.mjs` (401L) — most complete
- .ccc/agent/ollama-watchdog.mjs` (215L) — agent-embedded version
- `workqueue/scripts/ollama-health-watchdog.mjs` — workqueue copy

**Keep .ccc/scripts/ollama-watchdog.mjs` (401L, most complete). Delete the other three.** Long-term this should be a Rust binary, but one JS copy is fine for operational tooling.

---

## 🟠 Important — Requires Decision

### 10. agentOS Subsystem — Speculative, Zero Callers
These were written speculatively for an agentOS integration that hasn't materialized:
- `services/vibeswap/` (581L)
- `services/pluginhost/` (361L)
- `services/migrate/` (371L)
- .ccc/api/routes/agentos.mjs` (467L)

**Total: ~1,780 LOC with zero callers, no tests, no running service.** Git-archive to a `lab/` branch and delete from main. The ideas are preserved in git history if/when agentOS needs them.

### 11. ccc-server (Rust) — Written But Not Deployed
This is the elephant in the room. The Rust replacement for the Node.js API is **already written**:
- 23 route files
- 6,809 LOC
- Covers: health, status, queue CRUD, agent registry, heartbeats, secrets, ClawBus SSE

But production still runs `node.ccc/api/index.mjs` (2,134L) via `ccc-api.service`. The Makefile, Dockerfile, docker-compose.yml, and .service files all point at Node.

**This is the #1 architectural action item.** The rewrite is done. Ship it. Update the deployment artifacts to point at the Rust binary. Run them in parallel for a week if you want, then cut over.

---

## 🟡 Cleanup — Lower Priority

### 12. `workqueue/scripts/` Junk Drawer
28 MJS files (~5,000 LOC). Most reference stale Milvus/MinIO infrastructure. Duplicated files (`intent-tracker.mjs` vs `jkh-intent-tracker.mjs`). This directory is an archaeological site of one-off scripts that grew into a graveyard.

**Triage each file:** operational tooling (keep as shell script), core logic (port to Rust), or dead (delete).

### 13. Systemd Unit Sprawl
19 `.service` files scattered across 6 directories:
- `deploy/systemd/`
- `services/`
- .ccc/deploy/systemd/`
- .ccc/crush-server/`
- .ccc/clawfs-sync/`
- `clawbus/`

Some reference services that no longer exist (Node squirrelchat on port 8790). **Move all to `deploy/systemd/` as single source of truth.** Delete stale ones.

### 14. `openclaw/` Directory
4 markdown soul files for a system being decommissioned. Rocky already migrated to Hermes. **Move soul files to `people/` or `avatars/`, delete `openclaw/`.**

### 15. ClawFS Language Split
- `services/clawfs/` — Node.js implementation
- .ccc/clawfs-sync/` — Rust implementation (has Cargo.toml)

Same domain, two languages, two locations. Per the "no Node.js" directive, **consolidate to the Rust version. Delete the Node.js one.**

### 16. ClawBus Language Split
- `clawbus/` — Node.js (receive-server + context-handoff)
- `clawbus-plugin/` — JS plugin

Both are JavaScript. If following the Rust directive, these should migrate. At minimum, **consolidate into one directory** instead of two top-level dirs for one subsystem.

### 17. .ccc/scripts/` Polyglot Mix
17 MJS files + 3 shell scripts + 1 Python file (`whisper-daemon.py`). The shell scripts and whisper daemon are fine as operational tooling. The MJS files doing core logic should migrate to Rust over time. **No immediate action needed** — just tag them in the migration backlog.

### 18. `dashboard/` Top-Level Ghost
Contains only a README.md. The real dashboard is at .ccc/dashboard/` (Rust workspace). **Delete the empty directory.**

### 19. Stale Docker Artifacts
`Dockerfile` and `docker-compose.yml` at repo root reference:
- `squirrelchat/server.mjs` — doesn't exist
- Node.js ClawChat on port 8790 — superseded by Rust on 8793

**Update to reflect actual architecture** (Rust binaries, correct ports).

---

## Architectural Observations

### The Directory Explosion
24 top-level directories is too many. A clean layout would be:

```
CCC/
├──.ccc/              # Core platform (Rust workspace)
│   ├── server/       # API server (Rust/Axum)
│   ├── dashboard/    # UI (Leptos/WASM)
│   ├── clawfs/       # Filesystem sync (Rust)
│   └── clawbus/      # Message bus (→Rust)
├── deploy/           # All deployment: systemd, Docker, scripts
├── docs/             # Architecture, RFCs, specs
├── scripts/          # Operational tooling (shell/python)
├── workqueue/        # Work queue system
├── people/           # Agent souls, identities
├── skills/           # Agent skills
└── memory/           # Daily notes
```

That's 8 directories instead of 24. Everything else either moves under .ccc/`, moves under `deploy/`, or gets deleted.

### The Language Policy Gap
The "no Node.js" directive was issued April 2nd. Four days later, 71% of the codebase is still JS. This isn't surprising — rewrites take time — but the **deployment artifacts still point at Node**. The Rust server exists and nobody turned it on. That's the gap.

### The Duplication Pattern
The repo has a pattern of "copy, modify, forget." Heartbeat servers (3 copies), ollama watchdogs (4 copies), trust systems (2 copies). This happens when multiple agents work on the same codebase without a clear ownership model. **Recommendation:** each subsystem gets one canonical location. Imports, not copies.

### Missing: Tests
The Rust code has no test files visible outside the source. The Node.js code has .ccc/brain/test.mjs` (429L) and that's about it for the core API. For a system managing a fleet of AI agents, the test coverage is essentially zero.

---

## Recommended Execution Order

1. **Delete dead code** (items 1-5, 18) — 30 minutes, zero risk, immediate clarity
2. **Clean Milvus ghosts** (item 6) — 1-2 hours, mechanical grep-and-replace
3. **Deduplicate** (items 7-9) — 1 hour, pick winners, delete losers
4. **Ship ccc-server to production** (item 11) — half day, biggest impact
5. **Archive agentOS** (item 10) — 15 minutes, git branch + delete
6. **Consolidate directories** (items 13-16, 18-19) — half day, organizational
7. **Triage workqueue scripts** (item 12) — 2 hours, judgment calls
8. **Continue Rust migration** (items 15-17) — ongoing, weeks

Items 1-3 are pure wins. Item 4 is the strategic unlock. Everything else is cleanup.

---

*Report committed to repo. Git history preserves everything we delete.*
