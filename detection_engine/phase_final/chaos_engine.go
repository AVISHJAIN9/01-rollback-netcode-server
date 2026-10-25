package phase_final

import (
	"encoding/json"
	"fmt"
	"hash/crc32"
	"math/rand"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"
)

// ─── Features 139–144: Chaos Engineering, Fault Recovery & Convergence ────────

// ─── Feature 139: Chaos Fault Injector ───────────────────────────────────────

// ChaosFaultInjector wraps operations with configurable fault injection.
type ChaosFaultInjector struct {
	mu          sync.RWMutex
	dropRate    float64 // fraction of calls to silently drop [0,1]
	latencyMs   int64   // additional latency in ms (0 = none)
	corruptRate float64 // fraction of payloads to corrupt [0,1]
	rng         *rand.Rand
	injected    atomic.Int64 // total faults injected
}

// NewChaosFaultInjector constructs an injector (initially no-op).
func NewChaosFaultInjector() *ChaosFaultInjector {
	return &ChaosFaultInjector{rng: rand.New(rand.NewSource(time.Now().UnixNano()))}
}

// Configure sets the fault parameters atomically.
func (c *ChaosFaultInjector) Configure(dropRate float64, latencyMs int64, corruptRate float64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.dropRate = dropRate
	c.latencyMs = latencyMs
	c.corruptRate = corruptRate
}

// ShouldDrop returns true when this call should be dropped.
func (c *ChaosFaultInjector) ShouldDrop() bool {
	c.mu.RLock()
	rate := c.dropRate
	c.mu.RUnlock()
	if rate == 0 {
		return false
	}
	c.mu.Lock()
	drop := c.rng.Float64() < rate
	c.mu.Unlock()
	if drop {
		c.injected.Add(1)
	}
	return drop
}

// InjectLatency sleeps for the configured latency (if any).
func (c *ChaosFaultInjector) InjectLatency() {
	c.mu.RLock()
	ms := c.latencyMs
	c.mu.RUnlock()
	if ms > 0 {
		time.Sleep(time.Duration(ms) * time.Millisecond)
		c.injected.Add(1)
	}
}

// MaybeCorrupt randomly corrupts data if corruptRate > 0.
func (c *ChaosFaultInjector) MaybeCorrupt(data []byte) []byte {
	c.mu.Lock()
	rate := c.corruptRate
	corrupt := c.rng.Float64() < rate
	c.mu.Unlock()
	if !corrupt || len(data) == 0 {
		return data
	}
	out := make([]byte, len(data))
	copy(out, data)
	out[0] ^= 0xFF
	c.injected.Add(1)
	return out
}

// InjectedCount returns total faults injected since construction.
func (c *ChaosFaultInjector) InjectedCount() int64 { return c.injected.Load() }

// ─── Feature 140: WAL Crash Recovery & Verifier ───────────────────────────────

// WALRecord is one mutation entry written to the WAL.
type WALRecord struct {
	SeqNo uint64 `json:"seq"`
	Key   string `json:"key"`
	Value int64  `json:"value"`
}

// walFileEntry wraps a WALRecord with its CRC32C checksum.
type walFileEntry struct {
	Record   WALRecord `json:"record"`
	Checksum uint32    `json:"crc32c"`
}

var crc32cTableFinal = crc32.MakeTable(crc32.Castagnoli)

// WALEngine writes mutations to disk and replays them on cold restart.
type WALEngine struct {
	mu       sync.Mutex
	dir      string
	seqNo    uint64
	state    map[string]int64
	entries  []walFileEntry
}

// NewWALEngine constructs a WAL engine writing to dir.
func NewWALEngine(dir string) (*WALEngine, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	return &WALEngine{
		dir:   dir,
		state: make(map[string]int64),
	}, nil
}

// Write appends a mutation to the in-memory WAL and applies it to state.
func (w *WALEngine) Write(key string, value int64) {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.seqNo++
	rec := WALRecord{SeqNo: w.seqNo, Key: key, Value: value}
	payload, _ := json.Marshal(rec)
	crc := crc32.Checksum(payload, crc32cTableFinal)
	w.entries = append(w.entries, walFileEntry{Record: rec, Checksum: crc})
	w.state[key] += value
}

// Checkpoint flushes the WAL to disk.
func (w *WALEngine) Checkpoint(path string) error {
	w.mu.Lock()
	defer w.mu.Unlock()

	var lines []byte
	for _, e := range w.entries {
		line, err := json.Marshal(e)
		if err != nil {
			return err
		}
		lines = append(lines, line...)
		lines = append(lines, '\n')
	}
	return os.WriteFile(path, lines, 0o644)
}

// RecoverFromWAL replays a checkpoint file, rejecting entries with bad checksums.
// Returns the recovered state and any checksum errors.
func RecoverFromWAL(path string) (map[string]int64, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read WAL: %w", err)
	}

	state := make(map[string]int64)
	var seenCRCError bool

	for _, line := range splitLines(data) {
		if len(line) == 0 {
			continue
		}
		var entry walFileEntry
		if err := json.Unmarshal(line, &entry); err != nil {
			continue
		}
		payload, _ := json.Marshal(entry.Record)
		actualCRC := crc32.Checksum(payload, crc32cTableFinal)
		if actualCRC != entry.Checksum {
			seenCRCError = true
			continue // skip corrupt entry
		}
		state[entry.Record.Key] += entry.Record.Value
	}

	if seenCRCError {
		return state, fmt.Errorf("WAL contained CRC32C errors; corrupted entries were skipped")
	}
	return state, nil
}

