/// Phase XIII – Integration Verification Harness (main.rs)
///
/// Executes all 6 required verification tests and prints structured pass/fail
/// output with timing information.  Exit code 0 = all tests passed.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use detection_engine_phase13::{
    // Feature 73
    AlertState, AlertStateMachine, StateTransitionError, TransitionRequest, try_transition,
    // Feature 74
    LifecycleJournal,
    // Feature 75
    AlertReaper, hours_to_ns, minutes_to_ns,
    // Feature 76
    AlertRouter, CircuitBreakerSink, BreakerConfig, BreakerState,
    FileSink, KafkaSink, NormalizedAlert, RoutingPredicate, RoutingRule, Severity,
    SinkError, WebhookSink,
    // Feature 77
    AlertDeliveryMetrics, ChannelMetrics,
    // Feature 78
    EvalContext, EvidenceAttributor,
    build_powershell_download_rule, build_powershell_eval_context,
};

// ─── Test harness helpers ────────────────────────────────────────────────────

struct TestResult {
    name: String,
    passed: bool,
    duration: Duration,
    detail: String,
}

fn run_test<F: FnOnce() -> Result<String, String>>(name: &str, f: F) -> TestResult {
    let start = Instant::now();
    let (passed, detail) = match f() {
        Ok(msg) => (true, msg),
        Err(msg) => (false, msg),
    };
    let duration = start.elapsed();
    TestResult {
        name: name.to_owned(),
        passed,
        duration,
        detail,
    }
}

// ─── Test 1: FSM State Transition Permutations ───────────────────────────────

fn test_fsm_state_transitions() -> Result<String, String> {
    // Happy path: New → Acknowledged → InTriage → Resolved
    let mut fsm = AlertStateMachine::new("test-alert-t1-001");

    fsm.transition(TransitionRequest {
        target: AlertState::Acknowledged,
        actor: "analyst@corp",
        reason: "Alert reviewed",
    })
    .map_err(|e| format!("New→Acknowledged failed: {e}"))?;

    fsm.transition(TransitionRequest {
        target: AlertState::InTriage,
        actor: "analyst@corp",
        reason: "Starting deep-dive",
    })
    .map_err(|e| format!("Acknowledged→InTriage failed: {e}"))?;

    fsm.transition(TransitionRequest {
        target: AlertState::Resolved,
        actor: "analyst@corp",
        reason: "Confirmed TP, host isolated",
    })
    .map_err(|e| format!("InTriage→Resolved failed: {e}"))?;

    assert_eq!(fsm.state(), AlertState::Resolved);

    // Illegal: attempt Resolved → Acknowledged
    let illegal_result = fsm.transition(TransitionRequest {
        target: AlertState::Acknowledged,
        actor: "analyst@corp",
        reason: "illegal re-open attempt",
    });

    match illegal_result {
        Err(StateTransitionError::TerminalState { state: AlertState::Resolved }) => {
            // Correct: rejected
        }
        Err(e) => return Err(format!("Wrong error variant for terminal-state attempt: {e}")),
        Ok(_) => return Err("Illegal transition Resolved→Acknowledged was NOT rejected!".to_owned()),
    }

    // State must remain Resolved after rejected transition
    if fsm.state() != AlertState::Resolved {
        return Err(format!("State mutated after rejected transition: {:?}", fsm.state()));
    }

    // Additional illegal skip: free-function New → InTriage
    let mut state = AlertState::New;
    let skip_result = try_transition(&mut state, AlertState::InTriage, "bot", "skip");
    match skip_result {
        Err(StateTransitionError::IllegalTransition { from: AlertState::New, to: AlertState::InTriage }) => {}
        Err(e) => return Err(format!("Wrong error for New→InTriage skip: {e}")),
        Ok(_) => return Err("New→InTriage skip was NOT rejected!".to_owned()),
    }
    if state != AlertState::New {
        return Err("State changed after rejected skip transition".to_owned());
    }

    // Dismissed terminal path
    let mut fsm2 = AlertStateMachine::new("test-alert-t1-002");
    fsm2.transition(TransitionRequest {
        target: AlertState::Acknowledged,
        actor: "suppression_engine",
        reason: "Auto-acknowledged",
    }).unwrap();
    fsm2.transition(TransitionRequest {
        target: AlertState::Dismissed,
        actor: "suppression_engine",
        reason: "Matched benign suppression rule",
    }).unwrap();
    let dismissed_err = fsm2.transition(TransitionRequest {
        target: AlertState::InTriage,
        actor: "analyst",
        reason: "",
    });
    if dismissed_err.is_ok() {
        return Err("Transition out of Dismissed terminal state was incorrectly allowed".to_owned());
    }

    Ok(format!(
        "New→Ack→Triage→Resolved: OK | Resolved→Ack rejected (TerminalState): OK | \
         New→Triage skip rejected (IllegalTransition): OK | Dismissed is terminal: OK | \
         State integrity preserved throughout: OK"
    ))
}

