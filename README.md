# CLI Proxy API Prometheus Exporter

A Rust Prometheus exporter for [CLI Proxy API](https://github.com/router-for-me/CLIProxyAPI) usage records.

The exporter consumes CPA's usage queue and exposes aggregated metrics on `/metrics`.

## CPA prerequisites

Targets CPA v8 (API contract checked against the v8.1.2 source). HTTP polling uses
`GET /v8/management/observability/usage/queue?count=N`. The legacy v0 API is no longer used.

CPA must have usage statistics and management access enabled:

```yaml
observability:
  usage:
    usage-statistics-enabled: true
management:
  allow-remote: true
  secret-key: "<bcrypt hash managed by CPA>"
```

You can also use CPA's `MANAGEMENT_PASSWORD` environment variable. The exporter sends the key with `Authorization: Bearer <key>`.

## Run

HTTP polling mode is recommended:

```bash
CPA_MANAGEMENT_KEY='your-management-key' \
cargo run --release -- \
  --mode=http \
  --cpa-url=http://127.0.0.1:8317 \
  --poll-interval=5s \
  --poll-idle-interval=30s
```

Redis RESP subscription is retained only for older CPA versions. **CPA v8 disables
the local RESP usage output; use HTTP mode with v8.** Disable the v8 name lookup
when collecting from a legacy server:

```bash
CPA_MANAGEMENT_KEY='your-management-key' \
cargo run --release -- \
  --label-provider-name=false \
  --mode=redis \
  --redis-addr=127.0.0.1:8317
```

When Redis mode is subscribed, CPA publishes new usage records directly to subscribers instead of storing them in the FIFO queue.

In HTTP mode, `--poll-interval` is used after the exporter receives usage records. When the queue is empty, the exporter waits `--poll-idle-interval` before checking again, reducing empty requests during quiet periods.

Verbose collector logs are available for troubleshooting:

```bash
--verbose
```

or:

```bash
CPA_EXPORTER_VERBOSE=true
```

## Metrics

- `cpa_requests_total{provider,provider_name,model,alias,endpoint,auth_type,failed,status_code}`
- `cpa_tokens_total{provider,provider_name,model,alias,endpoint,auth_type,token_type}`
- `cpa_request_latency_seconds{provider,provider_name,model,alias,endpoint,auth_type,failed}`
- `cpa_usage_record_timestamp_seconds{provider,provider_name,model,alias,endpoint,auth_type}`
- `cpa_exporter_records_total{mode}`
- `cpa_exporter_decode_errors_total{mode}`
- `cpa_exporter_collector_errors_total{mode}`
- `cpa_exporter_up{mode}`

By default, `api_key` and `request_id` are never exported as labels. Optional account-level labels are available with:

```bash
--label-source --label-auth-index
```

`--label-source` exports a masked `source` label instead of the raw value. Email sources mask the local part with the same rule as API keys while keeping the domain, such as `user***1234@example.com` for long values or `***r@example.com` for short values. Non-email sources keep the first 4 and last 4 characters for long values with a compact `***` mask; short values mask at least two thirds of the characters. Use these only when you are comfortable with the extra cardinality.

You can also opt in to an API key prefix label:

```bash
--label-api-key-prefix-length=5
```

This adds `api_key_prefix` to request, token, latency, and last-record-time metrics. `0` disables it, and values greater than `64` are rejected. Even a prefix can reveal part of a credential, so enable this only for private Prometheus deployments.

## Test

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
python3 scripts/integration.py --binary target/release/cpa-exporter
```

Docker Compose integration test:

```bash
scripts/integration-compose.sh
```

The compose test starts a fake CPA management API plus the exporter, then verifies that `/metrics` contains usage-derived counters.

## Upstream names and Grafana

`provider_name` is enabled by default on request, token, latency, and last-record
metrics. The exporter reads `GET /v8/management/config/api-keys` at startup and
every minute, joining each key's `auth_index` to its group's `name`. Name lookup
requires HTTP management access to the same CPA instance as the
usage collector. CPA v8 uses HTTP collection; legacy Redis mode has no v8
provider-group metadata.

Names are shown exactly as configured, including Unicode. Secret `source` and
`api_key` values are never used as a name fallback. Unnamed, OAuth, unmatched,
or ambiguous deduplicated credentials use `provider_name="unknown"`. Failed
refreshes retain the last catalog and log an error; a missing API-key subtree
clears the catalog. Renaming a group creates new Prometheus series for future
records; historical labels are unchanged.

Disable lookup and the label with `--label-provider-name=false` or
`CPA_EXPORTER_LABEL_PROVIDER_NAME=false`. Optional source/auth-index/key-prefix
labels remain independent; none are needed to display upstream names.

Import [grafana/cpa-exporter.json](grafana/cpa-exporter.json) and select your
Prometheus datasource. Its upstream selector, legends, and grouping use
`provider_name`, for example:

```promql
sum by (provider_name) (rate(cpa_requests_total[$__rate_interval]))
sum by (provider_name, token_type) (rate(cpa_tokens_total[$__rate_interval]))
```

The management config response may include credentials. The exporter decodes
only names and indexes and does not persist or log response bodies.

For v8 records, token metrics use canonical `token_breakdown` accounting rather
than legacy `tokens` when present. Input/output totals include their cache and
reasoning subsets; do not sum all `token_type` series together. Use
`token_type="total"` for total usage. Legacy records without a breakdown retain
the earlier behavior.

## Implementation and deployment

The exporter is implemented in Rust with a blocking HTTP client and one
collector thread, without an async runtime or garbage collector. The metrics
endpoint uses a small HTTP server. Release builds enable LTO and strip symbols.
Prometheus series are kept in memory for the process lifetime, so high label
cardinality still increases memory use.

Build with Rust 1.94 or later:

```bash
cargo build --locked --release
CPA_MANAGEMENT_KEY=your-management-key target/release/cpa-exporter
```

Existing CLI flags, including Go-style single-dash flags such as
`-label-source` and `-label-api-key-prefix-length=6`, and environment variable
names are preserved. Boolean flags accept `=false` to disable them. Invalid
configuration is rejected at startup. The container still runs
`/usr/local/bin/cpa-exporter` on port 9321 as a non-root user. Its Docker build
runs natively for each Buildx target (including QEMU emulation when needed).
The Grafana dashboard and Prometheus metric names are unchanged.

HTTP requests have a 15-second timeout and redirects are disabled to keep the
management credential on the configured origin. Queue/config bodies are limited
to 16 MiB. Provider metadata refreshes every minute, even during idle polling.
SIGINT/SIGTERM stops the HTTP listener promptly; in-flight collector work is
terminated with the process. `/-/healthy` reports
process health; `cpa_exporter_up` reports collector health.

See [memory measurements and reproduction](docs/memory.md) for the local Go/Rust comparison.
