package phase11

import (
	"fmt"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"
)

// ─── Test 1: Alert Deduplication & Burst Suppression ─────────────────────────

func TestAlertDeduplicationBurstSuppression(t *testing.T) {
	t.Log("Test 1: Alert Deduplication & Burst Suppression")

	dedup := NewAlertDeduplicator()
	factory := NewDetectionMatchFactory()

	rule := &CompiledRule{
		RuleID:    "rule-lateral-001",
		RuleTitle: "Suspicious Lateral Movement",
		Severity:  SeverityHigh,
		Tags:      []string{"lateral_movement"},
	}
	event := &CanonicalEvent{
		EventID:     "evt-001",
		TimestampNs: time.Now().UnixNano(),
		EntityID:    "host-42",
		EventType:   "ProcessCreate",
	}

	cfg := TTLConfig{Mode: TTLFixed, WindowNs: 5 * int64(time.Second)}

	emitted := 0
	suppressed := 0
	var lastSupInfo *SuppressionInfo

	for i := 0; i < 100; i++ {
		m, err := factory.CreateMatch(rule, event, map[string]ValueVariant{
			"entity": StrVariant("host-42"),
		})
		if err != nil {
			t.Fatalf("CreateMatch error: %v", err)
		}
		action, info := dedup.CheckOrSuppress(m, cfg)
		switch action {
		case AlertActionEmit:
			emitted++
		case AlertActionSuppress:
			suppressed++
			lastSupInfo = info
		}
	}

	if emitted != 1 {
		t.Fatalf("FAIL: expected 1 emitted alert, got %d", emitted)
	}
	if suppressed != 99 {
		t.Fatalf("FAIL: expected 99 suppressed, got %d", suppressed)
	}
	if lastSupInfo == nil || lastSupInfo.Count < 99 {
		t.Fatalf("FAIL: suppression info incomplete: %+v", lastSupInfo)
	}

	t.Logf("  ✓ emitted=1, suppressed=99, occurrence_count=%d", lastSupInfo.Count)
}

// ─── Test 2: Sliding vs Fixed Deduplication TTL ───────────────────────────────

func TestSlidingVsFixedTTL(t *testing.T) {
	t.Log("Test 2: Sliding vs Fixed Deduplication TTL")

	wheel := NewSlidingDeduplicationWheel()
	now := int64(1_000_000_000) // t=1s in ns
	ttl := int64(10_000_000_000) // 10 seconds

	fpFixed := uint64(0xDEAD_BEEF_0001)
	fpSliding := uint64(0xDEAD_BEEF_0002)

	// Register both at t=1s
	wheel.Register(fpFixed, now, ttl)
	wheel.Register(fpSliding, now, ttl)

	// At t=6s, send a hit — only sliding should extend
	t6 := int64(6_000_000_000)
	wheel.UpdateTTL(fpFixed, t6, ttl, false)   // fixed: no change
	wheel.UpdateTTL(fpSliding, t6, ttl, true)  // sliding: extend to t6+10s = 16s

	// Advance to t=12s (just past the original fixed TTL of 1+10=11s)
	t12 := int64(12_000_000_000)
	fired := wheel.Advance(t12)

	firedSet := make(map[uint64]bool)
	for _, fp := range fired {
		firedSet[fp] = true
	}

	// Fixed TTL must have fired (expires t=11s ≤ t=12s)
	if !firedSet[fpFixed] {
		t.Fatal("FAIL: fixed-TTL fingerprint did not expire at t=12s")
	}

	// Sliding TTL must NOT have fired (expires t=16s > t=12s)
	if firedSet[fpSliding] {
		t.Fatal("FAIL: sliding-TTL fingerprint expired too early (expected t=16s, tested at t=12s)")
	}

	// Advance to t=17s — now sliding must also fire
	t17 := int64(17_000_000_000)
	fired2 := wheel.Advance(t17)
	firedSet2 := make(map[uint64]bool)
	for _, fp := range fired2 {
		firedSet2[fp] = true
	}
	if !firedSet2[fpSliding] {
		t.Fatal("FAIL: sliding-TTL fingerprint did not expire at t=17s")
	}

	t.Logf("  ✓ Fixed TTL expired at t=12s; Sliding TTL extended to t=16s, expired at t=17s")
}

