package phase16

import (
	"fmt"
	"sync/atomic"
	"unsafe"
)

// ─── Feature 92: Bounded High-Watermark Ingestion Ring Queue ─────────────────
//
// Lock-free single-producer / single-consumer circular ring buffer.
// Capacity must be a power-of-two; head and tail are atomic counters.
// The 64-byte padding around head/tail prevents false sharing.
//
// PushResult semantics:
//   PushOK              – item accepted.
//   PushHighWatermark   – item accepted but queue ≥ 80% full (signal flow control).
//   PushFull            – queue at capacity; item rejected.

const cacheLineSize = 64

// PushResult is returned by TryPush.
type PushResult int

const (
	PushOK             PushResult = iota
	PushHighWatermark             // accepted but ≥ 80% full
	PushFull                      // rejected — queue at capacity
)

func (p PushResult) String() string {
	switch p {
	case PushOK:
		return "OK"
	case PushHighWatermark:
		return "HighWatermarkReached"
	case PushFull:
		return "Full"
	default:
		return "Unknown"
	}
}

// paddedUint64 is an atomic uint64 padded to a full cache line to prevent
// false sharing between head and tail on adjacent cache lines.
type paddedUint64 struct {
	val  uint64
	_pad [cacheLineSize - 8]byte
}

// BoundedRingQueue is a fixed-capacity, cache-aligned ring buffer.
type BoundedRingQueue struct {
	head paddedUint64 // consumer reads from here
	tail paddedUint64 // producer writes here
	mask uint64       // capacity - 1 (capacity is always power-of-two)
	buf  []unsafe.Pointer // pre-allocated slot array

	highWatermark uint64 // 80% of capacity
	lowWatermark  uint64 // 60% of capacity
}

// NewBoundedRingQueue creates a queue with the given capacity (rounded up to
// the next power-of-two if needed, minimum 16).
func NewBoundedRingQueue(capacity int) *BoundedRingQueue {
	cap2 := nextPowerOfTwo(capacity)
	if cap2 < 16 {
		cap2 = 16
	}
	q := &BoundedRingQueue{
		mask:          uint64(cap2 - 1),
		buf:           make([]unsafe.Pointer, cap2),
		highWatermark: uint64(float64(cap2) * 0.80),
		lowWatermark:  uint64(float64(cap2) * 0.60),
	}
	return q
}

// nextPowerOfTwo returns the smallest power-of-two ≥ n.
func nextPowerOfTwo(n int) int {
	n--
	n |= n >> 1
	n |= n >> 2
	n |= n >> 4
	n |= n >> 8
	n |= n >> 16
	n |= n >> 32
	return n + 1
}

// Capacity returns the maximum number of items the queue can hold.
func (q *BoundedRingQueue) Capacity() int { return int(q.mask + 1) }

// Size returns the current number of items in the queue.
func (q *BoundedRingQueue) Size() uint64 {
	tail := atomic.LoadUint64(&q.tail.val)
	head := atomic.LoadUint64(&q.head.val)
	if tail >= head {
		return tail - head
	}
	return 0
}

// FillRatio returns the fraction of capacity currently used (in [0,1]).
func (q *BoundedRingQueue) FillRatio() float64 {
	return float64(q.Size()) / float64(q.Capacity())
}

// TryPush attempts to enqueue item.  item must not be nil.
// Returns PushFull without enqueuing if the queue is at capacity.
func (q *BoundedRingQueue) TryPush(item interface{}) PushResult {
	tail := atomic.LoadUint64(&q.tail.val)
	head := atomic.LoadUint64(&q.head.val)
	currentSize := tail - head

	cap64 := uint64(q.Capacity())
	if currentSize >= cap64 {
		return PushFull
	}

	slot := tail & q.mask
	atomic.StorePointer(&q.buf[slot], unsafe.Pointer(&item))
	atomic.AddUint64(&q.tail.val, 1)

	newSize := currentSize + 1
	if newSize >= q.highWatermark {
		return PushHighWatermark
	}
	return PushOK
}

// Pop dequeues the next item.  Returns (item, true) or (nil, false) if empty.
func (q *BoundedRingQueue) Pop() (interface{}, bool) {
	for {
		head := atomic.LoadUint64(&q.head.val)
		tail := atomic.LoadUint64(&q.tail.val)
		if head >= tail {
			return nil, false // empty
		}

		slot := head & q.mask
		ptr := atomic.LoadPointer(&q.buf[slot])
		if ptr == nil {
			continue // spin briefly — producer hasn't stored yet
		}

		if atomic.CompareAndSwapUint64(&q.head.val, head, head+1) {
			atomic.StorePointer(&q.buf[slot], nil)
			item := *(*interface{})(ptr)
			return item, true
		}
	}
}

// IsAboveHighWatermark returns true when the queue is ≥ 80% full.
func (q *BoundedRingQueue) IsAboveHighWatermark() bool {
	return q.Size() >= q.highWatermark
}

// IsBelowLowWatermark returns true when the queue is ≤ 60% full.
func (q *BoundedRingQueue) IsBelowLowWatermark() bool {
	return q.Size() <= q.lowWatermark
}

// String implements fmt.Stringer for debugging.
func (q *BoundedRingQueue) String() string {
	return fmt.Sprintf("BoundedRingQueue{size:%d, cap:%d, fill:%.1f%%}",
		q.Size(), q.Capacity(), q.FillRatio()*100)
}
