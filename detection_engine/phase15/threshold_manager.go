package phase15

import (
	"fmt"
	"sync"
	"sync/atomic"
)

// ─── Feature 88: Dynamic Atomic Threshold Manager ────────────────────────────
//
// ThresholdManager decouples detection threshold values from the rule AST by
// storing them as *atomic.Uint64 handles.  Rule evaluation code holds a direct
// pointer to the atomic so that the next event evaluation always sees the latest
// value — zero restarts, zero recompilation.

// AtomicThreshold wraps an atomic uint64 for lock-free threshold reads and writes.
type AtomicThreshold struct {
	value atomic.Uint64
}

// Load returns the current threshold value.
func (t *AtomicThreshold) Load() uint64 { return t.value.Load() }

// Store atomically updates the threshold to newVal.
func (t *AtomicThreshold) Store(newVal uint64) { t.value.Store(newVal) }

// ThresholdManager maintains a registry of rule thresholds keyed by rule ID.
type ThresholdManager struct {
	mu      sync.RWMutex
	handles map[string]*AtomicThreshold
}

// NewThresholdManager constructs an empty manager.
func NewThresholdManager() *ThresholdManager {
	return &ThresholdManager{
		handles: make(map[string]*AtomicThreshold),
	}
}

// RegisterThreshold creates a new atomic threshold for ruleID with initialVal.
// Returns the *AtomicThreshold so the rule's evaluation closure can bind directly.
// If a threshold for ruleID already exists, the existing handle is returned.
func (m *ThresholdManager) RegisterThreshold(ruleID string, initialVal uint64) *AtomicThreshold {
	m.mu.Lock()
	defer m.mu.Unlock()

	if existing, ok := m.handles[ruleID]; ok {
		return existing
	}

	t := &AtomicThreshold{}
	t.Store(initialVal)
	m.handles[ruleID] = t
	return t
}

// UpdateThreshold atomically sets the threshold for ruleID to newVal.
// Returns an error if the rule ID is not registered.
func (m *ThresholdManager) UpdateThreshold(ruleID string, newVal uint64) error {
	m.mu.RLock()
	t, ok := m.handles[ruleID]
	m.mu.RUnlock()

	if !ok {
		return fmt.Errorf("threshold for rule %q not registered", ruleID)
	}

	t.Store(newVal)
	return nil
}

// GetThreshold returns the current threshold value for ruleID.
// Returns (0, false) if the rule ID is not registered.
func (m *ThresholdManager) GetThreshold(ruleID string) (uint64, bool) {
	m.mu.RLock()
	t, ok := m.handles[ruleID]
	m.mu.RUnlock()

	if !ok {
		return 0, false
	}
	return t.Load(), true
}

// Handle returns the raw *AtomicThreshold for ruleID, or nil if not registered.
// Rule ASTs bind directly to this pointer for zero-overhead evaluation.
func (m *ThresholdManager) Handle(ruleID string) *AtomicThreshold {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.handles[ruleID]
}
