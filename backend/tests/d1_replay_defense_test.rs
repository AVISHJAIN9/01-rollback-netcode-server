/// D1: Replay Attack Defense
/// Tests monotonic sequence number enforcement to prevent replay attacks.

use rollback_netcode_server::protocol::{SequenceValidator, SequenceError};

/// Test 1: First frame with seq=1 is accepted
#[test]
fn test_first_frame_accepted() {
    let mut validator = SequenceValidator::new();
    validator.register_player(0);

    // First frame with seq=1 should be accepted
    assert!(validator.validate_and_ack(0, 1).is_ok());
    assert_eq!(validator.last_acked_seq(0), Some(1));

    println!("✓ First frame (seq=1) accepted, last_acked_seq=1");
}

/// Test 2: Replay of same sequence is rejected
#[test]
fn test_replay_rejected() {
    let mut validator = SequenceValidator::new();
    validator.register_player(0);

    // Accept seq=1
    assert!(validator.validate_and_ack(0, 1).is_ok());
    println!("✓ Seq 1 accepted");

    // Try to replay seq=1 again
    let result = validator.validate_and_ack(0, 1);
    assert!(
        matches!(result, Err(SequenceError::Replay { seq: 1, last_acked: 1 })),
        "Expected Replay error, got {:?}",
        result
    );
    println!("✓ Replay of seq=1 rejected with Replay error");
}

/// Test 3: Old sequence number is rejected
#[test]
fn test_old_seq_rejected() {
    let mut validator = SequenceValidator::new();
    validator.register_player(0);

    // Accept seq=5
    assert!(validator.validate_and_ack(0, 5).is_ok());
    println!("✓ Seq 5 accepted");

    // Try to send seq=3 (older)
    let result = validator.validate_and_ack(0, 3);
    assert!(
        matches!(result, Err(SequenceError::Replay { seq: 3, last_acked: 5 })),
        "Expected Replay error for old seq, got {:?}",
        result
    );
    println!("✓ Old seq (3 after 5) rejected with Replay error");
}

/// Bonus: Multiple players with independent sequences
#[test]
fn test_multiple_players_independent() {
    let mut validator = SequenceValidator::new();
    validator.register_player(0);
    validator.register_player(1);

    // Player 0: seq 100
    assert!(validator.validate_and_ack(0, 100).is_ok());
    println!("✓ Player 0: seq 100 accepted");

    // Player 1: seq 5 (much lower, but independent)
    assert!(validator.validate_and_ack(1, 5).is_ok());
    println!("✓ Player 1: seq 5 accepted (independent of player 0)");

    // Player 0: seq 101 (after 100)
    assert!(validator.validate_and_ack(0, 101).is_ok());
    println!("✓ Player 0: seq 101 accepted");

    // Player 1: replay seq 5 should be rejected
    let result = validator.validate_and_ack(1, 5);
    assert!(matches!(result, Err(SequenceError::Replay { .. })));
    println!("✓ Player 1: replay of seq 5 correctly rejected");
}
