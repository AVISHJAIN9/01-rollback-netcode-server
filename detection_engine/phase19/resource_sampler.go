package phase19

import (
	"bufio"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"time"
)

// ─── Feature 112: System Resource Gauges (CPU, RSS, Arena Memory, Page Faults) ─
//
// SystemResourceMonitor polls /proc/self/status (on Linux) or uses
// runtime.MemStats (on all platforms) to populate atomic gauges.
// Background goroutine refreshes every pollIntervalMs milliseconds.

// ResourceSnapshot holds the most recent reading from the OS.
type ResourceSnapshot struct {
	RSSBytes       uint64 // resident set size in bytes
	VirtualBytes   uint64 // virtual address space in bytes
	MinorFaults    uint64 // minor page faults since process start
	MajorFaults    uint64 // major page faults since process start
	HeapAllocBytes uint64 // Go heap allocated bytes (from runtime.MemStats)
	HeapSysBytes   uint64 // heap reserved from OS
	GCRuns         uint64 // total GC cycles
}

// SystemResourceMonitor periodically samples process resource usage.
type SystemResourceMonitor struct {
	pollIntervalMs int64
	stopCh         chan struct{}

	RSSBytes       atomic.Uint64
	VirtualBytes   atomic.Uint64
	MinorFaults    atomic.Uint64
	MajorFaults    atomic.Uint64
	HeapAllocBytes atomic.Uint64
	HeapSysBytes   atomic.Uint64
	GCRuns         atomic.Uint64

	SampleCount atomic.Uint64
}

// NewSystemResourceMonitor constructs a monitor and starts the background sampler.
func NewSystemResourceMonitor(pollIntervalMs int64) *SystemResourceMonitor {
	m := &SystemResourceMonitor{
		pollIntervalMs: pollIntervalMs,
		stopCh:         make(chan struct{}),
	}
	// Take one immediate sample so gauges are never zero on first read.
	m.sample()
	go m.run()
	return m
}

// Stop terminates the background sampling goroutine.
func (m *SystemResourceMonitor) Stop() {
	select {
	case <-m.stopCh:
	default:
		close(m.stopCh)
	}
}

func (m *SystemResourceMonitor) run() {
	ticker := time.NewTicker(time.Duration(m.pollIntervalMs) * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-m.stopCh:
			return
		case <-ticker.C:
			m.sample()
		}
	}
}

// sample reads current process stats and updates atomics.
func (m *SystemResourceMonitor) sample() {
	// Go runtime stats (cross-platform)
	var ms runtime.MemStats
	runtime.ReadMemStats(&ms)
	m.HeapAllocBytes.Store(ms.HeapAlloc)
	m.HeapSysBytes.Store(ms.HeapSys)
	m.GCRuns.Store(uint64(ms.NumGC))

	// Try Linux /proc/self/status for RSS
	snap := m.readProcStatus()
	m.RSSBytes.Store(snap.RSSBytes)
	m.VirtualBytes.Store(snap.VirtualBytes)
	m.MinorFaults.Store(snap.MinorFaults)
	m.MajorFaults.Store(snap.MajorFaults)

	m.SampleCount.Add(1)
}

// readProcStatus parses /proc/self/status on Linux; returns zeros on other OS.
func (m *SystemResourceMonitor) readProcStatus() ResourceSnapshot {
	var snap ResourceSnapshot
	f, err := os.Open("/proc/self/status")
	if err != nil {
		// Not Linux — use Go runtime as fallback
		var ms runtime.MemStats
		runtime.ReadMemStats(&ms)
		snap.RSSBytes = ms.HeapAlloc
		snap.VirtualBytes = ms.HeapSys
		return snap
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Text()
		fields := strings.Fields(line)
		if len(fields) < 2 {
			continue
		}
		key := strings.TrimSuffix(fields[0], ":")
		val, _ := strconv.ParseUint(fields[1], 10, 64)
		switch key {
		case "VmRSS":
			snap.RSSBytes = val * 1024 // kB → bytes
		case "VmSize":
			snap.VirtualBytes = val * 1024
		case "VmMinFlt":
			snap.MinorFaults = val
		case "VmMajFlt":
			snap.MajorFaults = val
		}
	}
	return snap
}

// Snapshot returns the most recently sampled resource state.
func (m *SystemResourceMonitor) Snapshot() ResourceSnapshot {
	return ResourceSnapshot{
		RSSBytes:       m.RSSBytes.Load(),
		VirtualBytes:   m.VirtualBytes.Load(),
		MinorFaults:    m.MinorFaults.Load(),
		MajorFaults:    m.MajorFaults.Load(),
		HeapAllocBytes: m.HeapAllocBytes.Load(),
		HeapSysBytes:   m.HeapSysBytes.Load(),
		GCRuns:         m.GCRuns.Load(),
	}
}

// PrometheusText returns gauge metrics in Prometheus exposition format.
func (m *SystemResourceMonitor) PrometheusText() string {
	snap := m.Snapshot()
	return fmt.Sprintf(
		"# HELP process_resident_memory_bytes RSS in bytes\n"+
			"# TYPE process_resident_memory_bytes gauge\n"+
			"process_resident_memory_bytes %d\n"+
			"# HELP process_heap_alloc_bytes Go heap allocated\n"+
			"# TYPE process_heap_alloc_bytes gauge\n"+
			"process_heap_alloc_bytes %d\n"+
			"# HELP process_gc_runs_total Total GC cycles\n"+
			"# TYPE process_gc_runs_total counter\n"+
			"process_gc_runs_total %d\n",
		snap.RSSBytes, snap.HeapAllocBytes, snap.GCRuns,
	)
}
