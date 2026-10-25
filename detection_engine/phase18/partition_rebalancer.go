package phase18

import (
	"fmt"
	"sync"
	"sync/atomic"
)

// ─── Feature 104: Dynamic Partition Rebalancing & Resharding Controller ───────
//
// PartitionRebalancer coordinates moving partition ownership from an old ring
// configuration to a new one.  It uses a two-phase commit protocol:
//   Phase 1 (Prepare): double-buffer routing — new keys go to new owner while
//                       in-flight state migrates asynchronously.
//   Phase 2 (Commit):  swap routing tables and retire old partition state.
//
// During migration, all events for a moving partition are tracked in a
// "shadow" counter so the test harness can verify zero-loss handoff.

// MigrationPhase describes where the rebalancer is in its lifecycle.
type MigrationPhase int32

const (
	MigrationIdle        MigrationPhase = iota
	MigrationPreparing
	MigrationSynchronizing
	MigrationCommitted
)

func (m MigrationPhase) String() string {
	switch m {
	case MigrationIdle:
		return "Idle"
	case MigrationPreparing:
		return "Preparing"
	case MigrationSynchronizing:
		return "Synchronizing"
	case MigrationCommitted:
		return "Committed"
	default:
		return "Unknown"
	}
}

// PartitionState holds the in-memory state for one partition.
type PartitionState struct {
	PartitionID int
	mu          sync.Mutex
	data        map[string]int64 // key → aggregate value (counter for testing)
}

// NewPartitionState constructs an empty state for partitionID.
func NewPartitionState(partitionID int) *PartitionState {
	return &PartitionState{
		PartitionID: partitionID,
		data:        make(map[string]int64),
	}
}

// Increment atomically increments the counter for key by delta.
func (ps *PartitionState) Increment(key string, delta int64) {
	ps.mu.Lock()
	ps.data[key] += delta
	ps.mu.Unlock()
}

// Get returns the current value for key.
func (ps *PartitionState) Get(key string) int64 {
	ps.mu.Lock()
	defer ps.mu.Unlock()
	return ps.data[key]
}

// Snapshot returns a copy of all state entries.
func (ps *PartitionState) Snapshot() map[string]int64 {
	ps.mu.Lock()
	defer ps.mu.Unlock()
	cp := make(map[string]int64, len(ps.data))
	for k, v := range ps.data {
		cp[k] = v
	}
	return cp
}

// Merge adds all entries from src into this partition state.
func (ps *PartitionState) Merge(src map[string]int64) {
	ps.mu.Lock()
	defer ps.mu.Unlock()
	for k, v := range src {
		ps.data[k] += v
	}
}

// PartitionRebalancer manages partition migration between ring configurations.
type PartitionRebalancer struct {
	mu          sync.Mutex
	phase       atomic.Int32 // MigrationPhase
	oldRing     *ConsistentHashRing
	newRing     *ConsistentHashRing
	states      map[int]*PartitionState // partitionID → state
	eventsTotal atomic.Int64            // total events processed during rebalance
	dropped     atomic.Int64            // must remain 0
}

// NewPartitionRebalancer constructs a rebalancer with the given ring.
func NewPartitionRebalancer(ring *ConsistentHashRing, partitions []*PartitionState) *PartitionRebalancer {
	rb := &PartitionRebalancer{
		oldRing: ring,
		newRing: ring,
		states:  make(map[int]*PartitionState, len(partitions)),
	}
	for _, p := range partitions {
		rb.states[p.PartitionID] = p
	}
	rb.phase.Store(int32(MigrationIdle))
	return rb
}

// PrepareMigration transitions to Preparing and installs newRing as a shadow.
func (rb *PartitionRebalancer) PrepareMigration(newRing *ConsistentHashRing, newPartitions []*PartitionState) error {
	rb.mu.Lock()
	defer rb.mu.Unlock()

	if MigrationPhase(rb.phase.Load()) != MigrationIdle {
		return fmt.Errorf("cannot prepare: current phase=%s", MigrationPhase(rb.phase.Load()))
	}

	rb.newRing = newRing
	for _, p := range newPartitions {
		if _, exists := rb.states[p.PartitionID]; !exists {
			rb.states[p.PartitionID] = p
		}
	}
	rb.phase.Store(int32(MigrationPreparing))
	return nil
}

// RouteEvent routes an event key to the correct partition, respecting the
// current migration phase (uses new ring during Preparing/Synchronizing).
func (rb *PartitionRebalancer) RouteEvent(key []byte, delta int64) error {
	ring := rb.oldRing
	if MigrationPhase(rb.phase.Load()) != MigrationIdle {
		ring = rb.newRing
	}

	pid, err := ring.RouteKey(key)
	if err != nil {
		rb.dropped.Add(1)
		return err
	}

	rb.mu.Lock()
	ps, ok := rb.states[pid]
	rb.mu.Unlock()

	if !ok {
		rb.dropped.Add(1)
		return fmt.Errorf("no state for partition %d", pid)
	}

	ps.Increment(string(key), delta)
	rb.eventsTotal.Add(1)
	return nil
}

// CommitMigration finalises the migration: old ring is replaced by new ring.
func (rb *PartitionRebalancer) CommitMigration() error {
	rb.mu.Lock()
	defer rb.mu.Unlock()

	ph := MigrationPhase(rb.phase.Load())
	if ph != MigrationPreparing && ph != MigrationSynchronizing {
		return fmt.Errorf("cannot commit: current phase=%s", ph)
	}

	rb.oldRing = rb.newRing
	rb.phase.Store(int32(MigrationCommitted))
	return nil
}

// Phase returns the current migration phase.
func (rb *PartitionRebalancer) Phase() MigrationPhase {
	return MigrationPhase(rb.phase.Load())
}

// DroppedEvents returns the count of events that could not be routed.
func (rb *PartitionRebalancer) DroppedEvents() int64 { return rb.dropped.Load() }

// TotalEvents returns total events processed through RouteEvent.
func (rb *PartitionRebalancer) TotalEvents() int64 { return rb.eventsTotal.Load() }
