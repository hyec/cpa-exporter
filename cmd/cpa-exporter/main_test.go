package main

import (
	"strings"
	"testing"
	"time"

	"github.com/hyec/cpa-exporter/internal/exporter"
)

func TestConfigValidateAPIKeyPrefixLength(t *testing.T) {
	base := config{
		mode:             "http",
		cpaURL:           "http://127.0.0.1:8317",
		managementKey:    "secret",
		pollInterval:     time.Second,
		pollIdleInterval: 30 * time.Second,
		pollCount:        1,
	}

	negative := base
	negative.apiKeyPrefixLength = -1
	if err := negative.validate(); err == nil || !strings.Contains(err.Error(), "greater than or equal to 0") {
		t.Fatalf("negative validate error = %v", err)
	}

	tooLarge := base
	tooLarge.apiKeyPrefixLength = exporter.APIKeyPrefixMaxLength + 1
	if err := tooLarge.validate(); err == nil || !strings.Contains(err.Error(), "must be <=") {
		t.Fatalf("too-large validate error = %v", err)
	}

	enabled := base
	enabled.apiKeyPrefixLength = 5
	if err := enabled.validate(); err != nil {
		t.Fatalf("enabled validate error: %v", err)
	}
}

func TestConfigValidatePollIdleInterval(t *testing.T) {
	cfg := config{
		mode:             "http",
		cpaURL:           "http://127.0.0.1:8317",
		managementKey:    "secret",
		pollInterval:     time.Second,
		pollIdleInterval: 0,
		pollCount:        1,
	}
	if err := cfg.validate(); err == nil || !strings.Contains(err.Error(), "--poll-idle-interval must be positive") {
		t.Fatalf("validate error = %v", err)
	}
}
