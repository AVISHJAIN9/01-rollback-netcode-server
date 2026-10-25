package phase10

import (
	"container/list"
	"sync"
)

// ─── Idle-Key Inactivity Eviction (Feature 56) ───────────────────────────────
//
// Each key is tracked in an access-ordered doubly-linked list.
// Touch() moves the key to the front (most-recently-used position).
// EvictStale() walks from the back (least-recently-used) and removes keys
// whose last_access_timestamp_ns is older than the supplied cutoff.

// accessEntry holds the metadata for one tracked key.
type accessEntry struct {
	key              []byte
	lastAccessTimeNs int64
}

// IdleKeyTracker maintains an LRU-ordered intrusive list for inactivity eviction.
type IdleKeyTracker struct {
	mu      sync.Mutex
	list    *list.List
	index   map[string]*list.Element // key string → list element
}

// NewIdleKeyTracker constructs an empty tracker.
func NewIdleKeyTracker() *IdleKeyTracker {
	return &IdleKeyTracker{
		list:  list.New(),
		index: make(map[string]*list.Element),
	}
}

// Touch records or updates the last-access timestamp for key.
// Moves the entry to the front of the access list (most-recently-used).
func (ikt *IdleKeyTracker) Touch(key []byte, currentTimeNs int64) {
	ikt.mu.Lock()
	defer ikt.mu.Unlock()

	k := string(key)
	if elem, ok := ikt.index[k]; ok {
		entry := elem.Value.(*accessEntry)
		entry.lastAccessTimeNs = currentTimeNs
		ikt.list.MoveToFront(elem)
		return
	}

	entry := &accessEntry{key: append([]byte(nil), key...), lastAccessTimeNs: currentTimeNs}
	elem := ikt.list.PushFront(entry)
	ikt.index[k] = elem
}

// EvictStale removes up to maxEvictions keys whose lastAccessTimeNs < cutoffNs
// and returns their byte-slice keys.  Walks from the back of the list (coldest).
func (ikt *IdleKeyTracker) EvictStale(cutoffNs int64, maxEvictions int) [][]byte {
	ikt.mu.Lock()
	defer ikt.mu.Unlock()

	var evicted [][]byte
	for len(evicted) < maxEvictions {
		back := ikt.list.Back()
		if back == nil {
			break
		}
		entry := back.Value.(*accessEntry)
		if entry.lastAccessTimeNs >= cutoffNs {
			// Tail is fresh enough — nothing older exists further forward.
			break
		}
		evicted = append(evicted, entry.key)
		delete(ikt.index, string(entry.key))
		ikt.list.Remove(back)
	}
	return evicted
}

// Remove explicitly unregisters a key (e.g., after TTL expiry from the wheel).
func (ikt *IdleKeyTracker) Remove(key []byte) {
	ikt.mu.Lock()
	defer ikt.mu.Unlock()

	k := string(key)
	if elem, ok := ikt.index[k]; ok {
		ikt.list.Remove(elem)
		delete(ikt.index, k)
	}
}

// Len returns the number of tracked keys.
func (ikt *IdleKeyTracker) Len() int {
	ikt.mu.Lock()
	defer ikt.mu.Unlock()
	return ikt.list.Len()
}
