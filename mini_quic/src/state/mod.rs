//! Connection and packet-space state management.
//!
//! Modules:
//! - `spaces`      — Packet number spaces (Feature 18)
//! - `timers`      — Handshake and PTO timers (Feature 19)
//! - `termination` — Closing and Draining state machines (Feature 20)
//! - `connection`  — Connection lifecycle FSM (Batch 2 Feature 13, extended here)

pub mod connection;
pub mod spaces;
pub mod termination;
pub mod timers;
