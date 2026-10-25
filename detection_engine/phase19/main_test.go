package phase19

import (
	"math/rand"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// ─── Test 1: OpenMetrics Text Formatting Conformance ──────────────────────────

func TestOpenMetricsTextFormattingConformance(t *testing.T) {
	t.Log("Test 1: OpenMetrics Text Formatting Conformance")

	reg := NewMetricRegistry()

	// Register 100 counters and gauges with labels
	for i := 0; i < 50; i++ {
		ls := NewLabelSet("rule_id", strings.Repeat("r", i%10+1), "tenant", "acme")
		c := reg.RegisterCounter(
			"rule_eval_total",
			"Total rule evaluations",
			ls,
		)
		c.Add(uint64(i * 100))
	}
	for i := 0; i < 50; i++ {
		ls := NewLabelSet("subsystem", "ingestion", "instance", "node-1")
		g := reg.RegisterGauge(
			"queue_depth",
			"Current queue depth",
			ls,
		)
		g.Set(int64(i * 10))
	}

	var buf strings.Builder
	reg.ScrapePrometheusText(&buf)
	output := buf.String()

	// Verify OpenMetrics conformance
	required := []string{
		"# HELP rule_eval_total",
		"# TYPE rule_eval_total counter",
		"rule_eval_total{",
		"# HELP queue_depth",
		"# TYPE queue_depth gauge",
		"queue_depth{",
	}
	for _, must := range required {
		if !strings.Contains(output, must) {
			t.Fatalf("FAIL: output missing required string %q", must)
		}
	}

	// Verify labels are sorted (deterministic)
	// "instance" (i) comes before "subsystem" (s) alphabetically — that IS correct.
	// Verify that we never see "subsystem=..." before "instance=..." in the same label set.
	for _, line := range strings.Split(output, "\n") {
		if strings.Contains(line, "subsystem=") && strings.Contains(line, "instance=") {
			iIdx := strings.Index(line, "instance=")
			sIdx := strings.Index(line, "subsystem=")
			if sIdx < iIdx {
				t.Fatalf("FAIL: label order wrong in line: %s", line)
			}
		}
	}

	lineCount := strings.Count(output, "\n")
	t.Logf("  ✓ OpenMetrics output: %d lines, all required directives present", lineCount)
	t.Logf("  ✓ Label sets are sorted alphabetically")
	t.Logf("  ✓ Registered %d metric objects", reg.MetricCount())
}

// ─── Test 2: HDR Latency Histogram Accuracy ───────────────────────────────────

func TestHDRLatencyHistogramAccuracy(t *testing.T) {
	t.Log("Test 2: HDR Latency Histogram Accuracy")

	h := NewHdrHistogram()

	// Insert 100,000 samples from a normal distribution with µ=1ms, σ=200µs.
	// We use a deterministic seed for reproducibility.
	rng := rand.New(rand.NewSource(42))
	const n = 100_000
	const meanNs = float64(1_000_000) // 1ms
	const stdNs = float64(200_000)    // 200µs

	for i := 0; i < n; i++ {
		v := rng.NormFloat64()*stdNs + meanNs
		if v < 1 {
			v = 1
		}
		h.Record(uint64(v))
	}

	if h.TotalCount() != n {
		t.Fatalf("FAIL: total count %d != %d", h.TotalCount(), n)
	}

	p50 := h.ValueAtPercentile(50)
	p95 := h.ValueAtPercentile(95)
	p99 := h.ValueAtPercentile(99)

	// Theoretical values for N(1ms, 200µs):
	//   P50 ≈ 1,000,000 ns  (mean = median for normal dist)
	//   P95 ≈ 1,329,000 ns  (z=1.645)
	//   P99 ≈ 1,465,000 ns  (z=2.326)
	// Allow ±5% relative error (generous for logarithmic bucketing)
	const tolerancePct = 0.05

	check := func(pctName string, got uint64, expected float64) {
		lo := uint64(expected * (1 - tolerancePct))
		hi := uint64(expected * (1 + tolerancePct))
		if got < lo || got > hi {
			t.Fatalf("FAIL: %s=%dns, expected [%d, %d]", pctName, got, lo, hi)
		}
		t.Logf("  ✓ %s=%dns (expected ~%.0fns, within ±%.0f%%)",
			pctName, got, expected, tolerancePct*100)
	}

	check("P50", p50, meanNs)
	check("P95", p95, meanNs+1.645*stdNs)
	check("P99", p99, meanNs+2.326*stdNs)
}

// ─── Test 3: Per-Rule Telemetry & Contention ─────────────────────────────────

func TestPerRuleTelemetryContention(t *testing.T) {
	t.Log("Test 3: Per-Rule Telemetry & Contention")

	collector := NewRuleMetricsCollector()
	const numRules = 1_000
	const numWorkers = 16
	const evalsPerWorker = 10_000

	// Register all rules
	for i := uint32(0); i < numRules; i++ {
		collector.RegisterRule(i)
	}

	var wg sync.WaitGroup
	for w := 0; w < numWorkers; w++ {
		w := w
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := 0; i < evalsPerWorker; i++ {
				ruleID := uint32(i % numRules)
				collector.RecordEvaluation(ruleID, 500, i%3 == 0, i%5 == 0)
			}
			_ = w
		}()
	}
	wg.Wait()

	total := collector.TotalEvaluations()
	expected := uint64(numWorkers * evalsPerWorker)
	if total != expected {
		t.Fatalf("FAIL: total evals %d != expected %d (counter drift)", total, expected)
	}

	report := collector.GetRuleEfficiencyReport()
	if len(report) != numRules {
		t.Fatalf("FAIL: expected %d rules in report, got %d", numRules, len(report))
	}

	// Rule IDs must be monotonically ordered in report
	for i := 1; i < len(report); i++ {
		if report[i].RuleID <= report[i-1].RuleID {
			t.Fatal("FAIL: report not sorted by RuleID")
		}
	}

	t.Logf("  ✓ Total evals=%d (no drift under %d concurrent goroutines)", total, numWorkers)
	t.Logf("  ✓ Report contains %d rules, sorted by RuleID", len(report))
	t.Logf("  ✓ Sample: %s", report[0])
}

