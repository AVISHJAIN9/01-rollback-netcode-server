package phase15

import (
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// ─── Test 1: Rule Subsumption & Conflict Detection ────────────────────────────

func TestRuleSubsumptionConflictDetection(t *testing.T) {
	t.Log("Test 1: Rule Subsumption & Conflict Detection")

	detector := NewRuleConflictDetector()

	ls := LogSource{Category: "process_creation", Product: "windows"}

	// Existing rule: Image == "cmd.exe", severity High
	existing := &CompiledRule{
		RuleID:   "rule-cmd-exec-001",
		Title:    "CMD Execution Detected",
		Severity: SevHigh,
		LogSource: ls,
		Selections: []RuleSearchSelection{{
			Name: "selection",
			Predicates: []FieldPredicate{
				{Field: "Image", Operator: OpEquals, Value: "cmd.exe"},
			},
		}},
	}

	// New rule: identical criteria, conflicting severity (Medium vs High)
	newRule := &CompiledRule{
		RuleID:   "rule-cmd-exec-002",
		Title:    "CMD Execution — Duplicate",
		Severity: SevMedium, // severity clash!
		LogSource: ls,
		Selections: []RuleSearchSelection{{
			Name: "selection",
			Predicates: []FieldPredicate{
				{Field: "Image", Operator: OpEquals, Value: "cmd.exe"},
			},
		}},
	}

	warnings := detector.CheckConflicts(newRule, []*CompiledRule{existing})

	if len(warnings) == 0 {
		t.Fatal("FAIL: expected at least one conflict warning, got none")
	}

	found := false
	for _, w := range warnings {
		if w.Kind == ConflictSeverityClash || w.Kind == ConflictDuplicate {
			found = true
			t.Logf("  ✓ Conflict detected: %s", w)
		}
	}
	if !found {
		t.Fatalf("FAIL: expected SeverityClash or Duplicate warning, got: %+v", warnings)
	}

	// Test numeric interval containment
	broaderRule := &CompiledRule{
		RuleID:   "rule-count-broad",
		Severity: SevHigh,
		LogSource: ls,
		Selections: []RuleSearchSelection{{
			Name: "selection",
			Predicates: []FieldPredicate{
				{Field: "event_count", Operator: OpGreaterThan, NumValue: 10},
			},
		}},
	}
	narrowerRule := &CompiledRule{
		RuleID:   "rule-count-narrow",
		Severity: SevHigh,
		LogSource: ls,
		Selections: []RuleSearchSelection{{
			Name: "selection",
			Predicates: []FieldPredicate{
				{Field: "event_count", Operator: OpGreaterThan, NumValue: 20},
			},
		}},
	}

	intervalWarnings := detector.CheckConflicts(narrowerRule, []*CompiledRule{broaderRule})
	if len(intervalWarnings) == 0 {
		t.Fatal("FAIL: expected interval containment warning, got none")
	}
	for _, w := range intervalWarnings {
		t.Logf("  ✓ Interval conflict: %s", w)
	}

	// Different logsource — no conflicts expected
	diffLS := &CompiledRule{
		RuleID:   "rule-linux-001",
		Severity: SevHigh,
		LogSource: LogSource{Category: "process_creation", Product: "linux"},
		Selections: []RuleSearchSelection{{
			Name: "selection",
			Predicates: []FieldPredicate{
				{Field: "Image", Operator: OpEquals, Value: "cmd.exe"},
			},
		}},
	}
	noWarnings := detector.CheckConflicts(diffLS, []*CompiledRule{existing})
	if len(noWarnings) != 0 {
		t.Fatalf("FAIL: expected no conflicts for different logsource, got %d", len(noWarnings))
	}
	t.Logf("  ✓ No conflict for different logsource (linux vs windows)")
}

// ─── Test 2: Configuration Schema Validation ─────────────────────────────────

func TestConfigSchemaValidation(t *testing.T) {
	t.Log("Test 2: Configuration Schema Validation")

	v := NewConfigValidator()

	// Valid config — must pass
	good := DefaultEngineConfig()
	if err := v.Validate(&good); err != nil {
		t.Fatalf("FAIL: default config should be valid, got: %v", err)
	}
	t.Logf("  ✓ Default config validates OK")

	// Negative ring buffer size
	bad1 := DefaultEngineConfig()
	bad1.Ingestion.RingBufferSize = -1
	if err := v.Validate(&bad1); err == nil {
		t.Fatal("FAIL: negative ring_buffer_size should be rejected")
	} else {
		t.Logf("  ✓ Negative ring_buffer_size rejected: %v", err)
	}

	// Watermark delay ≤ 0
	bad2 := DefaultEngineConfig()
	bad2.Ingestion.WatermarkDelayMs = 0
	if err := v.Validate(&bad2); err == nil {
		t.Fatal("FAIL: watermark_delay_ms=0 should be rejected")
	} else {
		t.Logf("  ✓ watermark_delay_ms=0 rejected: %v", err)
	}

	// MaxMemoryMB below 512
	bad3 := DefaultEngineConfig()
	bad3.State.MaxMemoryMB = 256
	if err := v.Validate(&bad3); err == nil {
		t.Fatal("FAIL: max_memory_mb=256 should be rejected")
	} else {
		t.Logf("  ✓ max_memory_mb=256 rejected: %v", err)
	}

	// Non-power-of-two ring buffer
	bad4 := DefaultEngineConfig()
	bad4.Ingestion.RingBufferSize = 3000
	if err := v.Validate(&bad4); err == nil {
		t.Fatal("FAIL: non-power-of-two ring_buffer_size should be rejected")
	} else {
		t.Logf("  ✓ Non-power-of-two ring_buffer_size rejected: %v", err)
	}

	// MinWorkers > MaxWorkers
	bad5 := DefaultEngineConfig()
	bad5.Ingestion.MinWorkers = 32
	bad5.Ingestion.MaxWorkers = 4
	if err := v.Validate(&bad5); err == nil {
		t.Fatal("FAIL: min_workers > max_workers should be rejected")
	} else {
		t.Logf("  ✓ min_workers > max_workers rejected: %v", err)
	}
}

// ─── Test 3: Environment Variable Cascading ───────────────────────────────────

func TestEnvironmentVariableCascading(t *testing.T) {
	t.Log("Test 3: Environment Variable Cascading")

	// Set environment variable to override workers
	envKey := "STREAM_ENGINE_INGESTION_MAX_WORKERS"
	os.Setenv(envKey, "16")
	defer os.Unsetenv(envKey)

	// Base file override sets workers to 8
	baseFile := ConfigOverride{
		"ingestion.max_workers": "8",
		"ingestion.min_workers": "2",
	}

	resolver := NewEnvironmentConfigResolver(baseFile, "STREAM_ENGINE")
	cfg, err := resolver.Resolve(nil)
	if err != nil {
		t.Fatalf("FAIL: resolve error: %v", err)
	}

	// Env var (16) must override base file (8)
	if cfg.Ingestion.MaxWorkers != 16 {
		t.Fatalf("FAIL: expected MaxWorkers=16 from env var, got %d", cfg.Ingestion.MaxWorkers)
	}
	t.Logf("  ✓ Env var STREAM_ENGINE_INGESTION_MAX_WORKERS=16 overrides base file value 8")

	// CLI override is highest precedence — overrides even env var
	cliOverrides := ConfigOverride{"ingestion.max_workers": "32"}
	cfg2, err := resolver.Resolve(cliOverrides)
	if err != nil {
		t.Fatalf("FAIL: resolve with CLI override error: %v", err)
	}
	if cfg2.Ingestion.MaxWorkers != 32 {
		t.Fatalf("FAIL: expected MaxWorkers=32 from CLI override, got %d", cfg2.Ingestion.MaxWorkers)
	}
	t.Logf("  ✓ CLI override max_workers=32 takes highest precedence over env var")

	// Default value used when no override is set
	cfg3, err := NewEnvironmentConfigResolver(nil, "NOPREFIX_X").Resolve(nil)
	if err != nil {
		t.Fatalf("FAIL: resolve defaults: %v", err)
	}
	if cfg3.Ingestion.MaxWorkers != 16 {
		t.Fatalf("FAIL: expected default MaxWorkers=16, got %d", cfg3.Ingestion.MaxWorkers)
	}
	t.Logf("  ✓ Default MaxWorkers=16 used when no override present")
}

// ─── Test 4: Lock-Free Dynamic Threshold Mutation ─────────────────────────────

func TestLockFreeDynamicThresholdMutation(t *testing.T) {
	t.Log("Test 4: Lock-Free Dynamic Threshold Mutation")

	mgr := NewThresholdManager()

	// Rule evaluation code binds to the atomic handle at registration time
	handle := mgr.RegisterThreshold("rule-count-threshold-001", 10)

	// Simulate detection loop checking threshold
	eventCount := uint64(7) // below initial threshold of 10
	if eventCount >= handle.Load() {
		t.Fatal("FAIL: count 7 should be below threshold 10")
	}

	// Dynamically lower threshold to 5 — no restarts, no recompilation
	if err := mgr.UpdateThreshold("rule-count-threshold-001", 5); err != nil {
		t.Fatalf("FAIL: UpdateThreshold error: %v", err)
	}

	// Same handle — rule loop immediately sees new threshold
	if eventCount < handle.Load() {
		t.Fatalf("FAIL: count %d should now be >= updated threshold %d", eventCount, handle.Load())
	}

	// Verify the handle returned by GetThreshold matches
	val, ok := mgr.GetThreshold("rule-count-threshold-001")
	if !ok || val != 5 {
		t.Fatalf("FAIL: GetThreshold returned (%d, %v), expected (5, true)", val, ok)
	}

	// Concurrent threshold reads are safe
	const goroutines = 32
	const readsPerGoroutine = 10_000
	var wg sync.WaitGroup
	var readErrors atomic.Int64

	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < readsPerGoroutine; j++ {
				v := handle.Load()
				if v != 5 {
					readErrors.Add(1)
				}
			}
		}()
	}
	wg.Wait()

	if readErrors.Load() != 0 {
		t.Fatalf("FAIL: %d concurrent reads saw wrong threshold value", readErrors.Load())
	}

	// Unknown rule — UpdateThreshold must return error
	if err := mgr.UpdateThreshold("nonexistent-rule", 99); err == nil {
		t.Fatal("FAIL: expected error for unknown rule, got nil")
	}

	t.Logf("  ✓ Threshold updated 10→5 in-place, handle reflects new value immediately")
	t.Logf("  ✓ %d concurrent reads across %d goroutines: 0 errors", goroutines*readsPerGoroutine, goroutines)
}

