//! Connection Lifecycle State Machine — RFC 9000 §5.
//!
//! Defines the full set of connection states and the valid transitions between
//! them.  All transitions are validated; attempting an illegal transition
//! returns a [`CodecError::InvalidStateTransition`].

use crate::error::CodecError;

// ─── Role ─────────────────────────────────────────────────────────────────────

/// Whether this endpoint initiated the connection (Client) or is responding (Server).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

impl core::fmt::Display for Role {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Role::Client => write!(f, "Client"),
            Role::Server => write!(f, "Server"),
        }
    }
}

// ─── ConnectionLifecycle ─────────────────────────────────────────────────────

/// The lifecycle state of a QUIC connection.
///
/// Valid transition graph:
/// ```text
/// Idle ──→ Handshaking ──→ Active1Rtt ──→ Closing ──→ Closed
///                     └──────────────────→ Closing
///                     └──────────────────→ Draining ──→ Closed
///          Active1Rtt ──────────────────→ Draining ──→ Closed
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionLifecycle {
    /// No connection has been established; no keys available.
    Idle,
    /// TLS handshake is in progress; Initial and Handshake keys are in use.
    Handshaking,
    /// Handshake is complete; 1-RTT (Application Data) encryption is active.
    Active1Rtt,
    /// Local endpoint sent CONNECTION_CLOSE; retransmitting close frame.
    Closing,
    /// Remote endpoint sent CONNECTION_CLOSE; in quiet period.
    Draining,
    /// Connection fully terminated; all resources may be freed.
    Closed,
}

impl ConnectionLifecycle {
    /// Return the state name as a static string.
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionLifecycle::Idle => "Idle",
            ConnectionLifecycle::Handshaking => "Handshaking",
            ConnectionLifecycle::Active1Rtt => "Active1Rtt",
            ConnectionLifecycle::Closing => "Closing",
            ConnectionLifecycle::Draining => "Draining",
            ConnectionLifecycle::Closed => "Closed",
        }
    }

    /// Return `true` when application data frames may be sent/received.
    pub fn can_exchange_data(self) -> bool {
        matches!(self, ConnectionLifecycle::Active1Rtt)
    }

    /// Return `true` when the connection is in a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(self, ConnectionLifecycle::Closed)
    }
}

impl core::fmt::Display for ConnectionLifecycle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ─── ConnectionState ─────────────────────────────────────────────────────────

/// The full state of a QUIC connection, combining lifecycle, role, and IDs.
#[derive(Debug)]
pub struct ConnectionState {
    /// Whether this endpoint is a Client or a Server.
    pub role: Role,
    /// Current lifecycle state.
    pub lifecycle: ConnectionLifecycle,
    /// Local connection ID in use for sending.
    pub local_cid: crate::cid::ConnectionId,
    /// The peer's connection ID (used as destination CID in outgoing packets).
    pub peer_cid: Option<crate::cid::ConnectionId>,
    /// Number of packets received in the current lifecycle.
    pub packets_received: u64,
    /// Number of packets sent in the current lifecycle.
    pub packets_sent: u64,
}

impl ConnectionState {
    /// Construct a new connection in the `Idle` state.
    pub fn new(role: Role, local_cid: crate::cid::ConnectionId) -> Self {
        ConnectionState {
            role,
            lifecycle: ConnectionLifecycle::Idle,
            local_cid,
            peer_cid: None,
            packets_received: 0,
            packets_sent: 0,
        }
    }

    // ── Transition helpers ────────────────────────────────────────────────────

    /// Perform a state transition.  Returns `Err` if the transition is
    /// not permitted.
    pub fn transition(
        &mut self,
        target: ConnectionLifecycle,
    ) -> Result<(), CodecError> {
        if !is_valid_transition(self.lifecycle, target) {
            return Err(CodecError::InvalidStateTransition {
                from: self.lifecycle.as_str(),
                to: target.as_str(),
            });
        }
        self.lifecycle = target;
        Ok(())
    }

    /// Start the TLS handshake (Idle → Handshaking).
    pub fn start_handshake(&mut self) -> Result<(), CodecError> {
        self.transition(ConnectionLifecycle::Handshaking)
    }

    /// Advance to 1-RTT (Handshaking → Active1Rtt).
    pub fn handshake_complete(&mut self) -> Result<(), CodecError> {
        self.transition(ConnectionLifecycle::Active1Rtt)
    }

