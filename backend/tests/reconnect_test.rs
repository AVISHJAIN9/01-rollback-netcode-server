/// Tests for reconnect token management and resumption logic
/// Uses tokio::time::pause() for fake-clock control of token expiry

use uuid::Uuid;
use std::time::Duration;

// Mock the reconnect module for testing
#[path = "../src/session/reconnect.rs"]
mod reconnect;

use reconnect::{ReconnectManager, ReconnectError};

/// Test 1: Valid reconnect flow
/// - Issue a token
/// - Record tick 50 for the player
/// - Validate with correct player+session
/// - Assert Ok(51) is returned (resume at tick+1)
/// - Assert token is consumed (second validate returns Err)
#[tokio::test]
async fn test_valid_reconnect() {
    let mut manager = ReconnectManager::new();
    let player_id = "player_a";
    let session_id = Uuid::new_v4();

    // Issue a token
    let token = manager.issue_token(player_id, session_id);
    println!("✓ Token issued for player_a with session {:?}", session_id);

    // Record tick 50
    manager.record_tick(player_id, 50);
    println!("✓ Recorded tick 50 for player_a");

    // Validate with correct credentials
    let result = manager.validate_token(&token.token_str, player_id, session_id);
    assert_eq!(result, Ok(51), "Expected resume_tick = 51 (last_tick + 1)");
    println!("✓ Token validated successfully, resume_tick = 51");

    // Token should be consumed; second validate should fail
    let result2 = manager.validate_token(&token.token_str, player_id, session_id);
    assert!(
        result2 == Err(ReconnectError::NotFound) || result2 == Err(ReconnectError::Reused),
        "Expected token to be consumed, got {:?}",
        result2
    );
    println!("✓ Token correctly consumed after first use");
}

/// Test 2: Expired token detection
/// - Use tokio::time::pause() for fake clock
/// - Issue a token
/// - Advance time by 31 seconds
/// - Validate token
/// - Assert Err(Expired)
#[tokio::test]
async fn test_expired_token() {
    tokio::time::pause();
    let mut manager = ReconnectManager::new();
    let player_id = "player_b";
    let session_id = Uuid::new_v4();

    // Issue a token
    let token = manager.issue_token(player_id, session_id);
    println!("✓ Token issued for player_b");

    // Advance time by 31 seconds (expires_at was 30 seconds)
    tokio::time::advance(Duration::from_secs(31)).await;
    println!("✓ Advanced fake clock by 31 seconds");

    // Validate token; should be expired
    let result = manager.validate_token(&token.token_str, player_id, session_id);
    assert_eq!(result, Err(ReconnectError::Expired), "Expected Expired error");
    println!("✓ Token correctly detected as expired");

    tokio::time::resume();
}

/// Test 3: Reused token detection
/// - Issue a token
/// - Validate once (success)
/// - Validate again with same token
/// - Assert Err(Reused) or Err(NotFound) on second call
#[tokio::test]
async fn test_reused_token() {
    let mut manager = ReconnectManager::new();
    let player_id = "player_c";
    let session_id = Uuid::new_v4();

    // Issue a token
    let token = manager.issue_token(player_id, session_id);
    println!("✓ Token issued for player_c");

    // First validation should succeed
    let result1 = manager.validate_token(&token.token_str, player_id, session_id);
    assert!(result1.is_ok(), "First validation should succeed");
    println!("✓ First validation succeeded, resume_tick = {:?}", result1);

    // Second validation should fail (token consumed)
    let result2 = manager.validate_token(&token.token_str, player_id, session_id);
    assert!(
        result2 == Err(ReconnectError::NotFound) || result2 == Err(ReconnectError::Reused),
        "Expected token to be consumed, got {:?}",
        result2
    );
    println!("✓ Token correctly rejected on reuse attempt");
}

/// Test 4: Wrong player validation
/// - Issue token for player_a + session_x
/// - Validate with player_b + session_x
/// - Assert Err(WrongPlayer)
#[tokio::test]
async fn test_wrong_player() {
    let mut manager = ReconnectManager::new();
    let player_a = "player_a";
    let player_b = "player_b";
    let session_id = Uuid::new_v4();

    // Issue token for player_a
    let token = manager.issue_token(player_a, session_id);
    println!("✓ Token issued for player_a");

    // Try to validate with player_b
    let result = manager.validate_token(&token.token_str, player_b, session_id);
    assert_eq!(result, Err(ReconnectError::WrongPlayer), "Expected WrongPlayer error");
    println!("✓ Token correctly rejected for wrong player");
}

/// Test 5: Wrong session validation
/// - Issue token for player_a + session_x
/// - Validate with player_a + session_y (different session)
/// - Assert Err(WrongSession)
#[tokio::test]
async fn test_wrong_session() {
    let mut manager = ReconnectManager::new();
    let player_id = "player_a";
    let session_x = Uuid::new_v4();
    let session_y = Uuid::new_v4();

    // Issue token for session_x
    let token = manager.issue_token(player_id, session_x);
    println!("✓ Token issued for player_a in session {:?}", session_x);

    // Try to validate with session_y
    let result = manager.validate_token(&token.token_str, player_id, session_y);
    assert_eq!(result, Err(ReconnectError::WrongSession), "Expected WrongSession error");
    println!("✓ Token correctly rejected for wrong session");
}
