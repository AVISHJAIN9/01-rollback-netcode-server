// Package phase11 implements Features 61–66: State conclusion & Alert lifecycle launch.
package phase11

import (
	"crypto/rand"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"hash/fnv"
	"sort"
	"sync/atomic"
	"time"
)

// ─── Shared domain types ──────────────────────────────────────────────────────

// Severity mirrors the master plan's severity enum.
type Severity int

const (
	SeverityInformational Severity = iota
	SeverityLow
	SeverityMedium
	SeverityHigh
	SeverityCritical
)

func (s Severity) String() string {
	switch s {
	case SeverityInformational:
		return "Informational"
	case SeverityLow:
		return "Low"
	case SeverityMedium:
		return "Medium"
	case SeverityHigh:
		return "High"
	case SeverityCritical:
		return "Critical"
	default:
		return fmt.Sprintf("Severity(%d)", int(s))
	}
}

// ValueVariant is a tagged-union over the types that context attributes may hold.
type ValueVariant struct {
	StrVal  string
	IntVal  int64
	BoolVal bool
	IsStr   bool
	IsInt   bool
	IsBool  bool
}

func StrVariant(s string) ValueVariant  { return ValueVariant{StrVal: s, IsStr: true} }
func IntVariant(i int64) ValueVariant   { return ValueVariant{IntVal: i, IsInt: true} }
func BoolVariant(b bool) ValueVariant   { return ValueVariant{BoolVal: b, IsBool: true} }

func (v ValueVariant) String() string {
	switch {
	case v.IsStr:
		return v.StrVal
	case v.IsInt:
		return fmt.Sprintf("%d", v.IntVal)
	case v.IsBool:
		return fmt.Sprintf("%v", v.BoolVal)
	default:
		return "<nil>"
	}
}

// CanonicalEvent represents a normalised security event from the ingestion pipeline.
type CanonicalEvent struct {
	EventID        string
	TimestampNs    int64
	EntityID       string
	EventType      string
	RawAttributes  map[string]ValueVariant
}

// CompiledRule is the metadata attached to a compiled detection rule.
type CompiledRule struct {
	RuleID    string
	RuleTitle string
	Severity  Severity
	Tags      []string
}

// ─── Feature 66: Canonical Detection Match Record Creation ────────────────────

// DetectionMatch is the canonical output of the detection engine when a rule fires.
type DetectionMatch struct {
	MatchID              string
	RuleID               string
	RuleTitle            string
	Severity             Severity
	DetectionTimestampNs int64
	EntityID             string
	TriggeringEvents     []CanonicalEvent
	ContextAttributes    map[string]ValueVariant
	SuppressionFingerprint uint64
}

// uuidV7 generates a time-ordered UUIDv7 using 48 bits of Unix ms + 74 bits random.
// Format: xxxxxxxx-xxxx-7xxx-yxxx-xxxxxxxxxxxx
func uuidV7() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	// Embed Unix ms in the high 48 bits
	ms := uint64(time.Now().UnixMilli())
	b[0] = byte(ms >> 40)
	b[1] = byte(ms >> 32)
	b[2] = byte(ms >> 24)
	b[3] = byte(ms >> 16)
	b[4] = byte(ms >> 8)
	b[5] = byte(ms)
	// Version 7
	b[6] = (b[6] & 0x0f) | 0x70
	// Variant bits
	b[8] = (b[8] & 0x3f) | 0x80
	return fmt.Sprintf("%s-%s-%s-%s-%s",
		hex.EncodeToString(b[0:4]),
		hex.EncodeToString(b[4:6]),
		hex.EncodeToString(b[6:8]),
		hex.EncodeToString(b[8:10]),
		hex.EncodeToString(b[10:16]),
	), nil
}

// computeSuppressionFingerprint hashes (ruleID ‖ entityID ‖ groupingKeys) to uint64.
func computeSuppressionFingerprint(ruleID, entityID string, groupingKeys []string) uint64 {
	h := fnv.New64a()
	h.Write([]byte(ruleID))
	h.Write([]byte{0x00})
	h.Write([]byte(entityID))
	h.Write([]byte{0x00})
	for _, gk := range groupingKeys {
		h.Write([]byte(gk))
		h.Write([]byte{0x01})
	}
	return h.Sum64()
}

// DetectionMatchFactory creates DetectionMatch records from compiled rules and events.
type DetectionMatchFactory struct {
	matchCounter atomic.Uint64
}

// NewDetectionMatchFactory constructs a factory.
func NewDetectionMatchFactory() *DetectionMatchFactory {
	return &DetectionMatchFactory{}
}

// CreateMatch generates a new DetectionMatch carrying full causal provenance.
func (f *DetectionMatchFactory) CreateMatch(
	rule *CompiledRule,
	event *CanonicalEvent,
	matchedContext map[string]ValueVariant,
) (*DetectionMatch, error) {
	f.matchCounter.Add(1)

	id, err := uuidV7()
	if err != nil {
		return nil, fmt.Errorf("generate match_id: %w", err)
	}

	// Build grouping keys from context attributes for fingerprint — sort for determinism
	groupingKeys := make([]string, 0, len(matchedContext))
	for k := range matchedContext {
		groupingKeys = append(groupingKeys, k)
	}
	sort.Strings(groupingKeys)

	fp := computeSuppressionFingerprint(rule.RuleID, event.EntityID, groupingKeys)

	// Merge rule tags into context
	ctx := make(map[string]ValueVariant, len(matchedContext)+len(rule.Tags))
	for k, v := range matchedContext {
		ctx[k] = v
	}
	for _, tag := range rule.Tags {
		ctx["tag:"+tag] = BoolVariant(true)
	}

	return &DetectionMatch{
		MatchID:                id,
		RuleID:                 rule.RuleID,
		RuleTitle:              rule.RuleTitle,
		Severity:               rule.Severity,
		DetectionTimestampNs:   time.Now().UnixNano(),
		EntityID:               event.EntityID,
		TriggeringEvents:       []CanonicalEvent{*event},
		ContextAttributes:      ctx,
		SuppressionFingerprint: fp,
	}, nil
}

// ─── UUIDv7 validator helper ──────────────────────────────────────────────────

// IsValidUUIDv7 returns true if s looks like a correctly formatted UUIDv7.
func IsValidUUIDv7(s string) bool {
	if len(s) != 36 {
		return false
	}
	// Must be: 8-4-4-4-12 hex groups separated by hyphens
	// and version nibble == 7
	decoded, err := hex.DecodeString(s[0:8] + s[9:13] + s[14:18] + s[19:23] + s[24:])
	if err != nil || len(decoded) != 16 {
		return false
	}
	_ = binary.BigEndian.Uint64(decoded[:8]) // just validates it's parseable
	return (decoded[6]>>4) == 0x7
}
