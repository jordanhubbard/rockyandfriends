#!/usr/bin/env bash
# Description: Rebrand CCC → ACC (Agent Control Center)
#
# - Copies ~/.ccc → ~/.acc (backward-compat copy, not rename)
# - Adds ACC_* env var aliases in ~/.acc/.env for all CCC_* vars
# - Stops old ccc-* / com.ccc.* services
# - Installs new acc-* / com.acc.* services via install scripts
#
# Safe to run multiple times. All operations are idempotent.

set -euo pipefail

# ACC_DIR and CCC_DIR are set by run-migrations.sh; use fallbacks if sourced standalone
ACC_DIR_VAL="${ACC_DIR:-$HOME/.acc}"
CCC_DIR_VAL="${CCC_DIR:-$HOME/.ccc}"

m_info  "Rebrand: CCC → ACC (Agent Control Center)"

# ── Step 1: Copy ~/.ccc → ~/.acc ──────────────────────────────────────────
if [[ -d "$CCC_DIR_VAL" && ! -d "$ACC_DIR_VAL" ]]; then
  m_info "Copying ${CCC_DIR_VAL} → ${ACC_DIR_VAL} ..."
  cp -a "$CCC_DIR_VAL" "$ACC_DIR_VAL"
  m_success "Created ${ACC_DIR_VAL}"
elif [[ -d "$ACC_DIR_VAL" ]]; then
  m_skip "${ACC_DIR_VAL} already exists — skipping copy"
else
  m_warn "Neither ${CCC_DIR_VAL} nor ${ACC_DIR_VAL} found — this is a fresh node; skipping copy"
fi

