// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: NewReno Congestion Control (RFC 9002 §7.2)
// Slow Start, Congestion Avoidance, Fast Recovery, and ssthresh management

use super::{CongestionController, CongestionEvent, CongestionState, CongestionTelemetry};
use std::time::Duration;

/// NewReno congestion control algorithm (RFC 9002 §7.2).
///
/// Implements:
/// - Slow Start: exponential window growth (cwnd += MSS per ACK)
/// - Congestion Avoidance: linear growth (cwnd += MSS^2/cwnd per RTT)
/// - Fast Recovery: ssthresh = max(cwnd/2, 2*MSS), cwnd = ssthresh
#[derive(Clone, Debug)]
pub struct NewReno {
    /// Congestion window in bytes.
    congestion_window: u64,

    /// Slow start threshold in bytes.
    ssthresh: u64,

    /// Maximum segment size (MTU in typical usage).
    mss: u64,

    /// Current congestion state.
    state: CongestionState,

    /// Bytes acknowledged in current round (for CA window growth).
    bytes_acked_in_round: u64,

    /// RTT sample at start of current congestion avoidance round.
    round_start_rtt: Option<Duration>,

    /// Telemetry: total bytes acked.
    total_bytes_acked: u64,

    /// Telemetry: congestion events count.
    congestion_events: u32,

    /// Telemetry: ACK samples count.
    samples_count: u32,
}

impl NewReno {
    pub fn new(mss: u64) -> Self {
        NewReno {
            congestion_window: mss * 10, // ~12KB initial window (RFC 9002)
            ssthresh: u64::MAX,
            mss,
            state: CongestionState::SlowStart,
            bytes_acked_in_round: 0,
            round_start_rtt: None,
            total_bytes_acked: 0,
            congestion_events: 0,
            samples_count: 0,
        }
    }

    /// RFC 9002 §7.2: Slow Start phase
    /// Increase congestion window by the number of bytes acknowledged.
    fn on_ack_slow_start(&mut self, bytes_acked: u64) {
        self.congestion_window = self.congestion_window.saturating_add(bytes_acked);

        // Transition to congestion avoidance if window reaches ssthresh
        if self.congestion_window >= self.ssthresh {
            self.state = CongestionState::CongestionAvoidance;
            self.bytes_acked_in_round = 0;
        }
    }

    /// RFC 9002 §7.2: Congestion Avoidance phase
    /// Increase congestion window by MSS^2 / congestion_window per RTT.
    fn on_ack_congestion_avoidance(&mut self, bytes_acked: u64) {
        self.bytes_acked_in_round = self.bytes_acked_in_round.saturating_add(bytes_acked);

        // Approximate RTT-wise window growth:
        // cwnd_increment = (bytes_acked * mss) / cwnd
        let increment = (bytes_acked.saturating_mul(self.mss)) / self.congestion_window.max(1);
        self.congestion_window = self.congestion_window.saturating_add(increment);
    }

    /// RFC 9002 §7.2: Fast Recovery after congestion event
    /// ssthresh = max(cwnd / 2, 2 * mss)
    /// cwnd = ssthresh + 3 * mss (fast retransmit)
    fn on_congestion_loss(&mut self, bytes_in_flight: u64) {
        self.ssthresh = ((self.congestion_window / 2).max(2 * self.mss)).max(bytes_in_flight / 2);
        self.congestion_window = self.ssthresh.saturating_add(3 * self.mss);
        self.state = CongestionState::FastRecovery;
        self.bytes_acked_in_round = 0;
    }

    /// Transition from Fast Recovery to Congestion Avoidance
    fn exit_fast_recovery(&mut self) {
        self.state = CongestionState::CongestionAvoidance;
        self.bytes_acked_in_round = 0;
    }
}

impl CongestionController for NewReno {
    fn on_init(&mut self, mtu: usize) {
        self.mss = mtu as u64;
        self.congestion_window = (self.mss * 10).min(14720); // RFC 9002: min(10*MSS, 14720)
        self.ssthresh = u64::MAX;
        self.state = CongestionState::SlowStart;
        self.bytes_acked_in_round = 0;
        self.round_start_rtt = None;
    }

    fn on_ack(&mut self, bytes_acked: u64, rtt_sample: Duration) {
        self.samples_count += 1;
        self.total_bytes_acked = self.total_bytes_acked.saturating_add(bytes_acked);

        // Track RTT for round start
        if self.round_start_rtt.is_none() {
            self.round_start_rtt = Some(rtt_sample);
        }

        match self.state {
            CongestionState::SlowStart => {
                self.on_ack_slow_start(bytes_acked);
            }
            CongestionState::CongestionAvoidance => {
                self.on_ack_congestion_avoidance(bytes_acked);
            }
            CongestionState::FastRecovery => {
                // In Fast Recovery, continue CA-like growth with extra ACKs for FACK
                self.on_ack_congestion_avoidance(bytes_acked);
            }
        }
    }

