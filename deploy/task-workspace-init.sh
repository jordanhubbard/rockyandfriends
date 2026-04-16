#!/usr/bin/env bash
# task-workspace-init.sh — Bootstrap an isolated AgentFS-backed workspace for a task.
#
# Clones the task's git repo into a local workspace, then mirrors it to AgentFS
# (MinIO). The queue-worker calls this before handing work to claude or hermes.
# The workspace is the agent's exclusive working copy for the duration of the task.
#
# Usage:
#   eval "$(bash deploy/task-workspace-init.sh --task-id <id> [--repo <url>] [--branch <branch>])"
#
# Outputs KEY=VALUE lines suitable for eval. The caller gets:
#   TASK_ID, TASK_WORKSPACE_LOCAL, TASK_WORKSPACE_AGENTFS, TASK_BRANCH, TASK_REPO
#
# Environment (from ~/.ccc/.env):
#   MINIO_ALIAS, MINIO_BUCKET, MINIO_ENDPOINT — for AgentFS sync (optional)

set -euo pipefail

TASK_ID=""
REPO_URL=""
BRANCH="main"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --task-id) TASK_ID="$2"; shift 2 ;;
    --repo)    REPO_URL="$2"; shift 2 ;;
    --branch)  BRANCH="$2"; shift 2 ;;
    *) echo "ERROR: unknown argument: $1" >&2; exit 1 ;;
  esac
done

[[ -z "$TASK_ID" ]] && { echo "ERROR: --task-id required" >&2; exit 1; }

# Load env
[[ -f "$HOME/.ccc/.env" ]] && set -a && source "$HOME/.ccc/.env" && set +a

WORKSPACE_LOCAL="$HOME/.ccc/task-workspaces/$TASK_ID"
mkdir -p "$WORKSPACE_LOCAL"

# ── Determine repo URL ────────────────────────────────────────────────────────

if [[ -z "$REPO_URL" ]]; then
  # Try CCC workspace repo
  CCC_WS="$HOME/.ccc/workspace"
  if [[ -d "$CCC_WS/.git" ]]; then
    REPO_URL=$(git -C "$CCC_WS" remote get-url origin 2>/dev/null || echo "")
  fi
fi

# owner/repo shorthand → GitHub HTTPS
if [[ -n "$REPO_URL" && "$REPO_URL" != *"://"* && "$REPO_URL" != *"@"* ]]; then
  REPO_URL="https://github.com/$REPO_URL"
fi

# ── Clone or reuse ────────────────────────────────────────────────────────────

if [[ -d "$WORKSPACE_LOCAL/.git" ]]; then
  echo "→ Reusing workspace at $WORKSPACE_LOCAL" >&2
  git -C "$WORKSPACE_LOCAL" fetch origin 2>/dev/null || true
elif [[ -n "$REPO_URL" ]]; then
  echo "→ Cloning $REPO_URL ($BRANCH) → $WORKSPACE_LOCAL" >&2
  # Try with explicit branch first; fall back to default branch
  git clone --depth=1 --branch "$BRANCH" "$REPO_URL" "$WORKSPACE_LOCAL" 2>/dev/null \
    || git clone --depth=1 "$REPO_URL" "$WORKSPACE_LOCAL"
  SHA=$(git -C "$WORKSPACE_LOCAL" rev-parse --short HEAD 2>/dev/null || echo "?")
  echo "→ Cloned at $SHA" >&2
else
  echo "→ No repo URL — using empty workspace" >&2
fi

# ── Mirror to AgentFS ─────────────────────────────────────────────────────────

WORKSPACE_AGENTFS=""
MC_ALIAS="${MINIO_ALIAS:-ccc-hub}"
BUCKET="${MINIO_BUCKET:-agents}"

if command -v mc &>/dev/null && [[ -n "${MINIO_ENDPOINT:-}" ]]; then
  WORKSPACE_AGENTFS="${MC_ALIAS}/${BUCKET}/tasks/${TASK_ID}/workspace"
  echo "→ Mirroring to AgentFS: $WORKSPACE_AGENTFS" >&2
  mc mirror --overwrite --quiet "$WORKSPACE_LOCAL/" "$WORKSPACE_AGENTFS" 2>/dev/null \
    || echo "⚠ AgentFS mirror failed (non-fatal)" >&2

  # Write task metadata
  SHA=$(git -C "$WORKSPACE_LOCAL" rev-parse HEAD 2>/dev/null || echo "")
  META="{\"task_id\":\"$TASK_ID\",\"repo\":\"$REPO_URL\",\"branch\":\"$BRANCH\",\"sha\":\"$SHA\",\"initiated_at\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"agent\":\"${AGENT_NAME:-unknown}\"}"
  echo "$META" | mc pipe "${MC_ALIAS}/${BUCKET}/tasks/${TASK_ID}/meta.json" 2>/dev/null || true
else
  [[ -z "${MINIO_ENDPOINT:-}" ]] && echo "⚠ MINIO_ENDPOINT not set — skipping AgentFS sync" >&2
  [[ ! -x "$(command -v mc 2>/dev/null)" ]] && echo "⚠ mc not found — skipping AgentFS sync" >&2
fi

# ── Output env vars for eval ──────────────────────────────────────────────────

echo "TASK_ID=$TASK_ID"
echo "TASK_WORKSPACE_LOCAL=$WORKSPACE_LOCAL"
echo "TASK_WORKSPACE_AGENTFS=$WORKSPACE_AGENTFS"
echo "TASK_BRANCH=task/$TASK_ID"
echo "TASK_REPO=$REPO_URL"
