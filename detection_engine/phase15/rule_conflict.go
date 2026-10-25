// Package phase15 implements Features 85–90: Rule Hot Reload & Configuration Governance.
package phase15

import (
	"fmt"
	"strings"
)

// ─── Feature 85: Rule Conflict & Subsumption Detector ────────────────────────
//
// Analyzes pairs of compiled rules for:
//   • Exact-duplicate search selections (identical logsource + all field criteria).
//   • Semantic subsumption: Pred(R1) ⊆ Pred(R2) — broader rule makes narrower one redundant.
//   • Severity contradiction: same trigger conditions but different severity levels.
//   • Numeric interval containment: R1 condition X>20 is subsumed by R2 condition X>10.

// LogSource identifies the telemetry category a rule targets.
type LogSource struct {
	Category string // e.g., "process_creation"
	Product  string // e.g., "windows"
	Service  string // e.g., "sysmon"
}

// MatchOperator is the comparison operator used in a field predicate.
type MatchOperator int

const (
	OpEquals        MatchOperator = iota // field == value
	OpContains                           // field contains value
	OpStartsWith                         // field starts with value
	OpRegex                              // field matches regex
	OpGreaterThan                        // field > numeric value
	OpLessThan                           // field < numeric value
	OpGreaterOrEqual                     // field >= numeric value
)

func (o MatchOperator) String() string {
	switch o {
	case OpEquals:
		return "=="
	case OpContains:
		return "contains"
	case OpStartsWith:
		return "startswith"
	case OpRegex:
		return "regex"
	case OpGreaterThan:
		return ">"
	case OpLessThan:
		return "<"
	case OpGreaterOrEqual:
		return ">="
	default:
		return "?"
	}
}

// FieldPredicate is a single field-level condition in a rule's search selection.
type FieldPredicate struct {
	Field    string
	Operator MatchOperator
	Value    string  // string value for equality/pattern operators
	NumValue float64 // numeric value for comparison operators
}

// RuleSearchSelection is one named search block in a rule (maps to Sigma "detection").
type RuleSearchSelection struct {
	Name       string
	Predicates []FieldPredicate
}

// RuleSeverity mirrors the detection engine severity enum.
type RuleSeverity int

const (
	SevInfo     RuleSeverity = iota
	SevLow
	SevMedium
	SevHigh
	SevCritical
)

func (s RuleSeverity) String() string {
	return [...]string{"informational", "low", "medium", "high", "critical"}[s]
}

// CompiledRule is the static representation of a detection rule used for conflict analysis.
type CompiledRule struct {
	RuleID     string
	Title      string
	Severity   RuleSeverity
	LogSource  LogSource
	Selections []RuleSearchSelection
	Tags       []string
}

// ConflictKind classifies the type of detected conflict.
type ConflictKind int

const (
	ConflictDuplicate   ConflictKind = iota // identical criteria, may differ in severity
	ConflictSubsumption                     // broader rule makes narrower one redundant
	ConflictSeverityClash                   // same trigger, different severity
	ConflictIntervalContainment             // numeric interval R1 ⊆ R2
)

func (k ConflictKind) String() string {
	switch k {
	case ConflictDuplicate:
		return "DUPLICATE"
	case ConflictSubsumption:
		return "SUBSUMPTION"
	case ConflictSeverityClash:
		return "SEVERITY_CLASH"
	case ConflictIntervalContainment:
		return "INTERVAL_CONTAINMENT"
	default:
		return "UNKNOWN"
	}
}

// ConflictWarning describes a detected conflict between two rules.
type ConflictWarning struct {
	Kind        ConflictKind
	NewRuleID   string
	ExistingID  string
	Description string
}

func (cw ConflictWarning) String() string {
	return fmt.Sprintf("[%s] new=%s conflicts with existing=%s: %s",
		cw.Kind, cw.NewRuleID, cw.ExistingID, cw.Description)
}

// RuleConflictDetector checks incoming rules against an existing ruleset.
type RuleConflictDetector struct{}

// NewRuleConflictDetector constructs a detector.
func NewRuleConflictDetector() *RuleConflictDetector { return &RuleConflictDetector{} }

// CheckConflicts evaluates newRule against every rule in existingRules.
// Returns a (possibly empty) slice of ConflictWarning values.
func (d *RuleConflictDetector) CheckConflicts(
	newRule *CompiledRule,
	existingRules []*CompiledRule,
) []ConflictWarning {
	var warnings []ConflictWarning

	for _, existing := range existingRules {
		// Fast-path: logsource must overlap for any conflict to be possible.
		if !logSourceOverlaps(newRule.LogSource, existing.LogSource) {
			continue
		}

		w := d.analyzeRulePair(newRule, existing)
		warnings = append(warnings, w...)
	}

	return warnings
}

