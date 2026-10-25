package phase10

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"hash/crc32"
	"io"
	"os"
	"path/filepath"
)

// ─── State Restore & Replay Engine (Feature 59) ──────────────────────────────
//
// RestoreFromSnapshot reads META.json, validates per-shard CRC32C checksums,
// and deserialises the binary key/value streams back into StateShard maps.
// If any shard checksum fails the function returns an error so the caller can
// fall back to a prior valid checkpoint.

// StateRestoreEngine restores keyed state from checkpoint artifacts.
type StateRestoreEngine struct{}

// NewStateRestoreEngine constructs a restore engine.
func NewStateRestoreEngine() *StateRestoreEngine { return &StateRestoreEngine{} }

// RestoreFromSnapshot loads the snapshot identified by snapshotID from
// checkpointDir into the provided shards slice.  Shard count must match.
func (re *StateRestoreEngine) RestoreFromSnapshot(checkpointDir string, snapshotID uint64, shards []*StateShard) (*SnapshotMeta, error) {
	dir := filepath.Join(checkpointDir, fmt.Sprintf("%020d", snapshotID))

	// Read and parse manifest
	metaPath := filepath.Join(dir, "META.json")
	metaBytes, err := os.ReadFile(metaPath)
	if err != nil {
		return nil, fmt.Errorf("read META.json: %w", err)
	}

	var meta SnapshotMeta
	if err := json.Unmarshal(metaBytes, &meta); err != nil {
		return nil, fmt.Errorf("parse META.json: %w", err)
	}

	if meta.ShardCount != len(shards) {
		return nil, fmt.Errorf("shard count mismatch: snapshot has %d, store has %d",
			meta.ShardCount, len(shards))
	}

	for _, sm := range meta.Shards {
		fpath := filepath.Join(dir, sm.FilePath)
		entries, actualCRC, err := readShardFile(fpath)
		if err != nil {
			return nil, fmt.Errorf("read shard %d: %w", sm.ShardIndex, err)
		}

		if actualCRC != sm.CRC32C {
			return nil, fmt.Errorf("CRC32C mismatch shard %d: expected %08x, got %08x",
				sm.ShardIndex, sm.CRC32C, actualCRC)
		}

		shard := shards[sm.ShardIndex]
		shard.mu.Lock()
		shard.data = entries
		shard.mu.Unlock()
	}

	return &meta, nil
}

// readShardFile deserialises a binary shard file, recomputing CRC32C on the fly.
// Returns (map, crc32c, error).
func readShardFile(fpath string) (map[string][]byte, uint32, error) {
	f, err := os.Open(fpath)
	if err != nil {
		return nil, 0, err
	}
	defer f.Close()

	h := crc32.New(crc32cTable)
	result := make(map[string][]byte)

	lenBuf := make([]byte, 4)

	readField := func() ([]byte, error) {
		if _, err := io.ReadFull(f, lenBuf); err != nil {
			if err == io.EOF {
				return nil, io.EOF
			}
			return nil, err
		}
		h.Write(lenBuf)
		n := binary.BigEndian.Uint32(lenBuf)
		data := make([]byte, n)
		if _, err := io.ReadFull(f, data); err != nil {
			return nil, err
		}
		h.Write(data)
		return data, nil
	}

	for {
		key, err := readField()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, 0, fmt.Errorf("read key: %w", err)
		}

		val, err := readField()
		if err != nil {
			return nil, 0, fmt.Errorf("read val: %w", err)
		}

		result[string(key)] = val
	}

	return result, h.Sum32(), nil
}