func splitLines(data []byte) [][]byte {
	var lines [][]byte
	start := 0
	for i, b := range data {
		if b == '\n' {
			if i > start {
				lines = append(lines, data[start:i])
			}
			start = i + 1
		}
	}
	if start < len(data) {
		lines = append(lines, data[start:])
	}
	return lines
}

// StateSnapshot returns a copy of the current in-memory state.
func (w *WALEngine) StateSnapshot() map[string]int64 {
	w.mu.Lock()
	defer w.mu.Unlock()
	cp := make(map[string]int64, len(w.state))
	for k, v := range w.state {
		cp[k] = v
	}
	return cp
}

// ─── Feature 141: Split-Brain Fencing ────────────────────────────────────────

// QuorumFencer enforces majority quorum for partition write operations.
type QuorumFencer struct {
	totalNodes int
	aliveNodes atomic.Int32
}

// NewQuorumFencer constructs a fencer for a cluster of totalNodes nodes.
func NewQuorumFencer(totalNodes int) *QuorumFencer {
	f := &QuorumFencer{totalNodes: totalNodes}
	f.aliveNodes.Store(int32(totalNodes))
	return f
}

// QuorumSize returns the minimum number of nodes needed for consensus.
func (f *QuorumFencer) QuorumSize() int { return f.totalNodes/2 + 1 }

// MarkNodeFailed reduces the alive-node count.
func (f *QuorumFencer) MarkNodeFailed() { f.aliveNodes.Add(-1) }

// HasQuorum returns true when the alive-node count meets quorum.
func (f *QuorumFencer) HasQuorum() bool {
	return int(f.aliveNodes.Load()) >= f.QuorumSize()
}

// AllowWrite returns true only when quorum is satisfied.
func (f *QuorumFencer) AllowWrite() bool { return f.HasQuorum() }

// AliveCount returns the current alive node count.
func (f *QuorumFencer) AliveCount() int { return int(f.aliveNodes.Load()) }

// ─── Feature 142–144: Catch-up, Replay, Self-Health ──────────────────────────

// CatchUpManager activates fast-path processing to drain a large backlog.
type CatchUpManager struct {
	active          atomic.Bool
	slaThreshold    int64 // events behind before catch-up activates
	currentLag      atomic.Int64
	enrichmentDisabled atomic.Bool
}

// NewCatchUpManager constructs a manager with the given SLA threshold.
func NewCatchUpManager(slaThreshold int64) *CatchUpManager {
	return &CatchUpManager{slaThreshold: slaThreshold}
}

// UpdateLag sets the current consumer lag and enables/disables catch-up mode.
func (cm *CatchUpManager) UpdateLag(lag int64) {
	cm.currentLag.Store(lag)
	if lag > cm.slaThreshold {
		cm.active.Store(true)
		cm.enrichmentDisabled.Store(true)
	} else {
		cm.active.Store(false)
		cm.enrichmentDisabled.Store(false)
	}
}

// IsActive returns true when catch-up mode is engaged.
func (cm *CatchUpManager) IsActive() bool { return cm.active.Load() }

// IsEnrichmentDisabled returns true during catch-up (non-essential ops suppressed).
func (cm *CatchUpManager) IsEnrichmentDisabled() bool { return cm.enrichmentDisabled.Load() }

// HealthSnapshot captures a point-in-time engine health summary.
type HealthSnapshot struct {
	TimestampNs    int64
	QueueDepth     int
	WorkerCount    int
	MemoryUsedMB   float64
	CatchUpActive  bool
	Description    string
}

// SelfHealthMonitor periodically captures engine state for diagnostics.
type SelfHealthMonitor struct {
	mu        sync.Mutex
	snapshots []HealthSnapshot
}

// NewSelfHealthMonitor constructs a monitor.
func NewSelfHealthMonitor() *SelfHealthMonitor { return &SelfHealthMonitor{} }

// CaptureSnapshot stores a health snapshot.
func (m *SelfHealthMonitor) CaptureSnapshot(snap HealthSnapshot) {
	snap.TimestampNs = time.Now().UnixNano()
	m.mu.Lock()
	m.snapshots = append(m.snapshots, snap)
	m.mu.Unlock()
}

// LatestSnapshot returns the most recent health snapshot, or zero value if empty.
func (m *SelfHealthMonitor) LatestSnapshot() (HealthSnapshot, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if len(m.snapshots) == 0 {
		return HealthSnapshot{}, false
	}
	return m.snapshots[len(m.snapshots)-1], true
}

// SnapshotCount returns total captured snapshots.
func (m *SelfHealthMonitor) SnapshotCount() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.snapshots)
}

// walCheckpointPath returns a deterministic path for a WAL checkpoint file.
func walCheckpointPath(dir, name string) string {
	return filepath.Join(dir, name+".wal")
}
