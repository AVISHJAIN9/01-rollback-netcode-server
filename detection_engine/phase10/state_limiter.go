package phase10

import (
	"sync/atomic"
)

// ─── State Size Limiter & Memory Pressure Shedding (Feature 57) ──────────────
//
// Two-tier thresholds:
//   SoftLimit  (e.g. 80 %) → SpillRequired: accelerate TTL sweeps / compress
//   HardLimit  (e.g. 95 %) → Throttle:      reject new allocations / backpressure
//
// All accounting is done through a single atomic int64 so that concurrent
// goroutines never need a mutex for the hot Alloc / Free path.

// MemoryStatus describes the current memory pressure level.
type MemoryStatus int

const (
	MemoryOK           MemoryStatus = iota // below soft limit
	MemorySpillRequired                    // ≥ soft limit, < hard limit
	MemoryThrottle                         // ≥ hard limit
)

func (m MemoryStatus) String() string {
	switch m {
	case MemoryOK:
		return "OK"
	case MemorySpillRequired:
		return "SpillRequired"
	case MemoryThrottle:
		return "Throttle"
	default:
		return "Unknown"
	}
}

// StateSizeLimiter tracks aggregate state memory and enforces limits.
type StateSizeLimiter struct {
	currentBytes    atomic.Int64
	maxBytes        int64
	softLimitBytes  int64 // typically 80 % of maxBytes
	hardLimitBytes  int64 // typically 95 % of maxBytes
}

// NewStateSizeLimiter constructs a limiter with the given ceiling.
// softPct and hardPct are fractions in (0,1), e.g. 0.80 and 0.95.
func NewStateSizeLimiter(maxBytes int64, softPct, hardPct float64) *StateSizeLimiter {
	return &StateSizeLimiter{
		maxBytes:       maxBytes,
		softLimitBytes: int64(float64(maxBytes) * softPct),
		hardLimitBytes: int64(float64(maxBytes) * hardPct),
	}
}

// CurrentBytes returns the current tracked allocation in bytes.
func (sl *StateSizeLimiter) CurrentBytes() int64 {
	return sl.currentBytes.Load()
}

// MemoryStatusNow returns the current pressure level without modifying state.
func (sl *StateSizeLimiter) MemoryStatusNow() MemoryStatus {
	cur := sl.currentBytes.Load()
	switch {
	case cur >= sl.hardLimitBytes:
		return MemoryThrottle
	case cur >= sl.softLimitBytes:
		return MemorySpillRequired
	default:
		return MemoryOK
	}
}

// TrackAlloc records a new allocation of bytes and returns the resulting
// MemoryStatus.  Does NOT block the caller — callers must respect Throttle.
func (sl *StateSizeLimiter) TrackAlloc(bytes int64) MemoryStatus {
	sl.currentBytes.Add(bytes)
	return sl.MemoryStatusNow()
}

// TrackFree reduces tracked allocation by bytes (clamped to 0).
func (sl *StateSizeLimiter) TrackFree(bytes int64) {
	for {
		cur := sl.currentBytes.Load()
		next := cur - bytes
		if next < 0 {
			next = 0
		}
		if sl.currentBytes.CompareAndSwap(cur, next) {
			return
		}
	}
}

// CanAllocate returns true when adding bytes would not breach the hard limit.
func (sl *StateSizeLimiter) CanAllocate(bytes int64) bool {
	return sl.currentBytes.Load()+bytes <= sl.hardLimitBytes
}

// EnforceLimits calls evictFn repeatedly until memory is below the soft limit
// or evictFn returns 0 (nothing left to evict).  Returns the total bytes freed.
// evictFn should evict one or more LRU entries and return bytes released.
func (sl *StateSizeLimiter) EnforceLimits(evictFn func() int64) int64 {
	var totalFreed int64
	for sl.currentBytes.Load() >= sl.softLimitBytes {
		freed := evictFn()
		if freed == 0 {
			break
		}
		sl.TrackFree(freed)
		totalFreed += freed
	}
	return totalFreed
}
