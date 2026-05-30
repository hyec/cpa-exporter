package exporter

import (
	"testing"
	"time"

	"github.com/prometheus/client_golang/prometheus"
)

func TestMetricsObserve(t *testing.T) {
	registry := prometheus.NewRegistry()
	metrics := NewMetrics(registry, MetricOptions{Mode: "http", LabelSource: true, LabelAuthIndex: true})
	metrics.Observe(UsageRecord{
		Timestamp: time.Unix(100, 0),
		LatencyMs: 250,
		Source:    "user@example.com",
		AuthIndex: "auth-1",
		Provider:  "gemini",
		Model:     "gemini-2.5-flash",
		Alias:     "flash",
		Endpoint:  "/v1beta/models/x:generateContent",
		AuthType:  "api-key",
		Tokens: TokenStats{
			InputTokens:  2,
			OutputTokens: 3,
			TotalTokens:  5,
		},
	})

	families, err := registry.Gather()
	if err != nil {
		t.Fatalf("Gather error: %v", err)
	}
	if !hasMetricFamily(families, "cpa_requests_total") {
		t.Fatal("missing cpa_requests_total")
	}
	if !hasMetricFamily(families, "cpa_tokens_total") {
		t.Fatal("missing cpa_tokens_total")
	}
	requests := metricFamilyByName(families, "cpa_requests_total")
	if requests == nil || len(requests.GetMetric()) == 0 {
		t.Fatal("missing cpa_requests_total metric")
	}
	if !metricHasLabel(requests.GetMetric()[0], "source", "***r@example.com") {
		t.Fatalf("missing masked source=%q label", "***r@example.com")
	}
	if metricHasLabel(requests.GetMetric()[0], "source", "user@example.com") {
		t.Fatal("source label should not expose the raw email")
	}
}

func TestMetricsAPIKeyPrefixLabelDisabledByDefault(t *testing.T) {
	registry := prometheus.NewRegistry()
	metrics := NewMetrics(registry, MetricOptions{Mode: "http"})
	metrics.Observe(UsageRecord{
		Provider: "gemini",
		Model:    "m",
		Endpoint: "e",
		AuthType: "api-key",
		APIKey:   "sk-abcdef",
	})

	families, err := registry.Gather()
	if err != nil {
		t.Fatalf("Gather error: %v", err)
	}
	requests := metricFamilyByName(families, "cpa_requests_total")
	if requests == nil || len(requests.GetMetric()) == 0 {
		t.Fatal("missing cpa_requests_total metric")
	}
	if metricHasLabelName(requests.GetMetric()[0], "api_key_prefix") {
		t.Fatal("api_key_prefix label should be disabled by default")
	}
}

func TestMetricsAPIKeyPrefixLabelUsesConfiguredLength(t *testing.T) {
	registry := prometheus.NewRegistry()
	metrics := NewMetrics(registry, MetricOptions{Mode: "http", APIKeyPrefixLength: 5})
	metrics.Observe(UsageRecord{
		Provider: "gemini",
		Model:    "m",
		Endpoint: "e",
		AuthType: "api-key",
		APIKey:   "sk-abcdef",
	})

	families, err := registry.Gather()
	if err != nil {
		t.Fatalf("Gather error: %v", err)
	}
	requests := metricFamilyByName(families, "cpa_requests_total")
	if requests == nil || len(requests.GetMetric()) == 0 {
		t.Fatal("missing cpa_requests_total metric")
	}
	if !metricHasLabel(requests.GetMetric()[0], "api_key_prefix", "sk-ab") {
		t.Fatalf("missing api_key_prefix=%q label", "sk-ab")
	}
}

func TestAPIKeyPrefixLabel(t *testing.T) {
	tests := []struct {
		name   string
		key    string
		length int
		want   string
	}{
		{name: "empty", key: "  ", length: 5, want: "unknown"},
		{name: "short", key: "abc", length: 5, want: "abc"},
		{name: "trimmed", key: "  sk-abcdef  ", length: 5, want: "sk-ab"},
		{name: "unicode", key: "钥匙abcdef", length: 3, want: "钥匙a"},
		{name: "disabled", key: "sk-abcdef", length: 0, want: ""},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got := apiKeyPrefixLabel(test.key, test.length)
			if got != test.want {
				t.Fatalf("apiKeyPrefixLabel(%q, %d) = %q, want %q", test.key, test.length, got, test.want)
			}
		})
	}
}

func TestMaskedSourceLabel(t *testing.T) {
	tests := []struct {
		name   string
		source string
		want   string
	}{
		{name: "empty", source: "  ", want: "unknown"},
		{name: "unknown", source: "unknown", want: "unknown"},
		{name: "email", source: "user@example.com", want: "***r@example.com"},
		{name: "trimmed email", source: "  admin@example.org  ", want: "****n@example.org"},
		{name: "long email", source: "username1234@example.net", want: "user***1234@example.net"},
		{name: "long api key", source: "sk-abcdef1234J0", want: "sk-a***34J0"},
		{name: "eight char api key", source: "sk-short", want: "******rt"},
		{name: "short api key", source: "abc123", want: "****23"},
		{name: "tiny api key", source: "ab", want: "**"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got := maskedSourceLabel(test.source)
			if got != test.want {
				t.Fatalf("maskedSourceLabel(%q) = %q, want %q", test.source, got, test.want)
			}
		})
	}
}
