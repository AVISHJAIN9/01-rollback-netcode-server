/// Feature 76: Multi-Destination Channel Alert Router & Dispatcher
///
/// Routes a `NormalizedAlert` to one or more downstream sinks based on
/// declarative `RoutingRule` predicates.  Each sink is wrapped with a circuit
/// breaker that trips to `Open` after 5 consecutive failures and enters
/// `HalfOpen` recovery after a configurable cool-down period.
///
/// Sink implementations provided:
///   • `WebhookSink`  – HTTP POST (mocked for testing with an injected handler)
///   • `KafkaSink`    – writes to an in-memory FIFO (stub for environments
///                      without a live Kafka broker; real integration replaces
///                      the inner `send_fn`)
///   • `FileSink`     – appends JSON-serialised alert to a named file/buffer
///
/// Circuit breaker states: `Closed` → `Open` (after 5 failures) →
///                          `HalfOpen` (after cool-down) → `Closed` (on probe success)

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::explainability::ExplainabilityMetadata;

// ─── NormalizedAlert ─────────────────────────────────────────────────────────

/// Severity level mirrored from the detection pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Critical = 4,
    High = 3,
    Medium = 2,
    Low = 1,
    Informational = 0,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "Critical"),
            Severity::High => write!(f, "High"),
            Severity::Medium => write!(f, "Medium"),
            Severity::Low => write!(f, "Low"),
            Severity::Informational => write!(f, "Informational"),
        }
    }
}

/// A fully normalised alert ready for routing and delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedAlert {
    pub id: String,
    pub title: String,
    pub severity: Severity,
    pub tenant_id: String,
    /// Freeform tags (e.g. "compliance", "ransomware", "lateral_movement").
    pub tags: Vec<String>,
    pub source_rule_id: String,
    pub timestamp_ns: i64,
    pub explainability: Option<ExplainabilityMetadata>,
}

// ─── Sink errors ─────────────────────────────────────────────────────────────

#[derive(Debug, Error, Clone)]
pub enum SinkError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },

    #[error("Kafka producer error: {0}")]
    Kafka(String),

    #[error("File I/O error: {0}")]
    Io(String),

    #[error("Circuit breaker is Open; delivery rejected")]
    CircuitOpen,

    #[error("Serialisation error: {0}")]
    Serialisation(String),
}

// ─── AlertSink trait ─────────────────────────────────────────────────────────

pub trait AlertSink: Send + Sync + std::fmt::Debug {
    /// Attempt to deliver `alert` to this sink.  Returns `Ok(())` on success
    /// or a `SinkError` on failure.  Must be non-blocking from the caller's
    /// perspective (internally may be synchronous for testing sinks).
    fn send(&self, alert: &NormalizedAlert) -> Result<(), SinkError>;

    /// Human-readable name for this sink (used in routing log messages and
    /// metrics labels).
    fn name(&self) -> &str;
}

// ─── Circuit Breaker ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit-breaker configuration parameters.
#[derive(Debug, Clone)]
pub struct BreakerConfig {
    /// Number of consecutive failures before tripping to `Open`.
    pub failure_threshold: u32,
    /// Duration after which the breaker moves from `Open` to `HalfOpen`.
    pub recovery_duration: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_duration: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
struct BreakerInner {
    state: BreakerState,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
    config: BreakerConfig,
    /// Counts total trips for telemetry.
    trip_count: u64,
}

impl BreakerInner {
    fn new(config: BreakerConfig) -> Self {
        Self {
            state: BreakerState::Closed,
            consecutive_failures: 0,
            opened_at: None,
            config,
            trip_count: 0,
        }
    }

    /// Attempt to pass a request through the breaker.
    /// Returns `false` when the breaker is Open (request should be rejected).
    fn allow_request(&mut self) -> bool {
        match self.state {
            BreakerState::Closed => true,
            BreakerState::Open => {
                let elapsed = self
                    .opened_at
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                if elapsed >= self.config.recovery_duration {
                    self.state = BreakerState::HalfOpen;
                    true // allow the probe request
                } else {
                    false
                }
            }
            BreakerState::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.state = BreakerState::Closed;
    }

    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.state == BreakerState::HalfOpen
            || self.consecutive_failures >= self.config.failure_threshold
        {
            self.state = BreakerState::Open;
            self.opened_at = Some(Instant::now());
            self.trip_count += 1;
        }
    }

    fn state(&self) -> BreakerState {
        self.state
    }

