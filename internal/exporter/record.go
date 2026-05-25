package exporter

import (
	"encoding/json"
	"strings"
	"time"
)

type UsageRecord struct {
	Timestamp       time.Time  `json:"timestamp"`
	LatencyMs       int64      `json:"latency_ms"`
	Source          string     `json:"source"`
	AuthIndex       string     `json:"auth_index"`
	Tokens          TokenStats `json:"tokens"`
	Failed          bool       `json:"failed"`
	Fail            FailDetail `json:"fail"`
	Provider        string     `json:"provider"`
	Model           string     `json:"model"`
	Alias           string     `json:"alias"`
	Endpoint        string     `json:"endpoint"`
	AuthType        string     `json:"auth_type"`
	APIKey          string     `json:"api_key"`
	RequestID       string     `json:"request_id"`
	ReasoningEffort string     `json:"reasoning_effort"`
}

type TokenStats struct {
	InputTokens         int64 `json:"input_tokens"`
	OutputTokens        int64 `json:"output_tokens"`
	ReasoningTokens     int64 `json:"reasoning_tokens"`
	CachedTokens        int64 `json:"cached_tokens"`
	CacheReadTokens     int64 `json:"cache_read_tokens"`
	CacheCreationTokens int64 `json:"cache_creation_tokens"`
	TotalTokens         int64 `json:"total_tokens"`
}

type FailDetail struct {
	StatusCode int    `json:"status_code"`
	Body       string `json:"body"`
}

func DecodeUsageRecord(payload []byte) (UsageRecord, error) {
	var record UsageRecord
	if err := json.Unmarshal(payload, &record); err != nil {
		return record, err
	}
	record.Normalize()
	return record, nil
}

func (r *UsageRecord) Normalize() {
	r.Provider = normalizeLabelValue(r.Provider)
	r.Model = normalizeLabelValue(r.Model)
	r.Alias = strings.TrimSpace(r.Alias)
	if r.Alias == "" {
		r.Alias = r.Model
	}
	r.Endpoint = normalizeLabelValue(r.Endpoint)
	r.AuthType = normalizeLabelValue(r.AuthType)
	r.Source = normalizeOptionalLabelValue(r.Source)
	r.AuthIndex = normalizeOptionalLabelValue(r.AuthIndex)
	if r.Tokens.TotalTokens == 0 {
		r.Tokens.TotalTokens = r.Tokens.InputTokens + r.Tokens.OutputTokens + r.Tokens.ReasoningTokens
	}
	if !r.Failed && r.Fail.StatusCode <= 0 {
		r.Fail.StatusCode = 200
	}
	if r.Failed && r.Fail.StatusCode <= 0 {
		r.Fail.StatusCode = 500
	}
}

func normalizeLabelValue(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return "unknown"
	}
	return value
}

func normalizeOptionalLabelValue(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return "unknown"
	}
	return value
}
