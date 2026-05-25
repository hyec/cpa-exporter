package collector

import (
	"bufio"
	"strings"
	"testing"
)

func TestParsePubSubMessage(t *testing.T) {
	payload := []byte(`{"provider":"gemini"}`)
	got, ok := parsePubSubMessage([]any{[]byte("message"), []byte("usage"), payload})
	if !ok {
		t.Fatal("parsePubSubMessage failed")
	}
	if string(got) != string(payload) {
		t.Fatalf("payload = %q, want %q", got, payload)
	}
}

func TestReadRESPValueArray(t *testing.T) {
	reader := bufio.NewReader(strings.NewReader("*3\r\n$7\r\nmessage\r\n$5\r\nusage\r\n$2\r\n{}\r\n"))
	value, err := readRESPValue(reader)
	if err != nil {
		t.Fatalf("readRESPValue error: %v", err)
	}
	payload, ok := parsePubSubMessage(value)
	if !ok {
		t.Fatalf("parsePubSubMessage failed for %#v", value)
	}
	if string(payload) != "{}" {
		t.Fatalf("payload = %q, want {}", payload)
	}
}