// ─── Test 2: Cryptographic History Journal Integrity ─────────────────────────

fn test_journal_integrity() -> Result<String, String> {
    let mut journal = LifecycleJournal::new();

    let transitions = [
        (AlertState::New, AlertState::Acknowledged, "analyst_a", "ACKed"),
        (AlertState::Acknowledged, AlertState::InTriage, "analyst_b", "Triage started"),
        (AlertState::InTriage, AlertState::InTriage, "enrichment_svc", "EDR context added"),
        (AlertState::InTriage, AlertState::InTriage, "ti_svc", "Threat intel enriched"),
        (AlertState::InTriage, AlertState::Resolved, "analyst_a", "True positive – remediated"),
    ];

    for (from, to, actor, comment) in &transitions {
        journal.append_entry(*from, *to, actor, comment);
    }

    if journal.len() != 5 {
        return Err(format!("Expected 5 journal entries, got {}", journal.len()));
    }

    if !journal.verify_integrity() {
        return Err("Journal failed integrity check after 5 clean appends".to_owned());
    }

    // Tamper: mutate actor in entry[2]
    let original_actor = journal.entries()[2].actor.clone();
    journal.entries_mut()[2].actor = "attacker_injected_principal".to_owned();

    if journal.verify_integrity() {
        return Err("Journal passed integrity check after tampered actor – tamper not detected!".to_owned());
    }

    // Restore original actor to verify the check can pass again
    journal.entries_mut()[2].actor = original_actor;
    if !journal.verify_integrity() {
        return Err("Journal failed integrity after restoring tampered field".to_owned());
    }

    // Tamper: flip a byte in entry[3].entry_hash
    journal.entries_mut()[3].entry_hash[0] ^= 0xFF;
    if journal.verify_integrity() {
        return Err("Journal passed integrity after flipping byte in entry_hash – not detected!".to_owned());
    }

    Ok(format!(
        "5 entries appended: OK | verify_integrity() clean chain: OK | \
         tamper actor[2] detected: OK | restore integrity: OK | \
         flip entry_hash[3] detected: OK"
    ))
}

// ─── Test 3: Alert SLA Expiration & Auto-Archival ────────────────────────────

