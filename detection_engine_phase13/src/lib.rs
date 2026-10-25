/// Phase XIII – Alerting & Detection Lifecycle Conclusion
///
/// Public re-exports for all six feature modules.
pub mod features;

// Convenience re-exports for commonly used types
pub use features::alert_state_machine::{
    AlertState, AlertStateMachine, StateTransitionError, TransitionRequest, TransitionResult,
    is_transition_allowed, is_terminal, try_transition,
};
pub use features::lifecycle_journal::{JournalEntry, LifecycleJournal};
pub use features::alert_reaper::{AlertRegistration, AlertReaper, hours_to_ns, minutes_to_ns, seconds_to_ns};
pub use features::alert_router::{
    AlertRouter, AlertSink, CircuitBreakerSink, BreakerConfig, BreakerState,
    FileSink, KafkaSink, NormalizedAlert, RoutingPredicate, RoutingRule, Severity,
    SinkError, WebhookSink,
};
pub use features::delivery_metrics::{AlertDeliveryMetrics, ChannelMetrics, NUM_BUCKETS};
pub use features::explainability::{
    AstNode, EvalContext, EvidenceAttributor, ExplainabilityMetadata, FieldCondition,
    FieldEvidence, PatternType, RuleAst,
    build_powershell_download_rule, build_powershell_eval_context,
};