// ─── Test 3: Idempotent State Transitions on Replay ──────────────────────────

func TestIdempotentStateTransitions(t *testing.T) {
	t.Log("Test 3: Idempotent State Transitions on Replay")

	barrier := NewIdempotentStateBarrier()
	const partition = PartitionID(0)

	var counter atomic.Int64

	// Feed offsets [1,2,3,2,3,4] — offsets 2 and 3 are replayed
	offsets := []uint64{1, 2, 3, 2, 3, 4}
	results := make([]CommitResult, len(offsets))

	for i, offset := range offsets {
		results[i] = barrier.TryCommit(partition, offset, func() {
			counter.Add(1)
		})
	}

	// Exactly offsets 1,2,3,4 should be Applied (4 distinct)
	applied := 0
	ignored := 0
	for _, r := range results {
		if r == CommitApplied {
			applied++
		} else {
			ignored++
		}
	}

	if applied != 4 {
		t.Fatalf("FAIL: expected 4 applied commits, got %d (results: %v)", applied, results)
	}
	if ignored != 2 {
		t.Fatalf("FAIL: expected 2 ignored replays, got %d", ignored)
	}
	if counter.Load() != 4 {
		t.Fatalf("FAIL: counter=%d, expected 4 (no double-counting)", counter.Load())
	}

	// Verify results[3] and results[4] (offsets 2,3 replay) are DuplicateIgnored
	if results[3] != CommitDuplicateIgnored {
		t.Fatalf("FAIL: offset 2 replay should be DuplicateIgnored, got %v", results[3])
	}
	if results[4] != CommitDuplicateIgnored {
		t.Fatalf("FAIL: offset 3 replay should be DuplicateIgnored, got %v", results[4])
	}

	t.Logf("  ✓ Applied=4, DuplicateIgnored=2, counter=%d (no double-counting)", counter.Load())
}

// ─── Test 4: State Telemetry Real-Time Tracking ───────────────────────────────

func TestStateTelemetryTracking(t *testing.T) {
	t.Log("Test 4: State Telemetry Real-Time Tracking")

	reg := NewStateMetricsRegistry()
	const keyCount = 10_000
	const bytesPerKey = 512

	// Allocate 10,000 keys
	reg.RecordAlloc(keyCount, keyCount*bytesPerKey)

	if reg.ActiveKeys.Load() != keyCount {
		t.Fatalf("FAIL: ActiveKeys=%d, expected %d", reg.ActiveKeys.Load(), keyCount)
	}
	if reg.AllocatedBytes.Load() != keyCount*bytesPerKey {
		t.Fatalf("FAIL: AllocatedBytes=%d, expected %d", reg.AllocatedBytes.Load(), keyCount*bytesPerKey)
	}

	// Evict 3,000 via TTL and 2,000 via LRU
	reg.RecordEviction(true, 3_000, 3_000*bytesPerKey)
	reg.RecordEviction(false, 2_000, 2_000*bytesPerKey)

	remaining := keyCount - 3_000 - 2_000 // 5,000
	if reg.ActiveKeys.Load() != int64(remaining) {
		t.Fatalf("FAIL: ActiveKeys=%d after evictions, expected %d", reg.ActiveKeys.Load(), remaining)
	}
	if reg.TTLEvictions.Load() != 3_000 {
		t.Fatalf("FAIL: TTLEvictions=%d, expected 3000", reg.TTLEvictions.Load())
	}
	if reg.LRUEvictions.Load() != 2_000 {
		t.Fatalf("FAIL: LRUEvictions=%d, expected 2000", reg.LRUEvictions.Load())
	}

	// Record reads: 4000 hits, 1000 misses
	for i := 0; i < 4_000; i++ {
		reg.RecordRead(true)
	}
	for i := 0; i < 1_000; i++ {
		reg.RecordRead(false)
	}

	expectedRatio := 0.80
	if ratio := reg.HitRatio(); ratio < expectedRatio-0.01 || ratio > expectedRatio+0.01 {
		t.Fatalf("FAIL: HitRatio=%.4f, expected ~%.2f", ratio, expectedRatio)
	}

	// Verify Prometheus output contains all metric names
	prom := reg.FormatPrometheus()
	required := []string{
		"state_active_keys",
		"state_allocated_bytes",
		"state_ttl_evictions_total",
		"state_lru_evictions_total",
		"state_read_hits_total",
		"state_read_misses_total",
		"state_cache_hit_ratio",
	}
	for _, name := range required {
		if !contains(prom, name) {
			t.Fatalf("FAIL: Prometheus output missing metric: %s", name)
		}
	}

	t.Logf("  ✓ active_keys=%d, ttl_evictions=%d, lru_evictions=%d, hit_ratio=%.2f",
		reg.ActiveKeys.Load(), reg.TTLEvictions.Load(), reg.LRUEvictions.Load(), reg.HitRatio())
}