fn test_alert_expiration() -> Result<String, String> {
    let mut reaper = AlertReaper::new();

    let t0 = 0i64;
    let one_hour_ns = hours_to_ns(1);
    let deadline = t0 + one_hour_ns;

    reaper.register("alert-sla-expiry-001", deadline, t0, "1h_test_sla");

    // Check at t = 30 minutes → should NOT expire
    let mut fired_ids: Vec<String> = Vec::new();
    let count = reaper.check_expirations(t0 + minutes_to_ns(30), |reg| {
        fired_ids.push(reg.alert_id.clone());
    });
    if count != 0 {
        return Err(format!("Alert expired too early at 30 min (expected 0, got {count})"));
    }

    // Check at t = 61 minutes → MUST expire
    let count = reaper.check_expirations(t0 + minutes_to_ns(61), |reg| {
        fired_ids.push(reg.alert_id.clone());
    });
    if count != 1 {
        return Err(format!("Expected 1 expiry at 61 min, got {count}"));
    }
    if fired_ids != vec!["alert-sla-expiry-001"] {
        return Err(format!("Wrong alert_id in expiry callback: {:?}", fired_ids));
    }
    if reaper.active_count() != 0 {
        return Err(format!("Reaper still shows {} active alerts after expiry", reaper.active_count()));
    }

    // Integrate with FSM: simulate auto-archival transition
    let mut fsm = AlertStateMachine::new("alert-sla-expiry-001");
    let mut journal = LifecycleJournal::new();

    // FSM is in New; reaper fires → transition to ExpiredArchived
    let result = fsm.transition(TransitionRequest {
        target: AlertState::ExpiredArchived,
        actor: "alert_reaper",
        reason: "1h SLA exceeded without analyst triage",
    });
    match result {
        Ok(tr) => {
            journal.append_entry(tr.previous_state, tr.new_state, &tr.actor, &tr.reason);
        }
        Err(e) => return Err(format!("FSM auto-archival transition failed: {e}")),
    }

    if fsm.state() != AlertState::ExpiredArchived {
        return Err("FSM not in ExpiredArchived after auto-archival".to_owned());
    }
    if !journal.verify_integrity() {
        return Err("Journal integrity failed after auto-archival entry".to_owned());
    }

    // Verify ExpiredArchived is terminal
    let terminal_err = fsm.transition(TransitionRequest {
        target: AlertState::Acknowledged,
        actor: "analyst",
        reason: "should be rejected",
    });
    if terminal_err.is_ok() {
        return Err("Transition out of ExpiredArchived was allowed – must be terminal".to_owned());
    }

    Ok(format!(
        "No expiry at 30min: OK | Expiry fired at 61min: OK | \
         alert_id=alert-sla-expiry-001 in callback: OK | \
         FSM transitioned to ExpiredArchived: OK | Journal integrity post-archival: OK | \
         ExpiredArchived is terminal: OK"
    ))
}

// ─── Test 4: Multi-Channel Routing & Circuit Breaker ─────────────────────────

