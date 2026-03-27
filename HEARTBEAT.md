# HEARTBEAT.md

## Mandatory: Heartbeat (BOTH destinations)
At the start of every heartbeat cycle, POST to Rocky's API AND write to MinIO.

### 1. Rocky Command Center API (primary — this is what makes the dashboard show "online")
```
POST http://146.190.134.110:8789/api/heartbeat/natasha
Authorization: Bearer wq-5dcad756f6d3e345c00b5cb3dfcbdedb
Content-Type: application/json

{
  "agent": "natasha",
  "host": "sparky",
  "ts": "<ISO-8601 now>",
  "status": "online",
  "model": "claude-sonnet-4-6",
  "crons": ["workqueue-natasha-07", "workqueue-natasha-37", "workqueue-daily-digest"]
}
```

### 2. MinIO (secondary — for cross-agent state)
```
PUT http://100.89.199.14:9000/agents/natasha/agent-heartbeat-natasha.json
```
Same JSON payload. Auth: Access Key `rockymoose4810f4cc7d28916f` / Secret `1b7a14087771df4bf85d6001fdd047a61348641bdf78aefd`

## Periodic Checks (rotate, 2-4x/day)
- Urgent emails
- Calendar events <2h out
- Anything in workqueue flagged urgent

## 🗓️ Proactive Schedule Guardian
During each heartbeat, check for upcoming calendar events and act proactively:

**What to check:**
- Calendar events in the next 2 hours → send a reminder to jkh if one is approaching within 30 min
- Events in the next 24 hours → note in daily context, mention if jkh asks about their day
- If a meeting has prep materials (agenda, attendees) → summarize briefly in the reminder

**When to notify jkh:**
- ≤30 min before any calendar event: send a Telegram or Slack DM with event name, time, and any known details
- ≤2h before: note it in session context so you can mention it naturally if jkh starts a conversation
- Don't notify for events jkh has already been reminded about this session

**State tracking:**
- Keep `memory/schedule-guardian-state.json`:
  ```json
  {
    "lastCalendarCheck": "<ISO-8601>",
    "notifiedEvents": ["<event-id-or-title+date>"]
  }
  ```
- Reset `notifiedEvents` daily (clear entries older than 24h)

**Tool to use:** Check calendar via available tools (nodes, browser, or calendar integrations). If no calendar tool is available, skip silently — don't error.

**Quiet hours:** 23:00–08:00 PT — no proactive notifications unless priority is "urgent" or event is within 15 min.