    /// Begin local-initiated close (Active1Rtt or Handshaking → Closing).
    pub fn begin_closing(&mut self) -> Result<(), CodecError> {
        self.transition(ConnectionLifecycle::Closing)
    }

    /// Enter draining on receipt of remote CONNECTION_CLOSE
    /// (Active1Rtt, Handshaking, or Closing → Draining).
    pub fn begin_draining(&mut self) -> Result<(), CodecError> {
        self.transition(ConnectionLifecycle::Draining)
    }

    /// Mark the connection fully closed (Closing or Draining → Closed).
    pub fn close(&mut self) -> Result<(), CodecError> {
        self.transition(ConnectionLifecycle::Closed)
    }

    /// Guard: return an error if application data cannot be exchanged in the
    /// current state.
    pub fn require_active(&self) -> Result<(), CodecError> {
        if !self.lifecycle.can_exchange_data() {
            return Err(CodecError::InvalidStateTransition {
                from: self.lifecycle.as_str(),
                to: "Active1Rtt (required for data exchange)",
            });
        }
        Ok(())
    }

    /// Set the peer connection ID (received from the peer during the handshake).
    pub fn set_peer_cid(&mut self, cid: crate::cid::ConnectionId) {
        self.peer_cid = Some(cid);
    }
}

// ─── Transition validation table ─────────────────────────────────────────────

/// Return `true` when the transition `from → to` is permitted.
fn is_valid_transition(from: ConnectionLifecycle, to: ConnectionLifecycle) -> bool {
    use ConnectionLifecycle::*;
    matches!(
        (from, to),
        (Idle,        Handshaking) |
        (Handshaking, Active1Rtt)  |
        (Handshaking, Closing)     |
        (Handshaking, Draining)    |
        (Active1Rtt,  Closing)     |
        (Active1Rtt,  Draining)    |
        (Closing,     Draining)    |
        (Closing,     Closed)      |
        (Draining,    Closed)
    )
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cid::ConnectionId;

    fn make_state(role: Role) -> ConnectionState {
        let cid = ConnectionId::new(&[0x01, 0x02, 0x03, 0x04]).unwrap();
        ConnectionState::new(role, cid)
    }

    #[test]
    fn happy_path_client_handshake_to_closed() {
        let mut cs = make_state(Role::Client);
        cs.start_handshake().unwrap();
        cs.handshake_complete().unwrap();
        cs.begin_closing().unwrap();
        cs.close().unwrap();
        assert!(cs.lifecycle.is_terminal());
    }

    #[test]
    fn server_draining_path() {
        let mut cs = make_state(Role::Server);
        cs.start_handshake().unwrap();
        cs.handshake_complete().unwrap();
        cs.begin_draining().unwrap();
        cs.close().unwrap();
        assert_eq!(cs.lifecycle, ConnectionLifecycle::Closed);
    }

    #[test]
    fn idle_cannot_send_data() {
        let cs = make_state(Role::Client);
        assert!(cs.require_active().is_err());
    }

    #[test]
    fn active_can_send_data() {
        let mut cs = make_state(Role::Client);
        cs.start_handshake().unwrap();
        cs.handshake_complete().unwrap();
        assert!(cs.require_active().is_ok());
    }

    #[test]
    fn illegal_transition_rejected() {
        let mut cs = make_state(Role::Client);
        // Cannot jump from Idle directly to Active1Rtt.
        let err = cs.transition(ConnectionLifecycle::Active1Rtt);
        assert!(matches!(err, Err(CodecError::InvalidStateTransition { .. })));
        // State must remain Idle.
        assert_eq!(cs.lifecycle, ConnectionLifecycle::Idle);
    }

    #[test]
    fn cannot_go_backwards_from_active() {
        let mut cs = make_state(Role::Server);
        cs.start_handshake().unwrap();
        cs.handshake_complete().unwrap();
        // Cannot go back to Handshaking.
        assert!(matches!(
            cs.transition(ConnectionLifecycle::Handshaking),
            Err(CodecError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn closed_is_terminal() {
        let mut cs = make_state(Role::Client);
        cs.start_handshake().unwrap();
        cs.handshake_complete().unwrap();
        cs.begin_closing().unwrap();
        cs.close().unwrap();
        // Cannot transition out of Closed.
        assert!(matches!(
            cs.transition(ConnectionLifecycle::Idle),
            Err(CodecError::InvalidStateTransition { .. })
        ));
    }
}
