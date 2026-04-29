# CLI-First Migration Runbook

This runbook covers the migration from mixed `/api/queue` plus ad hoc exec
behavior to the durable `/api/tasks` plane with persistent CLI sessions.

## Rollout Phases

1. **Schema compatibility**
   - Keep `/api/queue` and `/api/exec` available.
   - Require all agents to publish canonical `executors`, `sessions`, and
     `capacity` fields in heartbeats.
   - Watch server logs for `acc.compat` warnings from legacy route use.

2. **Minimal runtime**
   - Run `acc-agent supervise` everywhere.
   - Default children are `acc-agent tasks` and `acc-agent bus`.
   - Enable legacy queue only with `ACC_ENABLE_LEGACY_QUEUE=true`.
   - Enable Hermes durable polling only with `ACC_ENABLE_HERMES_POLL=true`.

3. **Dispatch cutover**
   - File durable coding work on `/api/tasks`.
   - Use `preferred_executor` / `required_executors` only for executor class.
   - Use `preferred_agent`, `assigned_agent`, and `assigned_session` for node
     or session affinity.
   - Confirm the dashboard shows executor readiness, sessions, and free slots.

4. **Legacy freeze**
   - Do not add new semantics to `/api/queue`.
   - Treat `/api/exec` as operator-only remote command execution.
   - Keep compatibility paths until every agent is on the minimal runtime and
     no `acc.compat` warnings appear for a full migration window.

## Operator Checks

Use the dashboard Agents tab first. It shows:
- executor readiness and auth state
- active CLI sessions
- idle, busy, stuck, dead, and unauthenticated states
- task slots, session slots, and spawn denial reason

Useful API checks:

```bash
curl -sf -H "Authorization: Bearer $ACC_AGENT_TOKEN" "$ACC_URL/api/agents"
curl -sf -H "Authorization: Bearer $ACC_AGENT_TOKEN" "$ACC_URL/api/tasks?status=open"
```

## Failure Modes

### Stuck Sessions

Symptoms:
- session state is `stuck`
- task claim remains active but no output changes

Remediation:
1. Check the session pane on the agent host.
2. If the CLI is waiting for input, answer or cancel in the pane.
3. If it is unrecoverable, kill the tmux session and let discovery mark it
   `dead`.
4. Unclaim or fail the task so dispatch can route it again.

### Expired Or Missing Auth

Symptoms:
- executor auth state is `unauthenticated` or `missing`
- session state is `unauthenticated`

Remediation:
1. Re-authenticate the CLI on that node (`claude login`, `codex`, `cursor`, or
   the relevant provider setup).
2. Restart the session or wait for the next registry refresh.
3. Confirm the dashboard shows auth state `ready` before assigning coding work.

### Memory Pressure

Symptoms:
- session spawn denial contains `memory_pressure:<mb>mb`
- free session slots may exist but spawn remains denied

Remediation:
1. Stop idle heavyweight sessions.
2. Lower `ACC_MAX_CLI_SESSIONS` or `ACC_MAX_SESSIONS_PER_EXECUTOR`.
3. Raise memory or lower `ACC_SESSION_MIN_FREE_MEMORY_MB` only if the node has
   proven headroom.

### Session Limit Reached

Symptoms:
- denial is `session_limit_reached`
- free session slots is `0`

Remediation:
1. Reuse an existing idle project session if possible.
2. Kill dead or stale sessions.
3. Increase `ACC_MAX_CLI_SESSIONS` only after checking RAM headroom.

### Executor Limit Reached

Symptoms:
- denial is `executor_session_limit_reached:<executor>`

Remediation:
1. Reuse or stop an idle session for that executor.
2. Increase `ACC_MAX_SESSIONS_PER_EXECUTOR` or
   `ACC_MAX_<EXECUTOR>_SESSIONS` if the node can support it.

### API Fallback Mode

API-backed execution remains acceptable for reviews, summaries, planning, and
coordination. For coding work, fallback to the API loop should happen only when
no ready or spawnable CLI executor is available.

If coding work falls back unexpectedly:
1. Check executor readiness in the dashboard.
2. Check session capacity and spawn denial reason.
3. Confirm the task does not pin an unavailable `preferred_executor`.
4. Confirm the agent has the matching CLI installed and authenticated.