// ─── Test 5: Crash Recovery Integrity & Checksum Verification ────────────────

func TestCrashRecoveryChecksumVerification(t *testing.T) {
	t.Log("Test 5: Crash Recovery Integrity & Checksum Verification")

	dir := t.TempDir()
	h := NewStateRecoveryHarness()

	// Write 1,000 mutations
	for i := 0; i < 1_000; i++ {
		h.Write(fmt.Sprintf("key-%04d", i), []byte(fmt.Sprintf("val-%08d", i*7+3)))
	}

	if h.Len() != 1_000 {
		t.Fatalf("FAIL: expected 1000 keys, got %d", h.Len())
	}

	// Checkpoint
	ckpt := filepath.Join(dir, "checkpoint-1")
	meta, err := h.Checkpoint(ckpt)
	if err != nil {
		t.Fatalf("FAIL: Checkpoint error: %v", err)
	}
	if meta.KeyCount != 1_000 {
		t.Fatalf("FAIL: meta.KeyCount=%d, expected 1000", meta.KeyCount)
	}

	// Simulate crash: create fresh harness and restore
	h2 := NewStateRecoveryHarness()
	restored, err := h2.Restore(ckpt)
	if err != nil {
		t.Fatalf("FAIL: Restore error: %v", err)
	}
	if restored.KeyCount != 1_000 {
		t.Fatalf("FAIL: restored.KeyCount=%d", restored.KeyCount)
	}
	if h2.Len() != 1_000 {
		t.Fatalf("FAIL: restored store has %d keys, expected 1000", h2.Len())
	}

	// Verify a sample of keys
	for i := 0; i < 100; i++ {
		key := fmt.Sprintf("key-%04d", i*10)
		expected := fmt.Sprintf("val-%08d", i*10*7+3)
		got, ok := h2.Read(key)
		if !ok || string(got) != expected {
			t.Fatalf("FAIL: key %s: expected %s, got %s (ok=%v)", key, expected, string(got), ok)
		}
	}

	// Corrupt the snapshot CRC and verify rejection
	dataPath := filepath.Join(ckpt, "state.bin")
	data, err := os.ReadFile(dataPath)
	if err != nil {
		t.Fatalf("FAIL: read state.bin: %v", err)
	}
	if len(data) > 20 {
		data[10] ^= 0xFF
		data[11] ^= 0xFF
	}
	if err := os.WriteFile(dataPath, data, 0o644); err != nil {
		t.Fatalf("FAIL: write corrupted state.bin: %v", err)
	}

	h3 := NewStateRecoveryHarness()
	_, err = h3.Restore(ckpt)
	if err == nil {
		t.Fatal("FAIL: expected error restoring corrupted snapshot, got nil")
	}

	t.Logf("  ✓ 1000 keys checkpointed and restored correctly")
	t.Logf("  ✓ Corrupted snapshot rejected: %v", err)
}

// ─── Test 6: Detection Match Envelope Lineage Fidelity ───────────────────────

