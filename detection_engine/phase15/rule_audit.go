package phase15

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"
	"sync"
	"time"
)

// ─── Feature 89: Cryptographically Chained Rule Change Audit Ledger ──────────
//
// Every administrative mutation is recorded in an append-only ledger.
// H_i = SHA-256(H_{i-1} ‖ Seq ‖ Timestamp ‖ Actor ‖ Action ‖ RuleID ‖ DiffSummary)
// H_0 uses the 32-byte zero vector as the sentinel previous hash.
//
// VerifyLedger re-derives every hash and confirms chain continuity.

// AuditAction classifies the type of rule mutation.
type AuditAction int

const (
	AuditActionActivate   AuditAction = iota // new rule added to active set
	AuditActionDeactivate                    // rule removed from active set
	AuditActionEdit                          // rule modified in-place
	AuditActionRollback                      // active ruleset rolled back to prior snapshot
	AuditActionThresholdUpdate               // threshold value updated without AST recompile
)

func (a AuditAction) String() string {
	switch a {
	case AuditActionActivate:
		return "ACTIVATE"
	case AuditActionDeactivate:
		return "DEACTIVATE"
	case AuditActionEdit:
		return "EDIT"
	case AuditActionRollback:
		return "ROLLBACK"
	case AuditActionThresholdUpdate:
		return "THRESHOLD_UPDATE"
	default:
		return fmt.Sprintf("AuditAction(%d)", int(a))
	}
}

// RuleAuditEntry is one immutable record in the audit chain.
type RuleAuditEntry struct {
	Seq         uint64
	TimestampNs int64
	Actor       string
	Action      AuditAction
	RuleID      string
	DiffSummary string
	PrevHash    [32]byte
	EntryHash   [32]byte
}

// computeEntryHash derives the SHA-256 digest over all fields plus the previous hash.
func computeEntryHash(
	prevHash [32]byte,
	seq uint64,
	timestampNs int64,
	actor, action, ruleID, diff string,
) [32]byte {
	h := sha256.New()
	h.Write(prevHash[:])

	var u64buf [8]byte
	binary.BigEndian.PutUint64(u64buf[:], seq)
	h.Write(u64buf[:])

	binary.BigEndian.PutUint64(u64buf[:], uint64(timestampNs))
	h.Write(u64buf[:])

	h.Write([]byte(actor))
	h.Write([]byte{0x00})
	h.Write([]byte(action))
	h.Write([]byte{0x00})
	h.Write([]byte(ruleID))
	h.Write([]byte{0x00})
	h.Write([]byte(diff))

	var out [32]byte
	copy(out[:], h.Sum(nil))
	return out
}

// AuditIntegrityError describes a chain verification failure.
type AuditIntegrityError struct {
	AtSeq   uint64
	Message string
}

func (e *AuditIntegrityError) Error() string {
	return fmt.Sprintf("audit integrity violation at seq=%d: %s", e.AtSeq, e.Message)
}

// RuleAuditLedger is the append-only cryptographic audit log.
type RuleAuditLedger struct {
	mu      sync.Mutex
	entries []RuleAuditEntry
}

// NewRuleAuditLedger constructs an empty ledger.
func NewRuleAuditLedger() *RuleAuditLedger { return &RuleAuditLedger{} }

// AppendRecord appends a new audit entry and returns it.
func (l *RuleAuditLedger) AppendRecord(
	actor string,
	action AuditAction,
	ruleID string,
	diff string,
) RuleAuditEntry {
	l.mu.Lock()
	defer l.mu.Unlock()

	seq := uint64(len(l.entries))
	ts := time.Now().UnixNano()

	var prevHash [32]byte
	if len(l.entries) > 0 {
		prevHash = l.entries[len(l.entries)-1].EntryHash
	}

	entryHash := computeEntryHash(prevHash, seq, ts, actor, action.String(), ruleID, diff)

	entry := RuleAuditEntry{
		Seq:         seq,
		TimestampNs: ts,
		Actor:       actor,
		Action:      action,
		RuleID:      ruleID,
		DiffSummary: diff,
		PrevHash:    prevHash,
		EntryHash:   entryHash,
	}
	l.entries = append(l.entries, entry)
	return entry
}

// Len returns the current number of entries in the ledger.
func (l *RuleAuditLedger) Len() int {
	l.mu.Lock()
	defer l.mu.Unlock()
	return len(l.entries)
}

// Entries returns a read-only copy of the ledger entries.
func (l *RuleAuditLedger) Entries() []RuleAuditEntry {
	l.mu.Lock()
	defer l.mu.Unlock()
	cp := make([]RuleAuditEntry, len(l.entries))
	copy(cp, l.entries)
	return cp
}

// EntriesRaw returns direct access to the internal slice — used ONLY in tests
// to simulate tamper attacks.
func (l *RuleAuditLedger) EntriesRaw() []RuleAuditEntry {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.entries
}

// VerifyLedger traverses the entire chain, re-deriving each entry's hash and
// verifying linkage.  Returns nil on success or an *AuditIntegrityError on failure.
func (l *RuleAuditLedger) VerifyLedger() error {
	l.mu.Lock()
	defer l.mu.Unlock()

	var expectedPrev [32]byte // zero vector for first entry

	for i, entry := range l.entries {
		if entry.PrevHash != expectedPrev {
			return &AuditIntegrityError{
				AtSeq: uint64(i),
				Message: fmt.Sprintf("prev_hash mismatch: stored=%x, expected=%x",
					entry.PrevHash, expectedPrev),
			}
		}

		recomputed := computeEntryHash(
			entry.PrevHash,
			entry.Seq,
			entry.TimestampNs,
			entry.Actor,
			entry.Action.String(),
			entry.RuleID,
			entry.DiffSummary,
		)

		if recomputed != entry.EntryHash {
			return &AuditIntegrityError{
				AtSeq: uint64(i),
				Message: fmt.Sprintf("entry_hash mismatch: stored=%x, recomputed=%x",
					entry.EntryHash, recomputed),
			}
		}

		expectedPrev = entry.EntryHash
	}

	return nil
}