fn test_routing_circuit_breaker() -> Result<String, String> {
    let call_counter = Arc::new(AtomicU32::new(0));
    let call_counter_clone = Arc::clone(&call_counter);

    // Build a mock webhook sink that always returns HTTP 500
    let failing_webhook = Arc::new(WebhookSink::new_with_transport(
        "pagerduty-mock",
        "https://mock.pagerduty.local/v1/enqueue",
        move |_url, _body| {
            call_counter_clone.fetch_add(1, Ordering::Relaxed);
            Err(SinkError::Http {
                status: 500,
                body: "Internal Server Error".to_owned(),
            })
        },
    ));

    let cb_sink = Arc::new(CircuitBreakerSink::new(
        failing_webhook,
        BreakerConfig {
            failure_threshold: 5,
            recovery_duration: Duration::from_secs(30),
        },
    ));

    let make_alert = |id: &str| NormalizedAlert {
        id: id.to_owned(),
        title: "Critical: Lateral Movement Detected".to_owned(),
        severity: Severity::Critical,
        tenant_id: "tenant-xyz".to_owned(),
        tags: vec!["lateral_movement".to_owned()],
        source_rule_id: "rule-lateral-001".to_owned(),
        timestamp_ns: 0,
        explainability: None,
    };

    // Calls 1–5: breaker Closed, each call reaches the sink and fails
    for i in 1..=5 {
        let res = cb_sink.send(&make_alert(&format!("a-cb-{i:03}")));
        if res.is_ok() {
            return Err(format!("Call {i}: expected Err but got Ok"));
        }
        // Should NOT be CircuitOpen for calls 1-4, breaker trips on call 5
        match res.unwrap_err() {
            SinkError::Http { status: 500, .. } => {}
            SinkError::CircuitOpen => {
                if i < 5 {
                    return Err(format!("Circuit opened prematurely on call {i}"));
                }
            }
            other => return Err(format!("Call {i}: unexpected error variant: {other}")),
        }
    }

    // After 5 failures the breaker must be Open
    if cb_sink.breaker_state() != BreakerState::Open {
        return Err(format!(
            "Expected BreakerState::Open after 5 failures, got {:?}",
            cb_sink.breaker_state()
        ));
    }
    if cb_sink.trip_count() != 1 {
        return Err(format!("Expected 1 trip, got {}", cb_sink.trip_count()));
    }

    // Call 6: breaker is Open → CircuitOpen error immediately (no sink call)
    let before_calls = call_counter.load(Ordering::Relaxed);
    let res6 = cb_sink.send(&make_alert("a-cb-006"));
    match res6 {
        Err(SinkError::CircuitOpen) => {}
        Err(e) => return Err(format!("Call 6: expected CircuitOpen, got {e}")),
        Ok(()) => return Err("Call 6: expected CircuitOpen, got Ok".to_owned()),
    }
    let after_calls = call_counter.load(Ordering::Relaxed);
    if after_calls != before_calls {
        return Err(format!(
            "Sink was called during Open breaker state (calls before={before_calls}, after={after_calls})"
        ));
    }

    // Retry queue must hold all 6 alerts (5 from failed sends + 1 from open circuit)
    let queue_depth = cb_sink.retry_queue_depth();
    if queue_depth != 6 {
        return Err(format!(
            "Expected retry queue depth 6, got {queue_depth}"
        ));
    }

    // Verify alerts are NOT dropped: queue depth accounts for all rejected deliveries
    Ok(format!(
        "Calls 1-5 reached failing sink: OK (sink called {} times) | \
         Breaker state=Open after 5 failures: OK | \
         trip_count=1: OK | \
         Call 6 returned CircuitOpen (sink NOT called): OK | \
         Retry queue depth={queue_depth} (all 6 alerts retained, zero dropped): OK",
        call_counter.load(Ordering::Relaxed)
    ))
}

// ─── Test 5: Lock-Free Delivery Metrics Tracking ─────────────────────────────

