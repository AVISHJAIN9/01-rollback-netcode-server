/// Sequence number validation for replay attack defense
/// Implements monotonic sequence checking per player

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceError {
    /// Sequence number is not greater than last acknowledged sequence
    Replay { seq: u32, last_acked: u32 },
    /// Player not found in sequence tracker
    UnknownPlayer { player_id: u8 },
}

impl std::fmt::Display for SequenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SequenceError::Replay { seq, last_acked } => {
                write!(f, "Replay attack: seq {} <= last_acked {}", seq, last_acked)
            }
            SequenceError::UnknownPlayer { player_id } => {
                write!(f, "Unknown player: {}", player_id)
            }
        }
    }
}

impl std::error::Error for SequenceError {}

/// Validates that incoming sequence numbers are strictly monotonic per player.
/// Rejects replayed packets where seq <= last_acked_seq[player_id].
#[derive(Debug, Clone)]
pub struct SequenceValidator {
    last_acked: HashMap<u8, u32>,
}

impl SequenceValidator {
    /// Create a new sequence validator with no players registered.
    pub fn new() -> Self {
        Self {
            last_acked: HashMap::new(),
        }
    }

    /// Register a player with an initial sequence number (typically 0).
    pub fn register_player(&mut self, player_id: u8) {
        self.last_acked.insert(player_id, 0);
    }

    /// Validate an incoming sequence number for a player.
    /// Returns Ok(()) if seq > last_acked[player_id], otherwise returns an error.
    /// Updates last_acked[player_id] if validation succeeds.
    pub fn validate_and_ack(&mut self, player_id: u8, seq: u32) -> Result<(), SequenceError> {
        // Ensure player is registered
        if !self.last_acked.contains_key(&player_id) {
            self.last_acked.insert(player_id, 0);
        }

        let last_acked = self.last_acked[&player_id];

        // Check monotonicity
        if seq <= last_acked {
            return Err(SequenceError::Replay {
                seq,
                last_acked,
            });
        }

        // Update and return success
        self.last_acked.insert(player_id, seq);
        Ok(())
    }

    /// Get the last acknowledged sequence number for a player.
    pub fn last_acked_seq(&self, player_id: u8) -> Option<u32> {
        self.last_acked.get(&player_id).copied()
    }
}

impl Default for SequenceValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monotonic_sequence_accepted() {
        let mut validator = SequenceValidator::new();
        validator.register_player(0);

        // Monotonic sequence should be accepted
        assert!(validator.validate_and_ack(0, 1).is_ok());
        assert!(validator.validate_and_ack(0, 2).is_ok());
        assert!(validator.validate_and_ack(0, 3).is_ok());

        assert_eq!(validator.last_acked_seq(0), Some(3));
    }

    #[test]
    fn test_replay_rejected() {
        let mut validator = SequenceValidator::new();
        validator.register_player(0);

        // Accept first frame
        assert!(validator.validate_and_ack(0, 5).is_ok());

        // Try to replay seq=5
        let result = validator.validate_and_ack(0, 5);
        assert!(matches!(result, Err(SequenceError::Replay { seq: 5, last_acked: 5 })));
    }

    #[test]
    fn test_old_sequence_rejected() {
        let mut validator = SequenceValidator::new();
        validator.register_player(0);

        // Accept seq 100
        assert!(validator.validate_and_ack(0, 100).is_ok());

        // Try to send seq 50 (older)
        let result = validator.validate_and_ack(0, 50);
        assert!(matches!(result, Err(SequenceError::Replay { seq: 50, last_acked: 100 })));
    }

    #[test]
    fn test_multiple_players_independent() {
        let mut validator = SequenceValidator::new();
        validator.register_player(0);
        validator.register_player(1);

        // Player 0: seq 1, 2
        assert!(validator.validate_and_ack(0, 1).is_ok());
        assert!(validator.validate_and_ack(0, 2).is_ok());

        // Player 1: seq 10, 20
        assert!(validator.validate_and_ack(1, 10).is_ok());
        assert!(validator.validate_and_ack(1, 20).is_ok());

        // Player 0 can accept 3 (not affected by player 1's seq)
        assert!(validator.validate_and_ack(0, 3).is_ok());

        // Player 1 replay should still be rejected
        let result = validator.validate_and_ack(1, 15);
        assert!(matches!(result, Err(SequenceError::Replay { seq: 15, last_acked: 20 })));
    }
}
