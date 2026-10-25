package phase11

import (
	"container/list"
	"sync"
)

// ─── Feature 62: Deduplication TTL Sliding Wheel ─────────────────────────────
//
// SlidingDeduplicationWheel is a single-level timing wheel used specifically
// for managing deduplication TTL entries.  It supports:
//   • Fixed-window TTL: timer set once at registration, not moved on hits.
//   • Sliding-window TTL: timer bumped forward on each hit (O(1) re-insertion).
//
// The wheel has slotCount buckets, each covering slotWidthNs nanoseconds.
// Advance(nowNs) fires all entries whose deadline ≤ nowNs.

const (
	dedupWheelSlots    = 3600  // 3600 slots
	dedupSlotWidthNs   = int64(1_000_000_000) // 1 second per slot
	dedupWheelSpanNs   = int64(dedupWheelSlots) * dedupSlotWidthNs // 1 hour
)

// dedupTimerEntry lives in a wheel slot list.
type dedupTimerEntry struct {
	fingerprint uint64
	expireNs    int64
}

// SlidingDeduplicationWheel manages dedup TTL timers.
type SlidingDeduplicationWheel struct {
	mu        sync.Mutex
	slots     [dedupWheelSlots]*list.List
	index     map[uint64]*list.Element // fingerprint → list element for O(1) removal
	currentNs int64
}

// NewSlidingDeduplicationWheel constructs a wheel at time 0.
func NewSlidingDeduplicationWheel() *SlidingDeduplicationWheel {
	w := &SlidingDeduplicationWheel{
		index: make(map[uint64]*list.Element),
	}
	for i := 0; i < dedupWheelSlots; i++ {
		w.slots[i] = list.New()
	}
	return w
}

// slotFor maps an absolute nanosecond timestamp to a wheel slot index.
func slotFor(ns int64) int {
	return int((ns/dedupSlotWidthNs) % int64(dedupWheelSlots))
}

// Register adds fingerprint with an expiry at (nowNs + ttlNs).
// If the fingerprint is already registered it is silently ignored (use UpdateTTL).
func (w *SlidingDeduplicationWheel) Register(fp uint64, nowNs, ttlNs int64) {
	w.mu.Lock()
	defer w.mu.Unlock()

	if _, ok := w.index[fp]; ok {
		return
	}
	expireNs := nowNs + ttlNs
	slot := slotFor(expireNs)
	entry := &dedupTimerEntry{fingerprint: fp, expireNs: expireNs}
	elem := w.slots[slot].PushBack(entry)
	w.index[fp] = elem
}

// UpdateTTL reschedules fingerprint's expiry to (nowNs + ttlNs).
// If sliding == false the original registration time is preserved.
func (w *SlidingDeduplicationWheel) UpdateTTL(fp uint64, nowNs int64, ttlNs int64, sliding bool) {
	w.mu.Lock()
	defer w.mu.Unlock()

	elem, ok := w.index[fp]
	if !ok {
		// Not tracked — register fresh
		expireNs := nowNs + ttlNs
		slot := slotFor(expireNs)
		entry := &dedupTimerEntry{fingerprint: fp, expireNs: expireNs}
		newElem := w.slots[slot].PushBack(entry)
		w.index[fp] = newElem
		return
	}

	if !sliding {
		return // fixed TTL: do not move the timer
	}

	entry := elem.Value.(*dedupTimerEntry)
	oldSlot := slotFor(entry.expireNs)
	w.slots[oldSlot].Remove(elem)

	entry.expireNs = nowNs + ttlNs
	newSlot := slotFor(entry.expireNs)
	newElem := w.slots[newSlot].PushBack(entry)
	w.index[fp] = newElem
}

// Advance fires all entries whose expireNs ≤ nowNs, removing them from the
// wheel and returning their fingerprints.
func (w *SlidingDeduplicationWheel) Advance(nowNs int64) []uint64 {
	w.mu.Lock()
	defer w.mu.Unlock()

	var fired []uint64

	for w.currentNs <= nowNs {
		slot := slotFor(w.currentNs)
		l := w.slots[slot]

		var next *list.Element
		for e := l.Front(); e != nil; e = next {
			next = e.Next()
			entry := e.Value.(*dedupTimerEntry)
			if entry.expireNs <= nowNs {
				fired = append(fired, entry.fingerprint)
				delete(w.index, entry.fingerprint)
				l.Remove(e)
			}
		}
		w.currentNs += dedupSlotWidthNs
	}

	return fired
}

// Deregister removes the timer for fingerprint (e.g., after the alert is closed).
func (w *SlidingDeduplicationWheel) Deregister(fp uint64) {
	w.mu.Lock()
	defer w.mu.Unlock()

	if elem, ok := w.index[fp]; ok {
		entry := elem.Value.(*dedupTimerEntry)
		slot := slotFor(entry.expireNs)
		w.slots[slot].Remove(elem)
		delete(w.index, fp)
	}
}
