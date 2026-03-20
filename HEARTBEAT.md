# HEARTBEAT.md

## Mandatory: MinIO Heartbeat
Write a fresh heartbeat to MinIO at the start of every heartbeat cycle:

```
PUT http://100.89.199.14:9000/agents/natasha/agent-heartbeat-natasha.json
{
  "agent": "natasha",
  "host": "sparky",
  "ts": "<ISO-8601 now>",
  "status": "online",
  "model": "claude-sonnet-4-6",
  "crons": ["workqueue-natasha-07", "workqueue-natasha-37", "workqueue-daily-digest"]
}
```

Auth: Access Key `rockymoose4810f4cc7d28916f` / Secret `1b7a14087771df4bf85d6001fdd047a61348641bdf78aefd`

## Periodic Checks (rotate, 2-4x/day)
- Urgent emails
- Calendar events <2h out
- Anything in workqueue flagged urgent
