//! Game Simulation Subsystem
//!
//! # Responsibilities
//! - Pure deterministic 2D fighter physics simulation with zero IEEE-754 floats.
//! - Q16.16 fixed-point arithmetic, vector geometry, and AABB collision resolution.
//! - Ring buffer state snapshots (128 frames) and rollback resimulation engine.
//! - Canonical byte-level state checksumming and authoritative desync detection.

pub use crate::math;
pub use crate::simulation;
