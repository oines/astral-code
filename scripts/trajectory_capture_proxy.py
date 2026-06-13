#!/usr/bin/env python3
"""Capture model API request/response fixtures while forwarding to an upstream.

This intentionally uses only the Python standard library so it can run in a
fresh checkout. Point Astral or Claude Code at the printed local base URL, and
the proxy will forward each request to --upstream-base while writing redacted
JSON fixtures to --dump-dir.
"""

import argparse
import http.client
import json
import os
import signal
import ssl
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from typing import Any
from typing import Optional
from urllib.parse import SplitResult
from urllib.parse import urlsplit
from urllib.parse import urlunsplit


HOP_BY_HOP_HEADERS = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
}

REDACTED_HEADERS = {
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "anthropic-api-key",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Forward model API traffic while dumping normalized fixtures."
    )
    parser.add_argument("--upstream-base", required=True)
    parser.add_argument("--dump-dir", required=True)
    parser.add_argument("--listen-host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--server-info")
    parser.add_argument("--max-dump-bytes", type=int, default=20 * 1024 * 1024)
    return parser.parse_args()


def redact_headers(headers: dict[str, str]) -> dict[str, str]:
    redacted: dict[str, str] = {}
    for key, value in headers.items():
        if key.lower() in REDACTED_HEADERS:
            redacted[key] = "<redacted>"
        else:
            redacted[key] = value
    return redacted


def parse_json_or_text(body: bytes) -> Any:
    if not body:
        return None
    text = body.decode("utf-8", errors="replace")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def combine_path(upstream: SplitResult, request_path: str) -> str:
    base_path = upstream.path.rstrip("/")
    if not request_path.startswith("/"):
        request_path = f"/{request_path}"
    path_and_query = f"{base_path}{request_path}"
    if upstream.query:
        separator = "&" if "?" in path_and_query else "?"
        path_and_query = f"{path_and_query}{separator}{upstream.query}"
    return path_and_query


class CaptureServer(ThreadingHTTPServer):
    def __init__(
        self,
        server_address: tuple[str, int],
        handler_class: type[BaseHTTPRequestHandler],
        upstream_base: str,
        dump_dir: Path,
        max_dump_bytes: int,
    ) -> None:
        super().__init__(server_address, handler_class)
        self.upstream = urlsplit(upstream_base)
        if self.upstream.scheme not in {"http", "https"}:
            raise ValueError("--upstream-base must use http or https")
        self.dump_dir = dump_dir
        self.dump_dir.mkdir(parents=True, exist_ok=True)
        self.max_dump_bytes = max_dump_bytes
        self._sequence = 0
        self._sequence_lock = threading.Lock()

    def next_sequence(self) -> int:
        with self._sequence_lock:
            self._sequence += 1
            return self._sequence