// analyzeRulePair compares two rules with overlapping logsources.
func (d *RuleConflictDetector) analyzeRulePair(nr, er *CompiledRule) []ConflictWarning {
	var warnings []ConflictWarning

	for _, nSel := range nr.Selections {
		for _, eSel := range er.Selections {
			kind, desc := compareSelections(nSel, eSel)
			if kind < 0 {
				continue
			}

			w := ConflictWarning{
				Kind:       ConflictKind(kind),
				NewRuleID:  nr.RuleID,
				ExistingID: er.RuleID,
			}

			if ConflictKind(kind) == ConflictDuplicate && nr.Severity != er.Severity {
				w.Kind = ConflictSeverityClash
				desc += fmt.Sprintf(" (severity: new=%s vs existing=%s)", nr.Severity, er.Severity)
			}

			w.Description = desc
			warnings = append(warnings, w)
		}
	}

	return warnings
}

// compareSelections returns (ConflictKind, description) or (-1, "") if no conflict.
func compareSelections(a, b RuleSearchSelection) (int, string) {
	if len(a.Predicates) == 0 || len(b.Predicates) == 0 {
		return -1, ""
	}

	aMap := predicateMap(a.Predicates)
	bMap := predicateMap(b.Predicates)

	// Check exact duplicate: both maps have identical keys and values.
	if mapsEqual(aMap, bMap) {
		return int(ConflictDuplicate), fmt.Sprintf(
			"selections '%s' and '%s' are exact duplicates", a.Name, b.Name)
	}

	// Check numeric interval containment on shared numeric fields.
	if w, ok := checkIntervalContainment(a.Predicates, b.Predicates); ok {
		return int(ConflictIntervalContainment), w
	}

	// Check subsumption: every predicate in 'a' is satisfied by 'b' (b is broader).
	if predicatesSubsumedBy(a.Predicates, b.Predicates) {
		return int(ConflictSubsumption), fmt.Sprintf(
			"new rule selection '%s' is subsumed by existing selection '%s'", a.Name, b.Name)
	}

	return -1, ""
}

// predicateMap builds a string-keyed map of predicate signatures for quick comparison.
func predicateMap(preds []FieldPredicate) map[string]string {
	m := make(map[string]string, len(preds))
	for _, p := range preds {
		key := p.Field + "|" + p.Operator.String()
		m[key] = p.Value + fmt.Sprintf("|%.6f", p.NumValue)
	}
	return m
}

// mapsEqual returns true when two string maps have the exact same key-value pairs.
func mapsEqual(a, b map[string]string) bool {
	if len(a) != len(b) {
		return false
	}
	for k, v := range a {
		if b[k] != v {
			return false
		}
	}
	return true
}

// checkIntervalContainment checks whether any numeric predicates on the same
// field form a containment relationship (e.g., X>10 contains X>20).
func checkIntervalContainment(aPreds, bPreds []FieldPredicate) (string, bool) {
	for _, ap := range aPreds {
		if ap.Operator != OpGreaterThan && ap.Operator != OpGreaterOrEqual {
			continue
		}
		for _, bp := range bPreds {
			if bp.Field != ap.Field {
				continue
			}
			if bp.Operator != OpGreaterThan && bp.Operator != OpGreaterOrEqual {
				continue
			}
			// b (existing) has a lower threshold: b is broader, a is subsumed
			if bp.NumValue < ap.NumValue {
				return fmt.Sprintf("field '%s': new rule threshold %.0f is subsumed by existing threshold %.0f",
					ap.Field, ap.NumValue, bp.NumValue), true
			}
		}
	}
	return "", false
}

// predicatesSubsumedBy returns true when every predicate in 'sub' is implied
// by at least one predicate in 'super' (super is the broader rule).
func predicatesSubsumedBy(sub, super []FieldPredicate) bool {
	if len(sub) == 0 || len(super) == 0 {
		return false
	}
	for _, sp := range sub {
		covered := false
		for _, ep := range super {
			if ep.Field == sp.Field && predicateCovers(ep, sp) {
				covered = true
				break
			}
		}
		if !covered {
			return false
		}
	}
	return true
}

// predicateCovers returns true when 'broad' semantically covers 'narrow'
// (i.e., any event matching 'narrow' also matches 'broad').
func predicateCovers(broad, narrow FieldPredicate) bool {
	if broad.Operator != narrow.Operator {
		return false
	}
	switch broad.Operator {
	case OpEquals:
		return broad.Value == narrow.Value
	case OpContains:
		// broad contains "cmd" covers narrow contains "cmd.exe" when "cmd.exe" contains "cmd"
		return strings.Contains(narrow.Value, broad.Value)
	case OpStartsWith:
		return strings.HasPrefix(narrow.Value, broad.Value)
	case OpGreaterThan, OpGreaterOrEqual:
		// broader threshold: broad.NumValue <= narrow.NumValue
		return broad.NumValue <= narrow.NumValue
	default:
		return false
	}
}

// logSourceOverlaps returns true when two log sources could share events.
func logSourceOverlaps(a, b LogSource) bool {
	if a.Category != "" && b.Category != "" && a.Category != b.Category {
		return false
	}
	if a.Product != "" && b.Product != "" && a.Product != b.Product {
		return false
	}
	if a.Service != "" && b.Service != "" && a.Service != b.Service {
		return false
	}
	return true
}
