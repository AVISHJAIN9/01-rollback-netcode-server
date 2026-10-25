// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: CUBIC Congestion Control
// RFC-compliant cubic window growth: W_cubic(t) = C*(t-K)^3 + W_max

use super::{CongestionController, CongestionEvent, CongestionState, CongestionTelemetry};
use std::time::{Duration, Instant};

/// CUBIC congestion control algorithm.
///
/// Implements RFC-compliant cubic growth:
/// W_cubic(t) = C * (t - K)^3 + W_max
/// where K = cubic_root(W_max * (1 - beta) / C)
/// C is a constant (typically 0.4), beta is congestion window reduction (0.7)
#[derive(Clone, Debug)]
pub struct Cubic {
    /// Congestion window in bytes.
    congestion_window: u64,

    /// W_max: window size at congestion event.
    w_max: u64,

    /// Slow start threshold.
    ssthresh: u64,

    /// Maximum segment size (MTU).
    mss: u64,

    /// Current congestion state.
    state: CongestionState,

    /// Time of last congestion event (for cubic function).
    congestion_event_time: Option<Instant>,

    /// Time connection started (reference for cubic epoch).
    epoch_start_time: Option<Instant>,

    /// Bytes acknowledged in current epoch.
    bytes_acked_in_epoch: u64,

    /// RTT sample used for cubic growth calculation.
    last_rtt: Duration,

    /// CUBIC constant (default 0.4).
    c: f64,

    /// Beta for window reduction on congestion (default 0.7).
    beta: f64,

    /// Telemetry counters.
    total_bytes_acked: u64,
    congestion_events: u32,
    samples_count: u32,
}

impl Cubic {
    pub fn new(mss: u64) -> Self {
        Cubic {
            congestion_window: mss * 10,
            w_max: mss * 10,
            ssthresh: u64::MAX,
            mss,
            state: CongestionState::SlowStart,
            congestion_event_time: None,
            epoch_start_time: None,
            bytes_acked_in_epoch: 0,
            last_rtt: Duration::from_millis(100),
            c: 0.4,
            beta: 0.7,
            total_bytes_acked: 0,
            congestion_events: 0,
            samples_count: 0,
        }
    }

    /// Compute cubic root using Newton's method.
    fn cubic_root(x: f64) -> f64 {
        if x == 0.0 {
            return 0.0;
        }
        let mut result = x / 3.0; // Initial guess
        for _ in 0..10 {
            result = (2.0 * result + x / (result * result)) / 3.0;
        }
        result
    }

    /// Compute W_cubic(t) = C * (t - K)^3 + W_max
    /// where K = cubic_root(W_max * (1 - beta) / C)
    fn cubic_window(&self, t: Duration) -> u64 {
        if let Some(_congestion_time) = self.congestion_event_time {
            let w_max_f = self.w_max as f64;

            // K = cubic_root(W_max * (1 - beta) / C)
            let k = Self::cubic_root(w_max_f * (1.0 - self.beta) / self.c);

            // Convert t to seconds
            let t_seconds = t.as_secs_f64();

            // W_cubic(t) = C * (t - K)^3 + W_max
            let window_f = if t_seconds >= k {
                self.c * (t_seconds - k).powi(3) + w_max_f
            } else {
                w_max_f * self.beta
            };

            window_f.max(self.mss as f64) as u64
        } else {
            self.mss * 10
        }
    }

    /// Compute TCP-friendly window growth (AIMD baseline).
    fn tcp_friendly_window(&self, rtt: Duration) -> u64 {
        // TCP increment: (1 - beta) / (3 - 2*beta) * MSS
        let tcp_increment_factor = (1.0 - self.beta) / (3.0 - 2.0 * self.beta);
        let increment = (tcp_increment_factor * self.mss as f64) as u64;

        // Grow window linearly over RTT
        let rtts_elapsed = if self.last_rtt.as_millis() > 0 {
            rtt.as_millis() as f64 / self.last_rtt.as_millis() as f64
        } else {
            1.0
        };

        self.w_max.saturating_add((increment as f64 * rtts_elapsed) as u64)
    }

    /// Check if in slow start and trigger exit condition.
    fn check_slow_start_exit(&mut self, bytes_acked: u64) {
        if self.state == CongestionState::SlowStart {
            self.congestion_window = self.congestion_window.saturating_add(bytes_acked);

            // Exit slow start at ssthresh
            if self.congestion_window >= self.ssthresh {
                self.state = CongestionState::CongestionAvoidance;
                self.epoch_start_time = Some(Instant::now());
                self.bytes_acked_in_epoch = 0;
            }
        }
    }

    /// Apply CUBIC window update in congestion avoidance.
    fn on_ack_cubic(&mut self, bytes_acked: u64, rtt_sample: Duration) {
        self.last_rtt = rtt_sample;

        if self.epoch_start_time.is_none() {
            self.epoch_start_time = Some(Instant::now());
        }

        self.bytes_acked_in_epoch = self.bytes_acked_in_epoch.saturating_add(bytes_acked);

        let epoch_start = self.epoch_start_time.unwrap();
        let now = Instant::now();
        let t = now.duration_since(epoch_start);

        // Compute target window
        let w_cubic = self.cubic_window(t);
        let w_tcp = self.tcp_friendly_window(rtt_sample);
        let w_target = w_cubic.max(w_tcp);

        // Update congestion window based on target
        if w_target > self.congestion_window {
            let increment = (w_target - self.congestion_window).min(bytes_acked);
            self.congestion_window = self.congestion_window.saturating_add(increment);
        } else if self.bytes_acked_in_epoch * self.mss >= self.congestion_window {
            // Modest linear increase if target is lower
            self.congestion_window = self.congestion_window.saturating_add(self.mss);
            self.bytes_acked_in_epoch = 0;
        }
    }
}

