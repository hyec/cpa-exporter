package collector

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/hyec/cpa-exporter/internal/exporter"

	"github.com/prometheus/client_golang/prometheus"
)

func TestUsageQueueURL(t *testing.T) {
	got, err := usageQueueURL("http://127.0.0.1:8317/base/", 10)
	if err != nil {
		t.Fatalf("usageQueueURL error: %v", err)
	}
	want := "http://127.0.0.1:8317/base/v0/management/usage-queue?count=10"
	if got != want {
		t.Fatalf("url = %q, want %q", got, want)
	}
}

func TestHTTPNextPollInterval(t *testing.T) {
	collector := NewHTTP(HTTPConfig{
		PollInterval:     5 * time.Second,
		PollIdleInterval: 30 * time.Second,
	})
	if got := collector.nextPollInterval(true); got != 5*time.Second {
		t.Fatalf("nextPollInterval(true) = %s, want 5s", got)
	}
	if got := collector.nextPollInterval(false); got != 30*time.Second {
		t.Fatalf("nextPollInterval(false) = %s, want 30s", got)
	}
}

func TestHTTPFetchProcessesRecords(t *testing.T) {
	var auth string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		auth = r.Header.Get("Authorization")
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`[{"provider":"gemini","model":"m","alias":"a","endpoint":"e","auth_type":"api-key","tokens":{"total_tokens":1}}]`))
	}))
	defer server.Close()

	registry := prometheus.NewRegistry()
	metrics := exporter.NewMetrics(registry, exporter.MetricOptions{Mode: "http"})
	collector := NewHTTP(HTTPConfig{
		BaseURL:       server.URL,
		ManagementKey: "secret",
		PollCount:     100,
		Processor:     exporter.NewProcessor(metrics),
	})

	count, err := collector.fetch(t.Context())
	if err != nil {
		t.Fatalf("fetch error: %v", err)
	}
	if count != 1 {
		t.Fatalf("count = %d, want 1", count)
	}
	if auth != "Bearer secret" {
		t.Fatalf("Authorization = %q, want Bearer secret", auth)
	}
}
