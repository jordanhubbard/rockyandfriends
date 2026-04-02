/**
 * AdaptiveTrust — Guardrails shim.
 *
 * Canonical implementation: rcc/trust/adaptive-trust.mjs (functional API)
 * This module exposes the original class-based API as a thin wrapper for
 * backward compatibility with any future consumers expecting AdaptiveTrust class.
 *
 * Trust levels (canonical, from trust/adaptive-trust.mjs):
 *   none  — always block (post-revocation default)
 *   ask   — ask user before proceeding (default)
 *   auto  — proceed without asking (explicit grant only)
 *
 * Safety floor: 'auto' NEVER granted automatically. Explicit grantTrust() only.
 */

import {
  getTrustLevel,
  recordSuccess,
  recordFailure,
  grantTrust,
  revokeTrust,
  getTrustProfile,
  summarizeTrust,
} from '../trust/adaptive-trust.mjs';

export class AdaptiveTrust {
  constructor(_opts = {}) {
    // No state — all state is persisted in ~/.rcc/trust/<agent>.json
  }

  getTrustLevel(agentName, service, operation) {
    return getTrustLevel(agentName, service, operation);
  }

  recordSuccess(agentName, service, operation) {
    return recordSuccess(agentName, service, operation);
  }

  recordFailure(agentName, service, operation, reason, revoke = false) {
    return recordFailure(agentName, service, operation, reason, revoke);
  }

  grantTrust(agentName, service, operation, grantedBy) {
    return grantTrust(agentName, service, operation, grantedBy);
  }

  revokeTrust(agentName, service, operation, reason) {
    return revokeTrust(agentName, service, operation, reason);
  }

  getTrustProfile(agentName) {
    return getTrustProfile(agentName);
  }

  summarizeTrust(agentName) {
    return summarizeTrust(agentName);
  }
}

// Also re-export functional API for direct callers
export { getTrustLevel, recordSuccess, recordFailure, grantTrust, revokeTrust, getTrustProfile, summarizeTrust };

export function getTrust(opts = {}) {
  return new AdaptiveTrust(opts);
}