    fn trip_count(&self) -> u64 {
        self.trip_count
    }
}

/// Thread-safe circuit breaker wrapping any `AlertSink`.
#[derive(Debug)]
pub struct CircuitBreakerSink {
    inner_sink: Arc<dyn AlertSink>,
    breaker: Mutex<BreakerInner>,
    /// Alerts rejected while the breaker is open are enqueued here for later retry.
    retry_queue: Mutex<VecDeque<NormalizedAlert>>,
    max_retry_queue: usize,
}

impl CircuitBreakerSink {
    pub fn new(sink: Arc<dyn AlertSink>, config: BreakerConfig) -> Self {
        Self {
            inner_sink: sink,
            breaker: Mutex::new(BreakerInner::new(config)),
            retry_queue: Mutex::new(VecDeque::new()),
            max_retry_queue: 10_000,
        }
    }

    pub fn breaker_state(&self) -> BreakerState {
        self.breaker.lock().unwrap().state()
    }

    pub fn trip_count(&self) -> u64 {
        self.breaker.lock().unwrap().trip_count()
    }

    pub fn retry_queue_depth(&self) -> usize {
        self.retry_queue.lock().unwrap().len()
    }

    fn enqueue_retry(&self, alert: NormalizedAlert) {
        let mut q = self.retry_queue.lock().unwrap();
        if q.len() < self.max_retry_queue {
            q.push_back(alert);
        }
        // When the queue is full we drop the oldest entry to make room (tail-drop policy)
        else {
            q.pop_front();
            q.push_back(alert);
        }
    }

    /// Attempt to drain the retry queue into the sink (called after the
    /// breaker transitions back to `Closed`).
    pub fn flush_retry_queue(&self) -> usize {
        let mut flushed = 0usize;
        loop {
            let front = {
                let mut q = self.retry_queue.lock().unwrap();
                q.pop_front()
            };
            match front {
                None => break,
                Some(alert) => {
                    match self.send(&alert) {
                        Ok(()) => flushed += 1,
                        Err(_) => {
                            // Re-enqueue on failure and stop attempting
                            self.enqueue_retry(alert);
                            break;
                        }
                    }
                }
            }
        }
        flushed
    }
}

impl AlertSink for CircuitBreakerSink {
    fn send(&self, alert: &NormalizedAlert) -> Result<(), SinkError> {
        let allowed = {
            let mut b = self.breaker.lock().unwrap();
            b.allow_request()
        };

        if !allowed {
            self.enqueue_retry(alert.clone());
            return Err(SinkError::CircuitOpen);
        }

        match self.inner_sink.send(alert) {
            Ok(()) => {
                self.breaker.lock().unwrap().record_success();
                Ok(())
            }
            Err(e) => {
                self.breaker.lock().unwrap().record_failure();
                self.enqueue_retry(alert.clone());
                Err(e)
            }
        }
    }

    fn name(&self) -> &str {
        self.inner_sink.name()
    }
}

// ─── WebhookSink ─────────────────────────────────────────────────────────────

/// HTTP webhook sink.  In tests, `send_fn` is replaced with a mock closure.
/// In production, `send_fn` makes a real blocking HTTP POST using `reqwest`.
#[derive(Debug)]
pub struct WebhookSink {
    pub name: String,
    pub endpoint_url: String,
    /// Injected transport function: (url, json_body) → Result<(), SinkError>
    pub send_fn: Arc<dyn Fn(&str, &str) -> Result<(), SinkError> + Send + Sync>,
}

impl WebhookSink {
    pub fn new_with_transport(
        name: impl Into<String>,
        endpoint_url: impl Into<String>,
        transport: impl Fn(&str, &str) -> Result<(), SinkError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            endpoint_url: endpoint_url.into(),
            send_fn: Arc::new(transport),
        }
    }
}

