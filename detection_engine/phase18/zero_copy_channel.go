package phase18

import (
	"fmt"
	"sync/atomic"
	"unsafe"
)

// ─── Feature 105: Zero-Copy Shared-Memory Event Hand-off Channels ─────────────
//
// SlabHandOffChannel is a lock-free SPSC ring queue that passes typed
// EventSlabRef descriptors without any heap allocation per transfer.
//
// Layout:
//   • Fixed-size circular descriptor ring of capacity (power-of-two).
//   • Pre-allocated slab arena of slabSize bytes backing actual event data.
//   • Producers write event bytes into the slab, then push an EventSlabRef
//     (arena offset + length) into the ring.
//   • Consumers read the ref and access the slab slice directly — zero copy.
//
// The ring uses the same padded-atomic head/tail pattern from the Phase 16
// BoundedRingQueue to prevent false sharing.

// EventSlabRef is a pointer-sized descriptor into the shared slab arena.
type EventSlabRef struct {
	Offset uint32 // byte offset in the arena
	Length uint32 // byte length of the event
}

// slabArena is a pre-allocated contiguous byte buffer.
type slabArena struct {
	buf    []byte
	cursor atomic.Uint64 // monotonically increasing write offset (wraps at capacity)
	size   uint64
}

func newSlabArena(sizeBytes int) *slabArena {
	return &slabArena{
		buf:  make([]byte, sizeBytes),
		size: uint64(sizeBytes),
	}
}

// Allocate carves out length bytes from the arena, returning the offset.
// Wraps around when the end is reached (circular).
func (a *slabArena) Allocate(length uint32) (offset uint32, ok bool) {
	if uint64(length) > a.size {
		return 0, false
	}
	cur := a.cursor.Add(uint64(length)) - uint64(length)
	off := cur % a.size
	// Simple linear allocator with wrap (no fragmentation checks in this impl)
	return uint32(off), true
}

// WriteAt writes data into the arena at offset.
func (a *slabArena) WriteAt(offset uint32, data []byte) {
	copy(a.buf[offset:], data)
}

// ReadAt returns a slice of arena memory at offset with the given length.
// Callers must not retain the slice past the next Allocate at overlapping offsets.
func (a *slabArena) ReadAt(offset uint32, length uint32) []byte {
	return a.buf[offset : offset+length]
}

// ─── SPSC ring of EventSlabRef ────────────────────────────────────────────────

type refHead struct {
	val  uint64
	_pad [56]byte
}
type refTail struct {
	val  uint64
	_pad [56]byte
}

// SlabHandOffChannel is a lock-free zero-copy SPSC event channel.
type SlabHandOffChannel struct {
	head  refHead
	tail  refTail
	mask  uint64
	slots []unsafe.Pointer // each slot holds *EventSlabRef
	arena *slabArena
}

// NewSlabHandOffChannel constructs a channel with given ring capacity and arena.
// capacity must be a power-of-two.
func NewSlabHandOffChannel(capacity, slabBytes int) *SlabHandOffChannel {
	cap2 := nextPow2(capacity)
	ch := &SlabHandOffChannel{
		mask:  uint64(cap2 - 1),
		slots: make([]unsafe.Pointer, cap2),
		arena: newSlabArena(slabBytes),
	}
	return ch
}

func nextPow2(n int) int {
	n--
	n |= n >> 1
	n |= n >> 2
	n |= n >> 4
	n |= n >> 8
	n |= n >> 16
	n |= n >> 32
	return n + 1
}

// PushRef enqueues an EventSlabRef.  Returns ChannelFullError if the ring is full.
func (c *SlabHandOffChannel) PushRef(ref EventSlabRef) error {
	tail := atomic.LoadUint64(&c.tail.val)
	head := atomic.LoadUint64(&c.head.val)

	if tail-head > c.mask {
		return fmt.Errorf("channel full (capacity=%d)", c.mask+1)
	}

	slot := tail & c.mask
	r := &ref
	atomic.StorePointer(&c.slots[slot], unsafe.Pointer(r))
	atomic.AddUint64(&c.tail.val, 1)
	return nil
}

// PopRef dequeues an EventSlabRef. Returns (zero, false) if empty.
func (c *SlabHandOffChannel) PopRef() (EventSlabRef, bool) {
	for {
		head := atomic.LoadUint64(&c.head.val)
		tail := atomic.LoadUint64(&c.tail.val)
		if head >= tail {
			return EventSlabRef{}, false
		}
		slot := head & c.mask
		ptr := atomic.LoadPointer(&c.slots[slot])
		if ptr == nil {
			continue
		}
		if !atomic.CompareAndSwapUint64(&c.head.val, head, head+1) {
			continue
		}
		atomic.StorePointer(&c.slots[slot], nil)
		return *(*EventSlabRef)(ptr), true
	}
}

// WriteEvent writes eventData into the slab arena and pushes a ref into the ring.
// This is the primary producer API.
func (c *SlabHandOffChannel) WriteEvent(data []byte) error {
	offset, ok := c.arena.Allocate(uint32(len(data)))
	if !ok {
		return fmt.Errorf("slab arena full: data len=%d exceeds arena size", len(data))
	}
	c.arena.WriteAt(offset, data)
	return c.PushRef(EventSlabRef{Offset: offset, Length: uint32(len(data))})
}

// ReadEvent reads the event bytes referenced by ref from the slab arena.
// Returns a zero-copy slice into the arena buffer.
func (c *SlabHandOffChannel) ReadEvent(ref EventSlabRef) []byte {
	return c.arena.ReadAt(ref.Offset, ref.Length)
}

// Size returns the current number of items in the ring.
func (c *SlabHandOffChannel) Size() uint64 {
	t := atomic.LoadUint64(&c.tail.val)
	h := atomic.LoadUint64(&c.head.val)
	if t >= h {
		return t - h
	}
	return 0
}
