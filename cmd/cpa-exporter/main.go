package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/hyec/cpa-exporter/internal/collector"
	"github.com/hyec/cpa-exporter/internal/exporter"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

type config struct {
	listenAddress      string
	metricsPath        string
	mode               string
	cpaURL             string
	managementKey      string
	pollInterval       time.Duration
	pollIdleInterval   time.Duration
	pollCount          int
	redisAddr          string
	verbose            bool
	labelSource        bool
	labelAuthIndex     bool
	apiKeyPrefixLength int
}

func main() {
	cfg := parseConfig()
	if err := cfg.validate(); err != nil {
		log.Fatalf("configuration error: %v", err)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	registry := prometheus.NewRegistry()
	metrics := exporter.NewMetrics(registry, exporter.MetricOptions{
		Mode:               cfg.mode,
		LabelSource:        cfg.labelSource,
		LabelAuthIndex:     cfg.labelAuthIndex,
		APIKeyPrefixLength: cfg.apiKeyPrefixLength,
	})
	processor := exporter.NewProcessor(metrics)

	var c collector.Collector
	switch cfg.mode {
	case "http":
		c = collector.NewHTTP(collector.HTTPConfig{
			BaseURL:          cfg.cpaURL,
			ManagementKey:    cfg.managementKey,
			PollInterval:     cfg.pollInterval,
			PollIdleInterval: cfg.pollIdleInterval,
			PollCount:        cfg.pollCount,
			Processor:        processor,
			Verbose:          cfg.verbose,
		})
	case "redis":
		c = collector.NewRedis(collector.RedisConfig{
			Addr:          cfg.redisAddr,
			ManagementKey: cfg.managementKey,
			Processor:     processor,
			Verbose:       cfg.verbose,
		})
	default:
		log.Fatalf("unsupported mode %q", cfg.mode)
	}

	go c.Run(ctx)

	mux := http.NewServeMux()
	mux.Handle(cfg.metricsPath, promhttp.HandlerFor(registry, promhttp.HandlerOpts{}))
	mux.HandleFunc("/-/healthy", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok\n"))
	})

	server := &http.Server{
		Addr:              cfg.listenAddress,
		Handler:           mux,
		ReadHeaderTimeout: 5 * time.Second,
	}

	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := server.Shutdown(shutdownCtx); err != nil {
			log.Printf("server shutdown error: %v", err)
		}
	}()

	log.Printf("cpa-exporter listening on %s%s in %s mode", cfg.listenAddress, cfg.metricsPath, cfg.mode)
	if err := server.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatalf("server error: %v", err)
	}
}

func parseConfig() config {
	var cfg config
	flag.StringVar(&cfg.listenAddress, "listen-address", envOrDefault("CPA_EXPORTER_LISTEN_ADDRESS", ":9321"), "address for the exporter HTTP server")
	flag.StringVar(&cfg.metricsPath, "metrics-path", envOrDefault("CPA_EXPORTER_METRICS_PATH", "/metrics"), "path for Prometheus metrics")
	flag.StringVar(&cfg.mode, "mode", envOrDefault("CPA_EXPORTER_MODE", "http"), "collector mode: http or redis")
	flag.StringVar(&cfg.cpaURL, "cpa-url", envOrDefault("CPA_URL", "http://127.0.0.1:8317"), "CLIProxyAPI base URL for HTTP mode")
	flag.StringVar(&cfg.managementKey, "management-key", envOrDefault("CPA_MANAGEMENT_KEY", ""), "CLIProxyAPI management key")
	flag.DurationVar(&cfg.pollInterval, "poll-interval", envDurationOrDefault("CPA_EXPORTER_POLL_INTERVAL", 5*time.Second), "HTTP polling interval")
	flag.DurationVar(&cfg.pollIdleInterval, "poll-idle-interval", envDurationOrDefault("CPA_EXPORTER_POLL_IDLE_INTERVAL", 30*time.Second), "HTTP polling interval when the usage queue is empty")
	flag.IntVar(&cfg.pollCount, "poll-count", envIntOrDefault("CPA_EXPORTER_POLL_COUNT", 100), "usage records to request per HTTP poll")
	flag.StringVar(&cfg.redisAddr, "redis-addr", envOrDefault("CPA_REDIS_ADDR", "127.0.0.1:8317"), "CLIProxyAPI RESP address for Redis mode")
	flag.BoolVar(&cfg.verbose, "verbose", envBoolOrDefault("CPA_EXPORTER_VERBOSE", false), "enable verbose collector logs")
	flag.BoolVar(&cfg.labelSource, "label-source", envBoolOrDefault("CPA_EXPORTER_LABEL_SOURCE", false), "include source as a metric label")
	flag.BoolVar(&cfg.labelAuthIndex, "label-auth-index", envBoolOrDefault("CPA_EXPORTER_LABEL_AUTH_INDEX", false), "include auth_index as a metric label")
	flag.IntVar(&cfg.apiKeyPrefixLength, "label-api-key-prefix-length", envIntOrDefault("CPA_EXPORTER_LABEL_API_KEY_PREFIX_LENGTH", 0), "include the first N characters of api_key as api_key_prefix label; 0 disables it")
	flag.Parse()

	cfg.mode = strings.ToLower(strings.TrimSpace(cfg.mode))
	cfg.metricsPath = normalizePath(cfg.metricsPath)
	return cfg
}

func (c config) validate() error {
	if c.managementKey == "" {
		return errors.New("management key is required via --management-key or CPA_MANAGEMENT_KEY")
	}
	if c.apiKeyPrefixLength < 0 {
		return errors.New("--label-api-key-prefix-length must be greater than or equal to 0")
	}
	if c.apiKeyPrefixLength > exporter.APIKeyPrefixMaxLength {
		return fmt.Errorf("--label-api-key-prefix-length must be <= %d", exporter.APIKeyPrefixMaxLength)
	}
	switch c.mode {
	case "http":
		if strings.TrimSpace(c.cpaURL) == "" {
			return errors.New("--cpa-url is required in http mode")
		}
		if c.pollInterval <= 0 {
			return errors.New("--poll-interval must be positive")
		}
		if c.pollIdleInterval <= 0 {
			return errors.New("--poll-idle-interval must be positive")
		}
		if c.pollCount <= 0 {
			return errors.New("--poll-count must be positive")
		}
	case "redis":
		if strings.TrimSpace(c.redisAddr) == "" {
			return errors.New("--redis-addr is required in redis mode")
		}
	default:
		return fmt.Errorf("--mode must be http or redis, got %q", c.mode)
	}
	return nil
}

func normalizePath(path string) string {
	path = strings.TrimSpace(path)
	if path == "" {
		return "/metrics"
	}
	if !strings.HasPrefix(path, "/") {
		return "/" + path
	}
	return path
}

func envOrDefault(key, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallback
}

func envDurationOrDefault(key string, fallback time.Duration) time.Duration {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		parsed, err := time.ParseDuration(value)
		if err == nil {
			return parsed
		}
	}
	return fallback
}

func envIntOrDefault(key string, fallback int) int {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		var parsed int
		if _, err := fmt.Sscanf(value, "%d", &parsed); err == nil {
			return parsed
		}
	}
	return fallback
}

func envBoolOrDefault(key string, fallback bool) bool {
	switch strings.ToLower(strings.TrimSpace(os.Getenv(key))) {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	default:
		return fallback
	}
}