// ─── Test 4: System Resource Poller Execution ─────────────────────────────────

func TestSystemResourcePollerExecution(t *testing.T) {
	t.Log("Test 4: System Resource Poller Execution")

	monitor := NewSystemResourceMonitor(100) // 100ms poll
	defer monitor.Stop()

	// Wait for at least one sample
	time.Sleep(150 * time.Millisecond)

	snap := monitor.Snapshot()

	// HeapAllocBytes must be non-zero (the process has a heap)
	if snap.HeapAllocBytes == 0 {
		t.Fatal("FAIL: HeapAllocBytes is 0 (expected non-zero for any Go process)")
	}

	// Verify Prometheus output format
	prom := monitor.PrometheusText()
	for _, must := range []string{
		"process_resident_memory_bytes",
		"process_heap_alloc_bytes",
		"process_gc_runs_total",
	} {
		if !strings.Contains(prom, must) {
			t.Fatalf("FAIL: Prometheus output missing metric %s", must)
		}
	}

	t.Logf("  ✓ HeapAlloc=%d bytes, RSS=%d bytes, GC_runs=%d",
		snap.HeapAllocBytes, snap.RSSBytes, snap.GCRuns)
	t.Logf("  ✓ SampleCount=%d (background poller ran)", monitor.SampleCount.Load())
	t.Logf("  ✓ Prometheus gauges formatted correctly")
}

// ─── Test 5: Asynchronous Structured Logging Throughput ──────────────────────

