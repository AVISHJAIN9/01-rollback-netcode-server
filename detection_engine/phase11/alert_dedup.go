package phase11

import (
	"sync"
	"time"
)

// ─── Feature 61: Alert Deduplication & Fingerprint Suppression ───────────────

// TTLMode controls whether the suppression window is fixed or sliding.
type TTLMode int

const (
	TTLFixed   TTLMode = iota // window expires at first_seen + TTL
	TTLSliding                // window extends on every hit: expire = last_seen + TTL
)

// TTLConfig parameterises the suppression window.
type TTLConfig struct {
	Mode      TTLMode
	WindowNs  int64 // suppression duration in nanoseconds
}

// ActiveSuppression holds live suppression state for one fingerprint.
type ActiveSuppression struct {
	Fingerprint  uint64
	FirstSeenNs  int64
	LastSeenNs   int64
	Count        int64
	ExpireAtNs   int64
}

// AlertAction is the result of CheckOrSuppress.
type AlertAction int

const (
	AlertActionEmit     AlertAction = iota // new alert, emit downstream
	AlertActionSuppress                    // duplicate, suppress
)

// SuppressionInfo is returned alongside AlertActionSuppress.
type SuppressionInfo struct {
	Fingerprint uint64
	Count       int64
	FirstSeenNs int64
	LastSeenNs  int64
}

// AlertDeduplicator manages an in-memory suppression cache sharded for
// reduced lock contention.
type AlertDeduplicator struct {
	shards [dedupShardCount]dedupShard
}

const dedupShardCount = 64

type dedupShard struct {
	mu    sync.Mutex
	cache map[uint64]*ActiveSuppression
}

// NewAlertDeduplicator constructs an empty deduplicator.
func NewAlertDeduplicator() *AlertDeduplicator {
	d := &AlertDeduplicator{}
	for i := 0; i < dedupShardCount; i++ {
		d.shards[i].cache = make(map[uint64]*ActiveSuppression)
	}
	return d
}

func (d *AlertDeduplicator) shardFor(fp uint64) *dedupShard {
	return &d.shards[fp%dedupShardCount]
}

// CheckOrSuppress examines the suppression cache for match's fingerprint.
// Returns (AlertActionEmit, nil, _) for new alerts.
// Returns (AlertActionSuppress, info, _) for duplicates.
func (d *AlertDeduplicator) CheckOrSuppress(
	match *DetectionMatch,
	cfg TTLConfig,
) (AlertAction, *SuppressionInfo) {
	fp := match.SuppressionFingerprint
	nowNs := time.Now().UnixNano()
	shard := d.shardFor(fp)

	shard.mu.Lock()
	defer shard.mu.Unlock()

	if sup, ok := shard.cache[fp]; ok && sup.ExpireAtNs > nowNs {
		// Active suppression hit
		sup.Count++
		sup.LastSeenNs = nowNs

		if cfg.Mode == TTLSliding {
			sup.ExpireAtNs = nowNs + cfg.WindowNs
		}

		return AlertActionSuppress, &SuppressionInfo{
			Fingerprint: fp,
			Count:       sup.Count,
			FirstSeenNs: sup.FirstSeenNs,
			LastSeenNs:  sup.LastSeenNs,
		}
	}

	// New or expired entry — create fresh suppression state
	shard.cache[fp] = &ActiveSuppression{
		Fingerprint: fp,
		FirstSeenNs: nowNs,
		LastSeenNs:  nowNs,
		Count:       1,
		ExpireAtNs:  nowNs + cfg.WindowNs,
	}
	return AlertActionEmit, nil
}

// PurgeExpired removes suppression entries that have passed their expiry time.
// Call periodically (e.g., from a background goroutine) to prevent memory leaks.
func (d *AlertDeduplicator) PurgeExpired() int {
	nowNs := time.Now().UnixNano()
	total := 0
	for i := 0; i < dedupShardCount; i++ {
		s := &d.shards[i]
		s.mu.Lock()
		for fp, sup := range s.cache {
			if sup.ExpireAtNs <= nowNs {
				delete(s.cache, fp)
				total++
			}
		}
		s.mu.Unlock()
	}
	return total
}

// ActiveCount returns the total number of live suppression entries.
func (d *AlertDeduplicator) ActiveCount() int {
	total := 0
	for i := 0; i < dedupShardCount; i++ {
		s := &d.shards[i]
		s.mu.Lock()
		total += len(s.cache)
		s.mu.Unlock()
	}
	return total
}
