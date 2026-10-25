package phase17

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
)

// ─── Feature 102: Append-Only Disk Spill Overflow Buffer ─────────────────────
//
// DiskSpillBuffer serialises CanonicalEvent records to rolling segment files
// on disk when in-memory ring buffers are saturated.  Background draining
// reads spilled segments back into memory once pressure subsides.
//
// Segment file layout:
//   [4-byte big-endian record length] [JSON-encoded CanonicalEvent bytes]
//   repeated for each spilled event.
//
// On full drain, segment files are deleted to reclaim disk space.

// CanonicalEvent (reuse from phase16 conceptually; defined locally here for self-containment).
type SpillEvent struct {
	EventID     string
	TimestampNs int64
	EntityID    string
	EventType   string
	Fields      map[string]string
}

// DiskSpillBuffer manages overflow segments on disk.
type DiskSpillBuffer struct {
	mu          sync.Mutex
	dir         string        // directory where segment files are written
	maxSegSize  int           // maximum bytes per segment before rotating
	segments    []string      // ordered list of active segment file paths
	segOffsets  []int         // read position within each segment
	curFile     *os.File      // currently open write segment
	curSize     int           // bytes written to current segment
	spillCount  atomic.Int64  // total events spilled to disk
	drainCount  atomic.Int64  // total events drained back from disk
}

// NewDiskSpillBuffer constructs a buffer writing segments under dir.
func NewDiskSpillBuffer(dir string, maxSegBytes int) (*DiskSpillBuffer, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, fmt.Errorf("create spill dir %s: %w", dir, err)
	}
	return &DiskSpillBuffer{
		dir:        dir,
		maxSegSize: maxSegBytes,
	}, nil
}
// Spill serialises event to the current disk segment.
// Returns the byte offset at which the record starts.
func (b *DiskSpillBuffer) Spill(event *SpillEvent) (int64, error) {
	b.mu.Lock()
	defer b.mu.Unlock()

	payload, err := json.Marshal(event)
	if err != nil {
		return 0, fmt.Errorf("marshal event: %w", err)
	}

	// Rotate segment if needed
	if b.curFile == nil || b.curSize+len(payload)+4 > b.maxSegSize {
		if err := b.rotateSegment(); err != nil {
			return 0, err
		}
	}

	var lenBuf [4]byte
	binary.BigEndian.PutUint32(lenBuf[:], uint32(len(payload)))

	offset, _ := b.curFile.Seek(0, 1) // current position

	if _, err := b.curFile.Write(lenBuf[:]); err != nil {
		return 0, err
	}
	if _, err := b.curFile.Write(payload); err != nil {
		return 0, err
	}
	b.curSize += 4 + len(payload)
	b.spillCount.Add(1)

	return offset, nil
}

// rotateSegment closes the current segment and opens a new one.
// Must be called with b.mu held.
func (b *DiskSpillBuffer) rotateSegment() error {
	if b.curFile != nil {
		b.curFile.Close()
		b.curFile = nil
	}

	fname := fmt.Sprintf("spill-%020d.seg", b.spillCount.Load())
	fpath := filepath.Join(b.dir, fname)
	f, err := os.Create(fpath)
	if err != nil {
		return fmt.Errorf("create segment %s: %w", fpath, err)
	}

	b.curFile = f
	b.curSize = 0
	b.segments = append(b.segments, fpath)
	b.segOffsets = append(b.segOffsets, 0)
	return nil
}

// FlushCurrentSegment syncs the current write segment to disk and closes it,
// making it available for draining.
func (b *DiskSpillBuffer) FlushCurrentSegment() error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.curFile != nil {
		name := b.curFile.Name()
		if err := b.curFile.Sync(); err != nil {
			return err
		}
		b.curFile.Close()
		b.curFile = nil
		b.curSize = 0
		// Ensure the flushed file is in the segment list
		alreadyListed := false
		for _, s := range b.segments {
			if s == name {
				alreadyListed = true
				break
			}
		}
		if !alreadyListed {
			b.segments = append(b.segments, name)
			b.segOffsets = append(b.segOffsets, 0)
		}
	}
	return nil
}

// DrainSpill reads up to maxBatch events from the oldest segment and returns them.
// Uses per-segment read offsets so a segment is only deleted when fully consumed.
// Returns (nil, nil) when no segments remain.
func (b *DiskSpillBuffer) DrainSpill(maxBatch int) ([]*SpillEvent, error) {
	b.mu.Lock()
	defer b.mu.Unlock()

	if len(b.segments) == 0 {
		return nil, nil
	}

	segPath := b.segments[0]
	startOffset := b.segOffsets[0]

	data, err := os.ReadFile(segPath)
	if err != nil {
		return nil, fmt.Errorf("read segment %s: %w", segPath, err)
	}

	var events []*SpillEvent
	pos := startOffset
	for pos < len(data) && len(events) < maxBatch {
		if pos+4 > len(data) {
			break
		}
		recLen := int(binary.BigEndian.Uint32(data[pos : pos+4]))
		pos += 4

		if pos+recLen > len(data) {
			break
		}

		var ev SpillEvent
		if err := json.Unmarshal(data[pos:pos+recLen], &ev); err != nil {
			return nil, fmt.Errorf("unmarshal record at offset %d: %w", pos, err)
		}
		events = append(events, &ev)
		pos += recLen
	}

	b.drainCount.Add(int64(len(events)))

	if pos >= len(data) {
		// Fully drained — remove segment
		os.Remove(segPath)
		b.segments = b.segments[1:]
		b.segOffsets = b.segOffsets[1:]
	} else {
		// Partial drain — advance read offset
		b.segOffsets[0] = pos
	}

	return events, nil
}

// SpillCount returns the total number of events written to disk.
func (b *DiskSpillBuffer) SpillCount() int64 { return b.spillCount.Load() }

// DrainCount returns the total number of events read back from disk.
func (b *DiskSpillBuffer) DrainCount() int64 { return b.drainCount.Load() }

// PendingSegments returns the count of undraned segment files.
func (b *DiskSpillBuffer) PendingSegments() int {
	b.mu.Lock()
	defer b.mu.Unlock()
	return len(b.segments)
}
