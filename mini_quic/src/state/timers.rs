//! Handshake and PTO timers — RFC 9000 §10.1, §10.2, Appendix A.
//!
//! # HandshakeTimer (§10.1)
//!
//! Tracks an absolute deadline by which the handshake must complete.  If
//! the deadline passes the connection should be silently closed.
//!
//! # PTO (Probe Timeout) calculation (RFC 9002 §6.2)
//!
//! PTO = smoothed_rtt + max(4 * rttvar, TIMER_GRANULARITY) + max_ack_delay
//!
//! The timer granularity used here is 1 ms (1_000_000 ns).

// The `state` and `termination` modules use `std::time::Instant` which
// requires std.  The `crypto` feature implies std via the transitive
// dependencies, so gating behind `crypto` is sufficient.
extern crate std;

use core::time::Duration;
use std::time::Instant;

/// Minimum timer granularity (1 ms), used as a floor in PTO calculations.
pub const TIMER_GRANULARITY: Duration = Duration::from_millis(1);

/// Default maximum handshake duration (10 seconds) per RFC 9000 §10.1.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The default initial RTT estimate before any measurements are available
/// (333 ms per RFC 9002 §6.2.2).
pub const INITIAL_RTT_NS: u64 = 333_000_000;

// ─── RttEstimator ─────────────────────────────────────────────────────────────

/// Maintains running estimates of smoothed RTT and RTT variance per
/// RFC 9002 §5.
#[derive(Debug, Clone)]
pub struct RttEstimator {
    /// Smoothed RTT estimate in nanoseconds.
    pub smoothed_rtt_ns: u64,
    /// RTT variance in nanoseconds.
    pub rttvar_ns: u64,
    /// Minimum RTT observed (used as floor for smoothed_rtt).
    pub min_rtt_ns: u64,
    /// Latest RTT sample used to derive ack_delay in PTO computation.
    pub max_ack_delay_ns: u64,
}

impl RttEstimator {
    /// Construct with RFC 9002 §6.2.2 initial values.
    pub fn new() -> Self {
        RttEstimator {
            smoothed_rtt_ns: INITIAL_RTT_NS,
            rttvar_ns: INITIAL_RTT_NS / 2,
            min_rtt_ns: u64::MAX,
            max_ack_delay_ns: 25_000_000, // 25 ms default
        }
    }

    /// Update estimates with a new RTT sample `latest_rtt_ns`.
    ///
    /// Implements RFC 9002 §5.3:
    /// ```text
    /// if first_sample:
    ///   smoothed_rtt = latest_rtt
    ///   rttvar       = latest_rtt / 2
    /// else:
    ///   rttvar       = 3/4 * rttvar + 1/4 * |smoothed_rtt - adjusted_rtt|
    ///   smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
    /// ```
    /// where `adjusted_rtt = max(latest_rtt - ack_delay, min_rtt)`.
    pub fn update(&mut self, latest_rtt_ns: u64, ack_delay_ns: u64) {
        if self.min_rtt_ns == u64::MAX {
            // First sample.
            self.min_rtt_ns = latest_rtt_ns;
            self.smoothed_rtt_ns = latest_rtt_ns;
            self.rttvar_ns = latest_rtt_ns / 2;
            return;
        }

        self.min_rtt_ns = self.min_rtt_ns.min(latest_rtt_ns);

        // Adjust for ack delay but not below min_rtt.
        let adjusted_rtt = if latest_rtt_ns > self.min_rtt_ns + ack_delay_ns {
            latest_rtt_ns - ack_delay_ns
        } else {
            self.min_rtt_ns
        };

        // rttvar = 3/4 * rttvar + 1/4 * |smoothed_rtt - adjusted_rtt|
        let diff = if self.smoothed_rtt_ns > adjusted_rtt {
            self.smoothed_rtt_ns - adjusted_rtt
        } else {
            adjusted_rtt - self.smoothed_rtt_ns
        };
        self.rttvar_ns = (3 * self.rttvar_ns / 4) + (diff / 4);

        // smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
        self.smoothed_rtt_ns = (7 * self.smoothed_rtt_ns / 8) + (adjusted_rtt / 8);
    }

