//! Gateway Subsystem
//!
//! # Responsibilities
//! - Ingress handling for UDP datagrams and WebSocket connections.
//! - Protocol framing, version negotiation, and packet routing to session actors.
//! - Distributed IP and token rate limiting to guard against packet floods.

pub mod ingress {
    // Ingress logic
}
