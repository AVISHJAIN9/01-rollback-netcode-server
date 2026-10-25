package phase18

import (
	"sync"
	"time"
)

// ─── Feature 106: Micro-Batch Aggregation Pipeline Stage ──────────────────────
//
// MicroBatchPipeline accumulates individual events and flushes them as a
// contiguous slice when either:
//   • The batch reaches maxBatchSize items, or
//   • The flush timeout (flushTimeoutUs microseconds) expires.
//
// FlushFn receives the accumulated slice and processes it downstream.

// CanonicalEvent is the unit of work flowing through the pipeline.
type CanonicalEvent struct {
	EventID     string
	TimestampNs int64
	EntityID    string
	EventType   string
	Fields      map[string]string
}

// MicroBatchPipeline buffers events and flushes in micro-batches.
type MicroBatchPipeline struct {
	mu              sync.Mutex
	buf             []CanonicalEvent
	maxBatchSize    int
	flushTimeoutUs  int64         // microseconds
	flushFn         func([]CanonicalEvent)
	timer           *time.Timer
	totalFlushed    int64
	flushCount      int64
	timerFlushCount int64
	sizeFlushCount  int64
}

// NewMicroBatchPipeline constructs a pipeline with the given limits.
// flushFn is called with each completed batch (under the internal mutex released first).
func NewMicroBatchPipeline(
	maxBatchSize int,
	flushTimeoutUs int64,
	flushFn func([]CanonicalEvent),
) *MicroBatchPipeline {
	p := &MicroBatchPipeline{
		maxBatchSize:   maxBatchSize,
		flushTimeoutUs: flushTimeoutUs,
		flushFn:        flushFn,
		buf:            make([]CanonicalEvent, 0, maxBatchSize),
	}
	p.armTimer()
	return p
}

// armTimer starts or restarts the inactivity flush timer.
// Must be called with p.mu held or before the pipeline is shared.
func (p *MicroBatchPipeline) armTimer() {
	d := time.Duration(p.flushTimeoutUs) * time.Microsecond
	if p.timer == nil {
		p.timer = time.AfterFunc(d, p.timerFlush)
	} else {
		p.timer.Reset(d)
	}
}

// timerFlush is called by the timer goroutine when the timeout expires.
func (p *MicroBatchPipeline) timerFlush() {
	p.mu.Lock()
	if len(p.buf) == 0 {
		p.armTimer()
		p.mu.Unlock()
		return
	}
	batch := p.buf
	p.buf = make([]CanonicalEvent, 0, p.maxBatchSize)
	p.totalFlushed += int64(len(batch))
	p.flushCount++
	p.timerFlushCount++
	p.armTimer()
	p.mu.Unlock()

	p.flushFn(batch)
}

// Push adds an event to the current batch, flushing immediately if the size
// limit is reached.
func (p *MicroBatchPipeline) Push(event CanonicalEvent) {
	p.mu.Lock()
	p.buf = append(p.buf, event)

	if len(p.buf) >= p.maxBatchSize {
		batch := p.buf
		p.buf = make([]CanonicalEvent, 0, p.maxBatchSize)
		p.totalFlushed += int64(len(batch))
		p.flushCount++
		p.sizeFlushCount++
		// Rearm the timer so the next batch starts fresh
		if p.timer != nil {
			p.timer.Reset(time.Duration(p.flushTimeoutUs) * time.Microsecond)
		}
		p.mu.Unlock()

		p.flushFn(batch)
		return
	}
	p.mu.Unlock()
}

// Flush forces an immediate flush of any buffered events.
func (p *MicroBatchPipeline) Flush() {
	p.mu.Lock()
	if len(p.buf) == 0 {
		p.mu.Unlock()
		return
	}
	batch := p.buf
	p.buf = make([]CanonicalEvent, 0, p.maxBatchSize)
	p.totalFlushed += int64(len(batch))
	p.flushCount++
	p.mu.Unlock()
	p.flushFn(batch)
}

// Stop cancels the background timer and performs a final flush.
func (p *MicroBatchPipeline) Stop() {
	if p.timer != nil {
		p.timer.Stop()
	}
	p.Flush()
}

// Stats returns (totalFlushed, flushCount, timerFlushCount, sizeFlushCount).
func (p *MicroBatchPipeline) Stats() (int64, int64, int64, int64) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.totalFlushed, p.flushCount, p.timerFlushCount, p.sizeFlushCount
}
