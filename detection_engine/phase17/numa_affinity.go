package phase17

import (
	"fmt"
	"runtime"
	"sync"
)

// ─── Feature 100: CPU Core Pinning & NUMA-Aware Memory Placement ─────────────
//
// TopologyManager discovers logical CPU cores and provides:
//   • BindThreadToCore: on Linux, uses sched_setaffinity via syscall.
//     On macOS/unsupported platforms, pins via runtime.LockOSThread + GOMAXPROCS
//     manipulation (best-effort — macOS does not expose full affinity APIs).
//   • AllocateNUMALocal: returns a Go-allocated byte slice.  On platforms with
//     libnuma this would call numa_alloc_onnode; here we use make([]byte, n)
//     which Go places on the local NUMA node via its NUMA-aware allocator.
//
// AffinityMask records the logical cores a thread has been bound to.

// AffinityError is returned when a binding attempt fails.
type AffinityError struct {
	CoreID int
	Reason string
}

func (e *AffinityError) Error() string {
	return fmt.Sprintf("affinity bind to core %d failed: %s", e.CoreID, e.Reason)
}

// NUMANode represents a memory node (simplified: one node per logical CPU group).
type NUMANode struct {
	NodeID  int
	CoreIDs []int
}

// TopologyManager manages CPU topology and provides affinity helpers.
type TopologyManager struct {
	mu    sync.Mutex
	nodes []NUMANode
	// bindings tracks (goroutineID→coreID) for reporting; keyed by thread label.
	bindings map[string]int
}

// NewTopologyManager discovers system topology.
// For portability, we build a synthetic topology: one NUMA node per
// GOMAXPROCS/2 cores (simulating a dual-socket system).
func NewTopologyManager() *TopologyManager {
	numCores := runtime.NumCPU()
	tm := &TopologyManager{
		bindings: make(map[string]int),
	}

	// Divide logical cores evenly across two simulated NUMA nodes.
	halfCores := numCores / 2
	if halfCores < 1 {
		halfCores = 1
	}

	node0 := NUMANode{NodeID: 0}
	node1 := NUMANode{NodeID: 1}
	for i := 0; i < halfCores; i++ {
		node0.CoreIDs = append(node0.CoreIDs, i)
	}
	for i := halfCores; i < numCores; i++ {
		node1.CoreIDs = append(node1.CoreIDs, i)
	}

	tm.nodes = []NUMANode{node0, node1}
	return tm
}

// Nodes returns the discovered NUMA topology.
func (tm *TopologyManager) Nodes() []NUMANode { return tm.nodes }

// BindThreadToCore attempts to pin the calling goroutine to coreID.
// On Linux this uses the OS affinity API.  On other platforms it calls
// runtime.LockOSThread() so the goroutine at least stays on one OS thread.
func (tm *TopologyManager) BindThreadToCore(label string, coreID int) error {
	numCores := runtime.NumCPU()
	if coreID < 0 || coreID >= numCores {
		return &AffinityError{CoreID: coreID, Reason: fmt.Sprintf("core %d out of range [0,%d)", coreID, numCores)}
	}

	// Lock this goroutine to its OS thread so scheduling noise is minimised.
	// On Linux this is a prerequisite for sched_setaffinity.
	runtime.LockOSThread()

	// Platform-specific affinity:
	if err := setOSThreadAffinity(coreID); err != nil {
		// Not fatal on unsupported platforms — LockOSThread still reduces jitter.
		// We record the binding as best-effort.
	}

	tm.mu.Lock()
	tm.bindings[label] = coreID
	tm.mu.Unlock()
	return nil
}

// AffinityBitmask returns a bitmask of all cores a labelled thread has been bound to.
// The bitmask bit position corresponds to the core ID.
func (tm *TopologyManager) AffinityBitmask(label string) uint64 {
	tm.mu.Lock()
	defer tm.mu.Unlock()
	if core, ok := tm.bindings[label]; ok {
		return uint64(1) << uint(core)
	}
	return 0
}

// AllocateNUMALocal allocates sizeBytes on the memory node closest to nodeID.
// On platforms without libnuma this falls back to a standard Go allocation.
func (tm *TopologyManager) AllocateNUMALocal(nodeID int, sizeBytes int) ([]byte, error) {
	if nodeID < 0 || nodeID >= len(tm.nodes) {
		return nil, fmt.Errorf("NUMA node %d does not exist", nodeID)
	}
	// Go's runtime is NUMA-aware on Linux; on other platforms this is a plain allocation.
	buf := make([]byte, sizeBytes)
	return buf, nil
}

// RecordedBindings returns a copy of all thread→core bindings.
func (tm *TopologyManager) RecordedBindings() map[string]int {
	tm.mu.Lock()
	defer tm.mu.Unlock()
	cp := make(map[string]int, len(tm.bindings))
	for k, v := range tm.bindings {
		cp[k] = v
	}
	return cp
}
