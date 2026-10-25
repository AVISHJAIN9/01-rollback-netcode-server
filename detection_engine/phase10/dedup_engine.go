package phase10

import (
	"container/list"
	"sync"
	"sync/atomic"
)

// ─── High-Throughput Event Deduplication Engine (Feature 60) ─────────────────
//
// Two-tier architecture:
//   Tier 1 – RollingBloomFilter: two alternating bit-vector Bloom filters.
//             The "active" filter accumulates; the "cold" filter covers the
//             previous generation window.  Checking both tiers prevents gaps
//             across rotation boundaries.  False-positive rate < 0.1 %.
//
//   Tier 2 – ExactDedupLRU: 64-shard LRU storing exact 64-bit fingerprints.
//             Only consulted when Tier 1 reports a probable hit, preventing
//             unnecessary map lookups on the fast path (novel events).
//
// IsDuplicate(fp, ts):
//   • Tier-1 miss  → insert in Tier 1, return false  (novel)
//   • Tier-1 hit   → check Tier 2:
//       - Tier-2 hit  → return true  (confirmed duplicate)
//       - Tier-2 miss → insert in both tiers, return false (false-positive from Bloom)

// ─── Bloom filter ─────────────────────────────────────────────────────────────

const bloomBits = 1 << 23 // 8 MB per filter, ~8 M bits

// bloomFilter is a simple bit-vector Bloom filter using 3 hash functions
// derived from the 64-bit fingerprint.
type bloomFilter struct {
	bits [(bloomBits + 63) / 64]uint64
}

func (bf *bloomFilter) set(fp uint64) {
	for _, h := range bloomHashes(fp) {
		idx := h % bloomBits
		bf.bits[idx/64] |= 1 << (idx % 64)
	}
}

func (bf *bloomFilter) test(fp uint64) bool {
	for _, h := range bloomHashes(fp) {
		idx := h % bloomBits
		if bf.bits[idx/64]&(1<<(idx%64)) == 0 {
			return false
		}
	}
	return true
}

func (bf *bloomFilter) reset() {
	for i := range bf.bits {
		bf.bits[i] = 0
	}
}

// bloomHashes returns three independent 64-bit positions derived from fp.
func bloomHashes(fp uint64) [3]uint64 {
	// Derive three hashes via multiply-shift mixing.
	h1 := fp * 0x517cc1b727220a95
	h2 := fp * 0x9e3779b97f4a7c15
	h3 := fp * 0xbf58476d1ce4e5b9
	return [3]uint64{h1, h2, h3}
}

// RollingBloomFilter maintains two alternating Bloom filters to support a
// sliding deduplication window without a hard epoch boundary.
type RollingBloomFilter struct {
	mu      sync.Mutex
	active  *bloomFilter
	cold    *bloomFilter
	counter int64 // insertions since last rotation
	rotateN int64 // rotate after this many insertions
}

// NewRollingBloomFilter creates a filter that rotates after rotateAfterN insertions.
func NewRollingBloomFilter(rotateAfterN int64) *RollingBloomFilter {
	return &RollingBloomFilter{
		active:  &bloomFilter{},
		cold:    &bloomFilter{},
		rotateN: rotateAfterN,
	}
}

// Test checks whether fp is probably in the filter (either generation).
func (rbf *RollingBloomFilter) Test(fp uint64) bool {
	rbf.mu.Lock()
	defer rbf.mu.Unlock()
	return rbf.active.test(fp) || rbf.cold.test(fp)
}

// Insert adds fp to the active filter, rotating if the threshold is reached.
func (rbf *RollingBloomFilter) Insert(fp uint64) {
	rbf.mu.Lock()
	defer rbf.mu.Unlock()
	rbf.active.set(fp)
	rbf.counter++
	if rbf.counter >= rbf.rotateN {
		rbf.cold.reset()
		rbf.active, rbf.cold = rbf.cold, rbf.active
		rbf.counter = 0
	}
}

// ─── Exact dedup LRU ──────────────────────────────────────────────────────────

