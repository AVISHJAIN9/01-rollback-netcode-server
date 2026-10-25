package phase12

import (
	"regexp"
	"sync"
)

// ─── Feature 70: Rule-Based Dynamic Alert Suppression Gates ──────────────────
//
// SuppressionGate holds a set of SuppressionRules.  Each rule carries:
//   • A set of attribute predicates evaluated against the alert.
//   • An optional list of active maintenance time ranges.
//   • An expiration timestamp (unix ns); expired rules are skipped.
//
// Evaluation is short-circuit: the first matching rule triggers suppression.
// The gate marks alert.Status = AlertStatusSuppressed and records the
// matching rule ID and reason in alert.Enrichment.

// PredicateType classifies a suppression predicate's comparison operation.
type PredicateType int

const (
	PredicateEquals     PredicateType = iota // field value == literal
	PredicateRegex                           // field value matches regex
	PredicateContains                        // field value contains substring
)

// AlertField is a named field accessor applied during predicate evaluation.
type AlertField int

const (
	FieldHostname    AlertField = iota
	FieldSourceIP
	FieldUsername
	FieldRuleID
	FieldTenantID
	FieldAttackTechnique
)

// SuppressionPredicate is a single attribute condition.
type SuppressionPredicate struct {
	Field   AlertField
	Type    PredicateType
	Literal string         // used for Equals and Contains
	Pattern *regexp.Regexp // used for Regex
}

// TimeRange is a [StartNs, EndNs] maintenance window (inclusive).
type TimeRange struct {
	StartNs int64
	EndNs   int64
}

// SuppressionRule defines when alerts should be suppressed.
type SuppressionRule struct {
	ID               string
	Name             string
	Predicates       []SuppressionPredicate // ALL must match (AND semantics)
	ActiveTimeRanges []TimeRange            // alert must fall within at least one range
	ExpirationNs     int64                  // rule is ignored after this time; 0 = no expiry
}

// SuppressionResult carries the outcome of a gate evaluation.
type SuppressionResult struct {
	Suppressed bool
	RuleID     string
	RuleName   string
	Reason     string
}

// SuppressionGate evaluates a chain of suppression rules against alerts.
type SuppressionGate struct {
	mu    sync.RWMutex
	rules []*SuppressionRule
}

// NewSuppressionGate constructs an empty gate.
func NewSuppressionGate() *SuppressionGate { return &SuppressionGate{} }

// RegisterRule appends a rule to the evaluation chain.
func (sg *SuppressionGate) RegisterRule(rule *SuppressionRule) {
	sg.mu.Lock()
	defer sg.mu.Unlock()
	sg.rules = append(sg.rules, rule)
}

// EvaluateSuppression checks the alert against all active rules.
// Returns SuppressionResult{Suppressed: true} on the first matching rule.
// If the alert is suppressed, its Status and Enrichment fields are updated.
func (sg *SuppressionGate) EvaluateSuppression(alert *NormalizedAlert, nowNs int64) SuppressionResult {
	sg.mu.RLock()
	defer sg.mu.RUnlock()

	for _, rule := range sg.rules {
		// Skip expired rules
		if rule.ExpirationNs > 0 && nowNs > rule.ExpirationNs {
			continue
		}

		// Check time ranges (if specified, at least one must match)
		if len(rule.ActiveTimeRanges) > 0 {
			inWindow := false
			for _, tr := range rule.ActiveTimeRanges {
				if nowNs >= tr.StartNs && nowNs <= tr.EndNs {
					inWindow = true
					break
				}
			}
			if !inWindow {
				continue
			}
		}

		// Check all predicates (AND logic)
		allMatch := true
		for _, pred := range rule.Predicates {
			fieldVal := extractField(alert, pred.Field)
			if !matchPredicate(fieldVal, pred) {
				allMatch = false
				break
			}
		}

		if allMatch {
			alert.Status = AlertStatusSuppressed
			alert.Enrichment.SuppressedByRule = rule.ID
			alert.Enrichment.SuppressedReason = rule.Name
			return SuppressionResult{
				Suppressed: true,
				RuleID:     rule.ID,
				RuleName:   rule.Name,
				Reason:     "Matched suppression rule: " + rule.Name,
			}
		}
	}

	return SuppressionResult{Suppressed: false}
}

// extractField reads the specified alert field as a string.
func extractField(alert *NormalizedAlert, field AlertField) string {
	switch field {
	case FieldHostname:
		return alert.Device.Hostname
	case FieldSourceIP:
		return alert.Network.SourceIP
	case FieldUsername:
		return alert.Actor.UserPrincipal
	case FieldRuleID:
		return alert.RuleID
	case FieldTenantID:
		return alert.TenantID
	case FieldAttackTechnique:
		return alert.AttackTechnique
	default:
		return ""
	}
}

// matchPredicate tests fieldVal against a single predicate.
func matchPredicate(fieldVal string, pred SuppressionPredicate) bool {
	switch pred.Type {
	case PredicateEquals:
		return fieldVal == pred.Literal
	case PredicateContains:
		return len(fieldVal) >= len(pred.Literal) && containsSubstr(fieldVal, pred.Literal)
	case PredicateRegex:
		if pred.Pattern == nil {
			return false
		}
		return pred.Pattern.MatchString(fieldVal)
	default:
		return false
	}
}

func containsSubstr(s, sub string) bool {
	for i := 0; i <= len(s)-len(sub); i++ {
		if s[i:i+len(sub)] == sub {
			return true
		}
	}
	return false
}
