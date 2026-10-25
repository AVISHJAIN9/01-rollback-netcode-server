package phase_final

import (
	"fmt"
	"sync"
	"sync/atomic"
	"time"
)

// ─── Features 121–126: Sinks, Storage & Egress Transports ────────────────────

// NormalizedAlert is the outbound alert envelope routed to sinks.
type NormalizedAlert struct {
	AlertID   string
	RuleID    string
	Severity  string
	TenantID  string
	Timestamp int64
	Payload   map[string]string
}

// SinkError describes a delivery failure.
type SinkError struct {
	SinkName string
	Msg      string
}

func (e *SinkError) Error() string {
	return fmt.Sprintf("sink[%s]: %s", e.SinkName, e.Msg)
}

// AlertSink is the common interface for all egress sinks.
type AlertSink interface {
	Emit(alert *NormalizedAlert) error
	Name() string
}

// ─── KafkaSink (Feature 121) ─────────────────────────────────────────────────

// KafkaSink buffers and delivers alerts to a Kafka topic.
type KafkaSink struct {
	topic    string
	mu       sync.Mutex
	messages []string
	produced atomic.Int64
}

func NewKafkaSink(topic string) *KafkaSink { return &KafkaSink{topic: topic} }

func (k *KafkaSink) Emit(alert *NormalizedAlert) error {
	k.mu.Lock()
	k.messages = append(k.messages, fmt.Sprintf("%s:%s", alert.AlertID, alert.Severity))
	k.mu.Unlock()
	k.produced.Add(1)
	return nil
}

func (k *KafkaSink) Name() string      { return "kafka." + k.topic }
func (k *KafkaSink) Count() int64      { return k.produced.Load() }
func (k *KafkaSink) Messages() []string {
	k.mu.Lock()
	defer k.mu.Unlock()
	cp := make([]string, len(k.messages))
	copy(cp, k.messages)
	return cp
}

// ─── ElasticsearchSink (Feature 122) ─────────────────────────────────────────

// ElasticsearchSink batches alerts for bulk indexing.
type ElasticsearchSink struct {
	index    string
	mu       sync.Mutex
	docs     []string
	indexed  atomic.Int64
}

func NewElasticsearchSink(index string) *ElasticsearchSink {
	return &ElasticsearchSink{index: index}
}

func (e *ElasticsearchSink) Emit(alert *NormalizedAlert) error {
	doc := fmt.Sprintf(`{"id":%q,"rule":%q,"sev":%q}`, alert.AlertID, alert.RuleID, alert.Severity)
	e.mu.Lock()
	e.docs = append(e.docs, doc)
	e.mu.Unlock()
	e.indexed.Add(1)
	return nil
}

func (e *ElasticsearchSink) Name() string  { return "elasticsearch." + e.index }
func (e *ElasticsearchSink) Count() int64  { return e.indexed.Load() }

// ─── WebhookSink with 3-state Circuit Breaker (Feature 124) ─────────────────

// BreakerState is the circuit breaker FSM state.
type BreakerState int

const (
	BreakerClosed   BreakerState = iota // normal operation
	BreakerOpen                         // rejecting — cooling down
	BreakerHalfOpen                     // probe mode — 3 trial requests
)

func (b BreakerState) String() string {
	switch b {
	case BreakerClosed:
		return "Closed"
	case BreakerOpen:
		return "Open"
	case BreakerHalfOpen:
		return "HalfOpen"
	default:
		return "Unknown"
	}
}

// WebhookSink delivers alerts via HTTP POST with an internal circuit breaker.
type WebhookSink struct {
	endpoint string

	mu               sync.Mutex
	state            BreakerState
	consecutiveFails int
	openedAt         time.Time
	trialCount       int    // HalfOpen trial count
	tripCount        int64

	// Mock transport: caller injects failure
	shouldFail     atomic.Bool
	delivered      atomic.Int64
	dlqEntries     []NormalizedAlert
}

const (
	cbFailThreshold = 5             // consecutive failures to trip
	cbCoolDown      = 30 * time.Second
	cbTrialLimit    = 3
)

func NewWebhookSink(endpoint string) *WebhookSink {
	return &WebhookSink{
		endpoint: endpoint,
		state:    BreakerClosed,
	}
}

func (w *WebhookSink) SetFailMode(fail bool) { w.shouldFail.Store(fail) }

