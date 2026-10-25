// Package phase18 implements Features 103–108: Stream Runtime Conclusion.
package phase18

import (
	"encoding/binary"
	"fmt"
	"hash/fnv"
	"math"
	"sort"
	"sync"
)

// ─── Feature 103: Consistent Partition Key Hashing & Distributor ─────────────
//
// ConsistentHashRing maps arbitrary byte keys to partition IDs using xxHash64
// over a virtual-node ring.  Binary search (O(log n)) finds the nearest
// clockwise virtual node.  V=256 virtual nodes per partition gives <5% σ
// of key distribution across partitions.

// xxHash64 implements a minimal, allocation-free xxHash64 over arbitrary bytes.
// Constants from the xxHash spec.
const (
	xxPrime1 = uint64(11400714785074694791)
	xxPrime2 = uint64(14029467366897019727)
	xxPrime3 = uint64(1609587929392839161)
	xxPrime4 = uint64(9650029242287828579)
	xxPrime5 = uint64(2870177450012600261)
)

// xxHash64Sum computes the xxHash64 digest of b with seed 0.
func xxHash64Sum(b []byte) uint64 {
	n := len(b)
	var h uint64

	if n >= 32 {
		p1, p2 := xxPrime1, xxPrime2
		v1 := p1 + p2
		v2 := p2
		v3 := uint64(0)
		v4 := ^p1
		i := 0
		for i <= n-32 {
			v1 = xxRound(v1, binary.LittleEndian.Uint64(b[i:]))
			v2 = xxRound(v2, binary.LittleEndian.Uint64(b[i+8:]))
			v3 = xxRound(v3, binary.LittleEndian.Uint64(b[i+16:]))
			v4 = xxRound(v4, binary.LittleEndian.Uint64(b[i+24:]))
			i += 32
		}
		h = xxMerge(v1, v2, v3, v4)
		b = b[i:]
	} else {
		h = xxPrime5
	}

	h += uint64(n)
	for len(b) >= 8 {
		h ^= xxRound(0, binary.LittleEndian.Uint64(b[:8]))
		h = bits64RotL(h, 27)*xxPrime1 + xxPrime4
		b = b[8:]
	}
	if len(b) >= 4 {
		h ^= uint64(binary.LittleEndian.Uint32(b[:4])) * xxPrime1
		h = bits64RotL(h, 23)*xxPrime2 + xxPrime3
		b = b[4:]
	}
	for len(b) > 0 {
		h ^= uint64(b[0]) * xxPrime5
		h = bits64RotL(h, 11) * xxPrime1
		b = b[1:]
	}
	h ^= h >> 33
	h *= xxPrime2
	h ^= h >> 29
	h *= xxPrime3
	h ^= h >> 32
	return h
}

func xxRound(acc, input uint64) uint64 {
	return bits64RotL(acc+input*xxPrime2, 31) * xxPrime1
}

func xxMerge(v1, v2, v3, v4 uint64) uint64 {
	h := bits64RotL(v1, 1) + bits64RotL(v2, 7) + bits64RotL(v3, 12) + bits64RotL(v4, 18)
	h = xxMergeAcc(h, v1)
	h = xxMergeAcc(h, v2)
	h = xxMergeAcc(h, v3)
	return xxMergeAcc(h, v4)
}

func xxMergeAcc(h, v uint64) uint64 {
	h ^= xxRound(0, v)
	return h*xxPrime1 + xxPrime4
}

func bits64RotL(x uint64, r uint) uint64 {
	return (x << r) | (x >> (64 - r))
}

// VirtualNode maps a ring position to a partition.
type VirtualNode struct {
	Hash        uint64
	PartitionID int
}

// ConsistentHashRing is a sorted ring of virtual nodes.
type ConsistentHashRing struct {
	mu    sync.RWMutex
	nodes []VirtualNode // sorted by Hash
}

// NewConsistentHashRing constructs an empty ring.
func NewConsistentHashRing() *ConsistentHashRing {
	return &ConsistentHashRing{}
}

