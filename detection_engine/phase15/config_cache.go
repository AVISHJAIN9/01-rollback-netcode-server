package phase15

import (
	"sync"
	"sync/atomic"
	"unsafe"
)

// ─── Feature 90: Tiered Read-Through Configuration Cache ─────────────────────
//
// ConfigurationCache provides a lock-free RCU (Read-Copy-Update) cache over
// the resolved EngineConfig.  The hot read path is a single atomic.Pointer load
// (≤ 15 ns on modern hardware).  The write path builds a full copy of the
// current map, mutates the copy, and atomically swaps the pointer — zero
// blocking for any concurrent reader.

// ConfigValue is a tagged union covering the types stored in the cache.
type ConfigValue struct {
	IntVal    int
	StrVal    string
	BoolVal   bool
	IsInt     bool
	IsStr     bool
	IsBool    bool
}

func IntConfigVal(n int) ConfigValue    { return ConfigValue{IntVal: n, IsInt: true} }
func StrConfigVal(s string) ConfigValue { return ConfigValue{StrVal: s, IsStr: true} }
func BoolConfigVal(b bool) ConfigValue  { return ConfigValue{BoolVal: b, IsBool: true} }

// configSnapshot is the immutable snapshot held by the atomic pointer.
type configSnapshot struct {
	entries map[string]ConfigValue
}

// ConfigurationCache is a lock-free RCU cache of string→ConfigValue mappings.
type ConfigurationCache struct {
	// ptr holds an *configSnapshot via unsafe.Pointer for atomic access without generics.
	ptr unsafe.Pointer // *configSnapshot
	// writeMu serialises concurrent writers (readers need no lock).
	writeMu sync.Mutex
}

// NewConfigurationCache constructs a cache pre-populated from an EngineConfig.
func NewConfigurationCache(cfg EngineConfig) *ConfigurationCache {
	snap := buildSnapshot(cfg)
	c := &ConfigurationCache{}
	atomic.StorePointer(&c.ptr, unsafe.Pointer(snap))
	return c
}

// buildSnapshot converts an EngineConfig into a flat ConfigValue map.
func buildSnapshot(cfg EngineConfig) *configSnapshot {
	m := make(map[string]ConfigValue, 16)
	m["ingestion.ring_buffer_size"] = IntConfigVal(cfg.Ingestion.RingBufferSize)
	m["ingestion.watermark_delay_ms"] = IntConfigVal(cfg.Ingestion.WatermarkDelayMs)
	m["ingestion.max_workers"] = IntConfigVal(cfg.Ingestion.MaxWorkers)
	m["ingestion.min_workers"] = IntConfigVal(cfg.Ingestion.MinWorkers)
	m["window.default_window_ms"] = IntConfigVal(cfg.Window.DefaultWindowMs)
	m["window.allowed_lateness_ms"] = IntConfigVal(cfg.Window.AllowedLatenessMs)
	m["window.max_active_windows"] = IntConfigVal(cfg.Window.MaxActiveWindows)
	m["state.max_memory_mb"] = IntConfigVal(cfg.State.MaxMemoryMB)
	m["state.shard_count"] = IntConfigVal(cfg.State.ShardCount)
	m["state.default_ttl_seconds"] = IntConfigVal(cfg.State.DefaultTTLSeconds)
	m["alerting.max_alert_queue_size"] = IntConfigVal(cfg.Alerting.MaxAlertQueueSize)
	m["alerting.dedup_window_seconds"] = IntConfigVal(cfg.Alerting.DeduplicationWindowSeconds)
	m["alerting.max_suppressed_per_alert"] = IntConfigVal(cfg.Alerting.MaxSuppressedPerAlert)
	return &configSnapshot{entries: m}
}

// loadSnapshot is the lock-free hot-path read that loads the current snapshot.
func (c *ConfigurationCache) loadSnapshot() *configSnapshot {
	return (*configSnapshot)(atomic.LoadPointer(&c.ptr))
}

// Get retrieves the ConfigValue for key.  Lock-free: single atomic pointer load.
func (c *ConfigurationCache) Get(key string) (ConfigValue, bool) {
	snap := c.loadSnapshot()
	v, ok := snap.entries[key]
	return v, ok
}

// GetInt is a convenience wrapper returning the int value for key.
func (c *ConfigurationCache) GetInt(key string) (int, bool) {
	v, ok := c.Get(key)
	if !ok || !v.IsInt {
		return 0, false
	}
	return v.IntVal, true
}

// Set atomically installs a new value for key using RCU copy-and-swap.
// At most one writer runs at a time (writeMu); readers are never blocked.
func (c *ConfigurationCache) Set(key string, val ConfigValue) {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()

	old := c.loadSnapshot()

	// Copy the existing map
	newMap := make(map[string]ConfigValue, len(old.entries)+1)
	for k, v := range old.entries {
		newMap[k] = v
	}
	newMap[key] = val

	newSnap := &configSnapshot{entries: newMap}
	atomic.StorePointer(&c.ptr, unsafe.Pointer(newSnap))
}

// Invalidate removes key from the cache (next Get returns (zero, false)).
func (c *ConfigurationCache) Invalidate(key string) {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()

	old := c.loadSnapshot()
	newMap := make(map[string]ConfigValue, len(old.entries))
	for k, v := range old.entries {
		if k != key {
			newMap[k] = v
		}
	}
	atomic.StorePointer(&c.ptr, unsafe.Pointer(&configSnapshot{entries: newMap}))
}

// Reload atomically replaces the entire cache with a new EngineConfig snapshot.
func (c *ConfigurationCache) Reload(cfg EngineConfig) {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	snap := buildSnapshot(cfg)
	atomic.StorePointer(&c.ptr, unsafe.Pointer(snap))
}