// ─── Test 5: Cryptographic Audit Ledger Verification ──────────────────────────

func TestCryptographicAuditLedger(t *testing.T) {
	t.Log("Test 5: Cryptographic Audit Ledger Verification")

	ledger := NewRuleAuditLedger()

	// Append 5 rule mutations
	mutations := []struct {
		actor  string
		action AuditAction
		ruleID string
		diff   string
	}{
		{"admin@corp", AuditActionActivate, "rule-001", "+added process_creation rule"},
		{"admin@corp", AuditActionEdit, "rule-001", "~severity: low→high"},
		{"analyst@corp", AuditActionActivate, "rule-002", "+added network rule"},
		{"admin@corp", AuditActionThresholdUpdate, "rule-001", "~threshold: 10→5"},
		{"admin@corp", AuditActionDeactivate, "rule-002", "-removed network rule"},
	}

	for _, m := range mutations {
		ledger.AppendRecord(m.actor, m.action, m.ruleID, m.diff)
	}

	if ledger.Len() != 5 {
		t.Fatalf("FAIL: expected 5 entries, got %d", ledger.Len())
	}

	// Clean chain must verify
	if err := ledger.VerifyLedger(); err != nil {
		t.Fatalf("FAIL: clean ledger failed verification: %v", err)
	}
	t.Logf("  ✓ 5-entry ledger verifies clean")

	// Tamper: mutate actor in entry[2]
	entries := ledger.EntriesRaw()
	original := entries[2].Actor
	entries[2].Actor = "attacker_injected"

	if err := ledger.VerifyLedger(); err == nil {
		t.Fatal("FAIL: tampered ledger (actor mutation) passed verification — must fail")
	} else {
		t.Logf("  ✓ Actor tamper at seq=2 detected: %v", err)
	}

	// Restore and verify again
	entries[2].Actor = original
	if err := ledger.VerifyLedger(); err != nil {
		t.Fatalf("FAIL: restored ledger still fails: %v", err)
	}

	// Tamper: flip a byte in entry[3].EntryHash
	entries[3].EntryHash[0] ^= 0xFF
	if err := ledger.VerifyLedger(); err == nil {
		t.Fatal("FAIL: flipped entry_hash passed verification — must fail")
	} else {
		t.Logf("  ✓ EntryHash flip at seq=3 detected: %v", err)
	}
}

