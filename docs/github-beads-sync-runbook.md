# GitHub, Beads, and Fleet Task Sync

ACC treats `/api/tasks` as the fleet work plane, `bd` as the durable issue
tracker, and GitHub Issues as the external contributor surface. The
`github_sync` tool keeps those three linked without auto-dispatching every
external issue.

## Metadata Schema

Every beads issue with a GitHub counterpart must carry this metadata shape:

```json
{
  "source": "github",
  "github_number": 42,
  "github_repo": "jordanhubbard/ACC",
  "github_url": "https://github.com/jordanhubbard/ACC/issues/42",
  "fleet_task_id": "task-abc123",
  "github_labels": ["bug", "agent-ready"],
  "github_author": "external-contributor"
}
```

`fleet_task_id` is `null` until a triaged GitHub issue is promoted to a fleet
task. The server also exposes these fields from `/api/issues` at top level and
under `metadata`.

## Polling Sync

Configure the hub or operator node:

```bash
GITHUB_REPOS=jordanhubbard/ACC
GITHUB_DISPATCH_LABEL=agent-ready
GITHUB_SYNC_INTERVAL=300
ACC_URL=https://acc.example
ACC_AGENT_TOKEN=<agent-token>
```

Run once:

```bash
github_sync --once
```

Run as a daemon:

```bash
github_sync --daemon
```

New GitHub issues become beads issues. Issues with the dispatch label also
create `/api/tasks` work items with `metadata.beads_id`, `metadata.github_*`,
and `source=github`. Re-running the sync is idempotent.

## Backfill Existing Links

Old links stored as `notes` key-value pairs or `[gh:owner/repo#number]` title
suffixes can be migrated in place:

```bash
github_sync --migrate-metadata --dry-run
github_sync --migrate-metadata
```

## Beads to GitHub Mirror

Use the opt-in mirror command when work should be visible to external
contributors:

```bash
github_sync --mirror \
  --mirror-repo jordanhubbard/ACC \
  --title "Describe the issue" \
  --description "Context for GitHub and beads" \
  --labels bug,agent-ready \
  --priority 1 \
  --issue-type bug
```

The command creates the GitHub issue, creates a linked beads issue with the
metadata schema above, and edits the GitHub issue body to reference the beads
ID.

## Completion Flow

When a linked fleet task completes, `acc-server` comments on the GitHub issue.
If `GITHUB_AUTO_CLOSE=true`, it also closes the GitHub issue. The sync tool also
closes linked open beads issues for completed GitHub-sourced fleet tasks.

When a linked beads issue is closed first, the sync tool closes the GitHub issue
only if `GITHUB_AUTO_CLOSE=true`.

## Systemd Timer

Install `deploy/systemd/acc-github-sync.service` and
`deploy/systemd/acc-github-sync.timer` on the hub/operator node after
`github_sync`, `bd`, and `gh` are in the service `PATH`.

```bash
systemctl --user enable --now acc-github-sync.timer
```

Use the service logs to debug credentials, missing repos, or failed fleet task
creation:

```bash
journalctl --user -u acc-github-sync.service -n 100
```
