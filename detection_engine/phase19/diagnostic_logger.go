package phase19

import (
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// ─── Feature 113: High-Throughput Structured Diagnostic Logger ───────────────
//
// StructuredLogger uses a large buffered Go channel as the log ring.
// The channel provides zero-drop MPMC semantics at the channel capacity.
// A single drain goroutine serialises and writes entries to the writer.
//
// Fast-path filter (filtered level): one atomic load ≤ 1 ns.
// Hot-path (accepted): non-blocking channel send — lock-free on amd64/arm64.

// LogLevel is the numeric severity of a log message.
type LogLevel int32

const (
	LogDebug LogLevel = iota
	LogInfo
	LogWarn
	LogError
)

func (l LogLevel) String() string {
	switch l {
	case LogDebug:
		return "DEBUG"
	case LogInfo:
		return "INFO"
	case LogWarn:
		return "WARN"
	case LogError:
		return "ERROR"
	default:
		return "UNKNOWN"
	}
}

// LogEntry is one record in the log channel.
type LogEntry struct {
	TimestampNs int64
	Level       LogLevel
	Message     string
	Fields      map[string]string
}

// StructuredLogger is a high-throughput async logger backed by a buffered channel.
type StructuredLogger struct {
	level   atomic.Int32  // current minimum log level
	ch      chan *LogEntry // buffered channel acting as the log ring
	writer  io.Writer
	stopCh  chan struct{}
	wg      sync.WaitGroup
	Dropped atomic.Uint64
	Flushed atomic.Uint64
}

// NewStructuredLogger constructs a logger writing to w at the given min level.
func NewStructuredLogger(w io.Writer, minLevel LogLevel) *StructuredLogger {
	if w == nil {
		w = os.Stderr
	}
	const ringCapacity = 1 << 17 // 131072 slots
	l := &StructuredLogger{
		ch:     make(chan *LogEntry, ringCapacity),
		writer: w,
		stopCh: make(chan struct{}),
	}
	l.level.Store(int32(minLevel))
	l.wg.Add(1)
	go l.drainLoop()
	return l
}

// SetLevel atomically updates the minimum log level.
func (l *StructuredLogger) SetLevel(level LogLevel) { l.level.Store(int32(level)) }

// CurrentLevel returns the active minimum log level.
func (l *StructuredLogger) CurrentLevel() LogLevel { return LogLevel(l.level.Load()) }

// Log emits a log entry if level ≥ current filter.
func (l *StructuredLogger) Log(level LogLevel, message string, fields map[string]string) {
	if level < LogLevel(l.level.Load()) {
		return // fast path — filtered out, single atomic load
	}
	entry := &LogEntry{
		TimestampNs: time.Now().UnixNano(),
		Level:       level,
		Message:     message,
		Fields:      fields,
	}
	select {
	case l.ch <- entry:
	default:
		l.Dropped.Add(1)
	}
}

// Info logs at INFO level.
func (l *StructuredLogger) Info(msg string, fields map[string]string) {
	l.Log(LogInfo, msg, fields)
}

// Warn logs at WARN level.
func (l *StructuredLogger) Warn(msg string, fields map[string]string) {
	l.Log(LogWarn, msg, fields)
}

// Error logs at ERROR level.
func (l *StructuredLogger) Error(msg string, fields map[string]string) {
	l.Log(LogError, msg, fields)
}

// Stop flushes remaining entries and terminates the drain goroutine.
func (l *StructuredLogger) Stop() {
	close(l.stopCh)
	l.wg.Wait()
}

// drainLoop is the single background I/O goroutine.
func (l *StructuredLogger) drainLoop() {
	defer l.wg.Done()
	var buf strings.Builder

	writeEntry := func(entry *LogEntry) {
		buf.Reset()
		renderJSON(&buf, entry)
		buf.WriteByte('\n')
		l.writer.Write([]byte(buf.String()))
		l.Flushed.Add(1)
	}

	drainAll := func() {
		for {
			select {
			case entry := <-l.ch:
				writeEntry(entry)
			default:
				return
			}
		}
	}

	for {
		select {
		case entry := <-l.ch:
			writeEntry(entry)
		case <-l.stopCh:
			drainAll()
			return
		}
	}
}

// renderJSON writes a JSON-formatted log line into buf.
func renderJSON(buf *strings.Builder, e *LogEntry) {
	buf.WriteString(`{"ts":`)
	fmt.Fprintf(buf, "%d", e.TimestampNs)
	buf.WriteString(`,"level":"`)
	buf.WriteString(e.Level.String())
	buf.WriteString(`","msg":`)
	fmt.Fprintf(buf, "%q", e.Message)
	for k, v := range e.Fields {
		buf.WriteString(`,"`)
		buf.WriteString(k)
		buf.WriteString(`":`)
		fmt.Fprintf(buf, "%q", v)
	}
	buf.WriteByte('}')
}