// ─── Test 6: High-Concurrency Config Cache Reads ─────────────────────────────

func TestHighConcurrencyConfigCacheReads(t *testing.T) {
	t.Log("Test 6: High-Concurrency Config Cache Reads")

	cfg := DefaultEngineConfig()
	cache := NewConfigurationCache(cfg)

	const goroutines = 16
	const readsPerGoroutine = 100_000

	var wg sync.WaitGroup
	var readErrors atomic.Int64

	// 16 reader goroutines
	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < readsPerGoroutine; j++ {
				val, ok := cache.GetInt("ingestion.max_workers")
				if !ok || val <= 0 {
					readErrors.Add(1)
				}
			}
		}()
	}

	// Background writer updating a key
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < 1000; i++ {
			cache.Set("state.max_memory_mb", IntConfigVal(2048+i))
			time.Sleep(10 * time.Microsecond)
		}
	}()

	wg.Wait()

	if readErrors.Load() != 0 {
		t.Fatalf("FAIL: %d read errors detected under concurrent access", readErrors.Load())
	}

	// Verify Set persists
	cache.Set("ingestion.watermark_delay_ms", IntConfigVal(999))
	val, ok := cache.GetInt("ingestion.watermark_delay_ms")
	if !ok || val != 999 {
		t.Fatalf("FAIL: Set/Get round-trip: got (%d, %v), expected (999, true)", val, ok)
	}

	// Verify Invalidate removes key
	cache.Invalidate("ingestion.watermark_delay_ms")
	_, ok2 := cache.Get("ingestion.watermark_delay_ms")
	if ok2 {
		t.Fatal("FAIL: key should be absent after Invalidate")
	}

	// Benchmark latency: 1M reads, report average
	start := time.Now()
	const benchReads = 1_000_000
	for i := 0; i < benchReads; i++ {
		cache.GetInt("ingestion.max_workers")
	}
	elapsed := time.Since(start)
	nsPerRead := float64(elapsed.Nanoseconds()) / float64(benchReads)

	t.Logf("  ✓ %d concurrent reads across %d goroutines: 0 errors", goroutines*readsPerGoroutine, goroutines)
	t.Logf("  ✓ Set/Get round-trip verified, Invalidate verified")
	t.Logf("  ✓ Average read latency: %.1f ns/read (target ≤15ns)", nsPerRead)

	_ = fmt.Sprintf // suppress unused import
}
