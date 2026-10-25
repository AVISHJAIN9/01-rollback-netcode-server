package phase11

import (
	"fmt"
	"strings"
	"sync/atomic"
)

// ─── Feature 64: Keyed State Telemetry Metrics Registry ──────────────────────
//
// All hot-path counters use sync/atomic to avoid lock contention.
// StateMetricsRegistry exposes a Prometheus-formatted text endpoint.

// StateMetricsRegistry holds global and per-shard atomic telemetry counters.
type StateMetricsRegistry struct {
	// Global counters
	ActiveKeys      atomic.Int64
	AllocatedBytes  atomic.Int64
	TTLEvictions    atomic.Int64
	LRUEvictions    atomic.Int64
	ReadHits        atomic.Int64
	ReadMisses      atomic.Int64
}

// NewStateMetricsRegistry constructs a zeroed registry.
func NewStateMetricsRegistry() *StateMetricsRegistry {
	return &StateMetricsRegistry{}
}

// RecordAlloc increments allocation counters.
func (r *StateMetricsRegistry) RecordAlloc(keys int64, bytes int64) {
	r.ActiveKeys.Add(keys)
	r.AllocatedBytes.Add(bytes)
}

// RecordEviction records an eviction (TTL or LRU type) and frees the accounting.
func (r *StateMetricsRegistry) RecordEviction(byTTL bool, keys int64, bytes int64) {
	if byTTL {
		r.TTLEvictions.Add(keys)
	} else {
		r.LRUEvictions.Add(keys)
	}
	r.ActiveKeys.Add(-keys)
	r.AllocatedBytes.Add(-bytes)
}

// RecordRead records a state read result.
func (r *StateMetricsRegistry) RecordRead(hit bool) {
	if hit {
		r.ReadHits.Add(1)
	} else {
		r.ReadMisses.Add(1)
	}
}

// HitRatio returns cache-hit ratio in [0,1].
func (r *StateMetricsRegistry) HitRatio() float64 {
	hits := r.ReadHits.Load()
	total := hits + r.ReadMisses.Load()
	if total == 0 {
		return 0
	}
	return float64(hits) / float64(total)
}

// FormatPrometheus returns a Prometheus text-format metrics document.
func (r *StateMetricsRegistry) FormatPrometheus() string {
	var sb strings.Builder

	metrics := []struct {
		help    string
		mtype   string
		name    string
		value   int64
	}{
		{"Number of active keyed-state entries", "gauge", "state_active_keys", r.ActiveKeys.Load()},
		{"Heap bytes allocated for keyed state", "gauge", "state_allocated_bytes", r.AllocatedBytes.Load()},
		{"Total TTL-triggered evictions", "counter", "state_ttl_evictions_total", r.TTLEvictions.Load()},
		{"Total LRU-triggered evictions", "counter", "state_lru_evictions_total", r.LRUEvictions.Load()},
		{"Total state read cache hits", "counter", "state_read_hits_total", r.ReadHits.Load()},
		{"Total state read cache misses", "counter", "state_read_misses_total", r.ReadMisses.Load()},
	}

	for _, m := range metrics {
		fmt.Fprintf(&sb, "# HELP %s %s\n", m.name, m.help)
		fmt.Fprintf(&sb, "# TYPE %s %s\n", m.name, m.mtype)
		fmt.Fprintf(&sb, "%s %d\n", m.name, m.value)
	}

	hitRatio := r.HitRatio()
	fmt.Fprintf(&sb, "# HELP state_cache_hit_ratio Cache hit ratio [0,1]\n")
	fmt.Fprintf(&sb, "# TYPE state_cache_hit_ratio gauge\n")
	fmt.Fprintf(&sb, "state_cache_hit_ratio %.6f\n", hitRatio)

	return sb.String()
}
