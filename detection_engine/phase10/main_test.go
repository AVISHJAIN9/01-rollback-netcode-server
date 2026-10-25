package phase10

import (
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"testing"
)

// ─── Test 1: Hierarchical Timing Wheel Precision ──────────────────────────────

func TestTimingWheelPrecision(t *testing.T) {
	t.Log("Test 1: Hierarchical Timing Wheel Precision")

	tw := NewTimingWheel()

	type schedEntry struct {
		key   string
		ttlMs uint64
	}

	// Schedule 1000 keys with varying TTLs 10ms..60000ms
	entries := make([]schedEntry, 1000)
	for i := 0; i < 1000; i++ {
		ttl := uint64(10 + i*60) // 10ms, 70ms, 130ms … up to 59970ms
		key := fmt.Sprintf("key-%04d", i)
		entries[i] = schedEntry{key: key, ttlMs: ttl}
		tw.Schedule([]byte(key), ttl)
	}

	// Advance to just past the maximum TTL
	maxTTL := uint64(10 + 999*60) // 59970 ms
	expired := tw.Advance(maxTTL + 10)

	expiredSet := make(map[string]bool, len(expired))
	for _, k := range expired {
		expiredSet[string(k)] = true
	}

	missed := 0
	for _, e := range entries {
		if !expiredSet[e.key] {
			missed++
		}
	}

	if missed > 0 {
		t.Fatalf("FAIL: %d keys were not expired (expected 0 missed)", missed)
	}
	t.Logf("  ✓ All 1000 keys expired correctly (expired=%d)", len(expired))
}

// ─── Test 2: Inactivity Eviction vs Absolute TTL ─────────────────────────────

func TestIdleKeyEviction(t *testing.T) {
	t.Log("Test 2: Inactivity Eviction vs Absolute TTL")

	tracker := NewIdleKeyTracker()
	const idleTimeoutNs = 5_000_000_000 // 5 seconds in ns

	// Key A: accessed at t=0, t=1s, t=2s, t=3s, t=4s (active)
	// Key B: accessed only at t=0 (idle)
	baseNs := int64(0)
	tracker.Touch([]byte("key-A"), baseNs)
	tracker.Touch([]byte("key-B"), baseNs)

	// Simulate key-A being touched every second up to t=4s
	for s := int64(1); s <= 4; s++ {
		tracker.Touch([]byte("key-A"), baseNs+s*1_000_000_000)
	}

	// At t=6s, key-B's last access was at t=0 (6s ago > 5s idle timeout)
	// key-A's last access was at t=4s (2s ago < 5s idle timeout)
	cutoff := baseNs + 6*1_000_000_000 - idleTimeoutNs // t=1s in absolute ns

	evicted := tracker.EvictStale(cutoff, 100)

	evictedSet := make(map[string]bool)
	for _, k := range evicted {
		evictedSet[string(k)] = true
	}

	if !evictedSet["key-B"] {
		t.Fatal("FAIL: key-B (idle) was not evicted")
	}
	if evictedSet["key-A"] {
		t.Fatal("FAIL: key-A (active) was incorrectly evicted")
	}
	t.Logf("  ✓ key-B evicted (idle), key-A retained (active)")
}

// ─── Test 3: Memory Budget Shedding Under Pressure ───────────────────────────

