#!/usr/bin/env python3
"""Dependency-free loopback server for the portable Forever Companion preview."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit
import json
import mimetypes
import os

APP = "forever-companion"
VERSION = "0.1.0-preview2"
ROOT = Path(__file__).resolve().parents[1] / "dist"
CSP = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
MIME = {".js": "text/javascript; charset=utf-8", ".webmanifest": "application/manifest+json", ".svg": "image/svg+xml", ".png": "image/png", ".json": "application/json", ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8"}


class Handler(BaseHTTPRequestHandler):
    server_version = "ForeverCompanion/0.1"

    def reply(self, status, content=b"", content_type="text/plain; charset=utf-8"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(content)))
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "same-origin")
        self.send_header("Content-Security-Policy", CSP)
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Service-Worker-Allowed", "/")
        if status == 405:
            self.send_header("Allow", "GET, HEAD")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(content)

    def do_GET(self):
        try:
            path = unquote(urlsplit(self.path).path, errors="strict")
            if "\0" in path or "\\" in path:
                return self.reply(400, b"Invalid path")
            if path == "/healthz":
                return self.reply(200, json.dumps({"app": APP, "version": VERSION}).encode(), "application/json")
            if path == "/api" or path.startswith("/api/"):
                return self.reply(503, json.dumps({"error": "live_bridge_unavailable", "message": "The game server connection is not configured."}).encode(), "application/json")
            relative = path.lstrip("/") or "index.html"
            file = (ROOT / relative).resolve()
            if not file.is_relative_to(ROOT.resolve()):
                return self.reply(403, b"Forbidden")
            if not file.is_file():
                return self.reply(404, b"Not found")
            return self.reply(200, file.read_bytes(), MIME.get(file.suffix.lower(), mimetypes.guess_type(str(file))[0] or "application/octet-stream"))
        except (OSError, ValueError, UnicodeError):
            return self.reply(404, b"Not found")

    do_HEAD = do_GET

    def unsupported(self):
        self.reply(405, b"Method not allowed")

    do_POST = do_PUT = do_PATCH = do_DELETE = do_OPTIONS = do_TRACE = do_CONNECT = unsupported


def main():
    port = int(os.environ.get("COMPANION_PORT", "4317"))
    if not 1 <= port <= 65535:
        raise SystemExit("COMPANION_PORT must be between 1 and 65535.")
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"Forever Companion {VERSION}: http://127.0.0.1:{port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
