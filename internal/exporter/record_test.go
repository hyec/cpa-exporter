package exporter

import "testing"

func TestDecodeUsageRecordNormalizesDefaults(t *testing.T) {
	record, err := DecodeUsageRecord([]byte(`{"tokens":{"input_tokens":3,"output_tokens":4},"failed":false}`))
	if err != nil {
		t.Fatalf("DecodeUsageRecord error: %v", err)
	}
	if record.Provider != "unknown" || record.Model != "unknown" || record.Alias != "unknown" || record.Endpoint != "unknown" || record.AuthType != "unknown" {
		t.Fatalf("unexpected normalized labels: %#v", record)
	}
	if record.Tokens.TotalTokens != 7 {
		t.Fatalf("total tokens = %d, want 7", record.Tokens.TotalTokens)
	}
	if record.Fail.StatusCode != 200 {
		t.Fatalf("status code = %d, want 200", record.Fail.StatusCode)
	}
}

func TestDecodeUsageRecordFailedDefaultStatus(t *testing.T) {
	record, err := DecodeUsageRecord([]byte(`{"provider":"gemini","model":"m","failed":true}`))
	if err != nil {
		t.Fatalf("DecodeUsageRecord error: %v", err)
	}
	if record.Fail.StatusCode != 500 {
		t.Fatalf("status code = %d, want 500", record.Fail.StatusCode)
	}
}
