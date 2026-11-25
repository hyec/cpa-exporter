"""Small deterministic CPA v8 fixture for the Compose integration test."""
import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

KEY = os.environ.get("MANAGEMENT_PASSWORD", "integration-secret")
LOCK = threading.Lock()
QUEUE = [
    {"timestamp": "2026-11-24T00:00:00Z", "latency_ms": 123,
     "provider": "gemini", "model": "gemini-2.5-flash", "alias": "flash",
     "endpoint": "/v1beta/models/gemini-2.5-flash:generateContent",
     "auth_type": "api-key", "source": "fake@example.com", "auth_index": "fake-1",
     "tokens": {"total_tokens": 999}, "failed": False,
     "token_breakdown": {"schema_version": 2, "total_tokens": 18,
                         "input": {"total_tokens": 7},
                         "output": {"total_tokens": 11, "reasoning_tokens": 3}}},
    {"timestamp": "2026-11-24T00:00:01Z", "latency_ms": 456,
     "provider": "claude", "model": "claude-sonnet-4", "alias": "sonnet",
     "endpoint": "/v1/messages", "auth_type": "oauth",
     "tokens": {"input_tokens": 5, "total_tokens": 5},
     "failed": True, "fail": {"status_code": 429}},
]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        url = urlsplit(self.path)
        if url.path == "/healthz":
            return self.reply(200, {"ok": True})
        if self.headers.get("Authorization") != "Bearer " + KEY:
            return self.reply(401, {"error": "unauthorized"})
        if url.path == "/v8/management/config/api-keys":
            return self.reply(200, {"gemini": [{"name": "Gemini Primary", "keys": [{"auth_index": "fake-1"}]}]})
        if url.path == "/v8/management/observability/usage/queue":
            try:
                count = int(parse_qs(url.query).get("count", [1])[0])
                if count <= 0:
                    raise ValueError()
            except ValueError:
                return self.reply(400, {"error": "count must be positive"})
            with LOCK:
                batch = QUEUE[:count]
                del QUEUE[:count]
            return self.reply(200, batch)
        return self.reply(404, {"error": "not_found"})


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8317), Handler).serve_forever()
