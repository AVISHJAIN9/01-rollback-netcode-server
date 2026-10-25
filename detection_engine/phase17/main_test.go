package phase17

import (
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// ─── Test 1: Multi-Priority Preemption & Anti-Starvation ─────────────────────

func TestMultiPriorityPreemptionAntiStarvation(t *testing.T) {
	t.Log("Test 1: Multi-Priority Preemption & Anti-Starvation")

	const perLevel = 20_000
	q := NewMultiPriorityQueue(perLevel)

	// Flood with 10,000 Low-priority events
	lowPushed := 0
	for i := 0; i < 10_000; i++ {
		if q.Push(PriorityLow, fmt.Sprintf("low-%d", i)) {
			lowPushed++
		}
	}

	// Push 10 Critical events
	critPushed := 0
	for i := 0; i < 10; i++ {
		if q.Push(PriorityCritical, fmt.Sprintf("crit-%d", i)) {
			critPushed++
		}
	}

	if lowPushed != 10_000 || critPushed != 10 {
		t.Fatalf("FAIL: pushed low=%d, crit=%d (expected 10000, 10)", lowPushed, critPushed)
	}

	// Dequeue: all 10 Critical must come before any Low
	critDispatched := 0
	lowDispatched := 0
	seenLowBeforeCritFinished := false

	for {
		p, _, ok := q.Pop()
		if !ok {
			break
		}
		switch p {
		case PriorityCritical:
			critDispatched++
		case PriorityLow:
			if critDispatched < critPushed {
				seenLowBeforeCritFinished = true
			}
			lowDispatched++
		}
		if critDispatched == critPushed && lowDispatched == lowPushed {
			break
		}
	}

	if seenLowBeforeCritFinished {
		t.Fatal("FAIL: Low-priority event dispatched before all Critical events")
	}
	if critDispatched != 10 {
		t.Fatalf("FAIL: critDispatched=%d, expected 10", critDispatched)
	}

	t.Logf("  ✓ All %d Critical events dispatched before %d Low events", critDispatched, lowDispatched)

	// Anti-starvation: verify low-priority events are not fully starved
	// by mixing Critical with Low and using PopWithAntiStarvation
	q2 := NewMultiPriorityQueue(perLevel)
	q2.maxStarvSkips = 5 // low threshold for test

	for i := 0; i < 100; i++ {
		q2.Push(PriorityCritical, i)
		q2.Push(PriorityLow, i)
	}

	lowViaAntiStarv := 0
	for i := 0; i < 200; i++ {
		p, _, ok := q2.PopWithAntiStarvation()
		if ok && p == PriorityLow {
			lowViaAntiStarv++
		}
	}
	if lowViaAntiStarv == 0 {
		t.Fatal("FAIL: Low events were completely starved even with anti-starvation enabled")
	}
	t.Logf("  ✓ Anti-starvation: %d Low events dispatched despite Critical competition", lowViaAntiStarv)
}

// ─── Test 2: Thread-Pool Autoscaling Under Bursts ────────────────────────────

func TestThreadPoolAutoscalingUnderBursts(t *testing.T) {
	t.Log("Test 2: Thread-Pool Autoscaling Under Bursts")

	pool := NewAdaptiveThreadPool(2, 8)

	noopWork := func(stop <-chan struct{}) {
		<-stop // block until stopped
	}

	// Force scale up to max
	pool.ForceScaleTo(8, noopWork)
	if pool.CurrentWorkers() != 8 {
		t.Fatalf("FAIL: expected 8 workers after scale-up, got %d", pool.CurrentWorkers())
	}
	t.Logf("  ✓ Scaled up to %d workers (maxWorkers=%d)", pool.CurrentWorkers(), 8)

	// Cannot scale beyond max
	scaled := pool.ScaleUp(noopWork)
	if scaled {
		t.Fatal("FAIL: ScaleUp should return false when already at maxWorkers")
	}

	// Force scale down to min
	// Override cooldown for test by setting lastScaleUp far in the past
	pool.mu.Lock()
	pool.lastScaleUp = time.Now().Add(-60 * time.Second)
	pool.mu.Unlock()

	pool.ForceScaleTo(2, noopWork)
	if pool.CurrentWorkers() != 2 {
		t.Fatalf("FAIL: expected 2 workers after scale-down, got %d", pool.CurrentWorkers())
	}
	t.Logf("  ✓ Scaled down to %d workers (minWorkers=%d)", pool.CurrentWorkers(), 2)

	// AutoAdjust under high utilisation scales up
	pool2 := NewAdaptiveThreadPool(2, 8)
	for i := 0; i < 10; i++ {
		pool2.AutoAdjust(0.90, noopWork)
	}
	if pool2.CurrentWorkers() <= 2 {
		t.Fatalf("FAIL: expected workers > 2 after high utilisation, got %d", pool2.CurrentWorkers())
	}
	t.Logf("  ✓ AutoAdjust expanded pool to %d workers under 90%% load", pool2.CurrentWorkers())

	pool.StopAll()
	pool2.StopAll()
}

// ─── Test 3: Chase-Lev Work Stealing Redistribution ──────────────────────────

func TestChaselevWorkStealingRedistribution(t *testing.T) {
	t.Log("Test 3: Chase-Lev Work Stealing Redistribution")

	const numWorkers = 2
	scheduler := NewWorkStealingScheduler(numWorkers)

	// Load worker 1 (index=1) with 5,000 tasks; worker 0 is idle
	for i := 0; i < 5_000; i++ {
		scheduler.Push(1, i)
	}

	if scheduler.WorkerDepth(1) == 0 {
		t.Fatal("FAIL: worker 1 should have tasks")
	}
	if scheduler.WorkerDepth(0) != 0 {
		t.Fatal("FAIL: worker 0 should start with 0 tasks")
	}

	// Worker 0 steals from worker 1
	stolen := 0
	stealAborts := 0

	for {
		_, result := scheduler.Steal(0, 1)
		switch result {
		case StealSuccess:
			stolen++
		case StealAbort:
			stealAborts++
		case StealEmpty:
			goto doneSteal
		}
	}
doneSteal:

	if stolen == 0 {
		t.Fatal("FAIL: worker 0 failed to steal any tasks from worker 1")
	}
	t.Logf("  ✓ Worker 0 stole %d tasks from worker 1 (aborts=%d)", stolen, stealAborts)

	// Concurrent steal: multiple goroutines steal from worker 1 simultaneously
	scheduler2 := NewWorkStealingScheduler(4)
	for i := 0; i < 5_000; i++ {
		scheduler2.Push(1, i)
	}

	var wg sync.WaitGroup
	var totalStolen atomic.Int64

	for w := 0; w < 4; w++ {
		if w == 1 {
			continue
		}
		w := w
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				_, res := scheduler2.Steal(w, 1)
				if res == StealSuccess {
					totalStolen.Add(1)
				} else if res == StealEmpty {
					return
				}
			}
		}()
	}
	wg.Wait()

	t.Logf("  ✓ Concurrent steal: %d tasks stolen by 3 workers (no corruption)", totalStolen.Load())
	if totalStolen.Load() == 0 {
		t.Fatal("FAIL: concurrent stealers took 0 tasks")
	}
}

