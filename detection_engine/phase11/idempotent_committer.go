package phase11

import "sync"

// ─── Feature 63: Exactly-Once-Oriented State Transitions ─────────────────────
//
// IdempotentStateBarrier tracks, per partition, the highest stream offset that
// has been successfully applied.  Attempts to apply an offset ≤ the recorded
// high-water mark are treated as replays and silently skipped.

// PartitionID is a type alias for a Kafka / stream partition identifier.
type PartitionID = int32

// CommitResult describes the outcome of a TryCommit call.
type CommitResult int

const (
	CommitApplied         CommitResult = iota // mutation was applied
	CommitDuplicateIgnored                    // replay — skipped
)

func (c CommitResult) String() string {
	if c == CommitApplied {
		return "Applied"
	}
	return "DuplicateIgnored"
}

// IdempotentStateBarrier enforces exactly-once semantics across replayed offsets.
type IdempotentStateBarrier struct {
	mu          sync.RWMutex
	lastApplied map[PartitionID]uint64
}

// NewIdempotentStateBarrier constructs an empty barrier.
func NewIdempotentStateBarrier() *IdempotentStateBarrier {
	return &IdempotentStateBarrier{
		lastApplied: make(map[PartitionID]uint64),
	}
}

// TryCommit attempts to apply mutationFn for the given (partition, offset) pair.
//
//   • If offset ≤ lastApplied[partition]: returns CommitDuplicateIgnored (no mutation).
//   • Otherwise: executes mutationFn, records offset, returns CommitApplied.
func (b *IdempotentStateBarrier) TryCommit(
	partition PartitionID,
	offset uint64,
	mutationFn func(),
) CommitResult {
	b.mu.Lock()
	defer b.mu.Unlock()

	if last, ok := b.lastApplied[partition]; ok && offset <= last {
		return CommitDuplicateIgnored
	}

	mutationFn()
	b.lastApplied[partition] = offset
	return CommitApplied
}

// LastAppliedOffset returns the highest applied offset for the given partition.
// Returns (0, false) if the partition has no recorded offset.
func (b *IdempotentStateBarrier) LastAppliedOffset(partition PartitionID) (uint64, bool) {
	b.mu.RLock()
	defer b.mu.RUnlock()
	v, ok := b.lastApplied[partition]
	return v, ok
}

// Reset clears all partition state (useful for tests or partition rebalancing).
func (b *IdempotentStateBarrier) Reset() {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.lastApplied = make(map[PartitionID]uint64)
}
