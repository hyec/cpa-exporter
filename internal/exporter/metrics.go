package exporter

import (
	"strconv"
	"strings"
	"time"

	"github.com/prometheus/client_golang/prometheus"
)

const APIKeyPrefixMaxLength = 64

type MetricOptions struct {
	Mode               string
	LabelSource        bool
	LabelAuthIndex     bool
	APIKeyPrefixLength int
}

type Metrics struct {
	options          MetricOptions
	requests         *prometheus.CounterVec
	tokens           *prometheus.CounterVec
	latency          *prometheus.HistogramVec
	lastRecordTime   *prometheus.GaugeVec
	exporterRecords  *prometheus.CounterVec
	exporterErrors   *prometheus.CounterVec
	exporterUp       *prometheus.GaugeVec
	collectorErrors  *prometheus.CounterVec
	collectorPolls   *prometheus.CounterVec
	collectorBatches *prometheus.CounterVec
}

func NewMetrics(registry *prometheus.Registry, options MetricOptions) *Metrics {
	baseLabels := []string{"provider", "model", "alias", "endpoint", "auth_type"}
	requestLabels := append([]string{}, baseLabels...)
	requestLabels = append(requestLabels, "failed", "status_code")
	latencyLabels := append([]string{}, baseLabels...)
	latencyLabels = append(latencyLabels, "failed")
	tokenLabels := append([]string{}, baseLabels...)
	tokenLabels = append(tokenLabels, "token_type")
	timeLabels := append([]string{}, baseLabels...)

	if options.LabelSource {
		requestLabels = append(requestLabels, "source")
		latencyLabels = append(latencyLabels, "source")
		tokenLabels = append(tokenLabels, "source")
		timeLabels = append(timeLabels, "source")
	}
	if options.LabelAuthIndex {
		requestLabels = append(requestLabels, "auth_index")
		latencyLabels = append(latencyLabels, "auth_index")
		tokenLabels = append(tokenLabels, "auth_index")
		timeLabels = append(timeLabels, "auth_index")
	}
	if options.APIKeyPrefixLength > 0 {
		requestLabels = append(requestLabels, "api_key_prefix")
		latencyLabels = append(latencyLabels, "api_key_prefix")
		tokenLabels = append(tokenLabels, "api_key_prefix")
		timeLabels = append(timeLabels, "api_key_prefix")
	}

	m := &Metrics{
		options: options,
		requests: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_requests_total",
			Help: "Total CLIProxyAPI usage records grouped as requests.",
		}, requestLabels),
		tokens: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_tokens_total",
			Help: "Total tokens reported by CLIProxyAPI usage records.",
		}, tokenLabels),
		latency: prometheus.NewHistogramVec(prometheus.HistogramOpts{
			Name:    "cpa_request_latency_seconds",
			Help:    "CLIProxyAPI request latency in seconds.",
			Buckets: prometheus.DefBuckets,
		}, latencyLabels),
		lastRecordTime: prometheus.NewGaugeVec(prometheus.GaugeOpts{
			Name: "cpa_usage_record_timestamp_seconds",
			Help: "Unix timestamp of the latest observed CLIProxyAPI usage record.",
		}, timeLabels),
		exporterRecords: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_exporter_records_total",
			Help: "Total usage records successfully decoded by the exporter.",
		}, []string{"mode"}),
		exporterErrors: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_exporter_decode_errors_total",
			Help: "Total usage records the exporter failed to decode.",
		}, []string{"mode"}),
		exporterUp: prometheus.NewGaugeVec(prometheus.GaugeOpts{
			Name: "cpa_exporter_up",
			Help: "Whether the collector is currently healthy.",
		}, []string{"mode"}),
		collectorErrors: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_exporter_collector_errors_total",
			Help: "Total collector errors while talking to CLIProxyAPI.",
		}, []string{"mode"}),
		collectorPolls: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_exporter_polls_total",
			Help: "Total HTTP poll requests made to CLIProxyAPI.",
		}, []string{"mode"}),
		collectorBatches: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "cpa_exporter_batches_total",
			Help: "Total non-empty usage batches received from CLIProxyAPI.",
		}, []string{"mode"}),
	}

	registry.MustRegister(
		m.requests,
		m.tokens,
		m.latency,
		m.lastRecordTime,
		m.exporterRecords,
		m.exporterErrors,
		m.exporterUp,
		m.collectorErrors,
		m.collectorPolls,
		m.collectorBatches,
	)
	m.exporterUp.WithLabelValues(options.Mode).Set(0)
	return m
}

