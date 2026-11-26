#!/usr/bin/env python3
"""Binary-level CPA v8 and legacy RESP regression tests; standard library only."""
import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit
from urllib.request import urlopen

KEY = "integration-secret"


def record(i):
    return {"timestamp": "2026-11-24T00:00:00Z", "provider": "codex",
            "model": "m" + str(i % 100), "alias": "alias", "endpoint": "/v1/responses",
            "auth_type": "api-key", "auth_index": "a" + str(i % 100),
            "source": "sk-upstream-secret-1234", "api_key": "client-secret-5678",
            "latency_ms": 123, "failed": i % 5 == 0,
            "fail": {"status_code": 429 if i % 5 == 0 else 200},
            "tokens": {"total_tokens": 999},
            "token_breakdown": {"schema_version": 2, "total_tokens": 12,
                                "input": {"total_tokens": 7, "cache_read_tokens": 2, "cache_write_tokens": 1},
                                "output": {"total_tokens": 5, "reasoning_tokens": 3}}}


class FakeCPA:
    def __init__(self, records):
        self.records = list(records)
        self.lock = threading.Lock()
        self.config_status = 200
        self.queue_status = 200
        self.credentials = {"files": []}
        self.invalid_paths = []
        self.queue_times = []
        self.config = {"codex": [{"name": "主上游", "keys": [{"auth_index": "a" + str(i), "api-key": "never-log-this"} for i in range(100)]}]}
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                pass

            def handle(self):
                try:
                    super().handle()
                except (ConnectionResetError, BrokenPipeError):
                    pass

            def do_GET(self):
                url = urlsplit(self.path)
                status = 200
                if self.headers.get("Authorization") != "Bearer " + KEY:
                    status, data = 401, {"error": "unauthorized"}
                elif url.path == "/base/v8/management/credentials":
                    data = owner.credentials
                elif url.path == "/base/v8/management/config/api-keys":
                    status, data = owner.config_status, owner.config
                elif url.path == "/base/v8/management/observability/usage/queue":
                    count = int(parse_qs(url.query)["count"][0])
                    with owner.lock:
                        owner.queue_times.append(time.monotonic())
                        status = owner.queue_status
                        if status == 200:
                            data, owner.records = owner.records[:count], owner.records[count:]
                        else:
                            data = {"error": "never-log-this"}
                else:
                    owner.invalid_paths.append(url.path)
                    status, data = 404, {"error": "not_found"}
                body = json.dumps(data, ensure_ascii=False).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.url = "http://127.0.0.1:" + str(self.server.server_port) + "/base"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def get(url):
    with urlopen(url, timeout=3) as response:
        return response.read().decode()


def wait_for(fn, timeout=10):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            result = fn()
            if result:
                return result
        except (OSError, ValueError):
            pass
        time.sleep(0.03)
    raise AssertionError("condition did not become true before deadline")


def samples(text):
    result = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        match = re.fullmatch(r'([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{(.*)\})? ([^ ]+)', line)
        assert match, line
        labels = tuple(sorted((k, json.loads('"' + v + '"')) for k, v in re.findall(r'(\w+)="((?:\\.|[^"\\])*)"', match[2] or "")))
        result[match[1], labels] = float(match[3])
    return result


def value(text, name):
    return sum(v for (n, _), v in samples(text).items() if n == name)


