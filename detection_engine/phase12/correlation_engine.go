package phase12

import (
	"encoding/binary"
	"hash/fnv"
	"sync"
)

// ─── Feature 72: Deterministic Cross-Alert Correlation ID Graph Linking ───────
//
// CorrelationEngine assigns a deterministic 64-bit correlation token to alerts
// that share entities within a configurable temporal epoch bucket.
//
// CorrelationID = FNV-64a( tenantID ‖ 0x00 ‖ rootEntity ‖ 0x00 ‖ epochBucket )
//
// rootEntity:  the canonical entity anchor (e.g., the device hostname or the
//              first lateral-movement source observed in the epoch).
// epochBucket: floor(alert.CreatedTimeNs / epochWidthNs) – all alerts within
//              the same epoch window share the same bucket integer.
//
// When an alert arrives whose entity is already tracked within the current
// epoch, it is assigned the SAME correlation ID as the entity's first alert.
// This enables multi-stage intrusion sequences to be linked by a single token.

// entityCorrelationKey maps one tracked entity within an epoch.
type entityCorrelationKey struct {
	tenantID    string
	rootEntity  string
	epochBucket int64
}

// CorrelationEngine tracks entity–epoch relationships and assigns shared tokens.
type CorrelationEngine struct {
	mu          sync.Mutex
	epochWidthNs int64                               // e.g., 600s = 10-minute epoch
	tokens      map[entityCorrelationKey]uint64      // entity+epoch → correlation ID
}

// NewCorrelationEngine constructs an engine with the given epoch width.
// A 10-minute epoch (600s) means alerts from the same entity within any
// consecutive 10-minute window receive the same correlation ID.
func NewCorrelationEngine(epochWidthNs int64) *CorrelationEngine {
	return &CorrelationEngine{
		epochWidthNs: epochWidthNs,
		tokens:       make(map[entityCorrelationKey]uint64),
	}
}

// computeToken derives the deterministic 64-bit correlation ID.
func computeToken(tenantID, rootEntity string, epochBucket int64) uint64 {
	h := fnv.New64a()
	h.Write([]byte(tenantID))
	h.Write([]byte{0x00})
	h.Write([]byte(rootEntity))
	h.Write([]byte{0x00})
	var buf [8]byte
	binary.LittleEndian.PutUint64(buf[:], uint64(epochBucket))
	h.Write(buf[:])
	return h.Sum64()
}

// AssignCorrelationID assigns a deterministic correlation token to alert and
// sets alert.CorrelationID.  Two alerts on the same entity within the same
// epoch bucket always receive the same token.
func (ce *CorrelationEngine) AssignCorrelationID(alert *NormalizedAlert) uint64 {
	epochBucket := alert.CreatedTimeNs / ce.epochWidthNs
	rootEntity := rootEntityFor(alert)

	key := entityCorrelationKey{
		tenantID:    alert.TenantID,
		rootEntity:  rootEntity,
		epochBucket: epochBucket,
	}

	ce.mu.Lock()
	defer ce.mu.Unlock()

	if existing, ok := ce.tokens[key]; ok {
		alert.CorrelationID = existing
		return existing
	}

	token := computeToken(alert.TenantID, rootEntity, epochBucket)
	ce.tokens[key] = token
	alert.CorrelationID = token
	return token
}

// rootEntityFor derives the canonical entity anchor for an alert.
// Preference order: Hostname → SourceIP → TenantID.
func rootEntityFor(alert *NormalizedAlert) string {
	if alert.Device.Hostname != "" {
		return alert.Device.Hostname
	}
	if alert.Network.SourceIP != "" {
		return alert.Network.SourceIP
	}
	return alert.TenantID
}

// PurgeEpoch removes all correlation tokens whose epochBucket is older than
// the one containing nowNs minus retentionEpochs epochs.
// Call periodically to prevent unbounded growth.
func (ce *CorrelationEngine) PurgeEpoch(nowNs int64, retentionEpochs int64) int {
	cutoffBucket := (nowNs/ce.epochWidthNs) - retentionEpochs

	ce.mu.Lock()
	defer ce.mu.Unlock()

	removed := 0
	for k := range ce.tokens {
		if k.epochBucket < cutoffBucket {
			delete(ce.tokens, k)
			removed++
		}
	}
	return removed
}

// ActiveTokenCount returns the current number of tracked correlation entries.
func (ce *CorrelationEngine) ActiveTokenCount() int {
	ce.mu.Lock()
	defer ce.mu.Unlock()
	return len(ce.tokens)
}