func (m *Metrics) Observe(record UsageRecord) {
	record.Normalize()

	base := m.baseLabelValues(record)
	requestLabels := append([]string{}, base...)
	requestLabels = append(requestLabels, strconv.FormatBool(record.Failed), strconv.Itoa(record.Fail.StatusCode))
	requestLabels = m.appendOptionalLabels(requestLabels, record)
	m.requests.WithLabelValues(requestLabels...).Inc()

	latencyLabels := append([]string{}, base...)
	latencyLabels = append(latencyLabels, strconv.FormatBool(record.Failed))
	latencyLabels = m.appendOptionalLabels(latencyLabels, record)
	if record.LatencyMs >= 0 {
		m.latency.WithLabelValues(latencyLabels...).Observe(float64(record.LatencyMs) / float64(time.Second/time.Millisecond))
	}

	tokenValues := []struct {
		name  string
		value int64
	}{
		{"input", record.Tokens.InputTokens},
		{"output", record.Tokens.OutputTokens},
		{"reasoning", record.Tokens.ReasoningTokens},
		{"cached", record.Tokens.CachedTokens},
		{"cache_read", record.Tokens.CacheReadTokens},
		{"cache_creation", record.Tokens.CacheCreationTokens},
		{"total", record.Tokens.TotalTokens},
	}
	for _, token := range tokenValues {
		if token.value <= 0 {
			continue
		}
		labels := append([]string{}, base...)
		labels = append(labels, token.name)
		labels = m.appendOptionalLabels(labels, record)
		m.tokens.WithLabelValues(labels...).Add(float64(token.value))
	}

	if !record.Timestamp.IsZero() {
		labels := append([]string{}, base...)
		labels = m.appendOptionalLabels(labels, record)
		m.lastRecordTime.WithLabelValues(labels...).Set(float64(record.Timestamp.Unix()))
	}
	m.exporterRecords.WithLabelValues(m.options.Mode).Inc()
}

func (m *Metrics) DecodeError() {
	m.exporterErrors.WithLabelValues(m.options.Mode).Inc()
}

func (m *Metrics) CollectorError() {
	m.collectorErrors.WithLabelValues(m.options.Mode).Inc()
	m.SetUp(false)
}

func (m *Metrics) CollectorPoll() {
	m.collectorPolls.WithLabelValues(m.options.Mode).Inc()
}

func (m *Metrics) CollectorBatch() {
	m.collectorBatches.WithLabelValues(m.options.Mode).Inc()
}

func (m *Metrics) SetUp(up bool) {
	value := 0.0
	if up {
		value = 1
	}
	m.exporterUp.WithLabelValues(m.options.Mode).Set(value)
}

func (m *Metrics) baseLabelValues(record UsageRecord) []string {
	return []string{record.Provider, record.Model, record.Alias, record.Endpoint, record.AuthType}
}

func (m *Metrics) appendOptionalLabels(labels []string, record UsageRecord) []string {
	if m.options.LabelSource {
		labels = append(labels, maskedSourceLabel(record.Source))
	}
	if m.options.LabelAuthIndex {
		labels = append(labels, record.AuthIndex)
	}
	if m.options.APIKeyPrefixLength > 0 {
		labels = append(labels, apiKeyPrefixLabel(record.APIKey, m.options.APIKeyPrefixLength))
	}
	return labels
}

func apiKeyPrefixLabel(apiKey string, length int) string {
	if length <= 0 {
		return ""
	}
	trimmed := strings.TrimSpace(apiKey)
	if trimmed == "" {
		return "unknown"
	}
	runes := []rune(trimmed)
	if len(runes) <= length {
		return trimmed
	}
	return string(runes[:length])
}

func maskedSourceLabel(source string) string {
	trimmed := strings.TrimSpace(source)
	if trimmed == "" || trimmed == "unknown" {
		return "unknown"
	}
	if masked, ok := maskedEmailLabel(trimmed); ok {
		return masked
	}
	return maskedKeyLabel(trimmed)
}

func maskedEmailLabel(value string) (string, bool) {
	at := strings.LastIndex(value, "@")
	if at <= 0 || at == len(value)-1 {
		return "", false
	}
	local := value[:at]
	domain := strings.TrimSpace(value[at+1:])
	if strings.Contains(local, "@") || strings.Contains(domain, "@") || domain == "" {
		return "", false
	}
	localRunes := []rune(local)
	if len(localRunes) == 0 {
		return "", false
	}
	return string(localRunes[0]) + "***@" + domain, true
}

func maskedKeyLabel(value string) string {
	runes := []rune(value)
	if len(runes) <= 8 {
		visible := len(runes) / 3
		if visible == 0 {
			return strings.Repeat("*", len(runes))
		}
		return strings.Repeat("*", len(runes)-visible) + string(runes[len(runes)-visible:])
	}
	return string(runes[:4]) + "***" + string(runes[len(runes)-4:])
}