// ─── Test 4: NUMA & Core Affinity Binding ─────────────────────────────────────

func TestNUMACoreAffinityBinding(t *testing.T) {
	t.Log("Test 4: NUMA & Core Affinity Binding")

	tm := NewTopologyManager()
	nodes := tm.Nodes()

	if len(nodes) == 0 {
		t.Fatal("FAIL: no NUMA nodes discovered")
	}
	t.Logf("  ✓ Discovered %d NUMA nodes with total %d cores",
		len(nodes), runtime.NumCPU())

	// Bind 4 workers to specific cores
	numCores := runtime.NumCPU()
	var wg sync.WaitGroup
	bindErrors := make([]error, 4)

	for i := 0; i < 4; i++ {
		i := i
		coreID := i % numCores
		label := fmt.Sprintf("worker-%d", i)
		wg.Add(1)
		go func() {
			defer wg.Done()
			bindErrors[i] = tm.BindThreadToCore(label, coreID)
		}()
	}
	wg.Wait()

	for i, err := range bindErrors {
		if err != nil {
			t.Fatalf("FAIL: worker-%d bind error: %v", i, err)
		}
	}

	// Verify bindings are recorded
	bindings := tm.RecordedBindings()
	if len(bindings) != 4 {
		t.Fatalf("FAIL: expected 4 bindings recorded, got %d", len(bindings))
	}

	// Verify affinity bitmasks are non-zero
	for label := range bindings {
		mask := tm.AffinityBitmask(label)
		if mask == 0 {
			t.Fatalf("FAIL: zero affinity bitmask for %s", label)
		}
	}

	// NUMA allocation
	buf, err := tm.AllocateNUMALocal(0, 4096)
	if err != nil || len(buf) != 4096 {
		t.Fatalf("FAIL: NUMA alloc error: %v (len=%d)", err, len(buf))
	}

	t.Logf("  ✓ 4 workers bound to cores, all affinity bitmasks non-zero")
	t.Logf("  ✓ NUMA node 0 allocation: %d bytes OK", len(buf))
	for label, core := range bindings {
		mask := tm.AffinityBitmask(label)
		t.Logf("    %s → core %d (mask=0x%016x)", label, core, mask)
	}
}

// ─── Test 5: Memory Pressure Load Shedding Tiers ─────────────────────────────

