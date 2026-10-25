// Package phase12 implements Features 67–72: Alert object lifecycle pipeline.
package phase12

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"hash/fnv"
	"time"
)

// ─── Core types shared across all Feature 67–72 modules ─────────────────────

// SeverityLevel is the normalised alert severity.
type SeverityLevel int

const (
	SeverityInformational SeverityLevel = iota
	SeverityLow
	SeverityMedium
	SeverityHigh
	SeverityCritical
)

func (s SeverityLevel) String() string {
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

// AlertStatus represents the current lifecycle status of an alert.
type AlertStatus int

const (
	AlertStatusActive     AlertStatus = iota
	AlertStatusSuppressed             // matched a suppression rule
	AlertStatusGrouped                // rolled into a GroupedAlertIncident
)

func (a AlertStatus) String() string {
	switch a {
	case AlertStatusActive:
		return "Active"
	case AlertStatusSuppressed:
		return "Suppressed"
	case AlertStatusGrouped:
		return "Grouped"
	default:
		return "Unknown"
	}
}

// ActorEntity represents the user/process initiating the activity.
type ActorEntity struct {
	UserPrincipal string
	UserDisplayName string
	ProcessName   string
	ProcessID     int32
}

// DeviceEntity represents the asset where activity was observed.
type DeviceEntity struct {
	Hostname    string
	IPAddress   string
	MACAddress  string
	OSName      string
	DeviceType  string
}

// NetworkEntity represents the network endpoint involved.
type NetworkEntity struct {
	SourceIP   string
	DestIP     string
	DestPort   int
	Protocol   string
}

// EventEvidence is one piece of raw evidence supporting the alert.
type EventEvidence struct {
	EventID     string
	EventTime   int64
	EventType   string
	RawPayload  map[string]string
}

// ContextEnrichment holds data injected by the enrichment pipeline.
type ContextEnrichment struct {
	// Asset metadata
	AssetOwner       string
	BusinessUnit     string
	AssetCriticality string // "Critical", "High", "Medium", "Low"
	NetworkZone      string
	OSVersion        string

	// User/identity metadata
	Department       string
	Manager          string
	PrivilegeTier    string // "Domain Admin", "Local Admin", "Standard"
	IsVIP            bool
	IsServiceAccount bool

	// Suppression context (populated by SuppressionGate)
	SuppressedByRule string
	SuppressedReason string
}

// NormalizedAlert is the OCSF/ASIM-compatible alert envelope.
type NormalizedAlert struct {
	AlertID       string
	CreatedTimeNs int64
	RuleID        string
	Title         string
	Severity      SeverityLevel
	RiskScore     uint8

	Actor   ActorEntity
	Device  DeviceEntity
	Network NetworkEntity

	Evidence    []EventEvidence
	Enrichment  ContextEnrichment

	Status         AlertStatus
	CorrelationID  uint64
	TenantID       string
	AttackTechnique string // e.g. "T1059.001"
	Tags           []string
}

// DetectionMatch is the raw detection engine output normalised to NormalizedAlert.
type DetectionMatch struct {
	MatchID        string
	RuleID         string
	RuleTitle      string
	Severity       SeverityLevel
	TimestampNs    int64
	EntityID       string
	SourceIP       string
	DestIP         string
	DestPort       int
	Username       string
	Hostname       string
	ProcessName    string
	ProcessID      int32
	RawEventID     string
	RawPayload     map[string]string
	TenantID       string
	AttackTechnique string
	Tags           []string
}

// ─── Feature 67: Alert Object Normalization ───────────────────────────────────

// AlertNormalizer transforms DetectionMatch records into NormalizedAlert envelopes.
type AlertNormalizer struct{}

// NewAlertNormalizer constructs a normalizer.
func NewAlertNormalizer() *AlertNormalizer { return &AlertNormalizer{} }

// Normalize converts a DetectionMatch into a fully populated NormalizedAlert.
func (n *AlertNormalizer) Normalize(m *DetectionMatch) *NormalizedAlert {
	id := newUUID()
	evidence := []EventEvidence{
		{
			EventID:    m.RawEventID,
			EventTime:  m.TimestampNs,
			EventType:  "ProcessCreate",
			RawPayload: m.RawPayload,
		},
	}

	return &NormalizedAlert{
		AlertID:       id,
		CreatedTimeNs: time.Now().UnixNano(),
		RuleID:        m.RuleID,
		Title:         m.RuleTitle,
		Severity:      m.Severity,
		RiskScore:     severityBaseScore(m.Severity),
		Actor: ActorEntity{
			UserPrincipal:   m.Username,
			UserDisplayName: m.Username,
			ProcessName:     m.ProcessName,
			ProcessID:       m.ProcessID,
		},
		Device: DeviceEntity{
			Hostname:  m.Hostname,
			IPAddress: m.SourceIP,
		},
		Network: NetworkEntity{
			SourceIP: m.SourceIP,
			DestIP:   m.DestIP,
			DestPort: m.DestPort,
		},
		Evidence:        evidence,
		Status:          AlertStatusActive,
		TenantID:        m.TenantID,
		AttackTechnique: m.AttackTechnique,
		Tags:            m.Tags,
	}
}

// severityBaseScore maps a SeverityLevel to its base numeric score.
func severityBaseScore(s SeverityLevel) uint8 {
	switch s {
	case SeverityInformational:
		return 10
	case SeverityLow:
		return 25
	case SeverityMedium:
		return 50
	case SeverityHigh:
		return 75
	case SeverityCritical:
		return 90
	default:
		return 10
	}
}

// newUUID generates a random UUID-like string (v4 simplified).
func newUUID() string {
	var b [16]byte
	rand.Read(b[:])
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return fmt.Sprintf("%s-%s-%s-%s-%s",
		hex.EncodeToString(b[0:4]),
		hex.EncodeToString(b[4:6]),
		hex.EncodeToString(b[6:8]),
		hex.EncodeToString(b[8:10]),
		hex.EncodeToString(b[10:16]),
	)
}

// fingerprintAlert computes a stable 64-bit hash over alert identity fields.
func fingerprintAlert(tenantID, ruleID, entityID string) uint64 {
	h := fnv.New64a()
	h.Write([]byte(tenantID))
	h.Write([]byte{0x00})
	h.Write([]byte(ruleID))
	h.Write([]byte{0x00})
	h.Write([]byte(entityID))
	return h.Sum64()
}
