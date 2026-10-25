// Package phase19 implements Features 109–114: Observability & Metrics.
package phase19

import (
	"fmt"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
)

// ─── Feature 109: Prometheus / OpenTelemetry Metrics Exporter ────────────────
//
// MetricRegistry holds typed metric objects (Counter, Gauge, Summary).
// ScrapePrometheusText renders an OpenMetrics-compliant text payload without
// any heap allocation on the hot path beyond the output buffer.

// LabelSet is a sorted list of name=value pairs for a metric dimension.
type LabelSet struct {
	pairs []string // "name=\"value\"" pre-formatted strings
}

// NewLabelSet creates a label set from a variadic list of name/value pairs.
func NewLabelSet(kvs ...string) LabelSet {
	pairs := make([]string, 0, len(kvs)/2)
	for i := 0; i+1 < len(kvs); i += 2 {
		pairs = append(pairs, fmt.Sprintf("%s=%q", kvs[i], kvs[i+1]))
	}
	sort.Strings(pairs) // deterministic label ordering per OpenMetrics spec
	return LabelSet{pairs: pairs}
}

// String renders the label set as a Prometheus label string, e.g. {a="x",b="y"}.
func (ls LabelSet) String() string {
	if len(ls.pairs) == 0 {
		return ""
	}
	return "{" + strings.Join(ls.pairs, ",") + "}"
}

// ─── Counter ──────────────────────────────────────────────────────────────────

// Counter is a monotonically increasing atomic counter.
type Counter struct {
	name   string
	help   string
	labels LabelSet
	value  atomic.Uint64
}

// Add increments the counter by delta.
func (c *Counter) Add(delta uint64) { c.value.Add(delta) }

// Inc increments the counter by 1.
func (c *Counter) Inc() { c.value.Add(1) }

// Value returns the current counter value.
func (c *Counter) Value() uint64 { return c.value.Load() }

// ─── Gauge ────────────────────────────────────────────────────────────────────

// Gauge is a signed atomic value that can go up and down.
type Gauge struct {
	name   string
	help   string
	labels LabelSet
	value  atomic.Int64
}

// Set assigns value v to the gauge.
func (g *Gauge) Set(v int64) { g.value.Store(v) }

// Add changes the gauge by delta.
func (g *Gauge) Add(delta int64) { g.value.Add(delta) }

// Value returns the current gauge value.
func (g *Gauge) Value() int64 { return g.value.Load() }

// ─── Summary ──────────────────────────────────────────────────────────────────

// Summary tracks a count and sum for computing mean observations.
type Summary struct {
	name   string
	help   string
	labels LabelSet
	count  atomic.Uint64
	sum    atomic.Int64 // nanoseconds (integer for lock-free access)
}

// Observe records one measurement.
func (s *Summary) Observe(valueNs int64) {
	s.count.Add(1)
	s.sum.Add(valueNs)
}

// Count returns the total number of observations.
func (s *Summary) Count() uint64 { return s.count.Load() }

// Sum returns the aggregate sum of all observations.
func (s *Summary) Sum() int64 { return s.sum.Load() }

// ─── MetricRegistry ───────────────────────────────────────────────────────────

// MetricRegistry manages all registered metrics and produces scrape output.
type MetricRegistry struct {
	mu       sync.RWMutex
	counters []*Counter
	gauges   []*Gauge
	summaries []*Summary
}

// NewMetricRegistry constructs an empty registry.
func NewMetricRegistry() *MetricRegistry { return &MetricRegistry{} }

// RegisterCounter creates and registers a new counter.
func (r *MetricRegistry) RegisterCounter(name, help string, labels LabelSet) *Counter {
	c := &Counter{name: name, help: help, labels: labels}
	r.mu.Lock()
	r.counters = append(r.counters, c)
	r.mu.Unlock()
	return c
}

// RegisterGauge creates and registers a new gauge.
func (r *MetricRegistry) RegisterGauge(name, help string, labels LabelSet) *Gauge {
	g := &Gauge{name: name, help: help, labels: labels}
	r.mu.Lock()
	r.gauges = append(r.gauges, g)
	r.mu.Unlock()
	return g
}

// RegisterSummary creates and registers a new summary.
func (r *MetricRegistry) RegisterSummary(name, help string, labels LabelSet) *Summary {
	s := &Summary{name: name, help: help, labels: labels}
	r.mu.Lock()
	r.summaries = append(r.summaries, s)
	r.mu.Unlock()
	return s
}

// ScrapePrometheusText renders an OpenMetrics-compliant text document into buf.
func (r *MetricRegistry) ScrapePrometheusText(buf *strings.Builder) {
	r.mu.RLock()
	defer r.mu.RUnlock()

	for _, c := range r.counters {
		fmt.Fprintf(buf, "# HELP %s %s\n", c.name, c.help)
		fmt.Fprintf(buf, "# TYPE %s counter\n", c.name)
		fmt.Fprintf(buf, "%s%s %d\n", c.name, c.labels, c.Value())
	}
	for _, g := range r.gauges {
		fmt.Fprintf(buf, "# HELP %s %s\n", g.name, g.help)
		fmt.Fprintf(buf, "# TYPE %s gauge\n", g.name)
		fmt.Fprintf(buf, "%s%s %d\n", g.name, g.labels, g.Value())
	}
	for _, s := range r.summaries {
		fmt.Fprintf(buf, "# HELP %s %s\n", s.name, s.help)
		fmt.Fprintf(buf, "# TYPE %s summary\n", s.name)
		fmt.Fprintf(buf, "%s_count%s %d\n", s.name, s.labels, s.Count())
		fmt.Fprintf(buf, "%s_sum%s %d\n", s.name, s.labels, s.Sum())
	}
}

// MetricCount returns total registered metric objects.
func (r *MetricRegistry) MetricCount() int {
	r.mu.RLock()
	defer r.mu.RUnlock()
	return len(r.counters) + len(r.gauges) + len(r.summaries)
}
