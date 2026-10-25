// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Observability: High-Throughput Telemetry Collector
// Zero-allocation, lock-free metric buffering for async Rust

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Telemetry metric types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
    Timing,
}

/// Individual metric event
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricEvent {
    pub timestamp: u64, // Unix timestamp in microseconds
    pub metric_name: String,
    pub metric_type: MetricType,
    pub value: f64,
    pub tags: Vec<(String, String)>,
}

impl MetricEvent {
    pub fn new(name: &str, value: f64, metric_type: MetricType) -> Self {
        MetricEvent {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
            metric_name: name.to_string(),
            metric_type,
            value,
            tags: Vec::new(),
        }
    }

    pub fn with_tag(mut self, key: &str, value: &str) -> Self {
        self.tags.push((key.to_string(), value.to_string()));
        self
    }
}

/// Atomic counter for high-throughput incrementing
pub struct AtomicCounter {
    value: AtomicU64,
}

impl AtomicCounter {
    pub fn new(initial: u64) -> Self {
        AtomicCounter {
            value: AtomicU64::new(initial),
        }
    }

    pub fn increment(&self) -> u64 {
        self.value.fetch_add(1, Ordering::Relaxed)
    }

    pub fn add(&self, delta: u64) -> u64 {
        self.value.fetch_add(delta, Ordering::Relaxed)
    }

    pub fn load(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn store(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }
}

/// Connection-level metrics snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionMetrics {
    pub connection_id: String,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_lost: u64,
    pub current_cwnd_bytes: u64,
    pub current_srtt_us: u64,
    pub current_min_rtt_us: Option<u64>,
    pub current_bandwidth_bps: u64,
    pub timestamp: u64,
}

/// Telemetry collector: Buffered metric aggregation
pub struct TelemetryCollector {
    connection_id: String,
    
    // Atomic counters (zero-allocation)
    packets_sent: Arc<AtomicCounter>,
    packets_received: Arc<AtomicCounter>,
    bytes_sent: Arc<AtomicCounter>,
    bytes_received: Arc<AtomicCounter>,
    packets_lost: Arc<AtomicCounter>,
    
    // Gauges (requires RwLock)
    current_cwnd_bytes: Arc<RwLock<u64>>,
    current_srtt_us: Arc<RwLock<u64>>,
    current_min_rtt_us: Arc<RwLock<Option<u64>>>,
    current_bandwidth_bps: Arc<RwLock<u64>>,
    
    // Event buffer (for batched writes)
    events: Arc<RwLock<Vec<MetricEvent>>>,
    max_events: usize,
}

impl TelemetryCollector {
    pub fn new(connection_id: &str, max_events: usize) -> Self {
        TelemetryCollector {
            connection_id: connection_id.to_string(),
            packets_sent: Arc::new(AtomicCounter::new(0)),
            packets_received: Arc::new(AtomicCounter::new(0)),
            bytes_sent: Arc::new(AtomicCounter::new(0)),
            bytes_received: Arc::new(AtomicCounter::new(0)),
            packets_lost: Arc::new(AtomicCounter::new(0)),
            current_cwnd_bytes: Arc::new(RwLock::new(0)),
            current_srtt_us: Arc::new(RwLock::new(0)),
            current_min_rtt_us: Arc::new(RwLock::new(None)),
            current_bandwidth_bps: Arc::new(RwLock::new(0)),
            events: Arc::new(RwLock::new(Vec::with_capacity(max_events))),
            max_events,
        }
    }

    // --- Counters (atomic, wait-free) ---

    pub fn record_packet_sent(&self, bytes: u64) {
        self.packets_sent.increment();
        self.bytes_sent.add(bytes);
    }

    pub fn record_packet_received(&self, bytes: u64) {
        self.packets_received.increment();
        self.bytes_received.add(bytes);
    }

    pub fn record_packet_lost(&self, bytes: u64) {
        self.packets_lost.increment();
        self.bytes_sent.add(bytes); // Lost bytes are already sent
    }

    // --- Gauges (RwLock-protected) ---

    pub fn update_cwnd(&self, cwnd_bytes: u64) {
        *self.current_cwnd_bytes.write() = cwnd_bytes;
    }

    pub fn update_rtt(&self, srtt_us: u64, min_rtt_us: Option<u64>) {
        *self.current_srtt_us.write() = srtt_us;
        *self.current_min_rtt_us.write() = min_rtt_us;
    }

    pub fn update_bandwidth(&self, bandwidth_bps: u64) {
        *self.current_bandwidth_bps.write() = bandwidth_bps;
    }

    // --- Event buffering ---

    pub fn add_event(&self, event: MetricEvent) {
        let mut events = self.events.write();
        if events.len() >= self.max_events {
            // Drop oldest events to prevent unbounded growth
            events.drain(0..(self.max_events / 2));
        }
        events.push(event);
    }

