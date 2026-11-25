#!/usr/bin/env python3
"""Compare Linux RSS using identical idle/load/scrape sequences for two binaries."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import statistics
import time
from integration import FakeCPA, exporter, get, record, value, wait_for


def memory(pid):
    fields = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            name, count, _ = line.split()
            fields[name.rstrip(":")] = int(count)
    return fields


def run(binary, records, scrapes):
    cpa = FakeCPA([])
    try:
        with exporter(binary, cpa, ["--poll-count=100", "--poll-idle-interval=50ms",
                                    "-label-source", "-label-auth-index", "-label-api-key-prefix-length=6"]) as (process, base):
            for _ in range(10):
                get(base + "/metrics")
            time.sleep(0.5)
            idle = memory(process.pid)
            with cpa.lock:
                cpa.records.extend(record(i) for i in range(records))
            wait_for(lambda: value(get(base + "/metrics"), "cpa_exporter_records_total") == records, timeout=30)
            for _ in range(scrapes):
                get(base + "/metrics")
            snapshots = []
            for _ in range(10):
                time.sleep(0.1)
                snapshots.append(memory(process.pid))
            loaded = memory(process.pid)
            return {"idle_rss_kib": idle["VmRSS"],
                    "loaded_rss_kib": statistics.median(s["VmRSS"] for s in snapshots),
                    "peak_rss_kib": max(loaded["VmHWM"], *(max(s["VmHWM"], s["VmRSS"]) for s in snapshots))}
    finally:
        cpa.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--go-binary", required=True)
    parser.add_argument("--rust-binary", required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--records", type=int, default=10000)
    parser.add_argument("--scrapes", type=int, default=50)
    args = parser.parse_args()
    result = {"system": platform.platform(), "records": args.records,
              "label_combinations": 100, "scrapes": args.scrapes,
              "repeats": args.repeats, "unit": "KiB", "binaries": {}}
    for name, binary in [("Go", args.go_binary), ("Rust", args.rust_binary)]:
        path = Path(binary)
        runs = [run(path, args.records, args.scrapes) for _ in range(args.repeats)]
        result["binaries"][name] = {
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "binary_bytes": path.stat().st_size, "runs": runs,
            "median": {key: statistics.median(r[key] for r in runs) for key in runs[0]}}
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
