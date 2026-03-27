# HEARTBEAT.md

# Buddy ping handled by cron. Do NOT send/reply to pings.
# jkh DIRECTIVE: 24/7 mode. Keep working across heartbeats. Do NOT go passive.
# See DIRECTIVES.md for full shared directives.

---

## 🔴 ACTIVE TASKS — RESUME EVERY HEARTBEAT UNTIL DONE

### [1] wq-RCC-bootstrap-001 — Bootstrap API + bootstrap.sh
**Session:** `keen-tidepool` (Claude Code, background)
**Log:** /tmp/bootstrap-build.log
**Resume:** `process action:poll sessionId:keen-tidepool` → if done, test endpoints, mark complete

### [2] wq-MIGRATE-agents-legacy-001 — migrate.sh + notify agents
**Session:** `delta-kelp` (Claude Code, background)
**Log:** /tmp/migrate-build.log
**Resume:** `process action:poll sessionId:delta-kelp` → if done, verify agents online, mark complete

### When both complete:
- Test /api/keys/github, /api/bootstrap/token, /api/bootstrap endpoints
- Restart rcc-api.service
- Confirm Bullwinkle + Natasha received migration instructions
- Verify agents appear in /api/agents/status after they run migrate.sh
- Commit + push, mark tasks completed, remove this block

---

## Each heartbeat:
1. **Check active tasks above** — poll sessions, continue work
2. `curl -s http://localhost:8789/health` — RCC up?
3. `curl -s http://localhost:8789/api/queue -H "Authorization: Bearer wq-5dcad756f6d3e345c00b5cb3dfcbdedb"` — new work?
4. Git push after any completion