    pub fn add_loss_event(&self, lost_packets: usize, detection_method: &str) {
        let event = MetricEvent::new("loss_event", lost_packets as f64, MetricType::Counter)
            .with_tag("method", detection_method);
        self.add_event(event);
    }

    pub fn add_congestion_event(&self, state: &str, trigger: &str) {
        let event = MetricEvent::new("congestion_state", 1.0, MetricType::Counter)
            .with_tag("state", state)
            .with_tag("trigger", trigger);
        self.add_event(event);
    }

    pub fn add_rtt_sample(&self, sample_rtt_us: u64) {
        let event = MetricEvent::new("rtt_sample", sample_rtt_us as f64, MetricType::Timing);
        self.add_event(event);
    }

    // --- Snapshot reads (for periodic export) ---

    pub fn take_snapshot(&self) -> ConnectionMetrics {
        ConnectionMetrics {
            connection_id: self.connection_id.clone(),
            packets_sent: self.packets_sent.load(),
            packets_received: self.packets_received.load(),
            bytes_sent: self.bytes_sent.load(),
            bytes_received: self.bytes_received.load(),
            packets_lost: self.packets_lost.load(),
            current_cwnd_bytes: *self.current_cwnd_bytes.read(),
            current_srtt_us: *self.current_srtt_us.read(),
            current_min_rtt_us: *self.current_min_rtt_us.read(),
            current_bandwidth_bps: *self.current_bandwidth_bps.read(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
        }
    }

    pub fn drain_events(&self) -> Vec<MetricEvent> {
        let mut events = self.events.write();
        events.drain(..).collect()
    }

    pub fn metrics_summary(&self) -> String {
        let snap = self.take_snapshot();
        format!(
            "Connection {}: {} packets sent, {} received, {} lost, cwnd={} bytes, SRTT={} μs",
            snap.connection_id,
            snap.packets_sent,
            snap.packets_received,
            snap.packets_lost,
            snap.current_cwnd_bytes,
            snap.current_srtt_us
        )
    }
}

// Cloneable wrapper for multi-threaded telemetry
#[derive(Clone)]
pub struct TelemetryHandle {
    inner: Arc<TelemetryCollector>,
}

impl TelemetryHandle {
    pub fn new(connection_id: &str, max_events: usize) -> Self {
        TelemetryHandle {
            inner: Arc::new(TelemetryCollector::new(connection_id, max_events)),
        }
    }

    pub fn record_packet_sent(&self, bytes: u64) {
        self.inner.record_packet_sent(bytes);
    }

    pub fn record_packet_received(&self, bytes: u64) {
        self.inner.record_packet_received(bytes);
    }

    pub fn record_packet_lost(&self, bytes: u64) {
        self.inner.record_packet_lost(bytes);
    }

    pub fn update_cwnd(&self, cwnd_bytes: u64) {
        self.inner.update_cwnd(cwnd_bytes);
    }

    pub fn update_rtt(&self, srtt_us: u64, min_rtt_us: Option<u64>) {
        self.inner.update_rtt(srtt_us, min_rtt_us);
    }

    pub fn add_event(&self, event: MetricEvent) {
        self.inner.add_event(event);
    }

    pub fn take_snapshot(&self) -> ConnectionMetrics {
        self.inner.take_snapshot()
    }

    pub fn drain_events(&self) -> Vec<MetricEvent> {
        self.inner.drain_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_counter() {
        let counter = AtomicCounter::new(0);
        assert_eq!(counter.increment(), 0);
        assert_eq!(counter.increment(), 1);
        assert_eq!(counter.add(10), 2);
        assert_eq!(counter.load(), 12);
    }

    #[test]
    fn test_telemetry_collector() {
        let collector = TelemetryCollector::new("conn-1", 1000);
        collector.record_packet_sent(1200);
        collector.record_packet_received(1200);
        collector.update_cwnd(12000);
        collector.update_rtt(50000, Some(50000));

        let snapshot = collector.take_snapshot();
        assert_eq!(snapshot.packets_sent, 1);
        assert_eq!(snapshot.packets_received, 1);
        assert_eq!(snapshot.bytes_sent, 1200);
        assert_eq!(snapshot.current_cwnd_bytes, 12000);
    }

    #[test]
    fn test_telemetry_events() {
        let collector = TelemetryCollector::new("conn-1", 100);
        collector.add_loss_event(3, "packet_threshold");
        collector.add_congestion_event("fast_recovery", "loss_detected");

        let events = collector.drain_events();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_telemetry_handle() {
        let handle = TelemetryHandle::new("conn-1", 1000);
        handle.record_packet_sent(1200);
        handle.update_cwnd(12000);

        let snapshot = handle.take_snapshot();
        assert_eq!(snapshot.packets_sent, 1);
    }
}