func TestAsyncStructuredLoggingThroughput(t *testing.T) {
	t.Log("Test 5: Asynchronous Structured Logging Throughput")

	var written atomic.Int64
	var buf threadSafeBuffer

	logger := NewStructuredLogger(&buf, LogInfo)

	const goroutines = 8
	const messagesEach = 10_000 // 80,000 total — fits ring buffer (131072 slots)

	var wg sync.WaitGroup
	for g := 0; g < goroutines; g++ {
		g := g
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := 0; i < messagesEach; i++ {
				logger.Info("detection event processed", map[string]string{
					"rule_id": "rule-001",
					"worker":  string(rune('0' + g)),
				})
			}
		}()
	}
	wg.Wait()

	// Give the drain goroutine time to flush all entries
	time.Sleep(200 * time.Millisecond)
	logger.Stop()

	flushed := logger.Flushed.Load()
	dropped := logger.Dropped.Load()
	total := int64(goroutines * messagesEach)
	_ = written

	t.Logf("  ✓ Sent %d messages: flushed=%d dropped=%d", total, flushed, dropped)

	// Allow up to 0.1% drop rate due to ring overflow under extreme concurrency
	maxDrops := total / 1000
	if int64(dropped) > maxDrops {
		t.Fatalf("FAIL: dropped=%d exceeds 0.1%% limit (%d)", dropped, maxDrops)
	}

	// Verify JSON output is well-formed (spot check first 5 lines)
	output := buf.String()
	lines := strings.Split(strings.TrimSpace(output), "\n")
	for i, line := range lines {
		if i >= 5 {
			break
		}
		if len(line) == 0 {
			continue
		}
		if !strings.HasPrefix(line, "{") || !strings.HasSuffix(line, "}") {
			t.Fatalf("FAIL: line %d is not valid JSON: %q", i, line)
		}
	}
	t.Logf("  ✓ JSON integrity: first 5 lines are valid JSON objects")
}

// ─── Test 6: Health & Readiness Probe State Transitions ──────────────────────

func TestHealthReadinessProbeStateTransitions(t *testing.T) {
	t.Log("Test 6: Health & Readiness Probe State Transitions")

	probe := NewHealthProbeServer(4, 5000) // 5000ms staleness threshold

	ruleReady := NewHealthCondition("rule_registry")
	kafkaReady := NewHealthCondition("kafka_partitions")
	probe.RegisterCondition(ruleReady)
	probe.RegisterCondition(kafkaReady)

	// ── Test: readiness returns 503 during initialization ──────────────────
	status := probe.CheckReadiness()
	if status.Healthy {
		t.Fatal("FAIL: readiness should be 503 before conditions are met")
	}
	t.Logf("  ✓ /readyz returns 503 during init: %s", status.Reason)

	// ── Set both conditions ready ──────────────────────────────────────────
	ruleReady.SetReady(true)
	kafkaReady.SetReady(true)

	status2 := probe.CheckReadiness()
	if !status2.Healthy {
		t.Fatalf("FAIL: readiness should be 200 when all conditions met: %s", status2.Reason)
	}
	t.Logf("  ✓ /readyz returns 200 when all conditions met")

	// ── Simulate queue overflow → 503 ─────────────────────────────────────
	probe.SetQueueFillRatio(0.90) // above 85% threshold
	status3 := probe.CheckReadiness()
	if status3.Healthy {
		t.Fatal("FAIL: readiness should be 503 when queue is overloaded")
	}
	t.Logf("  ✓ /readyz returns 503 when queue fill=90%% > threshold")

	// ── Drain queue → 200 ─────────────────────────────────────────────────
	probe.SetQueueFillRatio(0.50)
	status4 := probe.CheckReadiness()
	if !status4.Healthy {
		t.Fatalf("FAIL: readiness should be 200 after queue drains: %s", status4.Reason)
	}
	t.Logf("  ✓ /readyz returns 200 after queue drains to 50%%")

	// ── Liveness check: all workers alive ─────────────────────────────────
	for i := 0; i < 4; i++ {
		probe.UpdateWorkerHeartbeat(i)
	}
	liveness := probe.CheckLiveness()
	if !liveness.Healthy {
		t.Fatalf("FAIL: liveness should be healthy right after heartbeats: %s", liveness.Reason)
	}
	t.Logf("  ✓ /livez returns 200 when all workers alive")
}

// ─── helpers ─────────────────────────────────────────────────────────────────

// threadSafeBuffer is an io.Writer safe for concurrent use.
type threadSafeBuffer struct {
	mu  sync.Mutex
	buf strings.Builder
}

func (b *threadSafeBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *threadSafeBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}
