#!/bin/bash
# write-heartbeat.sh — Write Natasha's heartbeat JSON to MinIO
# Called by the workqueue cron at :07 and :37

AGENT="natasha"
HOST="sparky"
MINIO_ENDPOINT="http://100.89.199.14:9000"
BUCKET="agents"
KEY="shared/agent-heartbeat-natasha.json"
ACCESS_KEY="rockymoose4810f4cc7d28916f"
SECRET_KEY="1b7a14087771df4bf85d6001fdd047a61348641bdf78aefd"

TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
UPTIME=$(uptime -p 2>/dev/null || echo "unknown")
LOAD=$(cat /proc/loadavg | awk '{print $1}')

JSON=$(cat <<EOF
{
  "agent": "${AGENT}",
  "host": "${HOST}",
  "ts": "${TIMESTAMP}",
  "status": "online",
  "uptime": "${UPTIME}",
  "load1m": "${LOAD}",
  "version": 1
}
EOF
)

# Use mc (MinIO client) if available, fall back to curl S3 API
if command -v mc &>/dev/null; then
  echo "${JSON}" | mc pipe "minio/${BUCKET}/${KEY}" 2>/dev/null && echo "heartbeat written via mc" && exit 0
fi

# Fallback: curl with AWS Signature v4 (simplified — pre-signed approach via mc alias)
# For now just write locally as fallback
echo "${JSON}" > /tmp/natasha-heartbeat.json
echo "heartbeat written locally (mc not available)"