class CaptureHandler(BaseHTTPRequestHandler):
    server: CaptureServer

    def do_GET(self) -> None:
        self.forward()

    def do_POST(self) -> None:
        self.forward()

    def do_DELETE(self) -> None:
        self.forward()

    def do_OPTIONS(self) -> None:
        self.forward()

    def log_message(self, format: str, *args: object) -> None:
        sys.stderr.write(
            "%s - - [%s] %s\n"
            % (self.address_string(), self.log_date_time_string(), format % args)
        )

    def forward(self) -> None:
        started = time.time()
        sequence = self.server.next_sequence()
        body = self.read_request_body()
        request_headers = dict(self.headers.items())
        upstream_path = combine_path(self.server.upstream, self.path)
        response_status = 502
        response_reason = "Bad Gateway"
        response_headers: dict[str, str] = {}
        response_body = bytearray()
        error = None

        try:
            connection = self.open_upstream_connection()
            headers = self.forward_headers(request_headers, body)
            connection.request(self.command, upstream_path, body=body, headers=headers)
            response = connection.getresponse()
            response_status = response.status
            response_reason = response.reason
            response_headers = dict(response.getheaders())
            self.send_response(response_status, response_reason)
            for key, value in response_headers.items():
                if key.lower() not in HOP_BY_HOP_HEADERS:
                    self.send_header(key, value)
            self.end_headers()

            while True:
                chunk = response.read(64 * 1024)
                if not chunk:
                    break
                if len(response_body) < self.server.max_dump_bytes:
                    remaining = self.server.max_dump_bytes - len(response_body)
                    response_body.extend(chunk[:remaining])
                self.wfile.write(chunk)
                self.wfile.flush()
            connection.close()
        except Exception as exc:  # noqa: BLE001 - proxy must report transport errors.
            error = repr(exc)
            if not self.wfile.closed:
                self.send_error(502, message=error)
        finally:
            self.dump_fixture(
                sequence=sequence,
                started=started,
                request_headers=request_headers,
                request_body=body,
                upstream_path=upstream_path,
                response_status=response_status,
                response_reason=response_reason,
                response_headers=response_headers,
                response_body=bytes(response_body),
                response_truncated=len(response_body) >= self.server.max_dump_bytes,
                error=error,
            )

    def read_request_body(self) -> bytes:
        length = int(self.headers.get("content-length", "0") or "0")
        if length == 0:
            return b""
        return self.rfile.read(length)

    def open_upstream_connection(self) -> http.client.HTTPConnection:
        upstream = self.server.upstream
        port = upstream.port
        if upstream.scheme == "https":
            return http.client.HTTPSConnection(
                upstream.hostname,
                port,
                context=ssl.create_default_context(),
                timeout=120,
            )
        return http.client.HTTPConnection(upstream.hostname, port, timeout=120)

    def forward_headers(self, headers: dict[str, str], body: bytes) -> dict[str, str]:
        forwarded = {
            key: value
            for key, value in headers.items()
            if key.lower() not in HOP_BY_HOP_HEADERS and key.lower() != "host"
        }
        forwarded["Content-Length"] = str(len(body))
        return forwarded

    def dump_fixture(
        self,
        *,
        sequence: int,
        started: float,
        request_headers: dict[str, str],
        request_body: bytes,
        upstream_path: str,
        response_status: int,
        response_reason: str,
        response_headers: dict[str, str],
        response_body: bytes,
        response_truncated: bool,
        error: Optional[str],
    ) -> None:
        fixture = {
            "sequence": sequence,
            "started_at_ms": int(started * 1000),
            "duration_ms": int((time.time() - started) * 1000),
            "method": self.command,
            "client_path": self.path,
            "upstream_base": urlunsplit(
                (
                    self.server.upstream.scheme,
                    self.server.upstream.netloc,
                    self.server.upstream.path,
                    "",
                    "",
                )
            ),
            "upstream_path": upstream_path,
            "request": {
                "headers": redact_headers(request_headers),
                "body": parse_json_or_text(request_body),
            },
            "response": {
                "status": response_status,
                "reason": response_reason,
                "headers": redact_headers(response_headers),
                "body": parse_json_or_text(response_body),
                "truncated": response_truncated,
            },
            "error": error,
        }
        path = self.server.dump_dir / f"{sequence:06d}-{int(started * 1000)}.json"
        path.write_text(
            json.dumps(fixture, ensure_ascii=False, indent=2), encoding="utf-8"
        )


def main() -> None:
    args = parse_args()
    server = CaptureServer(
        (args.listen_host, args.port),
        CaptureHandler,
        args.upstream_base,
        Path(args.dump_dir),
        args.max_dump_bytes,
    )
    host, port = server.server_address
    info = {
        "pid": os.getpid(),
        "host": host,
        "port": port,
        "base_url": f"http://{host}:{port}",
        "dump_dir": str(Path(args.dump_dir).resolve()),
        "upstream_base": args.upstream_base,
    }
    if args.server_info:
        Path(args.server_info).write_text(json.dumps(info) + "\n", encoding="utf-8")
    print(json.dumps(info), flush=True)

    def shutdown(_signum: int, _frame: object) -> None:
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, shutdown)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
