package phase16

import (
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// ─── Test 1: Canary Sandbox Panic Isolation ───────────────────────────────────

func TestCanarySandboxPanicIsolation(t *testing.T) {
	t.Log("Test 1: Canary Sandbox Panic Isolation")

	sandbox := NewReloadSandbox(DefaultSandboxConfig())
	initialActive := sandbox.ActiveRuleSetVersion() // should be 0

	corpus := []*CanonicalEvent{
		{EventID: "e1", TimestampNs: 1000, EntityID: "host-1", EventType: "ProcessCreate"},
	}

	// Good candidate — should be promoted
	goodCandidate := &CandidateRuleSet{
		Version:      1,
		CompilePanic: false,
		EvalFns: []RuleEvalFn{
			func(e *CanonicalEvent) bool { return e.EventType == "ProcessCreate" },
		},
	}
	if err := sandbox.ValidateCandidate(goodCandidate, corpus); err != nil {
		t.Fatalf("FAIL: good candidate rejected: %v", err)
	}
	if sandbox.ActiveRuleSetVersion() != 1 {
		t.Fatalf("FAIL: expected active version=1, got %d", sandbox.ActiveRuleSetVersion())
	}
	t.Logf("  ✓ Good candidate (v1) promoted, activeVersion=%d", sandbox.ActiveRuleSetVersion())

	// Panicking candidate — must be rejected and active version must remain 1
	panicCandidate := &CandidateRuleSet{
		Version:      2,
		CompilePanic: true, // triggers panic("simulated compile-time panic: ...")
	}
	err := sandbox.ValidateCandidate(panicCandidate, corpus)
	if err == nil {
		t.Fatal("FAIL: panicking candidate should have been rejected, got nil error")
	}
	if sandbox.ActiveRuleSetVersion() != 1 {
		t.Fatalf("FAIL: active version changed after panic rejection: got %d, expected 1",
			sandbox.ActiveRuleSetVersion())
	}
	t.Logf("  ✓ Panicking candidate (v2) rejected: %v", err)
	t.Logf("  ✓ Active ruleset preserved at v%d (unchanged)", sandbox.ActiveRuleSetVersion())
	_ = initialActive
}

// ─── Test 2: Bounded Ring Queue Watermark Signaling ──────────────────────────

func TestBoundedRingQueueWatermark(t *testing.T) {
	t.Log("Test 2: Bounded Ring Queue Watermark Signaling")

	const capacity = 100
	q := NewBoundedRingQueue(capacity)
	// nextPowerOfTwo(100) = 128
	realCap := q.Capacity()
	highWM := int(float64(realCap) * 0.80)

	t.Logf("  Queue capacity=%d, high watermark=%d items", realCap, highWM)

	// Fill beyond 80% and check for HighWatermarkReached
	hwmSeen := false
	for i := 0; i < realCap; i++ {
		result := q.TryPush(i)
		if result == PushHighWatermark {
			hwmSeen = true
		}
		if result == PushFull {
			break
		}
	}

	if !hwmSeen {
		t.Fatal("FAIL: HighWatermarkReached was never signaled")
	}
	t.Logf("  ✓ HighWatermarkReached signaled at fill >80%%")
	t.Logf("  ✓ Queue above high watermark: %v (fill=%.1f%%)",
		q.IsAboveHighWatermark(), q.FillRatio()*100)

	// Drain below 60% (low watermark)
	lowWM := int(float64(realCap) * 0.60)
	drained := 0
	for q.Size() > uint64(lowWM) {
		_, ok := q.Pop()
		if !ok {
			break
		}
		drained++
	}

	if !q.IsBelowLowWatermark() {
		t.Fatalf("FAIL: queue should be below low watermark after draining %d items, fill=%.1f%%",
			drained, q.FillRatio()*100)
	}
	t.Logf("  ✓ Drained %d items; queue below low watermark (fill=%.1f%%)",
		drained, q.FillRatio()*100)

	// Full queue returns PushFull
	fullQ := NewBoundedRingQueue(4) // tiny queue for easy overflow
	for i := 0; i < fullQ.Capacity(); i++ {
		fullQ.TryPush(i)
	}
	result := fullQ.TryPush(999)
	if result != PushFull {
		t.Fatalf("FAIL: expected PushFull when queue is at capacity, got %s", result)
	}
	t.Logf("  ✓ PushFull returned when queue at capacity")
}

// ─── Test 3: Consumer Lag & Velocity Estimation ───────────────────────────────

func TestConsumerLagVelocityEstimation(t *testing.T) {
	t.Log("Test 3: Consumer Lag & Velocity Estimation")

	monitor := NewConsumerLagMonitor()
	monitor.RegisterPartition(0, 100_000)

	// Simulate production at 10,000 EPS, consumption at 5,000 EPS over 1 second
	// Initial state: both at offset 0
	monitor.UpdateOffsets(0, 0, 0)
	time.Sleep(50 * time.Millisecond)

	// After ~1 second: producer advanced 10,000, consumer advanced 5,000
	monitor.UpdateOffsets(0, 10_000, 5_000)

	report, ok := monitor.CalculateLag(0)
	if !ok {
		t.Fatal("FAIL: partition 0 not found")
	}

	if report.LagEvents != 5_000 {
		t.Fatalf("FAIL: expected lag=5000, got %d", report.LagEvents)
	}
	if report.LagVelocity <= 0 {
		t.Fatalf("FAIL: expected positive lag velocity (lag is growing), got %.1f", report.LagVelocity)
	}

	t.Logf("  ✓ Lag=%d events, ProductionEPS=%.0f, ConsumptionEPS=%.0f, Velocity=%.1f/s",
		report.LagEvents, report.ProductionEPS, report.ConsumptionEPS, report.LagVelocity)

	// Simulate consumer catching up
	monitor.UpdateOffsets(0, 20_000, 20_000)
	report2, _ := monitor.CalculateLag(0)
	if report2.LagEvents != 0 {
		t.Fatalf("FAIL: lag should be 0 when consumer caught up, got %d", report2.LagEvents)
	}
	t.Logf("  ✓ After catch-up: lag=%d", report2.LagEvents)
}

// ─── Test 4: Reactive Backpressure Pause & Resume ────────────────────────────

func TestReactiveBackpressurePauseResume(t *testing.T) {
	t.Log("Test 4: Reactive Backpressure Pause & Resume Flow")

	bp := NewBackpressureController(0.80, 0.60)

	// Queue at 0% — should continue
	action := bp.CheckPressure(0.0)
	if action != FlowContinue {
		t.Fatalf("FAIL: expected Continue at 0%% fill, got %s", action)
	}

	// Queue at 82% — must trigger pause
	action = bp.CheckPressure(0.82)
	if action != FlowPause {
		t.Fatalf("FAIL: expected Pause at 82%% fill, got %s", action)
	}
	if bp.CurrentState() != BPStatePaused {
		t.Fatal("FAIL: controller should be in Paused state")
	}
	t.Logf("  ✓ Pause triggered at 82%% fill")

	// Queue at 75% (still above low watermark) — continue pause (no state change)
	action = bp.CheckPressure(0.75)
	if action != FlowContinue {
		t.Fatalf("FAIL: expected Continue while paused at 75%%, got %s", action)
	}

	// Queue drains to 55% (below 60% low watermark) — must resume
	action = bp.CheckPressure(0.55)
	if action != FlowResume {
		t.Fatalf("FAIL: expected Resume at 55%% fill, got %s", action)
	}
	if bp.CurrentState() != BPStateActive {
		t.Fatal("FAIL: controller should be in Active state after resume")
	}
	t.Logf("  ✓ Resume triggered at 55%% fill")

	if bp.PauseCount() != 1 || bp.ResumeCount() != 1 {
		t.Fatalf("FAIL: pause_count=%d resume_count=%d, expected both=1",
			bp.PauseCount(), bp.ResumeCount())
	}
	t.Logf("  ✓ PauseCount=%d, ResumeCount=%d", bp.PauseCount(), bp.ResumeCount())
}

// ─── Test 5: Multi-Tenant Token-Bucket Rate Isolation ─────────────────────────

func TestMultiTenantRateLimiterIsolation(t *testing.T) {
	t.Log("Test 5: Multi-Tenant Token-Bucket Rate Isolation")

	limiter := NewTenantRateLimiter()

	// Tenant A: 1,000 EPS sustained, burst=1,000
	limiter.RegisterTenant("tenant-A", 1_000, 1_000)
	// Tenant B: 10,000 EPS sustained, burst=10,000
	limiter.RegisterTenant("tenant-B", 10_000, 10_000)

	// Tenant B should be able to acquire all burst tokens immediately
	allowedB := 0
	for i := 0; i < 10_000; i++ {
		if limiter.TryAcquire("tenant-B", 1) {
			allowedB++
		}
	}
	if allowedB != 10_000 {
		t.Fatalf("FAIL: tenant-B should acquire 10000 tokens, got %d", allowedB)
	}

	// Tenant A should exhaust its 1,000-token burst quickly
	allowedA := 0
	throttledA := 0
	for i := 0; i < 2_000; i++ {
		if limiter.TryAcquire("tenant-A", 1) {
			allowedA++
		} else {
			throttledA++
		}
	}
	if allowedA != 1_000 {
		t.Fatalf("FAIL: tenant-A should allow exactly 1000, got %d (throttled=%d)", allowedA, throttledA)
	}
	if throttledA != 1_000 {
		t.Fatalf("FAIL: tenant-A should throttle 1000, got %d", throttledA)
	}

	t.Logf("  ✓ Tenant-A: allowed=%d, throttled=%d (burst=1000 exhausted)", allowedA, throttledA)
	t.Logf("  ✓ Tenant-B: allowed=%d (unhindered)", allowedB)

	// Concurrent isolation: one goroutine for each tenant
	var wg sync.WaitGroup
	var aAllowed, bAllowed atomic.Int64

	// Re-register for concurrent test with fresh buckets
	limiter2 := NewTenantRateLimiter()
	limiter2.RegisterTenant("tenant-A", 500, 500)
	limiter2.RegisterTenant("tenant-B", 5_000, 5_000)

	wg.Add(2)
	go func() {
		defer wg.Done()
		for i := 0; i < 1_000; i++ {
			if limiter2.TryAcquire("tenant-A", 1) {
				aAllowed.Add(1)
			}
		}
	}()
	go func() {
		defer wg.Done()
		for i := 0; i < 5_000; i++ {
			if limiter2.TryAcquire("tenant-B", 1) {
				bAllowed.Add(1)
			}
		}
	}()
	wg.Wait()

	if aAllowed.Load() > 500 {
		t.Fatalf("FAIL: tenant-A allowed %d tokens, expected ≤500", aAllowed.Load())
	}
	if bAllowed.Load() != 5_000 {
		t.Fatalf("FAIL: tenant-B allowed %d tokens, expected 5000", bAllowed.Load())
	}
	t.Logf("  ✓ Concurrent: tenant-A=%d/1000 ≤500, tenant-B=%d/5000 unhindered",
		aAllowed.Load(), bAllowed.Load())
}

// ─── Test 6: Adaptive Batch Sizing Scaling ────────────────────────────────────

func TestAdaptiveBatchSizingScaling(t *testing.T) {
	t.Log("Test 6: Adaptive Batch Sizing Scaling")

	const (
		minBatch = 10
		maxBatch = 1000
		minFlush = int64(100)   // 100 µs
		maxFlush = int64(10_000) // 10 ms
	)

	controller := NewAdaptiveBatchController(minBatch, maxBatch, minFlush, maxFlush)

	// Low traffic (queue at 5% of capacity=10000): should produce small batch / short flush
	const maxQ = 10_000
	lowCfg := controller.Adjust(50, maxQ, 200)
	if lowCfg.BatchSize != minBatch {
		t.Fatalf("FAIL: low traffic batch=%d, expected minBatch=%d", lowCfg.BatchSize, minBatch)
	}
	if lowCfg.FlushIntervalUs != minFlush {
		t.Fatalf("FAIL: low traffic flush=%dµs, expected %dµs", lowCfg.FlushIntervalUs, minFlush)
	}
	t.Logf("  ✓ Low traffic: batch=%d (min), flush=%dµs (min)", lowCfg.BatchSize, lowCfg.FlushIntervalUs)

	// High traffic: repeatedly call Adjust with 85% queue depth to drive batch up to max
	for i := 0; i < 200; i++ {
		controller.Adjust(8_500, maxQ, 5_000)
	}
	highCfg := controller.Adjust(8_500, maxQ, 5_000)
	if highCfg.BatchSize != maxBatch {
		t.Fatalf("FAIL: high traffic batch=%d after surge, expected maxBatch=%d",
			highCfg.BatchSize, maxBatch)
	}
	if highCfg.FlushIntervalUs < maxFlush/2 {
		t.Fatalf("FAIL: high traffic flush=%dµs too short (expected ≥%dµs)", highCfg.FlushIntervalUs, maxFlush/2)
	}
	t.Logf("  ✓ High traffic: batch=%d (max), flush=%dµs", highCfg.BatchSize, highCfg.FlushIntervalUs)

	// Recovery to low traffic: batch should shrink back toward min
	for i := 0; i < 20; i++ {
		controller.Adjust(100, maxQ, 100)
	}
	recoveryCfg := controller.Adjust(100, maxQ, 100)
	if recoveryCfg.BatchSize >= maxBatch {
		t.Fatalf("FAIL: batch did not decrease after low traffic recovery: %d", recoveryCfg.BatchSize)
	}
	t.Logf("  ✓ Recovery: batch=%d (decreased from max=%d)", recoveryCfg.BatchSize, maxBatch)
	_ = fmt.Sprintf // suppress unused import
}
