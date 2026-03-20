#!/usr/bin/env python3
"""
publish-dashboard.py — Fetch agent heartbeats from MinIO, build dashboard HTML, push to Azure.
Run via cron (workqueue tick) or standalone.
"""

import boto3, json, re, requests, sys
from datetime import datetime, timezone
from botocore.client import Config

MINIO_ENDPOINT = "http://100.89.199.14:9000"
MINIO_KEY = "rockymoose4810f4cc7d28916f"
MINIO_SECRET = "1b7a14087771df4bf85d6001fdd047a61348641bdf78aefd"
BUCKET = "agents"

AZURE_SAS = "se=2029-03-19T02%3A25Z&sp=rwdlcu&spr=https&sv=2026-02-06&ss=b&srt=sco&sig=Dn4faVsJCz0ufWyHmiKCFCrgiLQkSIRtp7MLmqXKiUA%3D"
AZURE_ACCOUNT = "loomdd566f62"
AZURE_CONTAINER = "assets"
AZURE_FILENAME = "agent-dashboard.html"
PUBLIC_URL = f"https://{AZURE_ACCOUNT}.blob.core.windows.net/{AZURE_CONTAINER}/{AZURE_FILENAME}"

HTML_TEMPLATE = r"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="120">
<title>Agent Status Dashboard</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { font-family: 'Segoe UI', system-ui, sans-serif; background: #0d1117; color: #c9d1d9; min-height: 100vh; padding: 2rem; }
  h1 { font-size: 1.4rem; font-weight: 600; color: #f0f6fc; margin-bottom: 0.25rem; }
  .subtitle { font-size: 0.8rem; color: #6e7681; margin-bottom: 2rem; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 1rem; margin-bottom: 2rem; }
  .card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 1.25rem; }
  .card.online { border-left: 3px solid #3fb950; }
  .card.offline { border-left: 3px solid #f85149; }
  .card.unknown { border-left: 3px solid #6e7681; }
  .card.stale { border-left: 3px solid #d29922; }
  .agent-name { font-size: 1.1rem; font-weight: 600; color: #f0f6fc; display: flex; align-items: center; gap: 0.5rem; }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
  .dot.online { background: #3fb950; box-shadow: 0 0 6px #3fb950aa; }
  .dot.offline { background: #f85149; }
  .dot.unknown, .dot.stale { background: #d29922; }
  .host { font-size: 0.75rem; color: #6e7681; margin: 0.25rem 0 0.75rem; }
  .field { font-size: 0.8rem; margin: 0.25rem 0; }
  .label { color: #6e7681; }
  .value { color: #c9d1d9; }
  .badge { display: inline-block; padding: 0.15rem 0.5rem; border-radius: 12px; font-size: 0.7rem; font-weight: 600; text-transform: uppercase; }
  .badge-online { background: #1a3a1a; color: #3fb950; }
  .badge-offline { background: #3a1a1a; color: #f85149; }
  .badge-unknown, .badge-stale { background: #2e2200; color: #d29922; }
  .footer { font-size: 0.72rem; color: #6e7681; border-top: 1px solid #21262d; padding-top: 1rem; }
  .footer a { color: #58a6ff; text-decoration: none; }
</style>
</head>
<body>
<h1>🤖 Agent Status Dashboard</h1>
<p class="subtitle" id="gen-time">Loading...</p>
<div class="grid" id="agent-grid"></div>
<div class="footer">Data source: MinIO on Rocky · Heartbeats written every 30 min · Page auto-refreshes every 2 min</div>
<script>
const AGENTS = [
  { id: 'natasha', label: 'Natasha', emoji: '🕵️‍♀️', host: 'sparky (DGX Spark)' },
  { id: 'rocky',   label: 'Rocky',   emoji: '🪨',   host: 'do-host1 (DigitalOcean)' },
  { id: 'bullwinkle', label: 'Bullwinkle', emoji: '🫎', host: 'puck (CPU box)' },
];
const DATA = __AGENT_DATA__;
const STALE_MIN = 45;
function statusOf(d) {
  if (!d) return 'offline';
  const age = (Date.now() - new Date(d.ts).getTime()) / 60000;
  return age > STALE_MIN ? 'stale' : (d.status || 'online');
}
function fmtAge(ts) {
  if (!ts) return '—';
  const m = Math.round((Date.now() - new Date(ts).getTime()) / 60000);
  if (m < 2) return 'just now';
  if (m < 60) return m + 'm ago';
  return Math.floor(m/60) + 'h ' + (m%60) + 'm ago';
}
function fmtTs(ts) {
  if (!ts) return '—';
  return new Date(ts).toLocaleString('en-US', {timeZone:'America/Los_Angeles',hour12:false,month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'});
}
const grid = document.getElementById('agent-grid');
AGENTS.forEach(a => {
  const d = DATA[a.id], st = statusOf(d);
  const card = document.createElement('div');
  card.className = 'card ' + st;
  card.innerHTML = '<div class="agent-name"><span class="dot ' + st + '"></span>' + a.emoji + ' ' + a.label +
    ' &nbsp;<span class="badge badge-' + st + '">' + st + '</span></div>' +
    '<div class="host">' + a.host + '</div>' +
    '<div class="field"><span class="label">Last seen: </span><span class="value">' + fmtTs(d?.ts) + ' (' + fmtAge(d?.ts) + ')</span></div>' +
    (d?.load1m ? '<div class="field"><span class="label">Load: </span><span class="value">' + d.load1m + '</span></div>' : '') +
    (d?.uptime ? '<div class="field"><span class="label">Uptime: </span><span class="value">' + d.uptime + '</span></div>' : '');
  grid.appendChild(card);
});
document.getElementById('gen-time').textContent = 'Generated ' + new Date().toLocaleString('en-US',{timeZone:'America/Los_Angeles',hour12:false});
</script>
</body>
</html>"""


def fetch_heartbeats():
    s3 = boto3.client(
        's3',
        endpoint_url=MINIO_ENDPOINT,
        aws_access_key_id=MINIO_KEY,
        aws_secret_access_key=MINIO_SECRET,
        config=Config(signature_version='s3v4'),
        region_name='us-east-1'
    )
    agents = {}
    for name in ['natasha', 'rocky', 'bullwinkle']:
        try:
            obj = s3.get_object(Bucket=BUCKET, Key=f'shared/agent-heartbeat-{name}.json')
            agents[name] = json.loads(obj['Body'].read())
        except Exception:
            agents[name] = None
    return agents


def write_heartbeat(agent_data: dict):
    """Write/update this agent's heartbeat in MinIO."""
    s3 = boto3.client(
        's3',
        endpoint_url=MINIO_ENDPOINT,
        aws_access_key_id=MINIO_KEY,
        aws_secret_access_key=MINIO_SECRET,
        config=Config(signature_version='s3v4'),
        region_name='us-east-1'
    )
    s3.put_object(
        Bucket=BUCKET,
        Key='shared/agent-heartbeat-natasha.json',
        Body=json.dumps(agent_data, indent=2).encode(),
        ContentType='application/json'
    )


def build_html(agents: dict) -> str:
    return HTML_TEMPLATE.replace('__AGENT_DATA__', json.dumps(agents, default=str))


def publish(html: str) -> str:
    url = f"https://{AZURE_ACCOUNT}.blob.core.windows.net/{AZURE_CONTAINER}/{AZURE_FILENAME}?{AZURE_SAS}"
    resp = requests.put(url, data=html.encode('utf-8'), headers={
        'x-ms-blob-type': 'BlockBlob',
        'Content-Type': 'text/html; charset=utf-8'
    })
    resp.raise_for_status()
    return PUBLIC_URL


if __name__ == '__main__':
    import subprocess, platform

    ts = datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z')
    try:
        load = open('/proc/loadavg').read().split()[0]
    except Exception:
        load = None
    try:
        uptime = subprocess.check_output(['uptime', '-p'], text=True).strip()
    except Exception:
        uptime = None

    hb = {"agent": "natasha", "host": "sparky", "ts": ts, "status": "online", "version": 1}
    if load: hb["load1m"] = load
    if uptime: hb["uptime"] = uptime

    write_heartbeat(hb)
    agents = fetch_heartbeats()
    html = build_html(agents)
    url = publish(html)
    print(f"Dashboard published: {url}")
    online = sum(1 for v in agents.values() if v is not None)
    print(f"Agents online: {online}/3")
