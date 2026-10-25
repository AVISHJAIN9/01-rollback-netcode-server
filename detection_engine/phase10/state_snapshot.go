package phase10

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"hash/crc32"
	"os"
	"path/filepath"
	"sync"
	"time"
)

// ─── Consistent Point-in-Time State Snapshotting (Feature 58) ────────────────
//
// StateShard is the unit of snapshotting.  CreateSnapshot takes a copy of each
// shard under its read-lock (CoW semantics for the map), streams binary-encoded
// key/value pairs to disk, computes CRC32C per shard, and writes a META.json.
//
// Binary encoding per entry:
//   [4 bytes big-endian key length] [key bytes]
//   [4 bytes big-endian val length] [val bytes]

// StateShard holds one shard of keyed state.
type StateShard struct {
	mu   sync.RWMutex
	data map[string][]byte
}

// NewStateShard creates an empty shard.
func NewStateShard() *StateShard {
	return &StateShard{data: make(map[string][]byte)}
}

// Put stores value under key.
func (s *StateShard) Put(key string, value []byte) {
	s.mu.Lock()
	s.data[key] = value
	s.mu.Unlock()
}

// Get retrieves the value for key, returning nil if absent.
func (s *StateShard) Get(key string) ([]byte, bool) {
	s.mu.RLock()
	v, ok := s.data[key]
	s.mu.RUnlock()
	return v, ok
}

// Len returns the number of keys in this shard.
func (s *StateShard) Len() int {
	s.mu.RLock()
	n := len(s.data)
	s.mu.RUnlock()
	return n
}

// snapshot returns a shallow copy of the shard's data map under read lock.
func (s *StateShard) snapshot() map[string][]byte {
	s.mu.RLock()
	defer s.mu.RUnlock()
	cp := make(map[string][]byte, len(s.data))
	for k, v := range s.data {
		cp[k] = v
	}
	return cp
}

// ShardMeta captures per-shard snapshot metadata.
type ShardMeta struct {
	ShardIndex int    `json:"shard_index"`
	KeyCount   int    `json:"key_count"`
	CRC32C     uint32 `json:"crc32c"`
	FilePath   string `json:"file_path"`
}

// SnapshotMeta is written as META.json in the checkpoint directory.
type SnapshotMeta struct {
	SnapshotID  uint64      `json:"snapshot_id"`
	CreatedAtNs int64       `json:"created_at_ns"`
	ShardCount  int         `json:"shard_count"`
	Shards      []ShardMeta `json:"shards"`
}

// SnapshotManager creates and restores versioned checkpoint artifacts.
type SnapshotManager struct{}

// NewSnapshotManager creates a SnapshotManager.
func NewSnapshotManager() *SnapshotManager { return &SnapshotManager{} }

// crc32cTable is the Castagnoli polynomial table used for CRC32C.
var crc32cTable = crc32.MakeTable(crc32.Castagnoli)

// CreateSnapshot snapshots all shards into checkpointDir/snapshotID/.
// Each shard's data is serialised to a binary file; a META.json manifest is written.
func (sm *SnapshotManager) CreateSnapshot(checkpointDir string, snapshotID uint64, shards []*StateShard) (*SnapshotMeta, error) {
	dir := filepath.Join(checkpointDir, fmt.Sprintf("%020d", snapshotID))
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, fmt.Errorf("mkdir %s: %w", dir, err)
	}

	meta := &SnapshotMeta{
		SnapshotID:  snapshotID,
		CreatedAtNs: time.Now().UnixNano(),
		ShardCount:  len(shards),
		Shards:      make([]ShardMeta, len(shards)),
	}

	for i, shard := range shards {
		snap := shard.snapshot() // CoW copy under read lock

		fname := fmt.Sprintf("shard_%04d.bin", i)
		fpath := filepath.Join(dir, fname)

		crc, keyCount, err := writeShardFile(fpath, snap)
		if err != nil {
			return nil, fmt.Errorf("write shard %d: %w", i, err)
		}

		meta.Shards[i] = ShardMeta{
			ShardIndex: i,
			KeyCount:   keyCount,
			CRC32C:     crc,
			FilePath:   fname,
		}
	}

	// Write manifest
	metaPath := filepath.Join(dir, "META.json")
	metaBytes, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("marshal meta: %w", err)
	}
	if err := os.WriteFile(metaPath, metaBytes, 0o644); err != nil {
		return nil, fmt.Errorf("write meta: %w", err)
	}

	return meta, nil
}

// writeShardFile serialises snap to fpath, returning (crc32c, keyCount, error).
func writeShardFile(fpath string, snap map[string][]byte) (uint32, int, error) {
	f, err := os.Create(fpath)
	if err != nil {
		return 0, 0, err
	}
	defer f.Close()

	h := crc32.New(crc32cTable)

	buf := make([]byte, 4)
	writeField := func(data []byte) error {
		binary.BigEndian.PutUint32(buf, uint32(len(data)))
		if _, err := f.Write(buf); err != nil {
			return err
		}
		h.Write(buf)
		if _, err := f.Write(data); err != nil {
			return err
		}
		h.Write(data)
		return nil
	}

	for k, v := range snap {
		if err := writeField([]byte(k)); err != nil {
			return 0, 0, err
		}
		if err := writeField(v); err != nil {
			return 0, 0, err
		}
	}

	return h.Sum32(), len(snap), nil
}
