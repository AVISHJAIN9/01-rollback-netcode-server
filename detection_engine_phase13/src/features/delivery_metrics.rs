/// Feature 77: Lock-Free Alert Delivery Telemetry Metrics
///
/// All counters use `AtomicU64` with `Relaxed` ordering for absolute minimum
/// overhead (≤ 10 ns per increment on x86-64 and ARM64).  The latency
/// histogram uses lock-free logarithmic buckets: bucket index is derived from
/// `floor(log2(latency_ns))` clamped to [0, NUM_BUCKETS).
///
/// A Prometheus text-format serialiser is provided for scraping.
///
/// Per-channel `ChannelMetrics` can be registered and looked up by name.
/// The registry itself uses a standard `Mutex` only during registration
/// (cold path), never during the hot record path.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

/// Number of histogram buckets.  Bucket `i` counts latencies in the range
/// `[2^i, 2^(i+1))` nanoseconds.  Bucket 63 accumulates all values ≥ 2^63 ns
/// (i.e., longer than ~292 years, which should never occur in practice).
pub const NUM_BUCKETS: usize = 64;

/// Per-channel lock-free delivery metrics.
#[derive(Debug)]
pub struct ChannelMetrics {
    /// Human-readable channel label (e.g. "webhook.pagerduty").
    pub channel: String,

    /// Total alerts submitted to this channel.
    pub alerts_routed_total: AtomicU64,

    /// Alerts successfully acknowledged by the downstream sink.
    pub deliveries_succeeded: AtomicU64,

    /// Alerts that failed after all retries.
    pub deliveries_failed: AtomicU64,

    /// Alerts queued into the retry buffer at least once.
    pub retries_queued: AtomicU64,

    /// Number of times the circuit breaker transitioned to `Open`.
    pub circuit_breaker_trips: AtomicU64,

    /// Logarithmic histogram of end-to-end delivery latencies (nanoseconds).
    /// `latency_histogram[i]` counts alerts with latency in [2^i, 2^(i+1)) ns.
    pub latency_histogram: [AtomicU64; NUM_BUCKETS],
}

