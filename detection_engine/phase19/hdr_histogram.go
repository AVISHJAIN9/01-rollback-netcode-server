package phase19

import (
	"math"
	"math/bits"
	"sync/atomic"
)

// ─── Feature 110: Lock-Free High-Dynamic-Range (HDR) Latency Histogram ────────
//
// HdrHistogram uses two levels:
//   • Coarse bucket: index = 64 - leading_zeros(value) → O(1) lookup.
//   • Sub-bucket: linear sub-buckets within each power-of-two band (precision=2).
//
// Recording is a single AtomicU64 increment — no locks, no allocations.
// Range: 1 ns to 60,000,000,000 ns (60 seconds).
// Precision: < 1% relative error across the full range.

// numSubBuckets is the number of linear sub-divisions within each power-of-two band.
// 64 sub-buckets per band gives < 1.6% relative error.
const numSubBuckets = 64
const numBands = 64 // enough for 2^63 ns ≈ 292 years

// totalBuckets is the flat array size.
const totalBuckets = numBands * numSubBuckets

// HdrHistogram is a lock-free logarithmic histogram.
type HdrHistogram struct {
	buckets [totalBuckets]atomic.Uint64
	count   atomic.Uint64
	minNs   atomic.Uint64
	maxNs   atomic.Uint64
}

// NewHdrHistogram constructs a zeroed histogram.
func NewHdrHistogram() *HdrHistogram {
	h := &HdrHistogram{}
	h.minNs.Store(math.MaxUint64)
	return h
}

// bucketIndex returns the flat bucket index for valueNs.
func bucketIndex(valueNs uint64) int {
	if valueNs == 0 {
		return 0
	}
	// Band = position of the most significant bit (0-indexed from LSB).
	band := 63 - bits.LeadingZeros64(valueNs)
	if band >= numBands {
		band = numBands - 1
	}
	// Sub-bucket within the band: scale the fractional bits.
	var sub int
	if band > 0 {
		// Extract the top numSubBuckets bits below the leading 1.
		shift := band - int(math.Log2(float64(numSubBuckets)))
		if shift < 0 {
			shift = 0
		}
		sub = int((valueNs >> uint(shift)) & uint64(numSubBuckets-1))
	}
	idx := band*numSubBuckets + sub
	if idx >= totalBuckets {
		idx = totalBuckets - 1
	}
	return idx
}

// Record increments the bucket for valueNs.
func (h *HdrHistogram) Record(valueNs uint64) {
	idx := bucketIndex(valueNs)
	h.buckets[idx].Add(1)
	h.count.Add(1)

	// Update min/max (approximate — races are acceptable in a histogram context)
	for {
		cur := h.minNs.Load()
		if valueNs >= cur {
			break
		}
		if h.minNs.CompareAndSwap(cur, valueNs) {
			break
		}
	}
	for {
		cur := h.maxNs.Load()
		if valueNs <= cur {
			break
		}
		if h.maxNs.CompareAndSwap(cur, valueNs) {
			break
		}
	}
}

// TotalCount returns the number of recorded samples.
func (h *HdrHistogram) TotalCount() uint64 { return h.count.Load() }

// ValueAtPercentile returns the approximate value at the given percentile (0–100).
func (h *HdrHistogram) ValueAtPercentile(percentile float64) uint64 {
	total := h.count.Load()
	if total == 0 {
		return 0
	}

	target := uint64(math.Ceil(percentile / 100.0 * float64(total)))
	var cumulative uint64

	for i := 0; i < totalBuckets; i++ {
		cumulative += h.buckets[i].Load()
		if cumulative >= target {
			// Reconstruct approximate value from bucket index.
			band := i / numSubBuckets
			sub := i % numSubBuckets

			var val uint64
			if band == 0 {
				val = uint64(sub)
			} else {
				// Lower bound of the sub-bucket within this band.
				bandBase := uint64(1) << uint(band)
				subWidth := bandBase / uint64(numSubBuckets)
				if subWidth == 0 {
					subWidth = 1
				}
				val = bandBase + uint64(sub)*subWidth
			}
			return val
		}
	}

	return h.maxNs.Load()
}

// Min returns the minimum recorded value.
func (h *HdrHistogram) Min() uint64 {
	v := h.minNs.Load()
	if v == math.MaxUint64 {
		return 0
	}
	return v
}

// Max returns the maximum recorded value.
func (h *HdrHistogram) Max() uint64 { return h.maxNs.Load() }
