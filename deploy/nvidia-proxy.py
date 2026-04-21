#!/usr/bin/env python3
"""
nvidia-proxy.py — Local header-stripping HTTP proxy for NVIDIA LiteLLM inference.

Claude CLI sends anthropic-beta headers that NVIDIA's LiteLLM proxy rejects
with "invalid beta flag". This proxy strips those headers before forwarding.

Also sanitizes malformed message histories on /v1/messages requests: if an
assistant message has tool_use blocks without corresponding tool_result blocks
in the next user message, synthetic tool_result error blocks are injected.
This prevents Bedrock HTTP 400 errors caused by interrupted tool calls.

Runs on 127.0.0.1:9099. Set ANTHROPIC_BASE_URL=http://localhost:9099 in .env.
Handles both streaming (chunked/SSE) and non-streaming responses.

Usage:
    python3 nvidia-proxy.py [--port 9099] [--target https://inference-api.nvidia.com]
"""
from __future__ import annotations

import argparse
import http.server
import json
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


# ── Message sanitizer ─────────────────────────────────────────────────────────

def _collect_tool_use_ids(msg: dict) -> list[str]:
    """Return tool_use IDs from an assistant message's content blocks."""
    if msg.get("role") != "assistant":
        return []
    content = msg.get("content")
    if not isinstance(content, list):
        return []
    return [
        b["id"] for b in content
        if isinstance(b, dict) and b.get("type") == "tool_use" and "id" in b
    ]


def _collect_tool_result_ids(msg: dict) -> set[str]:
    """Return the tool_use IDs covered by tool_result blocks in a user message."""
    if msg.get("role") != "user":
        return set()
    content = msg.get("content")
    if not isinstance(content, list):
        return set()
    return {
        b["tool_use_id"] for b in content
        if isinstance(b, dict) and b.get("type") == "tool_result" and "tool_use_id" in b
    }


def _normalize_openai_tool_calls(messages: list[dict]) -> int:
    """
    Convert assistant messages that carry OpenAI-format ``tool_calls`` arrays
    into Anthropic-native ``content`` arrays with ``tool_use`` blocks.

    This is the root cause of the "Unable to convert openai tool calls" HTTP 500
    from NVIDIA LiteLLM: a conversation history can contain Anthropic ``tooluse_*``
    IDs wrapped in the OpenAI ``{type:"function", function:{name, arguments}}``
    envelope, which LiteLLM cannot reconcile.

    OpenAI shape:
        {"role": "assistant", "content": null,
         "tool_calls": [{"id": "tooluse_xxx", "type": "function",
                         "function": {"name": "...", "arguments": "{...}"}}]}

    Anthropic shape:
        {"role": "assistant",
         "content": [{"type": "tool_use", "id": "tooluse_xxx",
                      "name": "...", "input": {...}}]}
    """
    converted = 0
    for msg in messages:
        if msg.get("role") != "assistant":
            continue
        tool_calls = msg.get("tool_calls")
        if not isinstance(tool_calls, list) or not tool_calls:
            continue

        content_blocks = []
        # Preserve any existing text content
        existing = msg.get("content")
        if isinstance(existing, str) and existing:
            content_blocks.append({"type": "text", "text": existing})

        for tc in tool_calls:
            fn_obj = tc.get("function") or {}
            tc_id   = tc.get("id", "")
            tc_name = fn_obj.get("name", "")
            args_str = fn_obj.get("arguments", "{}")
            try:
                tc_input = json.loads(args_str)
            except (json.JSONDecodeError, ValueError):
                tc_input = {}
            content_blocks.append({
                "type":  "tool_use",
                "id":    tc_id,
                "name":  tc_name,
                "input": tc_input,
            })

        msg.pop("tool_calls", None)
        msg["content"] = content_blocks
        converted += 1
    return converted


def _inject_missing_tool_results(messages: list[dict]) -> int:
    """
    Walk the messages array and inject synthetic tool_result user messages for
    any orphaned tool_use blocks. Returns the number of tool_use IDs fixed.
    """
    fixed = 0
    i = 0
    while i < len(messages):
        tool_use_ids = _collect_tool_use_ids(messages[i])
        if not tool_use_ids:
            i += 1
            continue

        covered = _collect_tool_result_ids(messages[i + 1]) if i + 1 < len(messages) else set()
        missing = [tid for tid in tool_use_ids if tid not in covered]

        if not missing:
            i += 1
            continue

        synthetic = {
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": tid,
                    "content": "[result unavailable — session was interrupted before this tool completed]",
                    "is_error": True,
                }
                for tid in missing
            ],
        }
        messages.insert(i + 1, synthetic)
        fixed += len(missing)
        i += 2  # skip past assistant message and newly inserted user message

    return fixed


def _sanitize_body(path: str, body: bytes | None) -> bytes | None:
    """
    If this is a /v1/messages request, parse the body and inject synthetic
    tool_result blocks for any orphaned tool_use IDs. Returns the (possibly
    rewritten) body, or the original body unchanged if no fix was needed.
    """
    if not body:
        return body
    if not (path.endswith("/v1/messages") or path == "/v1/messages"):
        return body

    try:
        payload = json.loads(body)
    except (json.JSONDecodeError, ValueError):
        return body

    messages = payload.get("messages")
    if not isinstance(messages, list):
        return body

    normalized = _normalize_openai_tool_calls(messages)
    if normalized > 0:
        print(
            f"[nvidia-proxy] normalized {normalized} OpenAI-format tool_calls → Anthropic tool_use",
            flush=True,
        )

    fixed = _inject_missing_tool_results(messages)
    if fixed > 0:
        print(
            f"[nvidia-proxy] injected {fixed} synthetic tool_result(s) for orphaned tool_use blocks",
            flush=True,
        )

    if normalized == 0 and fixed == 0:
        return body

    return json.dumps(payload).encode()


# ── Proxy handler ─────────────────────────────────────────────────────────────

class ProxyHandler(http.server.BaseHTTPRequestHandler):
    timeout = 600

    def log_message(self, fmt: str, *args: Any) -> None:
        pass  # silence per-request logs

    def _proxy(self, method: str) -> None:
        target_url = _target.rstrip("/") + self.path

        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length) if length > 0 else None

        body = _sanitize_body(self.path, body)

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