    fn on_congestion_event(&mut self, event: CongestionEvent, bytes_in_flight: u64) {
        self.congestion_events += 1;

        match event {
            CongestionEvent::PacketLoss => {
                // RFC 9002: On packet loss, reduce cwnd and enter recovery
                if self.state != CongestionState::FastRecovery {
                    self.on_congestion_loss(bytes_in_flight);
                }
            }
            CongestionEvent::EcnCeMarked => {
                // ECN CE: treat similarly to loss but less aggressive
                self.ssthresh = ((self.congestion_window / 2).max(2 * self.mss))
                    .max(bytes_in_flight / 2);
                self.congestion_window = self.ssthresh;
                self.state = CongestionState::CongestionAvoidance;
                self.bytes_acked_in_round = 0;
            }
        }
    }

    fn congestion_window(&self) -> u64 {
        self.congestion_window
    }

    fn state(&self) -> CongestionState {
        self.state
    }

    fn telemetry(&self) -> CongestionTelemetry {
        CongestionTelemetry {
            congestion_window: self.congestion_window,
            ssthresh: self.ssthresh,
            state: self.state,
            samples_count: self.samples_count,
            congestion_events_count: self.congestion_events,
            bytes_sent: 0, // Set by caller if needed
            bytes_acked: self.total_bytes_acked,
            bandwidth_estimate: None, // Optionally compute from RTT and window
        }
    }

    fn reset(&mut self) {
        *self = NewReno::new(self.mss);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_newreno_init() {
        let mut nr = NewReno::new(1200);
        assert_eq!(nr.congestion_window, 12000);
        assert_eq!(nr.state, CongestionState::SlowStart);

        nr.on_init(1200);
        assert_eq!(nr.congestion_window, 12000);
    }

    #[test]
    fn test_newreno_slow_start() {
        let mut nr = NewReno::new(1200);
        let initial_cwnd = nr.congestion_window;

        // In slow start, cwnd += bytes_acked
        nr.on_ack(1200, Duration::from_millis(50));
        assert_eq!(nr.congestion_window, initial_cwnd + 1200);
        assert_eq!(nr.state, CongestionState::SlowStart);
    }

    #[test]
    fn test_newreno_transition_to_ca() {
        let mut nr = NewReno::new(1200);
        nr.ssthresh = 12000; // Set to trigger transition

        // Ack enough to reach ssthresh
        nr.on_ack(12000, Duration::from_millis(50));
        assert_eq!(nr.state, CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_newreno_congestion_avoidance() {
        let mut nr = NewReno::new(1200);
        nr.state = CongestionState::CongestionAvoidance;
        nr.congestion_window = 24000;

        let initial_cwnd = nr.congestion_window;
        // In CA, cwnd grows by (bytes_acked * mss) / cwnd
        nr.on_ack(1200, Duration::from_millis(50));
        assert!(nr.congestion_window > initial_cwnd);
        assert_eq!(nr.state, CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_newreno_loss_event() {
        let mut nr = NewReno::new(1200);
        nr.congestion_window = 24000;
        nr.state = CongestionState::CongestionAvoidance;

        // Simulate packet loss
        nr.on_congestion_event(CongestionEvent::PacketLoss, 24000);

        // ssthresh should be cwnd/2 = 12000
        assert_eq!(nr.ssthresh, 12000);
        // cwnd should be ssthresh + 3*mss
        assert_eq!(nr.congestion_window, 12000 + 3 * 1200);
        assert_eq!(nr.state, CongestionState::FastRecovery);
    }

    #[test]
    fn test_newreno_ecn_ce() {
        let mut nr = NewReno::new(1200);
        nr.congestion_window = 24000;

        nr.on_congestion_event(CongestionEvent::EcnCeMarked, 24000);

        assert_eq!(nr.ssthresh, 12000);
        assert_eq!(nr.congestion_window, 12000);
        assert_eq!(nr.state, CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_newreno_can_send() {
        let nr = NewReno::new(1200);
        assert!(nr.can_send(0));
        assert!(nr.can_send(6000));
        assert!(!nr.can_send(12000));
        assert!(!nr.can_send(24000));
    }

    #[test]
    fn test_newreno_reset() {
        let mut nr = NewReno::new(1200);
        nr.congestion_window = 5000;
        nr.congestion_events = 5;
        nr.reset();

        assert_eq!(nr.congestion_window, 12000);
        assert_eq!(nr.congestion_events, 0);
        assert_eq!(nr.state, CongestionState::SlowStart);
    }
}
