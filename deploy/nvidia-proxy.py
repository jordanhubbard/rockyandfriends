#!/usr/bin/env python3
"""
nvidia-proxy.py — Local header-stripping HTTP proxy for NVIDIA LiteLLM inference.

Claude CLI sends anthropic-beta headers that NVIDIA's LiteLLM proxy rejects
with "invalid beta flag". This proxy strips those headers before forwarding.

Runs on 127.0.0.1:9099. Set ANTHROPIC_BASE_URL=http://localhost:9099 in .env.
Handles both streaming (chunked/SSE) and non-streaming responses.

Usage:
    python3 nvidia-proxy.py [--port 9099] [--target https://inference-api.nvidia.com]
"""
from __future__ import annotations

import argparse
import http.server
import ssl
import threading
import urllib.error
import urllib.request
from typing import Any

STRIP_REQUEST_HEADERS = {
    "anthropic-beta",
    "host",
    "content-length",  # urllib recalculates
    "transfer-encoding",
}

DEFAULT_PORT   = 9099
DEFAULT_TARGET = "https://inference-api.nvidia.com"

_target: str = DEFAULT_TARGET


class ProxyHandler(http.server.BaseHTTPRequestHandler):
    timeout = 600

    def log_message(self, fmt: str, *args: Any) -> None:
        pass  # silence per-request logs

    def _proxy(self, method: str) -> None:
        target_url = _target.rstrip("/") + self.path

        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length) if length > 0 else None

        fwd = {
            k: v for k, v in self.headers.items()
            if k.lower() not in STRIP_REQUEST_HEADERS
        }

        req = urllib.request.Request(target_url, data=body, headers=fwd, method=method)

        ctx = ssl.create_default_context()
        opener = urllib.request.build_opener(urllib.request.HTTPSHandler(context=ctx))

        try:
            resp = opener.open(req, timeout=600)
        except urllib.error.HTTPError as e:
            resp = e

        self.send_response(resp.status if hasattr(resp, "status") else resp.code)
        for k, v in resp.headers.items():
            if k.lower() in ("transfer-encoding", "connection"):
                continue
            self.send_header(k, v)
        self.end_headers()

        # Stream response body in chunks so SSE / large responses work
        while True:
            chunk = resp.read(4096)
            if not chunk:
                break
            try:
                self.wfile.write(chunk)
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                break
        resp.close()

    def do_GET(self) -> None:   self._proxy("GET")
    def do_POST(self) -> None:  self._proxy("POST")
    def do_PUT(self) -> None:   self._proxy("PUT")
    def do_DELETE(self) -> None: self._proxy("DELETE")
    def do_HEAD(self) -> None:  self._proxy("HEAD")


def main() -> None:
    global _target

    p = argparse.ArgumentParser(description="Header-stripping proxy for NVIDIA LiteLLM")
    p.add_argument("--port",   type=int, default=DEFAULT_PORT)
    p.add_argument("--target", default=DEFAULT_TARGET)
    args = p.parse_args()
    _target = args.target

    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), ProxyHandler)
    print(f"[nvidia-proxy] listening on 127.0.0.1:{args.port} → {_target}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
