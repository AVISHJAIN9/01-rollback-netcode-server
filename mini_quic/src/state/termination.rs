//! Connection Close and Draining State Machines — RFC 9000 §10.2.
//!
//! # ClosingState (§10.2.1)
//!
//! The local endpoint sends a CONNECTION_CLOSE frame and enters the Closing
//! state.  While Closing it may retransmit the CONNECTION_CLOSE frame in
//! response to any received packet from the peer.  The Closing state ends
//! after `3 × PTO` has elapsed.
//!
//! # DrainingState (§10.2.2)
//!
//! The local endpoint received a CONNECTION_CLOSE frame and enters the
//! Draining state.  It does not transmit any further frames; it simply waits
//! for `3 × PTO` to ensure the peer has received its acknowledgement.
//!
//! Both states auto-transition to `Closed` once the timer expires.

use core::time::Duration;
use std::time::Instant;

// ─── TerminationTimer ─────────────────────────────────────────────────────────

/// A timer governing how long to stay in the Closing or Draining state.
///
/// Per RFC 9000 §10.2: the timer is set to `3 × PTO`.
#[derive(Debug)]
struct TerminationTimer {
    deadline: Instant,
}

impl TerminationTimer {
    /// Create a timer expiring at `now + 3 * pto`.
    fn new(now: Instant, pto: Duration) -> Self {
        TerminationTimer {
            deadline: now + pto.saturating_mul(3),
        }
    }

    /// Return `true` when `now >= deadline`.
    fn is_expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }
}

// ─── ClosingState ────────────────────────────────────────────────────────────

/// The Closing state (RFC 9000 §10.2.1).
///
/// The endpoint sends CONNECTION_CLOSE and retransmits it on receipt of any
/// packet until the `3 × PTO` timer expires.
#[derive(Debug)]
pub struct ClosingState {
    /// The serialised CONNECTION_CLOSE frame bytes to retransmit.
    connection_close_frame: alloc::vec::Vec<u8>,
    /// Error code identifying why the connection is being closed.
    pub error_code: u64,
    /// The retransmission deadline (`now + 3 × PTO`).
    timer: TerminationTimer,
    /// Whether the state has already transitioned to Closed.
    closed: bool,
}

impl ClosingState {
    /// Enter the Closing state.
    ///
    /// # Parameters
    /// - `connection_close_frame`: encoded bytes of the CONNECTION_CLOSE frame.
    /// - `error_code`: the transport error code.
    /// - `now`: current monotonic time.
    /// - `pto`: current probe timeout estimate.
    pub fn new(
        connection_close_frame: alloc::vec::Vec<u8>,
        error_code: u64,
        now: Instant,
        pto: Duration,
    ) -> Self {
        ClosingState {
            connection_close_frame,
            error_code,
            timer: TerminationTimer::new(now, pto),
            closed: false,
        }
    }

    /// Called when a packet is received from the peer while in the Closing
    /// state.  Returns the CONNECTION_CLOSE frame bytes that should be
    /// retransmitted, if the state has not yet expired.
    pub fn on_packet_received(&self, now: Instant) -> Option<&[u8]> {
        if self.closed || self.timer.is_expired(now) {
            None
        } else {
            Some(&self.connection_close_frame)
        }
    }

    /// Advance the state machine.  Returns `true` when the timer has expired
    /// and the connection should transition to `Closed`.
    pub fn poll(&mut self, now: Instant) -> bool {
        if self.timer.is_expired(now) {
            self.closed = true;
        }
        self.closed
    }

    /// Return `true` if already transitioned to Closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Return the absolute deadline for this closing period.
    pub fn deadline(&self) -> Instant {
        self.timer.deadline()
    }
}

// ─── DrainingState ────────────────────────────────────────────────────────────

/// The Draining state (RFC 9000 §10.2.2).
///
/// The endpoint received CONNECTION_CLOSE.  It enters a quiet period lasting
/// `3 × PTO` during which no further frames are sent.  All received packets
/// are silently discarded.
#[derive(Debug)]
pub struct DrainingState {
    /// The draining period deadline.
    timer: TerminationTimer,
    /// Whether the draining period has completed.
    closed: bool,
}

impl DrainingState {
    /// Enter the Draining state.
    ///
    /// # Parameters
    /// - `now`: current monotonic time.
    /// - `pto`: current probe timeout estimate.
    pub fn new(now: Instant, pto: Duration) -> Self {
        DrainingState {
            timer: TerminationTimer::new(now, pto),
            closed: false,
        }
    }

    /// Advance the state machine.  Returns `true` when the draining period
    /// has elapsed and the connection may be `Closed`.
    pub fn poll(&mut self, now: Instant) -> bool {
        if self.timer.is_expired(now) {
            self.closed = true;
        }
        self.closed
    }

    /// Return `true` when the draining period has completed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Return the absolute deadline for this draining period.
    pub fn deadline(&self) -> Instant {
        self.timer.deadline()
    }

    /// While draining, **no** frames should be sent.  This method returns
    /// `false` unconditionally to make that contract explicit.
    pub fn should_send(&self) -> bool {
        false
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PTO: Duration = Duration::from_millis(50);

    #[test]
    fn closing_state_retransmits_before_expiry() {
        let now = Instant::now();
        let frame = alloc::vec![0x1Cu8, 0x00, 0x00, 0x00]; // mock CONNECTION_CLOSE
        let closing = ClosingState::new(frame.clone(), 0, now, PTO);

        // Just after construction — should retransmit on receipt.
        let retx = closing.on_packet_received(now);
        assert_eq!(retx, Some(frame.as_slice()));
    }

    #[test]
    fn closing_state_does_not_retransmit_after_expiry() {
        let now = Instant::now();
        let frame = alloc::vec![0x1Cu8];
        let closing = ClosingState::new(frame, 0, now, PTO);

        // After 3 × PTO has elapsed.
        let future = now + PTO * 4;
        assert!(closing.on_packet_received(future).is_none());
    }

    #[test]
    fn closing_state_transitions_to_closed_on_poll() {
        let now = Instant::now();
        let frame = alloc::vec![0x1Cu8];
        let mut closing = ClosingState::new(frame, 0, now, PTO);

        // Poll before expiry.
        assert!(!closing.poll(now));

        // Poll after 3 × PTO.
        let future = now + PTO * 4;
        assert!(closing.poll(future));
        assert!(closing.is_closed());
    }

    #[test]
    fn draining_state_transitions_to_closed_on_poll() {
        let now = Instant::now();
        let mut draining = DrainingState::new(now, PTO);

        assert!(!draining.poll(now));
        let future = now + PTO * 4;
        assert!(draining.poll(future));
        assert!(draining.is_closed());
    }

    #[test]
    fn draining_state_should_not_send() {
        let now = Instant::now();
        let draining = DrainingState::new(now, PTO);
        assert!(!draining.should_send());
    }

    #[test]
    fn termination_timer_uses_3x_pto() {
        let now = Instant::now();
        let pto = Duration::from_millis(100);
        let timer = TerminationTimer::new(now, pto);

        // Not expired at 2.9 × PTO.
        let almost = now + Duration::from_millis(299);
        assert!(!timer.is_expired(almost));

        // Expired at 3 × PTO.
        let exactly = now + Duration::from_millis(300);
        assert!(timer.is_expired(exactly));
    }
}
