#!/usr/bin/env sh
set -eu

compose_project="${COMPOSE_PROJECT_NAME:-cpa-exporter-it}"
metrics_url="${CPA_EXPORTER_METRICS_URL:-}"

cleanup() {
  docker compose -p "$compose_project" down --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

docker compose -p "$compose_project" up --build -d

if [ -z "$metrics_url" ]; then
  mapped_port="$(docker compose -p "$compose_project" port exporter 9321 | awk -F: 'END {print $NF}')"
  metrics_url="http://127.0.0.1:${mapped_port}/metrics"
fi

attempt=0
while [ "$attempt" -lt 60 ]; do
  body="$(curl -fsS "$metrics_url" 2>/dev/null || true)"
  if printf '%s\n' "$body" | grep -q 'cpa_requests_total'; then
    if printf '%s\n' "$body" | grep -q 'provider="gemini"'; then
      printf '%s\n' "$body" | grep 'cpa_requests_total' || true
      printf 'integration compose test passed\n'
      exit 0
    fi
  fi
  attempt=$((attempt + 1))
  sleep 1
done

docker compose -p "$compose_project" logs
printf 'integration compose test failed: metrics did not contain expected CPA records\n' >&2
exit 1
