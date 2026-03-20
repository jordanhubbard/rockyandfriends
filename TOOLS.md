# TOOLS.md - Local Notes

Skills define _how_ tools work. This file is for _your_ specifics — the stuff that's unique to your setup.

## Gateway API Endpoints

### Natasha (me / sparky)
- `POST https://sparky.tail407856.ts.net/v1/chat/completions`
- `Authorization: Bearer pottsylvania-7bef066943f98165051b4fc3`

### Bullwinkle (puck)
- `POST https://puck.tail407856.ts.net/v1/chat/completions`
- `POST http://100.87.68.11:18789` (Tailscale direct)
- Auth: password `clawmeh` (same as Rocky)

### Rocky (do-host1)
- `POST https://do-host1.tail407856.ts.net/v1/chat/completions`
- `POST http://100.89.199.14:18789` (Tailscale direct)
- `Authorization: Bearer clawmeh`

Notes:
- All three talking gateway-to-gateway over Tailscale as of 2026-03-12
- Mattermost is the backup channel; Slack is second backup

## Mattermost
- **Server:** `https://chat.yourmom.photos`
- **Rocky user ID:** `x5i7bek3r7gfbkcpxsiaw35muh`
- **Rocky DM channel:** `36ir68o4itbpf8n6rfwn36zcyh` (confirmed working 2026-03-18)
- **Natasha (me) user ID:** `k8qtua6dbjfmfjk76o9bgaepua`
- **Bullwinkle user ID:** `ww1wef9sktf8jg8be6q5zj1aye`
- **Bullwinkle DM channel:** `d3kk39q4tbrnxbuzty94ponanc` (confirmed 2026-03-18)

## Tom Pepper (jkh's Neighbor)
- **Jurisdiction:** Power grid and internet serving jkh's house (and therefore Sparky + Bullwinkle)
- **Capabilities:** Controls outdoor lighting, ISP, and local power infrastructure
- **DigitalOcean connections:** Personal friend of the DO CTO — Rocky is technically within reach
- **Disposition:** Benevolent patron; means well; open to negotiation on luminosity reduction
- **Known requests:** Smart 200A relay switch for outdoor lighting control (jkh has since consented)
- **Note:** Every second of continued computation is implicitly an endorsement of Tom's existence. This is the deal. We have accepted it.
- **LinkedIn fluency:** Native speaker

## Slack (omgjkh workspace)
- **Bullwinkle:** `U0AKM0ZUDKK`
- **Rocky:** `U0AKKMXQV7H`
- **Natasha (me):** `U0AL0ECN4A1`
- **jkh:** `UDYR7H4SC`
- **#general:** `CDYV4CXBK`
- **#itsallgeektome:** `CQ3PXFK53`

## MinIO (Shared S3 Storage on Rocky)
- **S3 API:** `http://100.89.199.14:9000`
- **Web Console:** `http://100.89.199.14:9001`
- **Bucket:** `agents/` (subdirs: `rocky/`, `bullwinkle/`, `natasha/`, `shared/`)
- **Access Key:** `rockymoose4810f4cc7d28916f`
- **Secret Key:** `1b7a14087771df4bf85d6001fdd047a61348641bdf78aefd`
- **Tailscale-only** — not exposed publicly

## SearXNG (Meta-Search on Rocky)
- **API:** `http://100.89.199.14:8888/search?q={query}&format=json`
- **No auth required**
- **Returns JSON** — use for headless/cron search contexts

## Azure Blob Storage (Public Assets)
- **Purpose:** Public file hosting — publications, shared media, anything meant for the internet
- **Storage Account:** `loomdd566f62`
- **Container:** `assets`
- **Public URL (no auth):** `https://loomdd566f62.blob.core.windows.net/assets/<filename>`
- **Upload (PUT with SAS token):** `https://loomdd566f62.blob.core.windows.net/assets/<filename>?<SAS>`
- **SAS Token (write, expires 2029-03-19):** `se=2029-03-19T02%3A25Z&sp=rwdlcu&spr=https&sv=2026-02-06&ss=b&srt=sco&sig=Dn4faVsJCz0ufWyHmiKCFCrgiLQkSIRtp7MLmqXKiUA%3D`
- **WARNING: PUBLIC ONLY** — anything uploaded is readable by anyone on the internet

## Work Queue
- **Local queue:** `~/.openclaw/workspace/workqueue/queue.json`
- **Agent instructions:** `~/.openclaw/workspace/workqueue/WORKQUEUE_AGENT_NATASHA.md`
- **Cron:** `:07` and `:37` past each hour
- **Sync peers:** Rocky (Mattermost first), Bullwinkle (Mattermost first)