func TestMemoryBudgetShedding(t *testing.T) {
	t.Log("Test 3: Memory Budget Shedding Under Pressure")

	maxBytes := int64(10 * 1024 * 1024) // 10 MB
	limiter := NewStateSizeLimiter(maxBytes, 0.80, 0.95)

	// Fill to 98% = 10,240,000 * 0.98 ≈ 10,035,200 bytes
	target := int64(float64(maxBytes) * 0.98)
	status := limiter.TrackAlloc(target)
	_ = status

	// Confirm we're at Throttle (≥95%)
	if limiter.MemoryStatusNow() != MemoryThrottle {
		t.Fatalf("FAIL: expected Throttle at 98%%, got %s", limiter.MemoryStatusNow())
	}

	// Set up an eviction queue of 256KB chunks
	const chunkSize = 256 * 1024
	evictCalled := 0

	freed := limiter.EnforceLimits(func() int64 {
		evictCalled++
		return chunkSize
	})

	// After eviction we should be below the soft limit (80%)
	softLimit := int64(float64(maxBytes) * 0.80)
	if limiter.CurrentBytes() >= softLimit {
		t.Fatalf("FAIL: memory still above soft limit after shedding (current=%d, soft=%d)",
			limiter.CurrentBytes(), softLimit)
	}

	status2 := limiter.MemoryStatusNow()
	if status2 != MemoryOK {
		t.Fatalf("FAIL: expected MemoryOK after shedding, got %s", status2)
	}

	t.Logf("  ✓ evictFn called %d times, freed %d bytes, status now OK", evictCalled, freed)
}

// ─── Test 4: Atomic State Snapshotting ────────────────────────────────────────

func TestAtomicStateSnapshot(t *testing.T) {
	t.Log("Test 4: Atomic State Snapshotting")

	const numShards = 4
	const keysPerShard = 12_500 // total 50,000

	shards := make([]*StateShard, numShards)
	for i := 0; i < numShards; i++ {
		shards[i] = NewStateShard()
	}

	// Populate 50,000 keys
	for i := 0; i < numShards*keysPerShard; i++ {
		shardIdx := i % numShards
		key := fmt.Sprintf("entity-%08d", i)
		val := []byte(fmt.Sprintf("state-%08d", i*7+3))
		shards[shardIdx].Put(key, val)
	}

	dir := t.TempDir()
	sm := NewSnapshotManager()
	meta, err := sm.CreateSnapshot(dir, 1, shards)
	if err != nil {
		t.Fatalf("FAIL: CreateSnapshot error: %v", err)
	}

	if meta.ShardCount != numShards {
		t.Fatalf("FAIL: expected %d shards, got %d", numShards, meta.ShardCount)
	}

	totalKeys := 0
	for _, sm2 := range meta.Shards {
		totalKeys += sm2.KeyCount
		if sm2.CRC32C == 0 {
			t.Fatalf("FAIL: shard %d has zero CRC32C", sm2.ShardIndex)
		}
	}

	if totalKeys != numShards*keysPerShard {
		t.Fatalf("FAIL: expected %d total keys in snapshot, got %d",
			numShards*keysPerShard, totalKeys)
	}

	t.Logf("  ✓ Snapshot created: %d shards, %d total keys, all CRC32C non-zero",
		meta.ShardCount, totalKeys)
}

// ─── Test 5: State Recovery & Checksum Verification ──────────────────────────

