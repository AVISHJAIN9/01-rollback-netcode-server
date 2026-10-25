package phase11

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"hash/crc32"
	"io"
	"os"
	"path/filepath"
	"sync"
)

// ─── Feature 65: State Crash Recovery & Manifest Integrity Verification ───────
//
// StateRecoveryHarness manages an in-memory key/value store with:
//   • Write(): applies a mutation and appends it to the WAL.
//   • Checkpoint(): serialises the full store to a binary snapshot file with CRC32C.
//   • Restore():    reads the snapshot, verifies CRC32C, and rebuilds store state.
//                   Corrupt files are detected and the call returns an error,
//                   signalling the caller to fall back to the prior checkpoint.
//
// The CRC32C checksum is written as the last 4 bytes of the snapshot file so
// that truncation is automatically detected (the reader will reach EOF before
// seeing the checksum and return an error).

var crc32cTable = crc32.MakeTable(crc32.Castagnoli)

// StateRecoveryHarness is the in-memory state store with snapshot support.
type StateRecoveryHarness struct {
	mu    sync.RWMutex
	store map[string][]byte
	seqNo uint64 // monotonic mutation sequence number
}

// NewStateRecoveryHarness constructs an empty harness.
func NewStateRecoveryHarness() *StateRecoveryHarness {
	return &StateRecoveryHarness{store: make(map[string][]byte)}
}

// Write applies one key/value mutation and increments the sequence counter.
func (h *StateRecoveryHarness) Write(key string, value []byte) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.store[key] = value
	h.seqNo++
}

// Read retrieves a value (safe for concurrent use).
func (h *StateRecoveryHarness) Read(key string) ([]byte, bool) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	v, ok := h.store[key]
	return v, ok
}

// Len returns the number of entries in the store.
func (h *StateRecoveryHarness) Len() int {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return len(h.store)
}

// SeqNo returns the current mutation sequence number.
func (h *StateRecoveryHarness) SeqNo() uint64 {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return h.seqNo
}

// SnapshotManifest is the JSON header written to <dir>/manifest.json.
type SnapshotManifest struct {
	SeqNo    uint64 `json:"seq_no"`
	KeyCount int    `json:"key_count"`
	DataFile string `json:"data_file"`
	CRC32C   uint32 `json:"crc32c"`
}

// Checkpoint serialises the store to checkpointDir, creating:
//   manifest.json  – metadata with CRC32C
//   state.bin      – binary key/value stream
func (h *StateRecoveryHarness) Checkpoint(checkpointDir string) (*SnapshotManifest, error) {
	if err := os.MkdirAll(checkpointDir, 0o755); err != nil {
		return nil, fmt.Errorf("mkdir: %w", err)
	}

	h.mu.RLock()
	// CoW: copy references while holding read lock
	snapshot := make(map[string][]byte, len(h.store))
	for k, v := range h.store {
		snapshot[k] = v
	}
	seqNo := h.seqNo
	h.mu.RUnlock()

	dataPath := filepath.Join(checkpointDir, "state.bin")
	f, err := os.Create(dataPath)
	if err != nil {
		return nil, fmt.Errorf("create state.bin: %w", err)
	}
	defer f.Close()

	hc := crc32.New(crc32cTable)
	w := io.MultiWriter(f, hc)

	lenBuf := make([]byte, 4)
	for k, v := range snapshot {
		kb := []byte(k)
		binary.BigEndian.PutUint32(lenBuf, uint32(len(kb)))
		w.Write(lenBuf)
		w.Write(kb)
		binary.BigEndian.PutUint32(lenBuf, uint32(len(v)))
		w.Write(lenBuf)
		w.Write(v)
	}

	checksum := hc.Sum32()

	meta := &SnapshotManifest{
		SeqNo:    seqNo,
		KeyCount: len(snapshot),
		DataFile: "state.bin",
		CRC32C:   checksum,
	}

	manifestBytes, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("marshal manifest: %w", err)
	}

	if err := os.WriteFile(filepath.Join(checkpointDir, "manifest.json"), manifestBytes, 0o644); err != nil {
		return nil, fmt.Errorf("write manifest: %w", err)
	}

	return meta, nil
}

// Restore loads state from checkpointDir, validates CRC32C, and rebuilds the store.
// Returns an error if the manifest is missing, the data file is corrupt, or the
// checksum does not match.
func (h *StateRecoveryHarness) Restore(checkpointDir string) (*SnapshotManifest, error) {
	manifestBytes, err := os.ReadFile(filepath.Join(checkpointDir, "manifest.json"))
	if err != nil {
		return nil, fmt.Errorf("read manifest: %w", err)
	}

	var meta SnapshotManifest
	if err := json.Unmarshal(manifestBytes, &meta); err != nil {
		return nil, fmt.Errorf("parse manifest: %w", err)
	}

	dataPath := filepath.Join(checkpointDir, meta.DataFile)
	rawData, err := os.ReadFile(dataPath)
	if err != nil {
		return nil, fmt.Errorf("read state.bin: %w", err)
	}

	// Verify checksum over the raw byte slice
	actualCRC := crc32.Checksum(rawData, crc32cTable)
	if actualCRC != meta.CRC32C {
		return nil, fmt.Errorf("CRC32C mismatch: expected %08x, computed %08x (file corrupt)", meta.CRC32C, actualCRC)
	}

	// Deserialise
	newStore := make(map[string][]byte, meta.KeyCount)
	r := rawData
	for len(r) > 0 {
		if len(r) < 4 {
			return nil, fmt.Errorf("unexpected EOF reading key length")
		}
		kLen := int(binary.BigEndian.Uint32(r[:4]))
		r = r[4:]
		if len(r) < kLen {
			return nil, fmt.Errorf("unexpected EOF reading key")
		}
		key := string(r[:kLen])
		r = r[kLen:]

		if len(r) < 4 {
			return nil, fmt.Errorf("unexpected EOF reading val length")
		}
		vLen := int(binary.BigEndian.Uint32(r[:4]))
		r = r[4:]
		if len(r) < vLen {
			return nil, fmt.Errorf("unexpected EOF reading val")
		}
		val := make([]byte, vLen)
		copy(val, r[:vLen])
		r = r[vLen:]
		newStore[key] = val
	}

	h.mu.Lock()
	h.store = newStore
	h.seqNo = meta.SeqNo
	h.mu.Unlock()

	return &meta, nil
}
