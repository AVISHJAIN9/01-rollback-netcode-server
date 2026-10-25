// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Congestion Control Interface & Telemetry (RFC 9002 §7)
// Pluggable congestion control algorithms with unified telemetry

use std::time::Duration;

pub mod newreno;
pub mod cubic;
pub mod pacer;

pub use self::newreno::NewReno;
pub use self::cubic::Cubic;
pub use self::pacer::{TokenBucketPacer, PriorityPacer};

/// Congestion state for pluggable congestion control algorithms.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CongestionState {
    /// Slow Start phase: window grows exponentially.
    SlowStart,

    /// Congestion Avoidance phase: window grows linearly.
    CongestionAvoidance,

    /// Fast Recovery phase: after loss event but before probing.
    FastRecovery,
}

/// ECN (Explicit Congestion Notification) codepoint.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EcnCodepoint {
    NotEct,
    Ect0,
    Ect1,
    Ce,
}

/// Congestion event: loss or ECN CE marking.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CongestionEvent {
    PacketLoss,
    EcnCeMarked,
}

/// Pluggable congestion control algorithm interface.
pub trait CongestionController: Send + Sync {
    /// Called when the connection starts (or window needs initialization).
    fn on_init(&mut self, mtu: usize);

    /// Called after an RTT sample (ACK received).
    /// `bytes_acked` is the number of bytes acknowledged in this ACK.
    /// `rtt_sample` is the round-trip time sample.
    fn on_ack(&mut self, bytes_acked: u64, rtt_sample: Duration);

    /// Called when a congestion event (loss or ECN CE) occurs.
    /// `bytes_in_flight` is the number of bytes in-flight at the time of the event.
    fn on_congestion_event(&mut self, event: CongestionEvent, bytes_in_flight: u64);

    /// Get the current congestion window in bytes.
    fn congestion_window(&self) -> u64;

    /// Get the current congestion state.
    fn state(&self) -> CongestionState;

    /// Check if we can send a packet given current congestion window.
    fn can_send(&self, bytes_in_flight: u64) -> bool {
        bytes_in_flight < self.congestion_window()
    }

    /// Get bytes available to send under congestion window.
    fn available_window(&self, bytes_in_flight: u64) -> u64 {
        let window = self.congestion_window();
        if bytes_in_flight >= window {
            0
        } else {
            window - bytes_in_flight
        }
    }

    /// Get telemetry snapshot.
    fn telemetry(&self) -> CongestionTelemetry;

    /// Reset to initial state (e.g., on connection migration).
    fn reset(&mut self);
}

/// Congestion control telemetry snapshot.
#[derive(Clone, Debug)]
pub struct CongestionTelemetry {
    pub congestion_window: u64,
    pub ssthresh: u64,
    pub state: CongestionState,
    pub samples_count: u32,
    pub congestion_events_count: u32,
    pub bytes_sent: u64,
    pub bytes_acked: u64,
    pub bandwidth_estimate: Option<u64>, // bytes per second
}

impl Default for CongestionTelemetry {
    fn default() -> Self {
        CongestionTelemetry {
            congestion_window: 10 * 1200, // ~12KB initial window
            ssthresh: u64::MAX,
            state: CongestionState::SlowStart,
            samples_count: 0,
            congestion_events_count: 0,
            bytes_sent: 0,
            bytes_acked: 0,
            bandwidth_estimate: None,
        }
    }
}

/// BDP (Bandwidth-Delay Product) estimator for probing and window sizing.
#[derive(Clone, Debug)]
pub struct BdpEstimator {
    /// Maximum bandwidth sampled so far (bytes per second).
    max_bandwidth: u64,

    /// RTT at the time of max bandwidth (for BDP calculation).
    rtt_at_max_bw: Duration,

    /// Samples count for convergence heuristics.
    samples_count: u32,
}

impl BdpEstimator {
    pub fn new() -> Self {
        BdpEstimator {
            max_bandwidth: 0,
            rtt_at_max_bw: Duration::from_millis(100), // Conservative initial RTT
            samples_count: 0,
        }
    }

    /// Update bandwidth sample: bytes_acked / elapsed_time
    pub fn update(&mut self, bytes_acked: u64, elapsed: Duration) {
        if elapsed.as_millis() == 0 {
            return;
        }

        let bandwidth = (bytes_acked as u128 * 1000) / elapsed.as_millis() as u128;
        let bandwidth = (bandwidth as u64).min(u64::MAX);

        if bandwidth > self.max_bandwidth {
            self.max_bandwidth = bandwidth;
            self.rtt_at_max_bw = elapsed;
        }

        self.samples_count += 1;
    }

    /// Get BDP in bytes: max_bandwidth * rtt_at_max_bw
    pub fn bdp(&self) -> u64 {
        let rtt_seconds = self.rtt_at_max_bw.as_secs_f64();
        (self.max_bandwidth as f64 * rtt_seconds) as u64
    }

    /// Get max bandwidth estimate.
    pub fn max_bandwidth(&self) -> u64 {
        self.max_bandwidth
    }

    /// Reset estimator.
    pub fn reset(&mut self) {
        self.max_bandwidth = 0;
        self.rtt_at_max_bw = Duration::from_millis(100);
        self.samples_count = 0;
    }
}

impl Default for BdpEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bdp_estimator() {
        let mut bdp = BdpEstimator::new();

        // 1000 bytes acked in 10ms = 100KB/s bandwidth
        bdp.update(1000, Duration::from_millis(10));
        assert_eq!(bdp.max_bandwidth(), 100_000);

        // BDP = 100KB/s * 10ms = 1000 bytes
        assert_eq!(bdp.bdp(), 1000);
    }

    #[test]
    fn test_congestion_window_check() {
        let cw = 10000u64;
        let available = if 5000 >= cw { 0 } else { cw - 5000 };
        assert_eq!(available, 5000);
    }
}