# ── Step 2: Add ACC_* env var aliases to ~/.acc/.env ──────────────────────
ENV_FILE="${ACC_DIR_VAL}/.env"
if [[ -f "$ENV_FILE" ]]; then
  m_info "Adding ACC_* aliases to ${ENV_FILE} ..."
  # Map of old → new variable names
  declare -A VAR_MAP=(
    [CCC_URL]=ACC_URL
    [CCC_AGENT_TOKEN]=ACC_AGENT_TOKEN
    [CCC_AUTH_TOKENS]=ACC_AUTH_TOKENS
    [CCC_DATA_DIR]=ACC_DATA_DIR
    [CCC_LOG_LEVEL]=ACC_LOG_LEVEL
    [CCC_BIND_ADDR]=ACC_BIND_ADDR
    [CCC_HUB_URL]=ACC_HUB_URL
    [CCC_MINIO_ALIAS]=ACC_MINIO_ALIAS
    [CCC_MINIO_BUCKET]=ACC_MINIO_BUCKET
  )

  added=0
  while IFS='=' read -r key val; do
    [[ "$key" =~ ^# ]] && continue
    [[ -z "$key" ]] && continue
    new_key="${VAR_MAP[$key]:-}"
    if [[ -n "$new_key" ]] && ! grep -q "^${new_key}=" "$ENV_FILE" 2>/dev/null; then
      echo "${new_key}=${val}" >> "$ENV_FILE"
      ((added++)) || true
    fi
  done < "$ENV_FILE"

  if [[ $added -gt 0 ]]; then
    m_success "Added ${added} ACC_* aliases to ${ENV_FILE}"
  else
    m_skip "ACC_* aliases already present in ${ENV_FILE}"
  fi
else
  m_warn "No .env found at ${ENV_FILE} — skipping alias injection"
fi

# ── Step 3: Update MINIO_ALIAS from ccc-hub → acc-hub ────────────────────
if [[ -f "$ENV_FILE" ]]; then
  if grep -q "^MINIO_ALIAS=ccc-hub" "$ENV_FILE" 2>/dev/null; then
    sed -i.bak "s|^MINIO_ALIAS=ccc-hub|MINIO_ALIAS=acc-hub|g" "$ENV_FILE" && rm -f "${ENV_FILE}.bak"
    m_success "Updated MINIO_ALIAS: ccc-hub → acc-hub in ${ENV_FILE}"
  fi
  # Also reconfigure mc alias if mc is available
  if command -v mc &>/dev/null; then
    if mc alias ls ccc-hub &>/dev/null 2>&1; then
      # Copy ccc-hub config to acc-hub
      _mc_cfg="${HOME}/.mc/config.json"
      if [[ -f "$_mc_cfg" ]]; then
        python3 -c "
import json, sys
path = sys.argv[1]
d = json.load(open(path))
aliases = d.get('aliases', {})
if 'ccc-hub' in aliases and 'acc-hub' not in aliases:
    aliases['acc-hub'] = aliases['ccc-hub']
    d['aliases'] = aliases
    json.dump(d, open(path, 'w'), indent=2)
    print('acc-hub alias created from ccc-hub')
else:
    print('acc-hub already exists or ccc-hub not found')
" "$_mc_cfg" 2>/dev/null && m_success "mc alias acc-hub configured" || true
      fi
    fi
  fi
fi

# ── Step 4: Stop old ccc-* services ──────────────────────────────────────
m_info "Stopping old ccc-* services (best-effort)..."

if on_platform linux; then
  for svc in ccc-queue-worker ccc-bus-listener; do
    if systemctl is-active --quiet "$svc" 2>/dev/null; then
      sudo systemctl stop "$svc" 2>/dev/null && m_success "stopped $svc" || m_warn "failed to stop $svc"
    fi
    if systemctl is-enabled --quiet "$svc" 2>/dev/null; then
      sudo systemctl disable "$svc" 2>/dev/null || true
      m_success "disabled $svc"
    fi
  done
  sudo systemctl daemon-reload 2>/dev/null || true
fi

if on_platform macos; then
  for plist in \
    "${HOME}/Library/LaunchAgents/com.ccc.queue-worker.plist" \
    "${HOME}/Library/LaunchAgents/com.ccc.bus-listener.plist"; do
    if [[ -f "$plist" ]]; then
      launchctl unload "$plist" 2>/dev/null || true
      m_success "unloaded ${plist##*/}"
    fi
  done
fi

# supervisord
if command -v supervisorctl &>/dev/null 2>&1; then
  supervisorctl stop ccc-bus-listener 2>/dev/null && m_success "stopped supervisord ccc-bus-listener" || true
fi

# ── Step 5: Install new acc-* services ────────────────────────────────────
m_info "Installing new acc-* services..."
INSTALL_DIR="${WORKSPACE}/deploy"

if on_platform linux; then
  if [[ -f "${INSTALL_DIR}/install-bus-listener.sh" ]]; then
    bash "${INSTALL_DIR}/install-bus-listener.sh" linux && m_success "acc-bus-listener installed" \
      || m_warn "install-bus-listener.sh returned non-zero (check logs)"
  fi
  if [[ -f "${INSTALL_DIR}/install-queue-worker.sh" ]]; then
    bash "${INSTALL_DIR}/install-queue-worker.sh" linux && m_success "acc-queue-worker installed" \
      || m_warn "install-queue-worker.sh returned non-zero (check logs)"
  fi
fi

if on_platform macos; then
  if [[ -f "${INSTALL_DIR}/install-bus-listener.sh" ]]; then
    bash "${INSTALL_DIR}/install-bus-listener.sh" macos && m_success "acc-bus-listener installed" \
      || m_warn "install-bus-listener.sh returned non-zero (check logs)"
  fi
  if [[ -f "${INSTALL_DIR}/install-queue-worker.sh" ]]; then
    bash "${INSTALL_DIR}/install-queue-worker.sh" macos && m_success "acc-queue-worker installed" \
      || m_warn "install-queue-worker.sh returned non-zero (check logs)"
  fi
fi

# ── Done ──────────────────────────────────────────────────────────────────
touch "${ACC_DIR_VAL}/.rebrand-complete"
m_success "CCC → ACC rebrand complete on this node"
m_info "New services: acc-bus-listener, acc-queue-worker"
m_info "Config dir: ${ACC_DIR_VAL}"
m_info "Old ~/.ccc directory preserved for reference (safe to remove after verifying acc-* services)"