impl ChannelMetrics {
    /// Construct zeroed metrics for `channel`.
    pub fn new(channel: impl Into<String>) -> Self {
        // AtomicU64 does not implement Copy, so we must initialise with a macro.
        // We use a const-initialised array via `std::array::from_fn`.
        Self {
            channel: channel.into(),
            alerts_routed_total: AtomicU64::new(0),
            deliveries_succeeded: AtomicU64::new(0),
            deliveries_failed: AtomicU64::new(0),
            retries_queued: AtomicU64::new(0),
            circuit_breaker_trips: AtomicU64::new(0),
            latency_histogram: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    // ─── Hot-path record helpers ──────────────────────────────────────────

    #[inline]
    pub fn record_routed(&self) {
        self.alerts_routed_total.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_success(&self, latency_ns: u64) {
        self.deliveries_succeeded.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency_ns);
    }

    #[inline]
    pub fn record_failure(&self, latency_ns: u64) {
        self.deliveries_failed.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency_ns);
    }

    #[inline]
    pub fn record_retry(&self) {
        self.retries_queued.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_circuit_trip(&self) {
        self.circuit_breaker_trips.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a latency sample into the logarithmic histogram.
    ///
    /// Bucket index = `floor(log2(latency_ns))` clamped to [0, NUM_BUCKETS-1].
    /// Special case: latency_ns == 0 maps to bucket 0.
    #[inline]
    pub fn record_latency(&self, latency_ns: u64) {
        let bucket = if latency_ns == 0 {
            0
        } else {
            let msb = 63 - latency_ns.leading_zeros() as usize;
            msb.min(NUM_BUCKETS - 1)
        };
        self.latency_histogram[bucket].fetch_add(1, Ordering::Relaxed);
    }

    // ─── Snapshot helpers ─────────────────────────────────────────────────

    /// Load the current value of `alerts_routed_total`.
    pub fn routed_total(&self) -> u64 {
        self.alerts_routed_total.load(Ordering::Relaxed)
    }

    /// Load the current value of `deliveries_succeeded`.
    pub fn succeeded(&self) -> u64 {
        self.deliveries_succeeded.load(Ordering::Relaxed)
    }

    /// Load the current value of `deliveries_failed`.
    pub fn failed(&self) -> u64 {
        self.deliveries_failed.load(Ordering::Relaxed)
    }

    /// Load the current value of `retries_queued`.
    pub fn retried(&self) -> u64 {
        self.retries_queued.load(Ordering::Relaxed)
    }

    /// Load the current value of `circuit_breaker_trips`.
    pub fn breaker_trips(&self) -> u64 {
        self.circuit_breaker_trips.load(Ordering::Relaxed)
    }

    /// Return a snapshot of the histogram as a plain array.
    pub fn histogram_snapshot(&self) -> [u64; NUM_BUCKETS] {
        std::array::from_fn(|i| self.latency_histogram[i].load(Ordering::Relaxed))
    }

    /// Estimate the P-th percentile delivery latency from the histogram.
    ///
    /// The estimate is the lower bound of the bucket that contains the
    /// P-th percentile sample (i.e., `2^bucket_index` nanoseconds).
    pub fn percentile_ns(&self, percentile: f64) -> u64 {
        assert!(
            (0.0..=100.0).contains(&percentile),
            "percentile must be in [0, 100]"
        );

        let total: u64 = self
            .latency_histogram
            .iter()
            .map(|b| b.load(Ordering::Relaxed))
            .sum();

        if total == 0 {
            return 0;
        }

        let target = (total as f64 * percentile / 100.0).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, bucket) in self.latency_histogram.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= target {
                return if i == 0 { 0 } else { 1u64 << i };
            }
        }

        1u64 << (NUM_BUCKETS - 1)
    }

    /// Render Prometheus-compatible text exposition format for this channel.
    pub fn prometheus_text(&self) -> String {
        let ch = &self.channel;
        let mut out = String::with_capacity(512);

        out.push_str(&format!(
            "# HELP alerts_routed_total Total alerts routed to channel\n\
             # TYPE alerts_routed_total counter\n\
             alerts_routed_total{{channel=\"{ch}\"}} {}\n",
            self.routed_total()
        ));

        out.push_str(&format!(
            "# HELP deliveries_succeeded Successful alert deliveries\n\
             # TYPE deliveries_succeeded counter\n\
             deliveries_succeeded{{channel=\"{ch}\"}} {}\n",
            self.succeeded()
        ));

        out.push_str(&format!(
            "# HELP deliveries_failed Failed alert deliveries (post-retry)\n\
             # TYPE deliveries_failed counter\n\
             deliveries_failed{{channel=\"{ch}\"}} {}\n",
            self.failed()
        ));

        out.push_str(&format!(
            "# HELP retries_queued Alerts placed into retry queue\n\
             # TYPE retries_queued counter\n\
             retries_queued{{channel=\"{ch}\"}} {}\n",
            self.retried()
        ));

        out.push_str(&format!(
            "# HELP circuit_breaker_trips Circuit breaker Open transitions\n\
             # TYPE circuit_breaker_trips counter\n\
             circuit_breaker_trips{{channel=\"{ch}\"}} {}\n",
            self.breaker_trips()
        ));

        // Histogram
        out.push_str(&format!(
            "# HELP delivery_latency_ns_histogram Delivery latency logarithmic histogram\n\
             # TYPE delivery_latency_ns_histogram histogram\n"
        ));
        let snap = self.histogram_snapshot();
        let mut cumulative = 0u64;
        for (i, count) in snap.iter().enumerate() {
            cumulative += count;
            let upper_bound = if i == 0 { 1u64 } else { 1u64 << i };
            out.push_str(&format!(
                "delivery_latency_ns_histogram_bucket{{channel=\"{ch}\",le=\"{upper_bound}\"}} {cumulative}\n"
            ));
        }
        out.push_str(&format!(
            "delivery_latency_ns_histogram_bucket{{channel=\"{ch}\",le=\"+Inf\"}} {cumulative}\n"
        ));

        out
    }
}

// ─── AlertDeliveryMetrics registry ───────────────────────────────────────────

/// Global registry of per-channel `ChannelMetrics`.
///
/// Registration is protected by a `Mutex`; the hot-path `get` returns an
/// `Arc<ChannelMetrics>` that can be used without any lock.
#[derive(Debug, Default)]
pub struct AlertDeliveryMetrics {
    channels: Mutex<HashMap<String, Arc<ChannelMetrics>>>,
}

impl AlertDeliveryMetrics {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new channel.  Returns the `Arc<ChannelMetrics>` that callers
    /// should retain for lock-free recording.
    pub fn register_channel(&self, channel: impl Into<String>) -> Arc<ChannelMetrics> {
        let name: String = channel.into();
        let mut map = self.channels.lock().unwrap();
        let metrics = Arc::new(ChannelMetrics::new(name.clone()));
        map.insert(name, Arc::clone(&metrics));
        metrics
    }

