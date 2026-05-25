package main

import (
	"encoding/json"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"
)

type server struct {
	key   string
	mu    sync.Mutex
	queue []json.RawMessage
}

func main() {
	key := strings.TrimSpace(os.Getenv("MANAGEMENT_PASSWORD"))
	if key == "" {
		key = "integration-secret"
	}
	s := &server{
		key: key,
		queue: []json.RawMessage{
			json.RawMessage(`{"timestamp":"2026-05-24T00:00:00Z","latency_ms":123,"provider":"gemini","model":"gemini-2.5-flash","alias":"flash","endpoint":"/v1beta/models/gemini-2.5-flash:generateContent","auth_type":"api-key","source":"fake@example.com","auth_index":"fake-1","tokens":{"input_tokens":7,"output_tokens":11,"total_tokens":18},"failed":false,"fail":{"status_code":200}}`),
			json.RawMessage(`{"timestamp":"2026-05-24T00:00:01Z","latency_ms":456,"provider":"claude","model":"claude-sonnet-4","alias":"sonnet","endpoint":"/v1/messages","auth_type":"oauth","tokens":{"input_tokens":5,"output_tokens":0,"total_tokens":5},"failed":true,"fail":{"status_code":429,"body":"quota"}}`),
		},
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok\n"))
	})
	mux.HandleFunc("/v0/management/usage-queue", s.usageQueue)

	httpServer := &http.Server{
		Addr:              ":8317",
		Handler:           mux,
		ReadHeaderTimeout: 5 * time.Second,
	}
	log.Printf("fake CPA listening on %s", httpServer.Addr)
	if err := httpServer.ListenAndServe(); err != nil {
		log.Fatal(err)
	}
}

func (s *server) usageQueue(w http.ResponseWriter, r *http.Request) {
	if !s.authorized(r) {
		http.Error(w, "invalid management key", http.StatusUnauthorized)
		return
	}
	count := 1
	if raw := strings.TrimSpace(r.URL.Query().Get("count")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err == nil && parsed > 0 {
			count = parsed
		}
	}

	s.mu.Lock()
	if count > len(s.queue) {
		count = len(s.queue)
	}
	items := append([]json.RawMessage(nil), s.queue[:count]...)
	s.queue = s.queue[count:]
	s.mu.Unlock()

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(items)
}

func (s *server) authorized(r *http.Request) bool {
	provided := strings.TrimSpace(r.Header.Get("X-Management-Key"))
	if provided == "" {
		auth := strings.TrimSpace(r.Header.Get("Authorization"))
		if strings.HasPrefix(strings.ToLower(auth), "bearer ") {
			provided = strings.TrimSpace(auth[len("bearer "):])
		} else {
			provided = auth
		}
	}
	return provided == s.key
}
