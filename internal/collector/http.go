package collector

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/hyec/cpa-exporter/internal/exporter"
)

type HTTPConfig struct {
	BaseURL          string
	ManagementKey    string
	PollInterval     time.Duration
	PollIdleInterval time.Duration
	PollCount        int
	Processor        *exporter.Processor
	Verbose          bool
}

type HTTP struct {
	cfg    HTTPConfig
	client *http.Client
}

func NewHTTP(cfg HTTPConfig) *HTTP {
	return &HTTP{
		cfg: cfg,
		client: &http.Client{
			Timeout: 15 * time.Second,
		},
	}
}

func (c *HTTP) Run(ctx context.Context) {
	if c.cfg.PollInterval <= 0 {
		c.cfg.PollInterval = 5 * time.Second
	}
	if c.cfg.PollIdleInterval <= 0 {
		c.cfg.PollIdleInterval = 30 * time.Second
	}
	if c.cfg.PollCount <= 0 {
		c.cfg.PollCount = 100
	}
	c.verbosef("http collector started: base_url=%s poll_interval=%s poll_idle_interval=%s poll_count=%d", c.cfg.BaseURL, c.cfg.PollInterval, c.cfg.PollIdleInterval, c.cfg.PollCount)

	for {
		hadData, err := c.drain(ctx)
		if err != nil {
			log.Printf("http collector error: %v", err)
			c.cfg.Processor.Metrics().CollectorError()
		}
		next := c.nextPollInterval(hadData)
		c.verbosef("http collector sleeping for %s after had_data=%t", next, hadData)
		if !sleepContext(ctx, next) {
			return
		}
	}
}

func (c *HTTP) drain(ctx context.Context) (bool, error) {
	hadData := false
	for {
		count, err := c.fetch(ctx)
		if err != nil {
			return hadData, err
		}
		c.verbosef("http usage queue fetched %d record(s)", count)
		c.cfg.Processor.Metrics().SetUp(true)
		if count > 0 {
			hadData = true
		}
		if count == 0 || count < c.cfg.PollCount {
			return hadData, nil
		}
	}
}

func (c *HTTP) nextPollInterval(hadData bool) time.Duration {
	if hadData {
		return c.cfg.PollInterval
	}
	return c.cfg.PollIdleInterval
}

func (c *HTTP) verbosef(format string, args ...any) {
	if c != nil && c.cfg.Verbose {
		log.Printf(format, args...)
	}
}

func (c *HTTP) fetch(ctx context.Context) (int, error) {
	endpoint, err := usageQueueURL(c.cfg.BaseURL, c.cfg.PollCount)
	if err != nil {
		return 0, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return 0, err
	}
	req.Header.Set("Authorization", "Bearer "+c.cfg.ManagementKey)
	req.Header.Set("Accept", "application/json")
	c.cfg.Processor.Metrics().CollectorPoll()

	resp, err := c.client.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 1024))
		return 0, fmt.Errorf("usage queue returned HTTP %d: %s", resp.StatusCode, strings.TrimSpace(string(body)))
	}

	var items []json.RawMessage
	if err := json.NewDecoder(resp.Body).Decode(&items); err != nil {
		return 0, err
	}
	if len(items) > 0 {
		c.cfg.Processor.Metrics().CollectorBatch()
	}
	for _, item := range items {
		if !c.cfg.Processor.ProcessPayload(item) {
			c.verbosef("http usage queue decode failed for payload bytes=%d", len(item))
			continue
		}
	}
	return len(items), nil
}

func usageQueueURL(baseURL string, count int) (string, error) {
	parsed, err := url.Parse(strings.TrimRight(baseURL, "/"))
	if err != nil {
		return "", err
	}
	joined := parsed.JoinPath("v0", "management", "usage-queue")
	query := joined.Query()
	query.Set("count", fmt.Sprintf("%d", count))
	joined.RawQuery = query.Encode()
	return joined.String(), nil
}

func sleepContext(ctx context.Context, duration time.Duration) bool {
	timer := time.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-timer.C:
		return true
	}
}