    /// Compute the current Probe Timeout (PTO) value.
    ///
    /// PTO = smoothed_rtt + max(4 * rttvar, TIMER_GRANULARITY) + max_ack_delay
    pub fn pto_ns(&self) -> u64 {
        let granularity_ns = TIMER_GRANULARITY.as_nanos() as u64;
        let var_component = (4 * self.rttvar_ns).max(granularity_ns);
        self.smoothed_rtt_ns + var_component + self.max_ack_delay_ns
    }

    /// Return the PTO as a [`Duration`].
    pub fn pto(&self) -> Duration {
        Duration::from_nanos(self.pto_ns())
    }
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── HandshakeTimer ───────────────────────────────────────────────────────────

/// Tracks the handshake completion deadline (RFC 9000 §10.1).
///
/// If `is_expired(now)` returns `true`, the connection should be closed
/// silently without transmitting a CONNECTION_CLOSE frame.
#[derive(Debug)]
pub struct HandshakeTimer {
    /// Absolute deadline by which the handshake must complete.
    deadline: Instant,
    /// Whether the handshake has been successfully completed.
    completed: bool,
}

impl HandshakeTimer {
    /// Construct a timer with the default 10-second timeout starting from `start`.
    pub fn new(start: Instant) -> Self {
        HandshakeTimer {
            deadline: start + DEFAULT_HANDSHAKE_TIMEOUT,
            completed: false,
        }
    }

    /// Construct a timer with a custom timeout duration.
    pub fn with_timeout(start: Instant, timeout: Duration) -> Self {
        HandshakeTimer {
            deadline: start + timeout,
            completed: false,
        }
    }

    /// Mark the handshake as complete (disarms the timer).
    pub fn complete(&mut self) {
        self.completed = true;
    }

    /// Return `true` when the handshake deadline has passed and the handshake
    /// has not yet been completed.
    pub fn is_expired(&self, now: Instant) -> bool {
        !self.completed && now >= self.deadline
    }

    /// Return `true` when the handshake has been marked as complete.
    pub fn is_complete(&self) -> bool {
        self.completed
    }

    /// Return the absolute deadline `Instant`.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Return the remaining time before expiry, or `Duration::ZERO` if
    /// already expired or complete.
    pub fn remaining(&self, now: Instant) -> Duration {
        if self.completed || now >= self.deadline {
            Duration::ZERO
        } else {
            self.deadline - now
        }
    }
}

// ─── PtoTimer ────────────────────────────────────────────────────────────────

/// A single-shot retransmit probe timer driven by the PTO calculation.
#[derive(Debug)]
pub struct PtoTimer {
    /// The absolute time at which a probe should be sent.
    deadline: Option<Instant>,
    /// Number of consecutive PTO expirations without acknowledgement.
    /// Used to double the PTO (exponential backoff) per RFC 9002 §6.2.
    consecutive_pto_count: u32,
}

impl PtoTimer {
    pub fn new() -> Self {
        PtoTimer {
            deadline: None,
            consecutive_pto_count: 0,
        }
    }

    /// Arm (or re-arm) the PTO timer at `now + pto_duration * 2^backoff`.
    pub fn arm(&mut self, now: Instant, pto_duration: Duration) {
        let backoff = 1u32 << self.consecutive_pto_count.min(20);
        let timeout = pto_duration.saturating_mul(backoff);
        self.deadline = Some(now + timeout);
    }

    /// Disarm the timer (called when an ACK is received or all unacked
    /// packets are acknowledged).
    pub fn disarm(&mut self) {
        self.deadline = None;
        self.consecutive_pto_count = 0;
    }

    /// Return `true` when the timer is armed and has expired.
    pub fn is_expired(&self, now: Instant) -> bool {
        self.deadline.map_or(false, |dl| now >= dl)
    }