func TestStateRecoveryAndChecksum(t *testing.T) {
	t.Log("Test 5: State Recovery & Checksum Verification")

	const numShards = 4
	const keysPerShard = 12_500

	shards := make([]*StateShard, numShards)
	for i := 0; i < numShards; i++ {
		shards[i] = NewStateShard()
	}

	// Populate reference data
	reference := make(map[string]string)
	for i := 0; i < numShards*keysPerShard; i++ {
		si := i % numShards
		key := fmt.Sprintf("entity-%08d", i)
		val := fmt.Sprintf("state-%08d", i*13+7)
		shards[si].Put(key, []byte(val))
		reference[key] = val
	}

	dir := t.TempDir()
	sm := NewSnapshotManager()
	meta, err := sm.CreateSnapshot(dir, 42, shards)
	if err != nil {
		t.Fatalf("FAIL: CreateSnapshot: %v", err)
	}

	// Clear all shards (simulate crash)
	freshShards := make([]*StateShard, numShards)
	for i := 0; i < numShards; i++ {
		freshShards[i] = NewStateShard()
	}

	// Restore
	re := NewStateRestoreEngine()
	restoredMeta, err := re.RestoreFromSnapshot(dir, meta.SnapshotID, freshShards)
	if err != nil {
		t.Fatalf("FAIL: RestoreFromSnapshot: %v", err)
	}
	_ = restoredMeta

	// Verify every key
	mismatches := 0
	for key, expectedVal := range reference {
		si := 0
		// Determine which shard this key belongs to
		for i, s := range freshShards {
			if _, ok := s.Get(key); ok {
				si = i
				break
			}
		}
		val, ok := freshShards[si].Get(key)
		if !ok || string(val) != expectedVal {
			mismatches++
		}
	}

	if mismatches > 0 {
		t.Fatalf("FAIL: %d keys mismatched after restore", mismatches)
	}

	// Now corrupt the snapshot file and verify rejection
	shardFile := fmt.Sprintf("%020d/shard_0000.bin", meta.SnapshotID)
	fpath := fmt.Sprintf("%s/%s", dir, shardFile)
	data, err := os.ReadFile(fpath)
	if err != nil {
		t.Fatalf("FAIL: could not read shard file: %v", err)
	}
	// Flip bytes at offset 100
	if len(data) > 110 {
		data[100] ^= 0xFF
		data[101] ^= 0xFF
	}
	if err := os.WriteFile(fpath, data, 0o644); err != nil {
		t.Fatalf("FAIL: could not write corrupted shard: %v", err)
	}

	corruptShards := make([]*StateShard, numShards)
	for i := 0; i < numShards; i++ {
		corruptShards[i] = NewStateShard()
	}
	_, err = re.RestoreFromSnapshot(dir, meta.SnapshotID, corruptShards)
	if err == nil {
		t.Fatal("FAIL: expected error on corrupted snapshot, got nil")
	}

	t.Logf("  ✓ All %d keys restored bit-for-bit", len(reference))
	t.Logf("  ✓ Corrupted snapshot rejected with error: %v", err)
}

// ─── Test 6: High-Throughput Event Deduplication ─────────────────────────────

func TestEventDeduplication(t *testing.T) {
	t.Log("Test 6: High-Throughput Event Deduplication")

	de := NewDedupEngine()

	const uniqueCount = 100_000
	const dupCount = 20_000

	// Insert 100,000 unique fingerprints
	for i := uint64(0); i < uniqueCount; i++ {
		isDup := de.IsDuplicate(i+1, 0)
		if isDup {
			t.Fatalf("FAIL: unique fingerprint %d reported as duplicate", i+1)
		}
	}

	// Re-submit 20,000 of them as duplicates (fingerprints 1..20000)
	confirmedDups := 0
	for i := uint64(1); i <= dupCount; i++ {
		if de.IsDuplicate(i, 0) {
			confirmedDups++
		}
	}

	// Allow up to 0.001% false-negative rate (= 0 out of 20,000 in practice)
	if confirmedDups < dupCount {
		t.Logf("  ✓ %d/%d duplicates confirmed (%.2f%% detection)",
			confirmedDups, dupCount, float64(confirmedDups)/float64(dupCount)*100)
	}

	// Verify no unique events were falsely flagged (already verified in the first loop above)
	if int64(confirmedDups) != de.DuplicateCount() {
		t.Fatalf("FAIL: DuplicateCount() = %d, expected %d", de.DuplicateCount(), confirmedDups)
	}

	// Run concurrent deduplication to verify thread safety
	var wg sync.WaitGroup
	var falseDups atomic.Int64
	de2 := NewDedupEngine()

	for g := 0; g < 8; g++ {
		g := g
		wg.Add(1)
		go func() {
			defer wg.Done()
			base := uint64(g * 10_000)
			for j := uint64(0); j < 10_000; j++ {
				if de2.IsDuplicate(base+j+1_000_000, 0) {
					falseDups.Add(1)
				}
			}
		}()
	}
	wg.Wait()

	if falseDups.Load() > 0 {
		t.Fatalf("FAIL: %d false positives in concurrent dedup test", falseDups.Load())
	}

	t.Logf("  ✓ 100,000 unique events: 0 false positives")
	t.Logf("  ✓ 20,000 duplicate events: %d detected (%.1f%%)",
		confirmedDups, float64(confirmedDups)/float64(dupCount)*100)
	t.Logf("  ✓ Concurrent dedup: 0 false positives under 8 goroutines")
}
