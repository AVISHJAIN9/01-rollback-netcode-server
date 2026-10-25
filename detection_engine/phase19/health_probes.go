package phase19

import (
	"fmt"
	"net/http"
	"sync/atomic"
	"time"
)

// ─── Feature 114: Real-Time Stream Engine Health & Readiness Probes ──────────
//
// HealthProbeServer exposes /livez and /readyz HTTP endpoints.
//
// /livez (liveness): returns 200 when all worker watchdog heartbeats are fresh.
// /readyz (readiness): returns 200 when:
//   • Rule registry is loaded.
//   • Input partitions are connected.
//   • Ingress queue fill ratio is below the backpressure threshold.
// Returns 503 Service Unavailable on any failure condition.

// ProbeStatus describes the result of a health check.
type ProbeStatus struct {
	Healthy bool
	Reason  string
}

// HealthCondition is a boolean atomic that represents one readiness condition.
type HealthCondition struct {
	ready   atomic.Bool
	name    string
}

// NewHealthCondition creates a named condition, initially not-ready.
func NewHealthCondition(name string) *HealthCondition {
	return &HealthCondition{name: name}
}

// SetReady marks the condition as ready (or not).
func (c *HealthCondition) SetReady(ready bool) { c.ready.Store(ready) }

// IsReady returns the current readiness state.
func (c *HealthCondition) IsReady() bool { return c.ready.Load() }

// Name returns the condition's descriptive name.
func (c *HealthCondition) Name() string { return c.name }

// HealthProbeServer manages liveness and readiness endpoints.
type HealthProbeServer struct {
	// Liveness: worker watchdog heartbeats
	workerHeartbeats  []atomic.Int64 // per-worker last heartbeat (ns)
	maxHeartbeatStale int64          // max allowed heartbeat age in ns

	// Readiness conditions
	conditions []*HealthCondition

	// Queue fill ratio (updated externally)
	queueFillRatio      atomic.Value // float64
	maxQueueFillRatio   float64      // e.g., 0.85 → 503 if exceeded

	server *http.Server
}

// NewHealthProbeServer constructs a probe server for numWorkers workers.
func NewHealthProbeServer(numWorkers int, maxHeartbeatStaleMs int64) *HealthProbeServer {
	h := &HealthProbeServer{
		workerHeartbeats:    make([]atomic.Int64, numWorkers),
		maxHeartbeatStale:   maxHeartbeatStaleMs * int64(time.Millisecond),
		maxQueueFillRatio:   0.85,
	}
	now := time.Now().UnixNano()
	for i := range h.workerHeartbeats {
		h.workerHeartbeats[i].Store(now)
	}
	h.queueFillRatio.Store(float64(0))
	return h
}

// RegisterCondition adds a readiness condition.
func (h *HealthProbeServer) RegisterCondition(c *HealthCondition) {
	h.conditions = append(h.conditions, c)
}

// UpdateWorkerHeartbeat records a fresh heartbeat for workerID.
func (h *HealthProbeServer) UpdateWorkerHeartbeat(workerID int) {
	if workerID >= 0 && workerID < len(h.workerHeartbeats) {
		h.workerHeartbeats[workerID].Store(time.Now().UnixNano())
	}
}

// SetQueueFillRatio updates the current queue pressure reading.
func (h *HealthProbeServer) SetQueueFillRatio(ratio float64) {
	h.queueFillRatio.Store(ratio)
}

// CheckLiveness returns healthy if all worker heartbeats are fresh.
func (h *HealthProbeServer) CheckLiveness() ProbeStatus {
	now := time.Now().UnixNano()
	for i, hb := range h.workerHeartbeats {
		age := now - hb.Load()
		if age > h.maxHeartbeatStale {
			return ProbeStatus{
				Healthy: false,
				Reason:  fmt.Sprintf("worker %d heartbeat stale by %dms", i, age/int64(time.Millisecond)),
			}
		}
	}
	return ProbeStatus{Healthy: true, Reason: "all workers alive"}
}

// CheckReadiness returns healthy when all readiness conditions are met and
// the queue fill ratio is below the backpressure threshold.
func (h *HealthProbeServer) CheckReadiness() ProbeStatus {
	for _, c := range h.conditions {
		if !c.IsReady() {
			return ProbeStatus{
				Healthy: false,
				Reason:  fmt.Sprintf("readiness condition not met: %s", c.Name()),
			}
		}
	}

	fill := h.queueFillRatio.Load().(float64)
	if fill > h.maxQueueFillRatio {
		return ProbeStatus{
			Healthy: false,
			Reason:  fmt.Sprintf("ingress queue overloaded: fill=%.1f%% > max=%.1f%%",
				fill*100, h.maxQueueFillRatio*100),
		}
	}

	return ProbeStatus{Healthy: true, Reason: "ready"}
}

// ServeHTTP registers /livez and /readyz handlers on mux.
// Pass http.DefaultServeMux or a custom mux.
func (h *HealthProbeServer) RegisterHandlers(mux *http.ServeMux) {
	mux.HandleFunc("/livez", func(w http.ResponseWriter, r *http.Request) {
		status := h.CheckLiveness()
		if status.Healthy {
			w.WriteHeader(http.StatusOK)
			fmt.Fprintln(w, status.Reason)
		} else {
			w.WriteHeader(http.StatusServiceUnavailable)
			fmt.Fprintln(w, status.Reason)
		}
	})

	mux.HandleFunc("/readyz", func(w http.ResponseWriter, r *http.Request) {
		status := h.CheckReadiness()
		if status.Healthy {
			w.WriteHeader(http.StatusOK)
			fmt.Fprintln(w, status.Reason)
		} else {
			w.WriteHeader(http.StatusServiceUnavailable)
			fmt.Fprintln(w, status.Reason)
		}
	})
}
