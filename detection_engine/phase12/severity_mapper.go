package phase12

// ─── Feature 68: Dynamic Alert Severity Mapping & Risk Scoring ───────────────
//
// RiskScore = min(100, S_base × W_asset × W_user × W_ti)
//
// Weights:
//   W_asset ∈ [1.0, 3.0]  – asset criticality multiplier
//   W_user  ∈ [1.0, 2.5]  – user privilege level multiplier
//   W_ti    ∈ [1.0, 2.0]  – threat intelligence confidence multiplier

// SeverityScorer computes contextual risk scores and severity levels.
type SeverityScorer struct{}

// NewSeverityScorer constructs a scorer.
func NewSeverityScorer() *SeverityScorer { return &SeverityScorer{} }

// ComputeRiskScore calculates the final risk score and contextual severity.
//
//   base           – base SeverityLevel from the detection rule
//   assetCriticality – asset weight W_asset ∈ [1.0, 3.0]
//   userPrivilege    – user privilege weight W_user ∈ [1.0, 2.5]
//   threatIntelWeight – TI confidence weight W_ti ∈ [1.0, 2.0]
func (ss *SeverityScorer) ComputeRiskScore(
	base SeverityLevel,
	assetCriticality float64,
	userPrivilege float64,
	threatIntelWeight float64,
) (uint8, SeverityLevel) {
	baseScore := float64(severityBaseScore(base))
	raw := baseScore * assetCriticality * userPrivilege * threatIntelWeight
	if raw > 100 {
		raw = 100
	}
	score := uint8(raw)
	return score, mapScoreToSeverity(score)
}

// mapScoreToSeverity maps a [0,100] numeric score to a SeverityLevel.
//
//   [0,  14] → Informational
//   [15, 39] → Low
//   [40, 64] → Medium
//   [65, 84] → High
//   [85,100] → Critical
func mapScoreToSeverity(score uint8) SeverityLevel {
	switch {
	case score >= 85:
		return SeverityCritical
	case score >= 65:
		return SeverityHigh
	case score >= 40:
		return SeverityMedium
	case score >= 15:
		return SeverityLow
	default:
		return SeverityInformational
	}
}

// EnrichAlertSeverity updates alert.RiskScore and alert.Severity in-place.
func (ss *SeverityScorer) EnrichAlertSeverity(
	alert *NormalizedAlert,
	assetCriticality float64,
	userPrivilege float64,
	threatIntelWeight float64,
) {
	score, severity := ss.ComputeRiskScore(alert.Severity, assetCriticality, userPrivilege, threatIntelWeight)
	alert.RiskScore = score
	alert.Severity = severity
}
