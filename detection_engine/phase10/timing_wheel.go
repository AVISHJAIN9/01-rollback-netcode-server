// Package phase10 implements Features 55–60: State TTL, Snapshotting, and Deduplication.
package phase10

import (
	"container/list"
	"fmt"
	"sync"
)

// ─── Hierarchical Timing Wheel (Feature 55) ──────────────────────────────────
//
// Four-level wheel:  ms-level (64 slots) → sec-level (60 slots) →
//                    min-level (60 slots) → hour-level (24 slots).
// Each slot holds a doubly-linked list of TimerEntry values.
// Schedule / Cancel are O(1).  Advance cascades higher levels into lower ones.

const (
	msSlots   = 64
	secSlots  = 60
	minSlots  = 60
	hourSlots = 24
)

// TimerHandle is an opaque reference returned by Schedule.
type TimerHandle struct {
	level int
	slot  int
	elem  *list.Element
}

// timerEntry is stored inside a wheel slot.
type timerEntry struct {
	key      []byte
	expireMs uint64
	handle   *TimerHandle
}

// TimingWheel is a four-level hierarchical timer.
type TimingWheel struct {
	mu sync.Mutex

	ms   [msSlots]*list.List
	sec  [secSlots]*list.List
	min  [minSlots]*list.List
	hour [hourSlots]*list.List

	currentMs uint64 // last Advance timestamp
}

// NewTimingWheel constructs a wheel initialised at time 0.
func NewTimingWheel() *TimingWheel {
	tw := &TimingWheel{}
	for i := 0; i < msSlots; i++ {
		tw.ms[i] = list.New()
	}
	for i := 0; i < secSlots; i++ {
		tw.sec[i] = list.New()
	}
	for i := 0; i < minSlots; i++ {
		tw.min[i] = list.New()
	}
	for i := 0; i < hourSlots; i++ {
		tw.hour[i] = list.New()
	}
	return tw
}

// Schedule registers key with an expiry ttlMs milliseconds from now.
// Returns a TimerHandle that can be passed to Cancel.
func (tw *TimingWheel) Schedule(key []byte, ttlMs uint64) TimerHandle {
	tw.mu.Lock()
	defer tw.mu.Unlock()

	expireMs := tw.currentMs + ttlMs
	entry := &timerEntry{key: key, expireMs: expireMs}
	h := tw.placeEntry(entry)
	return h
}

// placeEntry inserts entry into the correct wheel level/slot.
// Must be called with tw.mu held.
func (tw *TimingWheel) placeEntry(entry *timerEntry) TimerHandle {
	delta := entry.expireMs - tw.currentMs
	var level, slot int
	var l *list.List

	switch {
	case delta < uint64(msSlots):
		level = 0
		slot = int(entry.expireMs % uint64(msSlots))
		l = tw.ms[slot]
	case delta < uint64(msSlots*secSlots):
		level = 1
		slot = int((entry.expireMs / uint64(msSlots)) % uint64(secSlots))
		l = tw.sec[slot]
	case delta < uint64(msSlots*secSlots*minSlots):
		level = 2
		slot = int((entry.expireMs / uint64(msSlots*secSlots)) % uint64(minSlots))
		l = tw.min[slot]
	default:
		level = 3
		slot = int((entry.expireMs / uint64(msSlots*secSlots*minSlots)) % uint64(hourSlots))
		l = tw.hour[slot]
	}

	elem := l.PushBack(entry)
	h := TimerHandle{level: level, slot: slot, elem: elem}
	entry.handle = &h
	return h
}

// Cancel removes the timer referenced by handle before it fires.
func (tw *TimingWheel) Cancel(h TimerHandle) {
	tw.mu.Lock()
	defer tw.mu.Unlock()
	if h.elem == nil {
		return
	}
	switch h.level {
	case 0:
		tw.ms[h.slot].Remove(h.elem)
	case 1:
		tw.sec[h.slot].Remove(h.elem)
	case 2:
		tw.min[h.slot].Remove(h.elem)
	case 3:
		tw.hour[h.slot].Remove(h.elem)
	}
}

// Advance moves the wheel to nowMs and returns all keys whose timers have
// expired on or before nowMs.  Higher-level entries cascade down as the
// wheel ticks past boundaries.
func (tw *TimingWheel) Advance(nowMs uint64) [][]byte {
	tw.mu.Lock()
	defer tw.mu.Unlock()

	var expired [][]byte

	for tw.currentMs < nowMs {
		tw.currentMs++

		// ── ms level ──────────────────────────────────────────────────────
		msSlot := int(tw.currentMs % uint64(msSlots))
		expired = append(expired, tw.drainSlot(tw.ms[msSlot], tw.currentMs)...)

		// ── sec level (cascades when ms wheel completes a revolution) ─────
		if tw.currentMs%uint64(msSlots) == 0 {
			secIdx := int((tw.currentMs / uint64(msSlots)) % uint64(secSlots))
			tw.cascadeSlot(tw.sec[secIdx])
		}

		// ── min level ──────────────────────────────────────────────────────
		if tw.currentMs%uint64(msSlots*secSlots) == 0 {
			minIdx := int((tw.currentMs / uint64(msSlots*secSlots)) % uint64(minSlots))
			tw.cascadeSlot(tw.min[minIdx])
		}

		// ── hour level ─────────────────────────────────────────────────────
		if tw.currentMs%uint64(msSlots*secSlots*minSlots) == 0 {
			hourIdx := int((tw.currentMs / uint64(msSlots*secSlots*minSlots)) % uint64(hourSlots))
			tw.cascadeSlot(tw.hour[hourIdx])
		}
	}

	return expired
}

// drainSlot removes all entries from l whose expireMs <= nowMs and returns keys.
func (tw *TimingWheel) drainSlot(l *list.List, nowMs uint64) [][]byte {
	var keys [][]byte
	var next *list.Element
	for e := l.Front(); e != nil; e = next {
		next = e.Next()
		entry := e.Value.(*timerEntry)
		if entry.expireMs <= nowMs {
			keys = append(keys, entry.key)
			l.Remove(e)
		}
	}
	return keys
}

// cascadeSlot re-inserts entries from a higher level slot into the correct
// lower-level bucket now that we have advanced to finer resolution.
func (tw *TimingWheel) cascadeSlot(l *list.List) {
	var next *list.Element
	for e := l.Front(); e != nil; e = next {
		next = e.Next()
		entry := e.Value.(*timerEntry)
		l.Remove(e)
		tw.placeEntry(entry)
	}
}

// CurrentMs returns the wheel's current tick position.
func (tw *TimingWheel) CurrentMs() uint64 {
	tw.mu.Lock()
	defer tw.mu.Unlock()
	return tw.currentMs
}

// String implements fmt.Stringer for debugging.
func (tw *TimingWheel) String() string {
	tw.mu.Lock()
	defer tw.mu.Unlock()
	return fmt.Sprintf("TimingWheel{currentMs:%d}", tw.currentMs)
}
