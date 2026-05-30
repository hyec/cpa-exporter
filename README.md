# CLI Proxy API Prometheus Exporter

Prometheus exporter for [CLI Proxy API](https://github.com/router-for-me/CLIProxyAPI) usage records.

The exporter consumes CPA's usage queue and exposes aggregated metrics on `/metrics`.

## CPA prerequisites

CPA must have usage statistics and management access enabled:

```yaml
usage-statistics-enabled: true
remote-management:
  allow-remote: true
  secret-key: "<bcrypt hash managed by CPA>"
```

You can also use CPA's `MANAGEMENT_PASSWORD` environment variable. The exporter sends the key with `Authorization: Bearer <key>`.

## Run

HTTP polling mode is recommended:

```bash
CPA_MANAGEMENT_KEY='your-management-key' \
go run ./cmd/cpa-exporter \
  --mode=http \
  --cpa-url=http://127.0.0.1:8317 \
  --poll-interval=5s \
  --poll-idle-interval=30s
```

Redis RESP subscription mode is also supported:

```bash
CPA_MANAGEMENT_KEY='your-management-key' \
go run ./cmd/cpa-exporter \
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

- `cpa_requests_total{provider,model,alias,endpoint,auth_type,failed,status_code}`
- `cpa_tokens_total{provider,model,alias,endpoint,auth_type,token_type}`
- `cpa_request_latency_seconds{provider,model,alias,endpoint,auth_type,failed}`
- `cpa_usage_record_timestamp_seconds{provider,model,alias,endpoint,auth_type}`
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
go test ./...
go test -race ./...
```

Docker Compose integration test:

```bash
scripts/integration-compose.sh
```

The compose test starts a fake CPA management API plus the exporter, then verifies that `/metrics` contains usage-derived counters.
