package phase17

import "sync/atomic"

// ─── Feature 101: Memory-Pressure Cooperative Load Shedder ───────────────────
//
// LoadShedder monitors simulated memory pressure and makes instant drop/pass
// decisions per event priority tier.
//
// Shedding stages:
//   Normal  (< 80 % RAM):  nothing dropped.
//   DropLow (≥ 80 %):      drop PriorityLow.
//   DropMed (≥ 90 %):      drop PriorityLow + PriorityMedium.
//   Critical (≥ 95 %):    drop Low + Medium + High; only Critical passes.

// SheddingLevel is the current load-shedding activation stage.
type SheddingLevel int

const (
	SheddingNormal  SheddingLevel = iota // < 80% — no shedding
	SheddingDropLow                      // ≥ 80% — drop Low
	SheddingDropMed                      // ≥ 90% — drop Low+Medium
	SheddingCritical                     // ≥ 95% — drop Low+Medium+High
)

func (sl SheddingLevel) String() string {
	switch sl {
	case SheddingNormal:
		return "Normal"
	case SheddingDropLow:
		return "DropLow"
	case SheddingDropMed:
		return "DropMediumAndLow"
	case SheddingCritical:
		return "CriticalThrottle"
	default:
		return "Unknown"
	}
}

// LoadShedder makes instant shedding decisions based on memory pressure.
type LoadShedder struct {
	pressureRatio   atomic.Value // float64 in [0,1]
	droppedLow      atomic.Int64
	droppedMedium   atomic.Int64
	droppedHigh     atomic.Int64
	passedCritical  atomic.Int64
}

// NewLoadShedder constructs a shedder at zero pressure.
func NewLoadShedder() *LoadShedder {
	ls := &LoadShedder{}
	ls.pressureRatio.Store(float64(0))
	return ls
}

// SetMemoryPressure updates the current memory pressure ratio (0.0–1.0).
// Call this from a background monitor goroutine that reads /proc/meminfo or cgroup stats.
func (ls *LoadShedder) SetMemoryPressure(ratio float64) {
	if ratio < 0 {
		ratio = 0
	}
	if ratio > 1 {
		ratio = 1
	}
	ls.pressureRatio.Store(ratio)
}

// CurrentPressure returns the most recently set memory pressure ratio.
func (ls *LoadShedder) CurrentPressure() float64 {
	return ls.pressureRatio.Load().(float64)
}

// CurrentLevel translates the current pressure ratio into a SheddingLevel.
func (ls *LoadShedder) CurrentLevel() SheddingLevel {
	r := ls.CurrentPressure()
	switch {
	case r >= 0.95:
		return SheddingCritical
	case r >= 0.90:
		return SheddingDropMed
	case r >= 0.80:
		return SheddingDropLow
	default:
		return SheddingNormal
	}
}

// ShouldShed returns true when the event at priority should be dropped.
// This is a lock-free hot path: single atomic load + integer compare.
func (ls *LoadShedder) ShouldShed(priority PriorityLevel) bool {
	level := ls.CurrentLevel()

	shed := false
	switch level {
	case SheddingCritical:
		shed = priority >= PriorityHigh // drop High, Medium, Low
	case SheddingDropMed:
		shed = priority >= PriorityMedium // drop Medium and Low
	case SheddingDropLow:
		shed = priority == PriorityLow
	default:
		shed = false
	}

	if shed {
		switch priority {
		case PriorityLow:
			ls.droppedLow.Add(1)
		case PriorityMedium:
			ls.droppedMedium.Add(1)
		case PriorityHigh:
			ls.droppedHigh.Add(1)
		}
	} else if priority == PriorityCritical {
		ls.passedCritical.Add(1)
	}

	return shed
}

// DroppedLow returns total Low-priority events dropped.
func (ls *LoadShedder) DroppedLow() int64 { return ls.droppedLow.Load() }

// DroppedMedium returns total Medium-priority events dropped.
func (ls *LoadShedder) DroppedMedium() int64 { return ls.droppedMedium.Load() }

// DroppedHigh returns total High-priority events dropped.
func (ls *LoadShedder) DroppedHigh() int64 { return ls.droppedHigh.Load() }

// PassedCritical returns total Critical-priority events passed through.
func (ls *LoadShedder) PassedCritical() int64 { return ls.passedCritical.Load() }
