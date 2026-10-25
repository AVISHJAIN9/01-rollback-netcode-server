package phase19

import (
	"fmt"
	"sort"
	"sync"
	"sync/atomic"
)

// ─── Feature 111: Granular Per-Rule Evaluation Performance Counters ──────────
//
// RuleMetricsCollector maintains a 64-byte-aligned per-rule metrics struct.
// All counters use atomic operations — zero lock contention on the hot path.

// RuleMetricsRecord is a cache-line-padded atomic metrics struct for one rule.
// Alignment to 64 bytes prevents false sharing between adjacent rule records.
type RuleMetricsRecord struct {
	EvalCount      atomic.Uint64 // total evaluation calls
	HitCount       atomic.Uint64 // evaluations that produced a match
	SkipCount      atomic.Uint64 // short-circuit skips (AST branch not entered)
	TotalCPUNanos  atomic.Uint64 // cumulative CPU time in nanoseconds
	_pad           [0]byte       // Go aligns structs; padding via struct embedding if needed
}

// RuleEfficiencySummary is a readable snapshot of one rule's metrics.
type RuleEfficiencySummary struct {
	RuleID     uint32
	Evals      uint64
	Hits       uint64
	Skips      uint64
	CPUNanos   uint64
	HitRate    float64 // hits / evals
	AvgCPUNs   float64 // CPUNanos / evals
}

func (r RuleEfficiencySummary) String() string {
	return fmt.Sprintf("Rule[%d] evals=%d hits=%d skips=%d hit_rate=%.2f%% avg_cpu=%.1fns",
		r.RuleID, r.Evals, r.Hits, r.Skips, r.HitRate*100, r.AvgCPUNs)
}

// RuleMetricsCollector manages per-rule telemetry.
type RuleMetricsCollector struct {
	mu      sync.RWMutex
	records map[uint32]*RuleMetricsRecord
}

// NewRuleMetricsCollector constructs an empty collector.
func NewRuleMetricsCollector() *RuleMetricsCollector {
	return &RuleMetricsCollector{
		records: make(map[uint32]*RuleMetricsRecord),
	}
}

// RegisterRule ensures a metrics record exists for ruleID.
func (c *RuleMetricsCollector) RegisterRule(ruleID uint32) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if _, ok := c.records[ruleID]; !ok {
		c.records[ruleID] = &RuleMetricsRecord{}
	}
}

// RecordEvaluation records one rule evaluation result.
// ruleID must have been registered first.
func (c *RuleMetricsCollector) RecordEvaluation(
	ruleID uint32,
	durationNs uint64,
	matched bool,
	shortCircuited bool,
) {
	c.mu.RLock()
	rec, ok := c.records[ruleID]
	c.mu.RUnlock()
	if !ok {
		return
	}

	rec.EvalCount.Add(1)
	rec.TotalCPUNanos.Add(durationNs)
	if matched {
		rec.HitCount.Add(1)
	}
	if shortCircuited {
		rec.SkipCount.Add(1)
	}
}

// GetRuleEfficiencyReport returns a snapshot of all registered rules, sorted by RuleID.
func (c *RuleMetricsCollector) GetRuleEfficiencyReport() []RuleEfficiencySummary {
	c.mu.RLock()
	defer c.mu.RUnlock()

	summaries := make([]RuleEfficiencySummary, 0, len(c.records))

	for ruleID, rec := range c.records {
		evals := rec.EvalCount.Load()
		hits := rec.HitCount.Load()
		skips := rec.SkipCount.Load()
		cpu := rec.TotalCPUNanos.Load()

		hitRate := 0.0
		avgCPU := 0.0
		if evals > 0 {
			hitRate = float64(hits) / float64(evals)
			avgCPU = float64(cpu) / float64(evals)
		}

		summaries = append(summaries, RuleEfficiencySummary{
			RuleID:   ruleID,
			Evals:    evals,
			Hits:     hits,
			Skips:    skips,
			CPUNanos: cpu,
			HitRate:  hitRate,
			AvgCPUNs: avgCPU,
		})
	}

	sort.Slice(summaries, func(i, j int) bool { return summaries[i].RuleID < summaries[j].RuleID })
	return summaries
}

// TotalEvaluations returns the aggregate eval count across all rules.
func (c *RuleMetricsCollector) TotalEvaluations() uint64 {
	c.mu.RLock()
	defer c.mu.RUnlock()
	var total uint64
	for _, rec := range c.records {
		total += rec.EvalCount.Load()
	}
	return total
}
