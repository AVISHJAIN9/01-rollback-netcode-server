package phase18

import (
	"fmt"
	"sync"
	"sync/atomic"
	"time"
)

// ─── Feature 107: Worker Thread Health Watchdog & Deadlock Detector ──────────
//
// WorkerWatchdog monitors a pool of worker goroutines via monotonic heartbeat
// timestamps.  Each worker calls Heartbeat(workerID) at the end of every batch.
// The supervisor calls CheckHealth(now, maxStaleness) to detect stalled workers.
//
// When a worker's heartbeat is older than maxStaleness, it is flagged as
// unresponsive and its cancellation token is fired, allowing a replacement to
// be spawned.

// UnresponsiveWorker describes a detected deadlock or stall.
type UnresponsiveWorker struct {
	WorkerID     int
	LastBeatNs   int64
	StalenessNs  int64
}

func (u UnresponsiveWorker) String() string {
	return fmt.Sprintf("Worker[%d] stale by %.1fms (lastBeat=%dns)",
		u.WorkerID, float64(u.StalenessNs)/1e6, u.LastBeatNs)
}

// workerRecord holds per-worker watchdog state.
type workerRecord struct {
	heartbeatNs atomic.Int64
	_pad        [56]byte  // cache-line padding
	stopCh      chan struct{}
	stopped     atomic.Bool
}

// WorkerWatchdog supervises a pool of worker goroutines.
type WorkerWatchdog struct {
	mu      sync.Mutex
	workers []*workerRecord
	// recoveredCount tracks how many stalled workers have been recovered.
	recoveredCount atomic.Int64
}

// NewWorkerWatchdog constructs a watchdog for numWorkers workers.
// All heartbeats are initialised to now so no false positives fire immediately.
func NewWorkerWatchdog(numWorkers int) *WorkerWatchdog {
	now := time.Now().UnixNano()
	workers := make([]*workerRecord, numWorkers)
	for i := 0; i < numWorkers; i++ {
		r := &workerRecord{stopCh: make(chan struct{}, 1)}
		r.heartbeatNs.Store(now)
		workers[i] = r
	}
	return &WorkerWatchdog{workers: workers}
}

// Heartbeat is called by worker workerID at the end of each batch iteration.
func (w *WorkerWatchdog) Heartbeat(workerID int) {
	if workerID < 0 || workerID >= len(w.workers) {
		return
	}
	w.workers[workerID].heartbeatNs.Store(time.Now().UnixNano())
}

// StopChannel returns the channel the worker should select on to receive a
// termination signal.
func (w *WorkerWatchdog) StopChannel(workerID int) <-chan struct{} {
	return w.workers[workerID].stopCh
}

// CheckHealth inspects all worker heartbeats and returns a slice of stalled workers.
// maxStalenessNs is the maximum allowed interval between heartbeats.
func (w *WorkerWatchdog) CheckHealth(nowNs, maxStalenessNs int64) []UnresponsiveWorker {
	var stalled []UnresponsiveWorker

	for i, rec := range w.workers {
		if rec.stopped.Load() {
			continue
		}
		last := rec.heartbeatNs.Load()
		staleness := nowNs - last
		if staleness > maxStalenessNs {
			stalled = append(stalled, UnresponsiveWorker{
				WorkerID:    i,
				LastBeatNs:  last,
				StalenessNs: staleness,
			})
		}
	}
	return stalled
}

// RecoverWorker signals workerID to stop and marks its slot as recoverable.
// The caller is responsible for spawning a replacement goroutine.
func (w *WorkerWatchdog) RecoverWorker(workerID int) {
	if workerID < 0 || workerID >= len(w.workers) {
		return
	}
	rec := w.workers[workerID]
	if rec.stopped.CompareAndSwap(false, true) {
		select {
		case rec.stopCh <- struct{}{}:
		default:
		}
		w.recoveredCount.Add(1)
	}
}

// ReplaceWorker resets a recovered worker slot so a new goroutine can use it.
func (w *WorkerWatchdog) ReplaceWorker(workerID int) {
	if workerID < 0 || workerID >= len(w.workers) {
		return
	}
	w.mu.Lock()
	defer w.mu.Unlock()

	rec := &workerRecord{stopCh: make(chan struct{}, 1)}
	rec.heartbeatNs.Store(time.Now().UnixNano())
	w.workers[workerID] = rec
}

// RecoveredCount returns total workers recovered since construction.
func (w *WorkerWatchdog) RecoveredCount() int64 { return w.recoveredCount.Load() }

// WorkerCount returns the number of worker slots.
func (w *WorkerWatchdog) WorkerCount() int { return len(w.workers) }
