package collector

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"strconv"
	"strings"
	"time"

	"github.com/hyec/cpa-exporter/internal/exporter"
)

type RedisConfig struct {
	Addr          string
	ManagementKey string
	Processor     *exporter.Processor
	Verbose       bool
}

type Redis struct {
	cfg RedisConfig
}

func NewRedis(cfg RedisConfig) *Redis {
	return &Redis{cfg: cfg}
}

func (c *Redis) Run(ctx context.Context) {
	backoff := time.Second
	c.verbosef("redis collector started: addr=%s", c.cfg.Addr)
	for {
		if err := c.runOnce(ctx); err != nil && !errors.Is(err, context.Canceled) {
			log.Printf("redis collector error: %v", err)
			c.cfg.Processor.Metrics().CollectorError()
		}
		if ctx.Err() != nil {
			return
		}
		c.verbosef("redis collector reconnecting after %s", backoff)
		if !sleepContext(ctx, backoff) {
			return
		}
		backoff *= 2
		if backoff > 30*time.Second {
			backoff = 30 * time.Second
		}
	}
}

func (c *Redis) runOnce(ctx context.Context) error {
	dialer := net.Dialer{Timeout: 10 * time.Second}
	c.verbosef("redis collector connecting to %s", c.cfg.Addr)
	conn, err := dialer.DialContext(ctx, "tcp", c.cfg.Addr)
	if err != nil {
		return err
	}
	defer conn.Close()

	reader := bufio.NewReader(conn)
	writer := bufio.NewWriter(conn)

	if err := writeRESPCommand(writer, "AUTH", c.cfg.ManagementKey); err != nil {
		return err
	}
	value, err := readRESPValue(reader)
	if err != nil {
		return err
	}
	if simple, ok := value.(string); !ok || !strings.EqualFold(simple, "OK") {
		return fmt.Errorf("unexpected AUTH response: %v", value)
	}
	c.verbosef("redis collector authenticated")

	if err := writeRESPCommand(writer, "SUBSCRIBE", "usage"); err != nil {
		return err
	}
	if _, err := readRESPValue(reader); err != nil {
		return err
	}
	c.cfg.Processor.Metrics().SetUp(true)
	c.verbosef("redis collector subscribed to usage channel")

	for {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		_ = conn.SetReadDeadline(time.Now().Add(30 * time.Second))
		value, err := readRESPValue(reader)
		if netErr, ok := err.(net.Error); ok && netErr.Timeout() {
			continue
		}
		if err != nil {
			return err
		}
		message, ok := parsePubSubMessage(value)
		if !ok {
			c.verbosef("redis collector ignored non-usage pubsub message")
			continue
		}
		c.cfg.Processor.Metrics().CollectorBatch()
		if c.cfg.Processor.ProcessPayload(message) {
			c.verbosef("redis collector processed usage message bytes=%d", len(message))
		} else {
			c.verbosef("redis collector decode failed for usage message bytes=%d", len(message))
		}
	}
}

func (c *Redis) verbosef(format string, args ...any) {
	if c != nil && c.cfg.Verbose {
		log.Printf(format, args...)
	}
}

func writeRESPCommand(writer *bufio.Writer, args ...string) error {
	if _, err := fmt.Fprintf(writer, "*%d\r\n", len(args)); err != nil {
		return err
	}
	for _, arg := range args {
		if _, err := fmt.Fprintf(writer, "$%d\r\n%s\r\n", len(arg), arg); err != nil {
			return err
		}
	}
	return writer.Flush()
}

func readRESPValue(reader *bufio.Reader) (any, error) {
	prefix, err := reader.ReadByte()
	if err != nil {
		return nil, err
	}
	switch prefix {
	case '+':
		return readRESPLine(reader)
	case '-':
		line, err := readRESPLine(reader)
		if err != nil {
			return nil, err
		}
		return nil, errors.New(line)
	case ':':
		line, err := readRESPLine(reader)
		if err != nil {
			return nil, err
		}
		return strconv.ParseInt(line, 10, 64)
	case '$':
		line, err := readRESPLine(reader)
		if err != nil {
			return nil, err
		}
		size, err := strconv.Atoi(line)
		if err != nil {
			return nil, err
		}
		if size < 0 {
			return nil, nil
		}
		payload := make([]byte, size+2)
		if _, err := io.ReadFull(reader, payload); err != nil {
			return nil, err
		}
		return payload[:size], nil
	case '*':
		line, err := readRESPLine(reader)
		if err != nil {
			return nil, err
		}
		size, err := strconv.Atoi(line)
		if err != nil {
			return nil, err
		}
		if size < 0 {
			return nil, nil
		}
		values := make([]any, 0, size)
		for i := 0; i < size; i++ {
			value, err := readRESPValue(reader)
			if err != nil {
				return nil, err
			}
			values = append(values, value)
		}
		return values, nil
	default:
		return nil, fmt.Errorf("unsupported RESP prefix %q", prefix)
	}
}

func readRESPLine(reader *bufio.Reader) (string, error) {
	line, err := reader.ReadString('\n')
	if err != nil {
		return "", err
	}
	return strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r"), nil
}

func parsePubSubMessage(value any) ([]byte, bool) {
	items, ok := value.([]any)
	if !ok || len(items) != 3 {
		return nil, false
	}
	kind, ok := respString(items[0])
	if !ok || !strings.EqualFold(kind, "message") {
		return nil, false
	}
	channel, ok := respString(items[1])
	if !ok || !strings.EqualFold(channel, "usage") {
		return nil, false
	}
	payload, ok := items[2].([]byte)
	return payload, ok
}

func respString(value any) (string, bool) {
	switch v := value.(type) {
	case string:
		return v, true
	case []byte:
		return string(v), true
	default:
		return "", false
	}
}