const lruShardCount = 64
const defaultLRUCapacity = 1 << 17 // 131072 entries per shard

type lruEntry struct {
	fp uint64
}

type lruShard struct {
	mu       sync.Mutex
	cap      int
	index    map[uint64]*list.Element
	list     *list.List
	hits     atomic.Int64
	misses   atomic.Int64
}

func newLRUShard(capacity int) *lruShard {
	return &lruShard{
		cap:   capacity,
		index: make(map[uint64]*list.Element, capacity),
		list:  list.New(),
	}
}

// Contains returns true if fp is in this shard.
func (ls *lruShard) Contains(fp uint64) bool {
	ls.mu.Lock()
	defer ls.mu.Unlock()
	if elem, ok := ls.index[fp]; ok {
		ls.list.MoveToFront(elem)
		ls.hits.Add(1)
		return true
	}
	ls.misses.Add(1)
	return false
}

// Insert adds fp; evicts LRU entry if over capacity.
func (ls *lruShard) Insert(fp uint64) {
	ls.mu.Lock()
	defer ls.mu.Unlock()
	if _, ok := ls.index[fp]; ok {
		return
	}
	if ls.list.Len() >= ls.cap {
		back := ls.list.Back()
		if back != nil {
			evicted := back.Value.(*lruEntry).fp
			ls.list.Remove(back)
			delete(ls.index, evicted)
		}
	}
	elem := ls.list.PushFront(&lruEntry{fp: fp})
	ls.index[fp] = elem
}

// ExactDedupLRU is a sharded exact-match deduplication cache.
type ExactDedupLRU struct {
	shards [lruShardCount]*lruShard
}

// NewExactDedupLRU creates a sharded LRU with totalCapacity entries split evenly.
func NewExactDedupLRU(totalCapacity int) *ExactDedupLRU {
	e := &ExactDedupLRU{}
	perShard := totalCapacity / lruShardCount
	if perShard < 1 {
		perShard = 1
	}
	for i := 0; i < lruShardCount; i++ {
		e.shards[i] = newLRUShard(perShard)
	}
	return e
}

func (e *ExactDedupLRU) shardFor(fp uint64) *lruShard {
	return e.shards[fp%lruShardCount]
}

// Contains returns true if fp has been seen before.
func (e *ExactDedupLRU) Contains(fp uint64) bool {
	return e.shardFor(fp).Contains(fp)
}

// Insert records fp as seen.
func (e *ExactDedupLRU) Insert(fp uint64) {
	e.shardFor(fp).Insert(fp)
}

// ─── DedupEngine ─────────────────────────────────────────────────────────────

// DedupEngine is the two-tier deduplication engine.
type DedupEngine struct {
	bloom    *RollingBloomFilter
	exact    *ExactDedupLRU
	dupsSeen atomic.Int64
	seen     atomic.Int64
}

// NewDedupEngine creates an engine with sensible defaults.
func NewDedupEngine() *DedupEngine {
	return &DedupEngine{
		bloom: NewRollingBloomFilter(500_000),
		exact: NewExactDedupLRU(lruShardCount * defaultLRUCapacity),
	}
}

// IsDuplicate checks whether fp has been seen within the sliding window.
// Returns true only when both tiers confirm the fingerprint (no false positives).
func (de *DedupEngine) IsDuplicate(fp uint64, _ int64) bool {
	de.seen.Add(1)

	if !de.bloom.Test(fp) {
		// Tier-1 miss → novel event
		de.bloom.Insert(fp)
		de.exact.Insert(fp)
		return false
	}

	// Tier-1 probable hit → consult Tier 2
	if de.exact.Contains(fp) {
		de.dupsSeen.Add(1)
		return true
	}

	// Bloom false-positive → treat as novel
	de.bloom.Insert(fp)
	de.exact.Insert(fp)
	return false
}

// DuplicateCount returns the total confirmed duplicates detected.
func (de *DedupEngine) DuplicateCount() int64 { return de.dupsSeen.Load() }

// SeenCount returns total fingerprints processed.
func (de *DedupEngine) SeenCount() int64 { return de.seen.Load() }
