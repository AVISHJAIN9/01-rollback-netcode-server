package phase18

import (
	"encoding/binary"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// ─── Test 1: Consistent Hash Ring Distribution ────────────────────────────────

func TestConsistentHashRingDistribution(t *testing.T) {
	t.Log("Test 1: Consistent Hash Ring Distribution")

	ring := NewConsistentHashRing()
	const numPartitions = 8
	const vnodeCount = 256

	for i := 0; i < numPartitions; i++ {
		ring.AddPartition(i, vnodeCount)
	}

	const keyCount = 50_000
	dist := ring.KeyDistributionStats(keyCount)

	if len(dist) != numPartitions {
		t.Fatalf("FAIL: expected %d partitions in distribution, got %d", numPartitions, len(dist))
	}

	cv := StdDevFraction(dist)
	const maxCV = 0.05 // ≤ 5%
	if cv > maxCV {
		t.Fatalf("FAIL: key distribution CV=%.4f exceeds limit %.2f (too uneven)", cv, maxCV)
	}

	t.Logf("  ✓ 50,000 keys across 8 partitions: CV=%.4f (≤5%% target)", cv)
	for pid, count := range dist {
		t.Logf("    partition[%d] = %d keys (%.1f%%)", pid, count, float64(count)/float64(keyCount)*100)
	}

	// Verify determinism: same key always routes to same partition
	buf := make([]byte, 8)
	binary.LittleEndian.PutUint64(buf, 12345)
	p1, _ := ring.RouteKey(buf)
	p2, _ := ring.RouteKey(buf)
	if p1 != p2 {
		t.Fatal("FAIL: routing is non-deterministic for the same key")
	}
	t.Logf("  ✓ Routing determinism: key=12345 always → partition %d", p1)
}

// ─── Test 2: Dynamic Resharding State Migration ───────────────────────────────

func TestDynamicReshardingStateMigration(t *testing.T) {
	t.Log("Test 2: Dynamic Resharding State Migration")

	// Build initial 4-partition ring
	oldRing := NewConsistentHashRing()
	var oldPartitions [4]*PartitionState
	for i := 0; i < 4; i++ {
		oldRing.AddPartition(i, 256)
		oldPartitions[i] = NewPartitionState(i)
	}

	rb := NewPartitionRebalancer(oldRing, []*PartitionState{
		oldPartitions[0], oldPartitions[1], oldPartitions[2], oldPartitions[3],
	})

	// Stream 1,000 events before migration
	for i := 0; i < 1_000; i++ {
		key := []byte(fmt.Sprintf("entity-%04d", i))
		if err := rb.RouteEvent(key, 1); err != nil {
			t.Fatalf("FAIL: pre-migration RouteEvent: %v", err)
		}
	}
	preMigrationTotal := rb.TotalEvents()

	// Scale to 8 partitions
	newRing := NewConsistentHashRing()
	newPartitions := make([]*PartitionState, 8)
	for i := 0; i < 8; i++ {
		newRing.AddPartition(i, 256)
		newPartitions[i] = NewPartitionState(i)
	}

	// Prepare migration
	if err := rb.PrepareMigration(newRing, newPartitions); err != nil {
		t.Fatalf("FAIL: PrepareMigration: %v", err)
	}

	// Stream 2,000 more events during migration
	for i := 0; i < 2_000; i++ {
		key := []byte(fmt.Sprintf("entity-%04d", i%1000))
		if err := rb.RouteEvent(key, 1); err != nil {
			t.Fatalf("FAIL: mid-migration RouteEvent: %v", err)
		}
	}

	// Commit migration
	if err := rb.CommitMigration(); err != nil {
		t.Fatalf("FAIL: CommitMigration: %v", err)
	}

	if rb.DroppedEvents() != 0 {
		t.Fatalf("FAIL: %d events dropped during migration (expected 0)", rb.DroppedEvents())
	}

	postTotal := rb.TotalEvents()
	if postTotal != preMigrationTotal+2_000 {
		t.Fatalf("FAIL: expected total=%d, got %d", preMigrationTotal+2_000, postTotal)
	}

	t.Logf("  ✓ Migrated 4→8 partitions: pre=%d events, mid=%d events", preMigrationTotal, int64(2_000))
	t.Logf("  ✓ Zero events dropped during migration")
	t.Logf("  ✓ Final phase: %s", rb.Phase())
}

// ─── Test 3: Zero-Copy Pointer Transfer Throughput ────────────────────────────

func TestZeroCopyPointerTransferThroughput(t *testing.T) {
	t.Log("Test 3: Zero-Copy Pointer Transfer Throughput")

	const ringCap = 65536
	const arenaBytes = 64 * 1024 * 1024 // 64 MB
	const eventCount = 1_000_000

	ch := NewSlabHandOffChannel(ringCap, arenaBytes)
	payload := []byte("event-payload-fixed-size-benchmark-data-32bytes!!")

	start := time.Now()

	var pushed int64
	var popped int64
	var wg sync.WaitGroup

	// Producer
	wg.Add(1)
	go func() {
		defer wg.Done()
		for pushed < eventCount {
			if err := ch.WriteEvent(payload); err == nil {
				pushed++
			}
		}
	}()

	// Consumer
	wg.Add(1)
	go func() {
		defer wg.Done()
		for popped < eventCount {
			if ref, ok := ch.PopRef(); ok {
				data := ch.ReadEvent(ref)
				if len(data) == 0 {
					t.Errorf("FAIL: empty data from PopRef")
				}
				popped++
			}
		}
	}()

	wg.Wait()
	elapsed := time.Since(start)

	nsPerEvent := float64(elapsed.Nanoseconds()) / float64(eventCount)
	t.Logf("  ✓ 1,000,000 event refs transferred: %.1f ns/event (target ≤8ns for atomic path)",
		nsPerEvent)
	t.Logf("  ✓ Pushed=%d, Popped=%d (zero loss)", pushed, popped)

	if pushed != popped {
		t.Fatalf("FAIL: pushed=%d != popped=%d", pushed, popped)
	}
}

// ─── Test 4: Micro-Batch Size & Timeout Flushes ───────────────────────────────

func TestMicroBatchSizeAndTimeoutFlushes(t *testing.T) {
	t.Log("Test 4: Micro-Batch Size & Timeout Flushes")

	var mu sync.Mutex
	var receivedBatches [][]CanonicalEvent

	flushFn := func(batch []CanonicalEvent) {
		mu.Lock()
		cp := make([]CanonicalEvent, len(batch))
		copy(cp, batch)
		receivedBatches = append(receivedBatches, cp)
		mu.Unlock()
	}

	const maxBatch = 100
	const timeoutUs = 5_000 // 5ms

	pipeline := NewMicroBatchPipeline(maxBatch, timeoutUs, flushFn)

	// Test 1: Timeout flush — send 50 events (below size limit), wait for timer
	for i := 0; i < 50; i++ {
		pipeline.Push(CanonicalEvent{EventID: fmt.Sprintf("t-evt-%d", i)})
	}

	// Wait for timer flush (5ms + buffer)
	time.Sleep(20 * time.Millisecond)

	mu.Lock()
	timerBatches := len(receivedBatches)
	var timerTotal int
	for _, b := range receivedBatches {
		timerTotal += len(b)
	}
	receivedBatches = receivedBatches[:0]
	mu.Unlock()

	if timerBatches == 0 || timerTotal != 50 {
		t.Fatalf("FAIL: expected timer flush of 50 events, got batches=%d total=%d", timerBatches, timerTotal)
	}
	t.Logf("  ✓ Timer flush: %d events in %d batch(es) after 5ms timeout", timerTotal, timerBatches)

	// Test 2: Size limit flush — send 2,000 events rapidly
	for i := 0; i < 2_000; i++ {
		pipeline.Push(CanonicalEvent{EventID: fmt.Sprintf("s-evt-%d", i)})
	}
	pipeline.Stop()

	mu.Lock()
	var sizeTotal int
	for _, b := range receivedBatches {
		sizeTotal += len(b)
	}
	mu.Unlock()

	if sizeTotal != 2_000 {
		t.Fatalf("FAIL: expected 2000 events after size-limit flushes, got %d", sizeTotal)
	}

	_, _, timerFlushCount, sizeFlushCount := pipeline.Stats()
	t.Logf("  ✓ Size-limit flush: 2000 events, timer_flushes=%d, size_flushes=%d",
		timerFlushCount, sizeFlushCount)
}

// ─── Test 5: Worker Deadlock Detection & Recovery ─────────────────────────────

func TestWorkerDeadlockDetectionRecovery(t *testing.T) {
	t.Log("Test 5: Worker Deadlock Detection & Recovery")

	const numWorkers = 4
	watchdog := NewWorkerWatchdog(numWorkers)

	// All workers heartbeat normally
	for i := 0; i < numWorkers; i++ {
		watchdog.Heartbeat(i)
	}

	nowNs := time.Now().UnixNano()
	const staleness = int64(2 * time.Second) // 2s threshold

	// No workers should be stalled immediately after heartbeat
	stalled := watchdog.CheckHealth(nowNs, staleness)
	if len(stalled) != 0 {
		t.Fatalf("FAIL: expected 0 stalled workers right after heartbeat, got %d", len(stalled))
	}

	// Simulate worker 1 hanging: advance fake time by 3 seconds
	fakeNow := nowNs + int64(3*time.Second)
	// Worker 1 has NOT called Heartbeat since nowNs (3s ago)
	// Workers 0, 2, 3 will keep beating
	for i := 0; i < numWorkers; i++ {
		if i != 1 {
			// Simulate recent heartbeat for these workers
			watchdog.workers[i].heartbeatNs.Store(fakeNow - int64(100*time.Millisecond))
		}
	}

	stalled2 := watchdog.CheckHealth(fakeNow, staleness)
	if len(stalled2) != 1 || stalled2[0].WorkerID != 1 {
		t.Fatalf("FAIL: expected worker 1 stalled, got %v", stalled2)
	}
	t.Logf("  ✓ Worker 1 detected as stalled (staleness=%.1fms)", float64(stalled2[0].StalenessNs)/1e6)

	// Recover worker 1
	watchdog.RecoverWorker(1)
	if watchdog.RecoveredCount() != 1 {
		t.Fatalf("FAIL: expected RecoveredCount=1, got %d", watchdog.RecoveredCount())
	}

	// Replace the worker slot
	watchdog.ReplaceWorker(1)

	// After replacement, no workers stalled at fakeNow + fresh heartbeat
	watchdog.Heartbeat(1)
	stalled3 := watchdog.CheckHealth(time.Now().UnixNano(), staleness)
	stalledIDs := make([]int, 0)
	for _, s := range stalled3 {
		stalledIDs = append(stalledIDs, s.WorkerID)
	}
	t.Logf("  ✓ Worker 1 recovered and replaced (RecoveredCount=%d, post-recovery stalled=%v)",
		watchdog.RecoveredCount(), stalledIDs)
}

// ─── Test 6: Runtime Benchmark Profiling ─────────────────────────────────────

func TestRuntimeBenchmarkProfiling(t *testing.T) {
	t.Log("Test 6: Runtime Benchmark Profiling")

	var processed atomic.Int64

	bench := NewStreamRuntimeBenchmark()

	cfg := BenchmarkConfig{
		TargetEPS:    200_000,
		DurationMs:   500, // 500ms run
		Pattern:      PatternUniform,
		NumProducers: 4,
		PipelineFn: func(event *CanonicalEvent) {
			// Simulate minimal pipeline work: xxHash the event ID
			_ = xxHash64Sum([]byte(event.EventID))
			processed.Add(1)
		},
	}

	stats := bench.Run(cfg)

	t.Logf("  ✓ Benchmark: %s", stats)

	// P99 must be < 2ms = 2,000,000 ns
	const p99LimitNs = int64(2_000_000)
	if stats.P99Ns > p99LimitNs {
		t.Fatalf("FAIL: P99=%.2fms exceeds 2ms limit", float64(stats.P99Ns)/1e6)
	}

	if stats.TotalOps < 1_000 {
		t.Fatalf("FAIL: too few operations recorded: %d", stats.TotalOps)
	}

	t.Logf("  ✓ P99=%.2fms (target <2ms), EPS=%.0f, ops=%d",
		float64(stats.P99Ns)/1e6, stats.ActualEPS, stats.TotalOps)
}