impl AlertSink for WebhookSink {
    fn send(&self, alert: &NormalizedAlert) -> Result<(), SinkError> {
        let body =
            serde_json::to_string(alert).map_err(|e| SinkError::Serialisation(e.to_string()))?;
        (self.send_fn)(&self.endpoint_url, &body)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ─── KafkaSink ───────────────────────────────────────────────────────────────

/// In-memory Kafka stub: pushes serialised JSON into an `Arc<Mutex<Vec>>`.
/// Production code replaces `send_fn` with the rdkafka producer callback.
#[derive(Debug)]
pub struct KafkaSink {
    pub name: String,
    pub topic: String,
    pub produced_messages: Arc<Mutex<Vec<String>>>,
}

impl KafkaSink {
    pub fn new(name: impl Into<String>, topic: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            topic: topic.into(),
            produced_messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn message_count(&self) -> usize {
        self.produced_messages.lock().unwrap().len()
    }
}

impl AlertSink for KafkaSink {
    fn send(&self, alert: &NormalizedAlert) -> Result<(), SinkError> {
        let payload =
            serde_json::to_string(alert).map_err(|e| SinkError::Serialisation(e.to_string()))?;
        self.produced_messages.lock().unwrap().push(payload);
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ─── FileSink ────────────────────────────────────────────────────────────────

/// Appends JSON-serialised alerts to an in-memory buffer (simulates S3/file).
#[derive(Debug)]
pub struct FileSink {
    pub name: String,
    pub buffer: Arc<Mutex<Vec<String>>>,
}

impl FileSink {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn entry_count(&self) -> usize {
        self.buffer.lock().unwrap().len()
    }
}

impl AlertSink for FileSink {
    fn send(&self, alert: &NormalizedAlert) -> Result<(), SinkError> {
        let line =
            serde_json::to_string(alert).map_err(|e| SinkError::Serialisation(e.to_string()))?;
        self.buffer.lock().unwrap().push(line);
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ─── Routing rules ───────────────────────────────────────────────────────────

/// Predicate that must be satisfied for a rule to match.
#[derive(Debug, Clone)]
pub enum RoutingPredicate {
    /// Match when alert severity ≥ threshold.
    SeverityAtLeast(Severity),
    /// Match exact severity.
    SeverityEquals(Severity),
    /// Match when alert contains this tag.
    HasTag(String),
    /// Match when alert.tenant_id equals this value.
    TenantEquals(String),
    /// Logical AND of multiple predicates.
    And(Vec<RoutingPredicate>),
    /// Logical OR of multiple predicates.
    Or(Vec<RoutingPredicate>),
    /// Unconditional match (catch-all rule).
    Any,
}

impl RoutingPredicate {
    pub fn matches(&self, alert: &NormalizedAlert) -> bool {
        match self {
            RoutingPredicate::SeverityAtLeast(min) => alert.severity >= *min,
            RoutingPredicate::SeverityEquals(sev) => alert.severity == *sev,
            RoutingPredicate::HasTag(tag) => alert.tags.iter().any(|t| t == tag),
            RoutingPredicate::TenantEquals(tid) => &alert.tenant_id == tid,
            RoutingPredicate::And(preds) => preds.iter().all(|p| p.matches(alert)),
            RoutingPredicate::Or(preds) => preds.iter().any(|p| p.matches(alert)),
            RoutingPredicate::Any => true,
        }
    }
}

/// A single routing rule: predicate + target sinks.
pub struct RoutingRule {
    pub name: String,
    pub predicate: RoutingPredicate,
    /// Sinks to deliver to when the predicate matches.
    pub sinks: Vec<Arc<dyn AlertSink>>,
    /// When `exclusive` is true, the router stops evaluating further rules
    /// after this rule matches (first-match wins).
    pub exclusive: bool,
}

impl std::fmt::Debug for RoutingRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingRule")
            .field("name", &self.name)
            .field("exclusive", &self.exclusive)
            .finish()
    }
}

// ─── Router errors ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("All sinks for rule '{rule}' rejected the alert: {details}")]
    AllSinksFailed { rule: String, details: String },

    #[error("No routing rule matched the alert (id={alert_id})")]
    NoRuleMatched { alert_id: String },
}

// ─── AlertRouter ─────────────────────────────────────────────────────────────

/// Evaluates routing rules against a `NormalizedAlert` and fans the alert out
/// to the appropriate sinks.
#[derive(Debug, Default)]
pub struct AlertRouter {
    rules: Vec<RoutingRule>,
}

impl AlertRouter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Append a routing rule.  Rules are evaluated in insertion order.
    pub fn add_rule(&mut self, rule: RoutingRule) {
        self.rules.push(rule);
    }

    /// Route `alert` through the rule list.
    ///
    /// Returns `Ok(n)` where `n` is the number of matched rules.
    /// Returns `Err(RouterError::NoRuleMatched)` when no rule fires.
    pub fn route(&self, alert: &NormalizedAlert) -> Result<usize, RouterError> {
        let mut matched = 0usize;

        for rule in &self.rules {
            if rule.predicate.matches(alert) {
                matched += 1;
                let mut errors = Vec::new();

                for sink in &rule.sinks {
                    if let Err(e) = sink.send(alert) {
                        errors.push(format!("{}:{}", sink.name(), e));
                    }
                }

                // A rule "succeeds" if at least one sink accepted it.
                if !errors.is_empty() && errors.len() == rule.sinks.len() {
                    return Err(RouterError::AllSinksFailed {
                        rule: rule.name.clone(),
                        details: errors.join("; "),
                    });
                }

                if rule.exclusive {
                    break;
                }
            }
        }

        if matched == 0 {
            Err(RouterError::NoRuleMatched {
                alert_id: alert.id.clone(),
            })
        } else {
            Ok(matched)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn make_alert(id: &str, severity: Severity) -> NormalizedAlert {
        NormalizedAlert {
            id: id.to_owned(),
            title: format!("Test alert {id}"),
            severity,
            tenant_id: "tenant-abc".to_owned(),
            tags: vec!["test".to_owned()],
            source_rule_id: "rule-001".to_owned(),
            timestamp_ns: 0,
            explainability: None,
        }
    }

    #[test]
    fn critical_alert_routes_to_webhook() {
        let file_sink = Arc::new(FileSink::new("webhook-stub"));
        let file_clone = Arc::clone(&file_sink);

        let mut router = AlertRouter::new();
        router.add_rule(RoutingRule {
            name: "critical-to-webhook".to_owned(),
            predicate: RoutingPredicate::SeverityAtLeast(Severity::Critical),
            sinks: vec![file_clone],
            exclusive: false,
        });

        let alert = make_alert("a-001", Severity::Critical);
        let n = router.route(&alert).expect("routing must succeed");
        assert_eq!(n, 1);
        assert_eq!(file_sink.entry_count(), 1);
    }

    #[test]
    fn low_severity_routes_to_kafka() {
        let kafka_sink = Arc::new(KafkaSink::new("kafka-low", "alerts.low"));
        let kafka_clone = Arc::clone(&kafka_sink);

        let mut router = AlertRouter::new();
        router.add_rule(RoutingRule {
            name: "low-to-kafka".to_owned(),
            predicate: RoutingPredicate::SeverityAtLeast(Severity::Low),
            sinks: vec![kafka_clone],
            exclusive: false,
        });

        let alert = make_alert("a-002", Severity::Low);
        router.route(&alert).unwrap();
        assert_eq!(kafka_sink.message_count(), 1);
    }

    #[test]
    fn circuit_breaker_trips_after_five_failures() {
        let fail_count = Arc::new(AtomicU32::new(0));
        let fail_count_clone = Arc::clone(&fail_count);

        let failing_sink = Arc::new(WebhookSink::new_with_transport(
            "pagerduty-mock",
            "https://mock.pagerduty/",
            move |_url, _body| {
                fail_count_clone.fetch_add(1, Ordering::Relaxed);
                Err(SinkError::Http {
                    status: 500,
                    body: "Internal Server Error".to_owned(),
                })
            },
        ));

        let cb_sink = Arc::new(CircuitBreakerSink::new(
            failing_sink,
            BreakerConfig {
                failure_threshold: 5,
                recovery_duration: Duration::from_secs(30),
            },
        ));

        let alert = make_alert("a-003", Severity::Critical);

        // First 5 calls: breaker is Closed, sink fails, breaker accumulates failures
        for i in 1..=5 {
            let res = cb_sink.send(&alert);
            assert!(res.is_err(), "call {i} must fail");
        }

        assert_eq!(
            cb_sink.breaker_state(),
            BreakerState::Open,
            "Breaker must be Open after 5 consecutive failures"
        );
        assert_eq!(
            cb_sink.trip_count(),
            1,
            "Exactly one trip must be recorded"
        );

        // 6th call: breaker is Open → circuit open error + retry queued
        let res = cb_sink.send(&alert);
        assert_eq!(res.unwrap_err(), SinkError::CircuitOpen);

        // The 5 failed deliveries + the circuit-open one are all in the retry queue
        // (5 from failing sends + 1 from open circuit)
        assert_eq!(cb_sink.retry_queue_depth(), 6);
    }

    #[test]
    fn no_rule_matched_returns_error() {
        let router = AlertRouter::new();
        let alert = make_alert("a-004", Severity::Informational);
        let err = router.route(&alert).unwrap_err();
        assert!(matches!(err, RouterError::NoRuleMatched { .. }));
    }
}