    /// Called when a PTO fires: increments the backoff counter.
    pub fn on_pto_fired(&mut self) {
        self.consecutive_pto_count += 1;
        self.deadline = None;
    }

    /// Reset the backoff counter (called when an ACK is received).
    pub fn reset_backoff(&mut self) {
        self.consecutive_pto_count = 0;
    }

    pub fn is_armed(&self) -> bool {
        self.deadline.is_some()
    }
}

impl Default for PtoTimer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_timer_not_expired_initially() {
        let start = Instant::now();
        let timer = HandshakeTimer::new(start);
        assert!(!timer.is_expired(start));
    }

    #[test]
    fn handshake_timer_expired_after_deadline() {
        let start = Instant::now();
        let timer = HandshakeTimer::with_timeout(start, Duration::from_nanos(1));
        // Move clock well past the deadline.
        let future = start + Duration::from_secs(100);
        assert!(timer.is_expired(future));
    }

    #[test]
    fn handshake_timer_not_expired_when_complete() {
        let start = Instant::now();
        let mut timer = HandshakeTimer::with_timeout(start, Duration::from_nanos(1));
        timer.complete();
        let future = start + Duration::from_secs(100);
        assert!(!timer.is_expired(future));
        assert!(timer.is_complete());
    }

    #[test]
    fn rtt_estimator_first_sample() {
        let mut estimator = RttEstimator::new();
        // Before any update, smoothed_rtt = INITIAL_RTT_NS.
        assert_eq!(estimator.smoothed_rtt_ns, INITIAL_RTT_NS);

        // First sample: sets both smoothed_rtt and rttvar.
        estimator.update(100_000_000, 0); // 100 ms
        assert_eq!(estimator.smoothed_rtt_ns, 100_000_000);
        assert_eq!(estimator.rttvar_ns, 50_000_000);
    }

    #[test]
    fn rtt_estimator_subsequent_samples() {
        let mut estimator = RttEstimator::new();
        estimator.update(100_000_000, 0);
        estimator.update(110_000_000, 5_000_000); // 110ms sample, 5ms ack delay
        // adjusted = 110 - 5 = 105 ms (> min_rtt=100ms so delay applies)
        // rttvar = 3/4 * 50 + 1/4 * |100 - 105| = 37.5 + 1.25 = 38.75ms
        // smoothed_rtt = 7/8 * 100 + 1/8 * 105 = 87.5 + 13.125 = 100.625ms
        // Check that smoothed_rtt moved toward 105ms (increased from 100ms).
        assert!(estimator.smoothed_rtt_ns > 100_000_000);
    }

    #[test]
    fn pto_timer_arm_and_expire() {
        let mut timer = PtoTimer::new();
        let now = Instant::now();
        timer.arm(now, Duration::from_millis(10));
        assert!(timer.is_armed());
        assert!(!timer.is_expired(now)); // not expired yet
        let future = now + Duration::from_secs(1);
        assert!(timer.is_expired(future));
    }

    #[test]
    fn pto_timer_disarm_resets_state() {
        let mut timer = PtoTimer::new();
        let now = Instant::now();
        timer.arm(now, Duration::from_millis(100));
        timer.disarm();
        assert!(!timer.is_armed());
        let future = now + Duration::from_secs(100);
        assert!(!timer.is_expired(future));
    }

    #[test]
    fn pto_backoff_doubles_timeout() {
        let mut timer = PtoTimer::new();
        let now = Instant::now();
        let base = Duration::from_millis(100);

        // First arm: no backoff.
        timer.arm(now, base);
        let dl1 = timer.deadline.unwrap();

        timer.on_pto_fired(); // increments backoff

        // Second arm: backoff = 2x.
        timer.arm(now, base);
        let dl2 = timer.deadline.unwrap();

        // dl2 should be roughly 2× longer than dl1.
        let gap1 = dl1 - now;
        let gap2 = dl2 - now;
        assert!(gap2 >= gap1 * 2 - Duration::from_millis(1));
    }
}
