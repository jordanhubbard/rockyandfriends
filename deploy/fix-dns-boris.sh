#!/usr/bin/env bash
# Repair Boris DNS after Tailscale enrollment.
#
# Boris is on NVIDIA infrastructure. Do not use Tailscale MagicDNS as the
# global recursive resolver when accept-dns=false; 100.100.100.100 is reachable
# for tailnet names but returns SERVFAIL for public names such as slack.com in
# this mode. Keep Tailscale DNS disabled and use NVIDIA DHCP resolvers.
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    exec sudo -n "$0" "$@"
  fi
  echo "fix-dns-boris: must run as root or via passwordless sudo" >&2
  exit 1
fi

if command -v tailscale >/dev/null 2>&1; then
  tailscale set --accept-dns=false >/dev/null 2>&1 || true
fi

mkdir -p /etc/systemd/resolved.conf.d

if [ -f /etc/systemd/resolved.conf.d/99-bullwinkle-dns.conf ]; then
  mv /etc/systemd/resolved.conf.d/99-bullwinkle-dns.conf \
    /etc/systemd/resolved.conf.d/99-bullwinkle-dns.conf.disabled
fi

tmp="$(mktemp)"
printf '%s\n' \
  "# Boris DNS fix — ACC Slack gateway requires public Slack DNS plus NVIDIA internal names." \
  "# Keep Tailscale DNS disabled before/while bringing Tailscale up:" \
  "#   tailscale set --accept-dns=false" \
  "[Resolve]" \
  "DNS=10.63.172.197 10.10.10.53 10.10.10.54" \
  "FallbackDNS=" \
  "Domains=nvidia.com cs1cloud.internal" > "$tmp"
install -m 0644 "$tmp" /etc/systemd/resolved.conf.d/99-boris-dns.conf
rm -f "$tmp"

if command -v systemctl >/dev/null 2>&1; then
  systemctl restart systemd-resolved
fi

if command -v getent >/dev/null 2>&1; then
  getent hosts slack.com >/dev/null
  getent hosts api.slack.com >/dev/null
fi

echo "fix-dns-boris: DNS repaired; slack.com and api.slack.com resolve"