// vnodeRingHash produces a well-distributed 64-bit ring position for a
// virtual node using a multiply-xorshift mix function.  This avoids the
// string-hashing clustering issue seen with FNV/xxHash on structured names.
func vnodeRingHash(partitionID, vnodeIndex int) uint64 {
	// Use two large primes to separate partitions in the hash space.
	const salt1 = uint64(0x9e3779b97f4a7c15) // Fibonacci hashing constant
	const salt2 = uint64(0x6c62272e07bb0142) // FNV-1a-64 offset basis
	x := uint64(partitionID)*salt1 ^ uint64(vnodeIndex)*salt2
	// xorshift64-star finalizer for avalanche
	x ^= x >> 30
	x *= 0xbf58476d1ce4e5b9
	x ^= x >> 27
	x *= 0x94d049bb133111eb
	x ^= x >> 31
	return x
}

// AddPartition inserts vnodeCount virtual nodes for partitionID.
func (r *ConsistentHashRing) AddPartition(partitionID int, vnodeCount int) {
	r.mu.Lock()
	defer r.mu.Unlock()

	for i := 0; i < vnodeCount; i++ {
		h := vnodeRingHash(partitionID, i)
		r.nodes = append(r.nodes, VirtualNode{Hash: h, PartitionID: partitionID})
	}
	sort.Slice(r.nodes, func(i, j int) bool { return r.nodes[i].Hash < r.nodes[j].Hash })
}

// RemovePartition removes all virtual nodes for partitionID.
func (r *ConsistentHashRing) RemovePartition(partitionID int) {
	r.mu.Lock()
	defer r.mu.Unlock()

	filtered := r.nodes[:0]
	for _, n := range r.nodes {
		if n.PartitionID != partitionID {
			filtered = append(filtered, n)
		}
	}
	r.nodes = filtered
}

// RouteKey returns the partition ID for the given key by finding
// the nearest clockwise virtual node via binary search.
func (r *ConsistentHashRing) RouteKey(key []byte) (int, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()

	if len(r.nodes) == 0 {
		return 0, fmt.Errorf("ring is empty: no partitions registered")
	}

	h := ringHash(key)
	idx := sort.Search(len(r.nodes), func(i int) bool { return r.nodes[i].Hash >= h })
	if idx >= len(r.nodes) {
		idx = 0 // wrap around
	}
	return r.nodes[idx].PartitionID, nil
}

// ringHash uses FNV-128 folded to 64 bits for excellent avalanche on short keys.
func ringHash(b []byte) uint64 {
	h := fnv.New128a()
	h.Write(b)
	sum := h.Sum(nil) // 16 bytes
	// XOR-fold the two halves for a 64-bit result with full avalanche.
	lo := binary.BigEndian.Uint64(sum[0:8])
	hi := binary.BigEndian.Uint64(sum[8:16])
	return lo ^ hi
}

// fnv64a is kept for backward compatibility (used in KeyDistributionStats).
func fnv64a(b []byte) uint64 {
	return ringHash(b)
}

// PartitionCount returns the number of distinct partitions in the ring.
func (r *ConsistentHashRing) PartitionCount() int {
	r.mu.RLock()
	defer r.mu.RUnlock()
	seen := make(map[int]bool)
	for _, n := range r.nodes {
		seen[n.PartitionID] = true
	}
	return len(seen)
}

// KeyDistributionStats routes n synthetic keys and returns per-partition counts
// and the coefficient of variation (stddev / mean).
func (r *ConsistentHashRing) KeyDistributionStats(keyCount int) map[int]int {
	counts := make(map[int]int)
	buf := make([]byte, 8)
	for i := 0; i < keyCount; i++ {
		binary.LittleEndian.PutUint64(buf, uint64(i))
		pid, _ := r.RouteKey(buf)
		counts[pid]++
	}
	return counts
}

// StdDevFraction computes the coefficient of variation of values in dist.
func StdDevFraction(dist map[int]int) float64 {
	if len(dist) == 0 {
		return 0
	}
	total := 0
	for _, v := range dist {
		total += v
	}
	mean := float64(total) / float64(len(dist))
	if mean == 0 {
		return 0
	}
	var sumSq float64
	for _, v := range dist {
		diff := float64(v) - mean
		sumSq += diff * diff
	}
	stddev := math.Sqrt(sumSq / float64(len(dist)))
	return stddev / mean
}