func (w *WebhookSink) Emit(alert *NormalizedAlert) error {
	w.mu.Lock()
	defer w.mu.Unlock()

	switch w.state {
	case BreakerOpen:
		if time.Since(w.openedAt) >= cbCoolDown {
			w.state = BreakerHalfOpen
			w.trialCount = 0
		} else {
			w.dlqEntries = append(w.dlqEntries, *alert)
			return &SinkError{SinkName: "webhook", Msg: "circuit open — diverted to DLQ"}
		}

	case BreakerHalfOpen:
		if w.trialCount >= cbTrialLimit {
			// All trials pending — queue to DLQ until result is known
			w.dlqEntries = append(w.dlqEntries, *alert)
			return nil
		}
	}

	// Attempt delivery
	if w.shouldFail.Load() {
		w.consecutiveFails++
		if w.state == BreakerHalfOpen || w.consecutiveFails >= cbFailThreshold {
			w.state = BreakerOpen
			w.openedAt = time.Now()
			w.tripCount++
			w.consecutiveFails = 0
		}
		w.dlqEntries = append(w.dlqEntries, *alert)
		return &SinkError{SinkName: "webhook", Msg: "HTTP 503"}
	}

	// Success
	w.consecutiveFails = 0
	if w.state == BreakerHalfOpen {
		w.trialCount++
		if w.trialCount >= cbTrialLimit {
			w.state = BreakerClosed
		}
	}
	w.delivered.Add(1)
	return nil
}

func (w *WebhookSink) Name() string          { return "webhook." + w.endpoint }
func (w *WebhookSink) BreakerStateVal() BreakerState {
	w.mu.Lock()
	defer w.mu.Unlock()
	return w.state
}
func (w *WebhookSink) TripCount() int64       { w.mu.Lock(); defer w.mu.Unlock(); return w.tripCount }
func (w *WebhookSink) DLQDepth() int          { w.mu.Lock(); defer w.mu.Unlock(); return len(w.dlqEntries) }
func (w *WebhookSink) DeliveredCount() int64  { return w.delivered.Load() }

// ─── DeadLetterSink (Feature 126) ────────────────────────────────────────────

// QuarantineEnvelope wraps an unprocessable alert with diagnostic context.
type QuarantineEnvelope struct {
	Alert      NormalizedAlert
	Reason     string
	ReceivedNs int64
}

// DeadLetterSink isolates corrupt or undeliverable alerts.
type DeadLetterSink struct {
	mu       sync.Mutex
	entries  []QuarantineEnvelope
	received atomic.Int64
}

func NewDeadLetterSink() *DeadLetterSink { return &DeadLetterSink{} }

func (d *DeadLetterSink) Emit(alert *NormalizedAlert) error {
	d.mu.Lock()
	d.entries = append(d.entries, QuarantineEnvelope{
		Alert:      *alert,
		Reason:     "undeliverable",
		ReceivedNs: time.Now().UnixNano(),
	})
	d.mu.Unlock()
	d.received.Add(1)
	return nil
}

func (d *DeadLetterSink) Name() string   { return "dlq" }
func (d *DeadLetterSink) Count() int64   { return d.received.Load() }
func (d *DeadLetterSink) Entries() []QuarantineEnvelope {
	d.mu.Lock()
	defer d.mu.Unlock()
	cp := make([]QuarantineEnvelope, len(d.entries))
	copy(cp, d.entries)
	return cp
}

// ─── At-Least-Once Commit Ledger (Feature 125) ───────────────────────────────

// CommitLedger tracks downstream delivery confirmations before committing offsets.
type CommitLedger struct {
	mu       sync.Mutex
	inflight map[string]bool // alertID → committed
}

func NewCommitLedger() *CommitLedger {
	return &CommitLedger{inflight: make(map[string]bool)}
}

// MarkInFlight registers an alert as pending delivery.
func (l *CommitLedger) MarkInFlight(alertID string) {
	l.mu.Lock()
	l.inflight[alertID] = false
	l.mu.Unlock()
}

// Confirm marks an alert as successfully delivered.
func (l *CommitLedger) Confirm(alertID string) {
	l.mu.Lock()
	l.inflight[alertID] = true
	l.mu.Unlock()
}

// AllConfirmed returns true when every in-flight alert has been confirmed.
func (l *CommitLedger) AllConfirmed() bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	for _, v := range l.inflight {
		if !v {
			return false
		}
	}
	return true
}

// PendingCount returns the number of unconfirmed in-flight alerts.
func (l *CommitLedger) PendingCount() int {
	l.mu.Lock()
	defer l.mu.Unlock()
	count := 0
	for _, v := range l.inflight {
		if !v {
			count++
		}
	}
	return count
}