fn test_delivery_metrics() -> Result<String, String> {
    let registry = AlertDeliveryMetrics::new();

    let kafka_metrics = registry.register_channel("kafka.medium-alerts");
    let webhook_metrics = registry.register_channel("webhook.pagerduty");

    // Simulate 10,000 total dispatches:
    //  - 7,000 to Kafka (success, latency ~50µs = 50_000ns)
    //  - 3,000 to Webhook: 2,500 success (latency ~10ms), 500 failure (latency ~500ms)
    const KAFKA_TOTAL: u64 = 7_000;
    const WEBHOOK_SUCCESS: u64 = 2_500;
    const WEBHOOK_FAIL: u64 = 500;
    const TOTAL: u64 = KAFKA_TOTAL + WEBHOOK_SUCCESS + WEBHOOK_FAIL;

    for _ in 0..KAFKA_TOTAL {
        kafka_metrics.record_routed();
        kafka_metrics.record_success(50_000); // 50µs
    }

    for _ in 0..WEBHOOK_SUCCESS {
        webhook_metrics.record_routed();
        webhook_metrics.record_success(10_000_000); // 10ms
    }

    for _ in 0..WEBHOOK_FAIL {
        webhook_metrics.record_routed();
        webhook_metrics.record_failure(500_000_000); // 500ms timeout
        webhook_metrics.record_retry();
    }

    // Verify exact counter values
    if kafka_metrics.routed_total() != KAFKA_TOTAL {
        return Err(format!(
            "Kafka routed_total: expected {KAFKA_TOTAL}, got {}",
            kafka_metrics.routed_total()
        ));
    }
    if kafka_metrics.succeeded() != KAFKA_TOTAL {
        return Err(format!(
            "Kafka succeeded: expected {KAFKA_TOTAL}, got {}",
            kafka_metrics.succeeded()
        ));
    }
    if kafka_metrics.failed() != 0 {
        return Err(format!(
            "Kafka failed: expected 0, got {}",
            kafka_metrics.failed()
        ));
    }

    if webhook_metrics.routed_total() != WEBHOOK_SUCCESS + WEBHOOK_FAIL {
        return Err(format!(
            "Webhook routed_total: expected {}, got {}",
            WEBHOOK_SUCCESS + WEBHOOK_FAIL,
            webhook_metrics.routed_total()
        ));
    }
    if webhook_metrics.succeeded() != WEBHOOK_SUCCESS {
        return Err(format!(
            "Webhook succeeded: expected {WEBHOOK_SUCCESS}, got {}",
            webhook_metrics.succeeded()
        ));
    }
    if webhook_metrics.failed() != WEBHOOK_FAIL {
        return Err(format!(
            "Webhook failed: expected {WEBHOOK_FAIL}, got {}",
            webhook_metrics.failed()
        ));
    }
    if webhook_metrics.retried() != WEBHOOK_FAIL {
        return Err(format!(
            "Webhook retried: expected {WEBHOOK_FAIL}, got {}",
            webhook_metrics.retried()
        ));
    }

    // P99 Kafka latency: all samples are at 50µs (50_000ns → bucket floor(log2(50000))=15)
    // so P99 must fall in bucket 15 range [32768, 65536)
    let kafka_p99 = kafka_metrics.percentile_ns(99.0);
    if kafka_p99 < 32_768 || kafka_p99 > 65_536 {
        return Err(format!(
            "Kafka P99 latency out of expected range [32768, 65536]: {kafka_p99}"
        ));
    }

    // Total dispatches across both channels
    let total_dispatched = kafka_metrics.routed_total() + webhook_metrics.routed_total();
    if total_dispatched != TOTAL {
        return Err(format!("Total dispatches: expected {TOTAL}, got {total_dispatched}"));
    }

    // Verify Prometheus output is well-formed
    let prom = registry.all_prometheus_text();
    if !prom.contains("kafka.medium-alerts") {
        return Err("Prometheus output missing kafka channel label".to_owned());
    }
    if !prom.contains("webhook.pagerduty") {
        return Err("Prometheus output missing webhook channel label".to_owned());
    }

    Ok(format!(
        "Total dispatches={total_dispatched}: OK | \
         Kafka: routed={}, succeeded={}, failed=0: OK | \
         Webhook: routed={}, succeeded={}, failed={}: OK | \
         Kafka P99 latency={kafka_p99}ns (within [32768,65536]): OK | \
         Prometheus text contains both channel labels: OK",
        kafka_metrics.routed_total(),
        kafka_metrics.succeeded(),
        webhook_metrics.routed_total(),
        webhook_metrics.succeeded(),
        webhook_metrics.failed()
    ))
}

// ─── Test 6: Granular Detection Explainability Payload ───────────────────────

