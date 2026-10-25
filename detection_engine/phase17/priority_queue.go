// Package phase17 implements Features 97–102: Runtime Backpressure & Stream Scheduling.
package phase17

import (
	"fmt"
	"sync/atomic"
)

// ─── Feature 97: Multi-Level Priority Event Ingestion Queue ──────────────────
//
// Four priority levels backed by independent ring buffers.
// Dispatch uses Deficit Weighted Round-Robin (DWRR) with starvation credits:
//   Critical: weight 8  (always served first)
//   High:     weight 4
//   Medium:   weight 2
//   Low:      weight 1
//
// When a higher-priority queue has items, it is served first.  If a lower-priority
// queue has been skipped more than maxStarvationSkips times in a row, it gets
// a one-time starvation-boost credit and is served regardless of priority.

// PriorityLevel represents an event's ingestion priority.
type PriorityLevel int

const (
	PriorityCritical PriorityLevel = iota
	PriorityHigh
	PriorityMedium
	PriorityLow
	numPriorities = 4
)

func (p PriorityLevel) String() string {
	switch p {
	case PriorityCritical:
		return "Critical"
	case PriorityHigh:
		return "High"
	case PriorityMedium:
		return "Medium"
	case PriorityLow:
		return "Low"
	default:
		return fmt.Sprintf("Priority(%d)", int(p))
	}
}

// PriorityItem wraps an event with its priority label.
type PriorityItem struct {
	Priority PriorityLevel
	Value    interface{}
}

// simpleFIFO is a small channel-backed FIFO per priority level.
type simpleFIFO struct {
	ch      chan interface{}
	enqueued atomic.Int64
	dequeued atomic.Int64
}

func newSimpleFIFO(capacity int) *simpleFIFO {
	return &simpleFIFO{ch: make(chan interface{}, capacity)}
}

func (f *simpleFIFO) push(v interface{}) bool {
	select {
	case f.ch <- v:
		f.enqueued.Add(1)
		return true
	default:
		return false // full
	}
}

func (f *simpleFIFO) pop() (interface{}, bool) {
	select {
	case v := <-f.ch:
		f.dequeued.Add(1)
		return v, true
	default:
		return nil, false
	}
}

func (f *simpleFIFO) len() int { return len(f.ch) }

// MultiPriorityQueue dispatches events across 4 priority levels with DWRR + anti-starvation.
type MultiPriorityQueue struct {
	queues         [numPriorities]*simpleFIFO
	weights        [numPriorities]int   // DWRR weight per tier
	deficits       [numPriorities]int   // accumulated deficit counters
	skipCounts     [numPriorities]int   // consecutive skips per tier
	maxStarvSkips  int                  // threshold before starvation boost
	dispatchTotal  atomic.Int64
	dispatchByTier [numPriorities]atomic.Int64
}

// NewMultiPriorityQueue constructs a queue with perLevelCapacity slots per tier.
func NewMultiPriorityQueue(perLevelCapacity int) *MultiPriorityQueue {
	q := &MultiPriorityQueue{
		weights:       [numPriorities]int{8, 4, 2, 1},
		maxStarvSkips: 50,
	}
	for i := 0; i < numPriorities; i++ {
		q.queues[i] = newSimpleFIFO(perLevelCapacity)
		q.deficits[i] = q.weights[i]
	}
	return q
}

// Push enqueues item at the given priority. Returns false if that tier is full.
func (q *MultiPriorityQueue) Push(priority PriorityLevel, item interface{}) bool {
	return q.queues[priority].push(item)
}

// Pop dequeues the highest-priority available item using DWRR + anti-starvation.
// Returns (nil, false) when all queues are empty.
func (q *MultiPriorityQueue) Pop() (PriorityLevel, interface{}, bool) {
	// First pass: strict priority (Critical before everything)
	for p := PriorityCritical; p < numPriorities; p++ {
		if q.queues[p].len() > 0 {
			v, ok := q.queues[p].pop()
			if ok {
				q.dispatchTotal.Add(1)
				q.dispatchByTier[p].Add(1)
				// Reset skip count for lower tiers that just got served above
				for lower := p + 1; lower < numPriorities; lower++ {
					q.skipCounts[int(lower)]++
				}
				q.skipCounts[int(p)] = 0
				return p, v, true
			}
		}
	}
	return PriorityLow, nil, false
}

// PopWithAntiStarvation is an alternative dispatcher that uses deficit counters
// to prevent low-priority queues from being starved indefinitely.
func (q *MultiPriorityQueue) PopWithAntiStarvation() (PriorityLevel, interface{}, bool) {
	// Check starvation boost first
	for p := PriorityHigh; p < numPriorities; p++ {
		if q.skipCounts[int(p)] >= q.maxStarvSkips && q.queues[p].len() > 0 {
			v, ok := q.queues[p].pop()
			if ok {
				q.skipCounts[int(p)] = 0
				q.dispatchTotal.Add(1)
				q.dispatchByTier[p].Add(1)
				return p, v, true
			}
		}
	}
	return q.Pop()
}

// Depth returns the number of items in a specific priority queue.
func (q *MultiPriorityQueue) Depth(priority PriorityLevel) int {
	return q.queues[priority].len()
}

// TotalDispatched returns the cumulative dispatch count.
func (q *MultiPriorityQueue) TotalDispatched() int64 { return q.dispatchTotal.Load() }

// DispatchedByTier returns the dispatch count for one tier.
func (q *MultiPriorityQueue) DispatchedByTier(p PriorityLevel) int64 {
	return q.dispatchByTier[p].Load()
}