impl CongestionController for Cubic {
    fn on_init(&mut self, mtu: usize) {
        self.mss = mtu as u64;
        self.congestion_window = (self.mss * 10).min(14720);
        self.w_max = self.congestion_window;
        self.ssthresh = u64::MAX;
        self.state = CongestionState::SlowStart;
        self.epoch_start_time = None;
        self.congestion_event_time = None;
        self.bytes_acked_in_epoch = 0;
    }

    fn on_ack(&mut self, bytes_acked: u64, rtt_sample: Duration) {
        self.samples_count += 1;
        self.total_bytes_acked = self.total_bytes_acked.saturating_add(bytes_acked);

        match self.state {
            CongestionState::SlowStart => {
                self.check_slow_start_exit(bytes_acked);
            }
            CongestionState::CongestionAvoidance => {
                self.on_ack_cubic(bytes_acked, rtt_sample);
            }
            CongestionState::FastRecovery => {
                // In recovery, behave like CA
                self.on_ack_cubic(bytes_acked, rtt_sample);
            }
        }
    }

    fn on_congestion_event(&mut self, event: CongestionEvent, _bytes_in_flight: u64) {
        self.congestion_events += 1;
        self.congestion_event_time = Some(Instant::now());

        match event {
            CongestionEvent::PacketLoss => {
                // CUBIC on loss: reduce window by beta factor
                self.w_max = self.congestion_window;
                self.ssthresh = ((self.congestion_window as f64 * self.beta) as u64)
                    .max(2 * self.mss);
                self.congestion_window = self.ssthresh;

                self.state = CongestionState::FastRecovery;
                self.epoch_start_time = Some(Instant::now());
                self.bytes_acked_in_epoch = 0;
            }
            CongestionEvent::EcnCeMarked => {
                // ECN: lighter reduction than loss
                self.w_max = self.congestion_window;
                self.congestion_window = ((self.congestion_window as f64 * (1.0 - (1.0 - self.beta) / 2.0)) as u64)
                    .max(2 * self.mss);

                self.state = CongestionState::CongestionAvoidance;
                self.epoch_start_time = Some(Instant::now());
                self.bytes_acked_in_epoch = 0;
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
            bytes_sent: 0,
            bytes_acked: self.total_bytes_acked,
            bandwidth_estimate: None,
        }
    }

    fn reset(&mut self) {
        *self = Cubic::new(self.mss);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cubic_init() {
        let mut cubic = Cubic::new(1200);
        assert_eq!(cubic.congestion_window, 12000);
        assert_eq!(cubic.state, CongestionState::SlowStart);

        cubic.on_init(1200);
        assert_eq!(cubic.congestion_window, 12000);
    }

    #[test]
    fn test_cubic_slow_start() {
        let mut cubic = Cubic::new(1200);
        let initial_cwnd = cubic.congestion_window;

        cubic.on_ack(1200, Duration::from_millis(50));
        assert_eq!(cubic.congestion_window, initial_cwnd + 1200);
        assert_eq!(cubic.state, CongestionState::SlowStart);
    }

    #[test]
    fn test_cubic_root() {
        let root = Cubic::cubic_root(8.0);
        assert!((root - 2.0).abs() < 0.01);

        let root = Cubic::cubic_root(27.0);
        assert!((root - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_cubic_window_computation() {
        let mut cubic = Cubic::new(1200);
        cubic.congestion_window = 24000;
        cubic.w_max = 24000;
        cubic.congestion_event_time = Some(Instant::now());

        let w = cubic.cubic_window(Duration::from_secs(1));
        assert!(w > 0);
    }

    #[test]
    fn test_cubic_loss_event() {
        let mut cubic = Cubic::new(1200);
        cubic.congestion_window = 24000;
        cubic.state = CongestionState::CongestionAvoidance;

        cubic.on_congestion_event(CongestionEvent::PacketLoss, 24000);

        assert_eq!(cubic.state, CongestionState::FastRecovery);
        assert!(cubic.congestion_window < 24000);
        assert!(cubic.ssthresh > 0);
    }

    #[test]
    fn test_cubic_ecn_ce() {
        let mut cubic = Cubic::new(1200);
        cubic.congestion_window = 24000;

        cubic.on_congestion_event(CongestionEvent::EcnCeMarked, 24000);

        assert_eq!(cubic.state, CongestionState::CongestionAvoidance);
        assert!(cubic.congestion_window < 24000);
    }

    #[test]
    fn test_cubic_reset() {
        let mut cubic = Cubic::new(1200);
        cubic.congestion_window = 5000;
        cubic.congestion_events = 3;
        cubic.reset();

        assert_eq!(cubic.congestion_window, 12000);
        assert_eq!(cubic.congestion_events, 0);
        assert_eq!(cubic.state, CongestionState::SlowStart);
    }
}