func TestDetectionMatchLineageFidelity(t *testing.T) {
	t.Log("Test 6: Detection Match Envelope Lineage Fidelity")

	factory := NewDetectionMatchFactory()

	rule := &CompiledRule{
		RuleID:    "sigma-ps-download-001",
		RuleTitle: "PowerShell Download Cradle Detected",
		Severity:  SeverityCritical,
		Tags:      []string{"execution", "T1059.001"},
	}

	event := &CanonicalEvent{
		EventID:     "evt-canonical-001",
		TimestampNs: time.Now().UnixNano(),
		EntityID:    "host-dc01",
		EventType:   "ProcessCreate",
		RawAttributes: map[string]ValueVariant{
			"process.name":         StrVariant("powershell.exe"),
			"process.command_line": StrVariant("powershell -enc BASE64PAYLOAD"),
			"parent.name":          StrVariant("winword.exe"),
		},
	}

	ctx := map[string]ValueVariant{
		"matched_field": StrVariant("process.command_line"),
		"tenant_id":     StrVariant("tenant-acme"),
	}

	match, err := factory.CreateMatch(rule, event, ctx)
	if err != nil {
		t.Fatalf("FAIL: CreateMatch error: %v", err)
	}

	// Validate UUIDv7
	if !IsValidUUIDv7(match.MatchID) {
		t.Fatalf("FAIL: invalid UUIDv7: %s", match.MatchID)
	}

	// Validate severity
	if match.Severity != SeverityCritical {
		t.Fatalf("FAIL: severity=%v, expected Critical", match.Severity)
	}

	// Validate suppression fingerprint is non-zero
	if match.SuppressionFingerprint == 0 {
		t.Fatal("FAIL: SuppressionFingerprint is zero")
	}

	// Validate triggering events carry source provenance
	if len(match.TriggeringEvents) != 1 {
		t.Fatalf("FAIL: expected 1 triggering event, got %d", len(match.TriggeringEvents))
	}
	if match.TriggeringEvents[0].EventID != event.EventID {
		t.Fatalf("FAIL: triggering event ID mismatch: %s != %s",
			match.TriggeringEvents[0].EventID, event.EventID)
	}
	if match.TriggeringEvents[0].EntityID != event.EntityID {
		t.Fatalf("FAIL: triggering event EntityID mismatch")
	}

	// Validate context attributes contain rule tags and matched context
	if _, ok := match.ContextAttributes["tag:execution"]; !ok {
		t.Fatal("FAIL: context missing tag:execution")
	}
	if _, ok := match.ContextAttributes["tag:T1059.001"]; !ok {
		t.Fatal("FAIL: context missing tag:T1059.001")
	}
	if _, ok := match.ContextAttributes["matched_field"]; !ok {
		t.Fatal("FAIL: context missing matched_field")
	}

	// Validate entity_id is preserved
	if match.EntityID != "host-dc01" {
		t.Fatalf("FAIL: EntityID=%s, expected host-dc01", match.EntityID)
	}

	// Determinism: same inputs → same fingerprint
	match2, _ := factory.CreateMatch(rule, event, ctx)
	if match.SuppressionFingerprint != match2.SuppressionFingerprint {
		t.Fatalf("FAIL: fingerprint non-deterministic: %d != %d",
			match.SuppressionFingerprint, match2.SuppressionFingerprint)
	}

	t.Logf("  ✓ MatchID=%s (valid UUIDv7)", match.MatchID)
	t.Logf("  ✓ Severity=%s", match.Severity)
	t.Logf("  ✓ SuppressionFingerprint=%016x", match.SuppressionFingerprint)
	t.Logf("  ✓ TriggeringEvents=%d (EventID=%s)", len(match.TriggeringEvents), match.TriggeringEvents[0].EventID)
	t.Logf("  ✓ ContextAttributes keys=%d", len(match.ContextAttributes))
	t.Logf("  ✓ Fingerprint deterministic across two calls")
}

// ─── helpers ─────────────────────────────────────────────────────────────────

func contains(s, sub string) bool {
	return len(s) >= len(sub) && (s == sub || len(s) > 0 && containsStr(s, sub))
}

func containsStr(s, sub string) bool {
	for i := 0; i <= len(s)-len(sub); i++ {
		if s[i:i+len(sub)] == sub {
			return true
		}
	}
	return false
}