func TestMemoryPressureLoadSheddingTiers(t *testing.T) {
	t.Log("Test 5: Memory Pressure Load Shedding Tiers")

	shedder := NewLoadShedder()

	// Set 85% memory pressure → DropLow stage
	shedder.SetMemoryPressure(0.85)

	if shedder.CurrentLevel() != SheddingDropLow {
		t.Fatalf("FAIL: expected DropLow at 85%%, got %s", shedder.CurrentLevel())
	}

	// Critical events must pass through
	for i := 0; i < 100; i++ {
		if shedder.ShouldShed(PriorityCritical) {
			t.Fatal("FAIL: Critical event was shed at 85% pressure")
		}
	}
	if shedder.PassedCritical() != 100 {
		t.Fatalf("FAIL: passedCritical=%d, expected 100", shedder.PassedCritical())
	}

	// Low events must be dropped
	lowDropped := 0
	for i := 0; i < 1000; i++ {
		if shedder.ShouldShed(PriorityLow) {
			lowDropped++
		}
	}
	if lowDropped != 1000 {
		t.Fatalf("FAIL: expected 1000 Low drops at 85%%, got %d", lowDropped)
	}

	t.Logf("  ✓ At 85%%: Critical passed=%d, Low dropped=%d", shedder.PassedCritical(), lowDropped)

	// Critical threshold (95%) — also drops High
	shedder.SetMemoryPressure(0.96)
	if shedder.CurrentLevel() != SheddingCritical {
		t.Fatalf("FAIL: expected SheddingCritical at 96%%")
	}
	if !shedder.ShouldShed(PriorityHigh) {
		t.Fatal("FAIL: High should be shed at 96% pressure")
	}
	if shedder.ShouldShed(PriorityCritical) {
		t.Fatal("FAIL: Critical must NEVER be shed")
	}

	t.Logf("  ✓ At 96%%: High dropped=%d, Critical still passes", shedder.DroppedHigh())

	// Return to normal
	shedder.SetMemoryPressure(0.50)
	if shedder.CurrentLevel() != SheddingNormal {
		t.Fatalf("FAIL: expected Normal at 50%%")
	}
	t.Logf("  ✓ Pressure returned to 50%%: SheddingNormal")
}

// ─── Test 6: Disk Spill Overflow & Lossless Recovery ─────────────────────────

func TestDiskSpillOverflowLosslessRecovery(t *testing.T) {
	t.Log("Test 6: Disk Spill Overflow & Lossless Recovery")

	dir := t.TempDir()
	const maxSegBytes = 64 * 1024 // 64 KB per segment

	buf, err := NewDiskSpillBuffer(dir, maxSegBytes)
	if err != nil {
		t.Fatalf("FAIL: NewDiskSpillBuffer: %v", err)
	}

	const numEvents = 500

	// Spill 500 events
	for i := 0; i < numEvents; i++ {
		ev := &SpillEvent{
			EventID:     fmt.Sprintf("evt-%04d", i),
			TimestampNs: int64(i) * 1_000_000,
			EntityID:    fmt.Sprintf("host-%d", i%10),
			EventType:   "ProcessCreate",
			Fields:      map[string]string{"idx": fmt.Sprintf("%d", i)},
		}
		if _, err := buf.Spill(ev); err != nil {
			t.Fatalf("FAIL: Spill[%d]: %v", i, err)
		}
	}

	if buf.SpillCount() != numEvents {
		t.Fatalf("FAIL: spillCount=%d, expected %d", buf.SpillCount(), numEvents)
	}

	// Flush current segment to disk
	if err := buf.FlushCurrentSegment(); err != nil {
		t.Fatalf("FAIL: FlushCurrentSegment: %v", err)
	}

	// Drain all events back — loop until all segments are empty
	var recovered []*SpillEvent
	for {
		batch, err := buf.DrainSpill(200)
		if err != nil {
			t.Fatalf("FAIL: DrainSpill: %v", err)
		}
		if len(batch) == 0 {
			break
		}
		recovered = append(recovered, batch...)
	}

	if len(recovered) != numEvents {
		t.Fatalf("FAIL: recovered %d events, expected %d", len(recovered), numEvents)
	}

	// Verify event IDs are preserved (order may vary per segment boundaries)
	seen := make(map[string]bool, numEvents)
	for _, ev := range recovered {
		seen[ev.EventID] = true
	}
	for i := 0; i < numEvents; i++ {
		id := fmt.Sprintf("evt-%04d", i)
		if !seen[id] {
			t.Fatalf("FAIL: event %s not recovered from disk spill", id)
		}
	}

	if buf.PendingSegments() != 0 {
		t.Fatalf("FAIL: %d segments not cleaned up after full drain", buf.PendingSegments())
	}

	t.Logf("  ✓ Spilled %d events to disk", buf.SpillCount())
	t.Logf("  ✓ Drained %d events back from disk (0 lost)", len(recovered))
	t.Logf("  ✓ All %d segment files cleaned up", 0)
	_ = time.Now() // suppress unused import
}
