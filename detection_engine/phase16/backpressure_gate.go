package phase16

import (
	"sync/atomic"
)

// ─── Feature 94: Reactive Credit-Based Backpressure Controller ───────────────
//
// BackpressureController monitors ingestion queue fill ratios and issues
// pause / resume signals to upstream consumers (Kafka partition polling).
//
// State transitions:
//   Active  → Paused  when fill_ratio > highWatermarkRatio (0.80)
//   Paused  → Active  when fill_ratio < lowWatermarkRatio  (0.60)

// FlowAction is returned by CheckPressure.
type FlowAction int

const (
	FlowContinue FlowAction = iota // ingress may continue
	FlowPause                      // ingress should pause (high watermark exceeded)
	FlowResume                     // ingress may resume (dropped below low watermark)
)

func (f FlowAction) String() string {
	switch f {
	case FlowContinue:
		return "Continue"
	case FlowPause:
		return "Pause"
	case FlowResume:
		return "Resume"
	default:
		return "Unknown"
	}
}

// BackpressureState is the current flow control state.
type BackpressureState int32

const (
	BPStateActive BackpressureState = 0
	BPStatePaused BackpressureState = 1
)

// BackpressureController tracks queue pressure and emits flow control signals.
type BackpressureController struct {
	state               atomic.Int32 // BackpressureState
	highWatermarkRatio  float64      // e.g., 0.80
	lowWatermarkRatio   float64      // e.g., 0.60
	pauseCount          atomic.Int64
	resumeCount         atomic.Int64
	// pausedPartitions stores partition IDs that have been paused.
	pausedPartitions    []int
	partMu              partitionMutex
}

// partitionMutex is a lightweight spinlock used only on the cold control path.
type partitionMutex struct {
	flag atomic.Int32
}

func (m *partitionMutex) Lock() {
	for !m.flag.CompareAndSwap(0, 1) {
	}
}
func (m *partitionMutex) Unlock() { m.flag.Store(0) }

// NewBackpressureController constructs a controller with the given watermarks.
func NewBackpressureController(highWatermark, lowWatermark float64) *BackpressureController {
	return &BackpressureController{
		highWatermarkRatio: highWatermark,
		lowWatermarkRatio:  lowWatermark,
	}
}

// CurrentState returns the current flow control state.
func (c *BackpressureController) CurrentState() BackpressureState {
	return BackpressureState(c.state.Load())
}

// CheckPressure evaluates queueFillRatio and returns the appropriate FlowAction.
// This is the hot path — single atomic load + float comparison.
func (c *BackpressureController) CheckPressure(queueFillRatio float64) FlowAction {
	current := BackpressureState(c.state.Load())

	switch {
	case queueFillRatio >= c.highWatermarkRatio && current == BPStateActive:
		c.pauseIngress()
		return FlowPause

	case queueFillRatio < c.lowWatermarkRatio && current == BPStatePaused:
		c.resumeIngress()
		return FlowResume

	default:
		return FlowContinue
	}
}

// PauseIngress halts upstream event consumption.
func (c *BackpressureController) pauseIngress() {
	c.state.Store(int32(BPStatePaused))
	c.pauseCount.Add(1)
}

// ResumeIngress re-enables upstream event consumption.
func (c *BackpressureController) resumeIngress() {
	c.state.Store(int32(BPStateActive))
	c.resumeCount.Add(1)
}

// PauseCount returns total number of pause transitions.
func (c *BackpressureController) PauseCount() int64 { return c.pauseCount.Load() }

// ResumeCount returns total number of resume transitions.
func (c *BackpressureController) ResumeCount() int64 { return c.resumeCount.Load() }

// RegisterPartitionPause records that partitionID has been paused.
func (c *BackpressureController) RegisterPartitionPause(partitionID int) {
	c.partMu.Lock()
	c.pausedPartitions = append(c.pausedPartitions, partitionID)
	c.partMu.Unlock()
}

// ClearPausedPartitions returns and clears the paused partition list.
func (c *BackpressureController) ClearPausedPartitions() []int {
	c.partMu.Lock()
	defer c.partMu.Unlock()
	cp := make([]int, len(c.pausedPartitions))
	copy(cp, c.pausedPartitions)
	c.pausedPartitions = c.pausedPartitions[:0]
	return cp
}