    /// Retrieve metrics for an already-registered channel.
    pub fn get_channel(&self, channel: &str) -> Option<Arc<ChannelMetrics>> {
        self.channels.lock().unwrap().get(channel).cloned()
    }

    /// Render all channels' Prometheus text output concatenated.
    pub fn all_prometheus_text(&self) -> String {
        let map = self.channels.lock().unwrap();
        map.values()
            .map(|m| m.prometheus_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_increment_correctly() {
        let m = ChannelMetrics::new("webhook.pagerduty");
        for _ in 0..100 {
            m.record_routed();
            m.record_success(1_000_000);
        }
        for _ in 0..5 {
            m.record_routed();
            m.record_failure(5_000_000);
            m.record_retry();
        }
        assert_eq!(m.routed_total(), 105);
        assert_eq!(m.succeeded(), 100);
        assert_eq!(m.failed(), 5);
        assert_eq!(m.retried(), 5);
    }

    #[test]
    fn latency_histogram_bucket_mapping() {
        let m = ChannelMetrics::new("kafka.siem");
        // latency 0 → bucket 0
        m.record_latency(0);
        // latency 1 → bucket 0 (2^0=1, msb=0)
        m.record_latency(1);
        // latency 2 → bucket 1 (msb=1)
        m.record_latency(2);
        // latency 1_000_000_000 (1s) → bucket floor(log2(10^9))=29
        m.record_latency(1_000_000_000);

        let snap = m.histogram_snapshot();
        assert_eq!(snap[0], 2, "two samples in bucket 0");
        assert_eq!(snap[1], 1, "one sample in bucket 1");
        assert_eq!(snap[29], 1, "one 1-second sample in bucket 29");
    }

    #[test]
    fn percentile_p99_estimation() {
        let m = ChannelMetrics::new("test.p99");
        // Record 99 fast samples (~1µs = 1_000ns → bucket 9) and 1 slow (1ms = 10^6ns → bucket 19)
        for _ in 0..99 {
            m.record_latency(1_000);
        }
        m.record_latency(1_000_000);

        // P99 should land in the fast bucket (bucket 9: [512ns, 1024ns))
        let p99 = m.percentile_ns(99.0);
        assert!(
            p99 <= 1_024,
            "P99 should be ≤ 1024 ns; got {p99}"
        );

        // P100 should reflect the slow sample (bucket 19: [524288ns, ~1048576ns))
        let p100 = m.percentile_ns(100.0);
        assert!(
            p100 >= 524_288,
            "P100 should be ≥ 524288 ns; got {p100}"
        );
    }

    #[test]
    fn registry_channel_lookup() {
        let registry = AlertDeliveryMetrics::new();
        let m = registry.register_channel("kafka.medium-alerts");
        m.record_routed();
        m.record_success(500_000);

        let fetched = registry.get_channel("kafka.medium-alerts").unwrap();
        assert_eq!(fetched.routed_total(), 1);
        assert_eq!(fetched.succeeded(), 1);
    }

    #[test]
    fn prometheus_text_contains_channel_label() {
        let m = ChannelMetrics::new("webhook.test");
        m.record_routed();
        let text = m.prometheus_text();
        assert!(
            text.contains("channel=\"webhook.test\""),
            "Prometheus output must include channel label"
        );
        assert!(text.contains("alerts_routed_total"));
        assert!(text.contains("deliveries_succeeded"));
        assert!(text.contains("circuit_breaker_trips"));
    }
}
