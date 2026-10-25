package phase16

import (
	"sync"
	"sync/atomic"
	"time"
)

// ─── Feature 95: Multi-Tenant Token-Bucket Rate Limiter ──────────────────────
//
// Each tenant has its own token bucket with:
//   • burstCapacity  – maximum tokens (allows short bursts above sustained rate).
//   • refillRateEPS  – tokens added per second (sustained rate).
//
// TryAcquire refills tokens based on elapsed monotonic time, then deducts count
// via a CAS loop.  No locks on the hot path.

// tokenBucket holds per-tenant rate-limiting state.
type tokenBucket struct {
	burstCapacity  uint64       // maximum tokens
	refillRateEPS  float64      // tokens per second
	tokens         atomic.Uint64
	lastRefillNs   atomic.Int64 // last refill timestamp (UnixNano)
}

func newTokenBucket(burstCapacity uint64, refillRateEPS float64) *tokenBucket {
	b := &tokenBucket{
		burstCapacity: burstCapacity,
		refillRateEPS: refillRateEPS,
	}
	b.tokens.Store(burstCapacity)
	b.lastRefillNs.Store(time.Now().UnixNano())
	return b
}

// refill calculates tokens earned since the last refill and adds them (capped at burst).
func (b *tokenBucket) refill(nowNs int64) {
	last := b.lastRefillNs.Load()
	elapsedSec := float64(nowNs-last) / 1e9
	if elapsedSec <= 0 {
		return
	}

	earned := uint64(elapsedSec * b.refillRateEPS)
	if earned == 0 {
		return
	}

	// Update lastRefillNs — only one goroutine should win (best-effort; over-refill is capped)
	if b.lastRefillNs.CompareAndSwap(last, nowNs) {
		for {
			cur := b.tokens.Load()
			newVal := cur + earned
			if newVal > b.burstCapacity {
				newVal = b.burstCapacity
			}
			if b.tokens.CompareAndSwap(cur, newVal) {
				return
			}
		}
	}
}

// tryAcquire attempts to consume count tokens.  Returns true on success.
func (b *tokenBucket) tryAcquire(count uint64, nowNs int64) bool {
	b.refill(nowNs)

	for {
		cur := b.tokens.Load()
		if cur < count {
			return false
		}
		if b.tokens.CompareAndSwap(cur, cur-count) {
			return true
		}
	}
}

// AvailableTokens returns the current token balance.
func (b *tokenBucket) AvailableTokens() uint64 { return b.tokens.Load() }

// TenantRateLimiter manages per-tenant token buckets.
type TenantRateLimiter struct {
	mu      sync.RWMutex
	buckets map[string]*tokenBucket
}

// NewTenantRateLimiter constructs an empty limiter.
func NewTenantRateLimiter() *TenantRateLimiter {
	return &TenantRateLimiter{
		buckets: make(map[string]*tokenBucket),
	}
}

// RegisterTenant creates a bucket for tenantID.
// sustainedEPS is the refill rate; burstCapacity allows short spikes above that.
func (l *TenantRateLimiter) RegisterTenant(tenantID string, sustainedEPS float64, burstCapacity uint64) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.buckets[tenantID] = newTokenBucket(burstCapacity, sustainedEPS)
}

// TryAcquire attempts to consume count events for tenantID.
// Returns false if the tenant is throttled.
func (l *TenantRateLimiter) TryAcquire(tenantID string, count uint64) bool {
	l.mu.RLock()
	b, ok := l.buckets[tenantID]
	l.mu.RUnlock()

	if !ok {
		return false // unknown tenant — reject
	}
	return b.tryAcquire(count, time.Now().UnixNano())
}

// AvailableTokens returns the current token balance for a tenant.
func (l *TenantRateLimiter) AvailableTokens(tenantID string) (uint64, bool) {
	l.mu.RLock()
	b, ok := l.buckets[tenantID]
	l.mu.RUnlock()
	if !ok {
		return 0, false
	}
	return b.AvailableTokens(), true
}
