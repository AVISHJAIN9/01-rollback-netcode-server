/// D3: Session Hijacking Defense
/// Tests reconnect token validation with player_id binding to prevent session hijacking.

use rollback_netcode_server::session::*;
use uuid::Uuid;
use chrono::Utc;

/// Test 1: Valid token with correct player_id is accepted
#[tokio::test]
async fn test_valid_token_accepted() {
    let repo = InMemorySessionRepository::new();
    
    let player_id = PlayerId("player1".to_string());
    let token = ReconnectToken::new(player_id.clone());
    
    // Store the token
    repo.store_reconnect_token(token.clone())
        .await
        .expect("Failed to store token");
    
    println!("✓ Token stored for player1");

    // Validate with correct player_id
    let result = repo
        .validate_reconnect_token_for_player(&token.token, &player_id)
        .await;
    
    assert!(result.is_ok(), "Token validation failed with correct player_id");
    println!("✓ Valid token with correct player_id accepted");
}

/// Test 2: Token with wrong player_id is rejected
#[tokio::test]
async fn test_wrong_player_id_rejected() {
    let repo = InMemorySessionRepository::new();
    
    let player1_id = PlayerId("player1".to_string());
    let player2_id = PlayerId("player2".to_string());
    
    let token = ReconnectToken::new(player1_id.clone());
    
    // Store the token for player1
    repo.store_reconnect_token(token.clone())
        .await
        .expect("Failed to store token");
    
    println!("✓ Token stored for player1");

    // Try to validate with player2's ID
    let result = repo
        .validate_reconnect_token_for_player(&token.token, &player2_id)
        .await;
    
    assert!(
        matches!(result, Err(SessionError::PlayerMismatch { .. })),
        "Expected PlayerMismatch error, got {:?}",
        result
    );
    println!("✓ Token validation correctly rejected for wrong player_id");
}

/// Test 3: Expired token is rejected
#[tokio::test]
async fn test_expired_token_rejected() {
    tokio::time::pause();
    
    let repo = InMemorySessionRepository::new();
    
    let player_id = PlayerId("player3".to_string());
    let mut token = ReconnectToken::new(player_id.clone());
    
    // Manually expire the token by setting expires_at to the past
    token.expires_at = Utc::now() - chrono::Duration::seconds(1);
    
    // Store the expired token
    repo.store_reconnect_token(token.clone())
        .await
        .expect("Failed to store token");
    
    println!("✓ Expired token stored");

    // Try to validate the expired token
    let result = repo
        .validate_reconnect_token_for_player(&token.token, &player_id)
        .await;
    
    assert!(
        matches!(result, Err(SessionError::TokenExpired)),
        "Expected TokenExpired error, got {:?}",
        result
    );
    println!("✓ Expired token correctly rejected");

    tokio::time::resume();
}

/// Test 4: Nonexistent token is rejected
#[tokio::test]
async fn test_nonexistent_token_rejected() {
    let repo = InMemorySessionRepository::new();
    
    let player_id = PlayerId("player4".to_string());

    // Try to validate a token that was never stored
    let result = repo
        .validate_reconnect_token_for_player("fake-token-12345", &player_id)
        .await;
    
    assert!(
        matches!(result, Err(SessionError::TokenNotFound)),
        "Expected TokenNotFound error, got {:?}",
        result
    );
    println!("✓ Nonexistent token correctly rejected");
}

/// Test 5: Multiple tokens for different players are independent
#[tokio::test]
async fn test_multiple_players_independent() {
    let repo = InMemorySessionRepository::new();
    
    let player1 = PlayerId("player1".to_string());
    let player2 = PlayerId("player2".to_string());
    
    let token1 = ReconnectToken::new(player1.clone());
    let token2 = ReconnectToken::new(player2.clone());
    
    // Store both tokens
    repo.store_reconnect_token(token1.clone())
        .await
        .expect("Failed to store token1");
    repo.store_reconnect_token(token2.clone())
        .await
        .expect("Failed to store token2");
    
    println!("✓ Both tokens stored");

    // Each token should only work for its own player
    assert!(
        repo.validate_reconnect_token_for_player(&token1.token, &player1)
            .await
            .is_ok(),
        "Token1 should work for player1"
    );
    
    assert!(
        matches!(
            repo.validate_reconnect_token_for_player(&token1.token, &player2)
                .await,
            Err(SessionError::PlayerMismatch { .. })
        ),
        "Token1 should not work for player2"
    );
    
    assert!(
        repo.validate_reconnect_token_for_player(&token2.token, &player2)
            .await
            .is_ok(),
        "Token2 should work for player2"
    );
    
    assert!(
        matches!(
            repo.validate_reconnect_token_for_player(&token2.token, &player1)
                .await,
            Err(SessionError::PlayerMismatch { .. })
        ),
        "Token2 should not work for player1"
    );
    
    println!("✓ Multiple player tokens correctly isolated");
}