@contextlib.contextmanager
def exporter(binary, cpa, extra=(), check_logs=True):
    port = free_port()
    env = {k: v for k, v in os.environ.items() if not k.startswith("CPA_")}
    env["CPA_MANAGEMENT_KEY"] = KEY
    process = subprocess.Popen([str(Path(binary).resolve()), "--cpa-url=" + cpa.url,
                                "--listen-address=127.0.0.1:" + str(port),
                                "--poll-interval=50ms", "--poll-idle-interval=200ms",
                                "--poll-count=7", *extra], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    base = "http://127.0.0.1:" + str(port)
    try:
        wait_for(lambda: get(base + "/-/healthy") == "ok\n")
        yield process, base
    finally:
        process.terminate()
        try:
            _, stderr = process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
            raise AssertionError("SIGTERM shutdown exceeded three seconds")
        assert KEY.encode() not in stderr
        if check_logs:
            assert b"never-log-this" not in stderr
        assert b"sk-upstream-secret" not in stderr
        assert process.returncode == 0, (process.returncode, stderr.decode())


def collect(binary, check_logs=True):
    records = [record(i) for i in range(35)]
    records += ["malformed record", {"provider": "gemini", "source": "sk-unknown-secret", "tokens": {"input_tokens": 3, "output_tokens": 4}}]
    cpa = FakeCPA(records)
    try:
        with exporter(binary, cpa, ["-label-source", "-label-auth-index", "-label-api-key-prefix-length=6", "--metrics-path=custom"], check_logs=check_logs) as (_, base):
            text = wait_for(lambda: (t if value(t, "cpa_exporter_records_total") == 36 else None) if (t := get(base + "/custom")) else None)
            assert not cpa.invalid_paths
            assert value(text, "cpa_requests_total") == 36
            assert value(text, "cpa_exporter_decode_errors_total") == 1
            assert 'name="主上游"' in text
            assert 'name="unknown"' in text
            assert 'api_key_prefix="client"' in text
            assert 'source="sk-u***1234"' in text
            assert "sk-upstream-secret" not in text
            assert "client-secret-5678" not in text
            assert sum(v for (n, labels), v in samples(text).items() if n == "cpa_tokens_total" and ("token_type", "total") in labels) == 35 * 12 + 7
            # Empty queue uses idle polling. A failed poll sets health to zero;
            # successful subsequent requests restore it without restarting.
            time.sleep(0.5)
            with cpa.lock:
                intervals = [b-a for a, b in zip(cpa.queue_times[-3:], cpa.queue_times[-2:])]
            assert intervals and max(intervals) > 0.15
            cpa.queue_status = 503
            wait_for(lambda: value(get(base + "/custom"), "cpa_exporter_up") == 0)
            cpa.queue_status = 200
            wait_for(lambda: value(get(base + "/custom"), "cpa_exporter_up") == 1)
            return samples(text)
    finally:
        cpa.close()


def test_names_disabled(binary):
    cpa = FakeCPA([record(1)])
    try:
        with exporter(binary, cpa, ["--label-name=false"]) as (_, base):
            text = wait_for(lambda: (t if value(t, "cpa_requests_total") == 1 else None) if (t := get(base + "/metrics")) else None)
            assert "name=" not in text
            assert "source=" not in text
            assert "api_key_prefix=" not in text
            assert "auth_index=" not in text
    finally:
        cpa.close()


def test_codex_oauth_email(binary):
    oauth = record(1)
    oauth.update(auth_type="oauth", source="user@example.com", auth_index="oauth-account")
    invalid = dict(oauth, source="sk-not-an-email", auth_index="invalid-account")
    cpa = FakeCPA([oauth, invalid])
    try:
        with exporter(binary, cpa) as (_, base):
            text = wait_for(lambda: (t if value(t, "cpa_requests_total") == 2 else None) if (t := get(base + "/metrics")) else None)
            assert 'name="***r@example.com"' in text
            assert 'name="unknown"' in text
            assert 'user@example.com' not in text
            assert 'sk-not-an-email' not in text
            assert 'source=' not in text
        with cpa.lock:
            cpa.records.append(oauth)
        with exporter(binary, cpa, ["--label-name=false"]) as (_, base):
            text = wait_for(lambda: (t if value(t, "cpa_requests_total") == 1 else None) if (t := get(base + "/metrics")) else None)
            assert 'name=' not in text
            assert 'example.com' not in text
    finally:
        cpa.close()


def test_credential_note(binary):
    oauth = record(1)
    oauth.update(auth_type="oauth", source="user@example.com", auth_index="oauth-account")
    cpa = FakeCPA([oauth])
    cpa.credentials = {"files": [{"auth_index": "oauth-account", "note": '  工作账号 "A"\\测试  ', "email": "never-log-this@example.com"}]}
    try:
        with exporter(binary, cpa) as (_, base):
            text = wait_for(lambda: (t if value(t, "cpa_requests_total") == 1 else None) if (t := get(base + "/metrics")) else None)
            assert any(("name", '工作账号 "A"\\测试') in labels for (n, labels) in samples(text) if n == "cpa_requests_total")
            assert 'provider_name=' not in text
            assert 'user@example.com' not in text
            assert 'never-log-this' not in text
    finally:
        cpa.close()


def read_resp(reader):
    prefix = reader.read(1)
    line = reader.readline().rstrip(b"\r\n")
    if prefix == b"*":
        return [read_resp(reader) for _ in range(int(line))]
    if prefix == b"$":
        payload = reader.read(int(line))
        assert reader.read(2) == b"\r\n"
        return payload
    raise AssertionError("unexpected command")


def test_redis(binary):
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    errors = []
    done = threading.Event()

    def serve():
        try:
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(5)
                reader = conn.makefile("rb")
                assert read_resp(reader) == [b"AUTH", KEY.encode()]
                conn.sendall(b"+OK\r\n")
                assert read_resp(reader) == [b"SUBSCRIBE", b"usage"]
                conn.sendall(b"*3\r\n$9\r\nsubscribe\r\n$5\r\nusage\r\n:1\r\n")
                payload = json.dumps(record(1)).encode()
                # Partial payload across a read timeout must not corrupt framing.
                conn.sendall(b"*3\r\n$7\r\nmessage\r\n$5\r\nusage\r\n$" + str(len(payload)).encode() + b"\r\n" + payload[:10])
                time.sleep(1.2)
                conn.sendall(payload[10:] + b"\r\n")
                done.wait(5)
                reader.close()
        except Exception as exc:
            errors.append(exc)
        finally:
            listener.close()

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    cpa = FakeCPA([])
    try:
        with exporter(binary, cpa, ["--mode=redis", "--label-name=false", "--redis-addr=127.0.0.1:" + str(listener.getsockname()[1])]) as (_, base):
            wait_for(lambda: value(get(base + "/metrics"), "cpa_requests_total") == 1)
    finally:
        done.set()
        thread.join(5)
        cpa.close()
    assert not errors, errors


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--reference-binary", help="optional previous Go binary for exact business metric comparison")
    args = parser.parse_args()
    actual = collect(args.binary)
    test_names_disabled(args.binary)
    test_codex_oauth_email(args.binary)
    test_credential_note(args.binary)
    test_redis(args.binary)
    if args.reference_binary:
        reference = collect(args.reference_binary, check_logs=False)
        business = lambda data: {key: val for key, val in data.items() if not key[0].startswith("cpa_exporter_")}
        a, b = business(actual), business(reference)
        differences = [(key, a.get(key), b.get(key)) for key in a.keys() | b.keys() if a.get(key) != b.get(key)]
        assert not differences, f"Rust metrics differ from Go: {differences[:5]}"
        print("Rust/Go business metrics match exactly")
    print("Integration passed: v8 queue, labels, canonical tokens, malformed records, idle polling, error recovery, legacy RESP framing, SIGTERM")


if __name__ == "__main__":
    main()