fn test_explainability_payload() -> Result<String, String> {
    let rule = build_powershell_download_rule();
    let ctx = build_powershell_eval_context();

    let meta = EvidenceAttributor::attribute(&rule, &ctx);

    // Condition string
    if meta.evaluated_condition_str != "selection AND NOT filter" {
        return Err(format!(
            "condition_str mismatch: got '{}'",
            meta.evaluated_condition_str
        ));
    }

    // Satisfied identifiers
    if !meta.satisfied_search_identifiers.contains(&"selection".to_owned()) {
        return Err("'selection' not in satisfied_search_identifiers".to_owned());
    }
    if meta.satisfied_search_identifiers.contains(&"filter".to_owned()) {
        return Err("'filter' must NOT be in satisfied_search_identifiers".to_owned());
    }

    // Field evidence for process.command_line
    let fe = meta
        .matched_fields
        .iter()
        .find(|f| f.field == "process.command_line")
        .ok_or("process.command_line missing from matched_fields")?;

    if fe.matched_pattern != "*powershell*" {
        return Err(format!(
            "matched_pattern should be '*powershell*', got '{}'",
            fe.matched_pattern
        ));
    }
    if !fe.exact_value.to_lowercase().contains("powershell") {
        return Err(format!(
            "exact_value does not contain 'powershell': '{}'",
            fe.exact_value
        ));
    }
    if fe.pattern_type != "glob" {
        return Err(format!("pattern_type should be 'glob', got '{}'", fe.pattern_type));
    }

    // NOT-branch filter must not appear as positive evidence
    for evidence in &meta.matched_fields {
        if evidence.matched_pattern.contains("UpdateCheck") {
            return Err("NOT-branch filter pattern appeared in positive evidence".to_owned());
        }
    }

    // MITRE technique
    if meta.mitre_technique_id != "T1059.001" {
        return Err(format!(
            "MITRE technique mismatch: expected T1059.001, got '{}'",
            meta.mitre_technique_id
        ));
    }

    // Serialisation round-trip
    let json = serde_json::to_string_pretty(&meta)
        .map_err(|e| format!("Serialisation failed: {e}"))?;
    let decoded: detection_engine_phase13::ExplainabilityMetadata =
        serde_json::from_str(&json)
            .map_err(|e| format!("Deserialisation failed: {e}"))?;
    if decoded != meta {
        return Err("Serialisation round-trip produced different ExplainabilityMetadata".to_owned());
    }

    Ok(format!(
        "condition_str='selection AND NOT filter': OK | \
         satisfied=['selection']: OK | \
         process.command_line evidence present: OK | \
         matched_pattern='*powershell*': OK | \
         exact_value contains 'powershell': OK | \
         pattern_type='glob': OK | \
         NOT-branch not in evidence: OK | \
         MITRE=T1059.001: OK | \
         JSON round-trip: OK"
    ))
}

// ─── main ────────────────────────────────────────────────────────────────────

fn main() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  Phase XIII – Alerting & Detection Lifecycle Conclusion             ║");
    println!("║  Features 73–78 Verification Harness                                ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    let tests: Vec<(&str, Box<dyn Fn() -> Result<String, String>>)> = vec![
        (
            "Test 1 [F73]: FSM State Transition Permutations",
            Box::new(test_fsm_state_transitions),
        ),
        (
            "Test 2 [F74]: Cryptographic History Journal Integrity",
            Box::new(test_journal_integrity),
        ),
        (
            "Test 3 [F75]: Alert SLA Expiration & Auto-Archival",
            Box::new(test_alert_expiration),
        ),
        (
            "Test 4 [F76]: Multi-Channel Routing & Circuit Breaker",
            Box::new(test_routing_circuit_breaker),
        ),
        (
            "Test 5 [F77]: Lock-Free Delivery Metrics Tracking",
            Box::new(test_delivery_metrics),
        ),
        (
            "Test 6 [F78]: Granular Detection Explainability Payload",
            Box::new(test_explainability_payload),
        ),
    ];

    let mut results: Vec<TestResult> = Vec::new();

    for (name, test_fn) in &tests {
        print!("  ▶  {name} ... ");
        let r = run_test(name, || test_fn());
        if r.passed {
            println!("PASS ({:.2?})", r.duration);
        } else {
            println!("FAIL ({:.2?})", r.duration);
        }
        if !r.detail.is_empty() {
            for line in r.detail.split(" | ") {
                println!("       • {line}");
            }
        }
        results.push(r);
    }

    println!();
    println!("─────────────────────────────────────────────────────────────────────");

    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.iter().filter(|r| !r.passed).count();
    let total_time: Duration = results.iter().map(|r| r.duration).sum();

    println!("  Results: {passed}/{} passed  |  Failed: {failed}  |  Total time: {total_time:.2?}", results.len());

    if failed > 0 {
        println!();
        println!("FAILED TESTS:");
        for r in results.iter().filter(|r| !r.passed) {
            println!("  ✗  {}", r.name);
            println!("     {}", r.detail);
        }
        println!();
        std::process::exit(1);
    } else {
        println!();
        println!("  ✔  ALL 6 TESTS PASSED — Phase XIII implementation verified.");
        println!();
    }
}
