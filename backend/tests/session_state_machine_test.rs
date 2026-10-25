use rollback_netcode_server::session::*;
use uuid::Uuid;

// ============================================================================
// TEST: Session creation and initial state
// ============================================================================

#[tokio::test]
async fn test_session_creation() {
    let match_id = Uuid::new_v4();
    let session = Session::new(
        match_id,
        "ROOM-001".to_string(),
        2,
    ).expect("Failed to create session");

    assert_eq!(session.state, SessionState::Lobby);
    assert_eq!(session.players.len(), 0);
    assert_eq!(session.max_players, 2);
    println!("✓ Session created in Lobby state");
}

// ============================================================================
// TEST: Invalid capacity rejected
// ============================================================================

#[tokio::test]
async fn test_invalid_capacity() {
    let match_id = Uuid::new_v4();
    
    let result_0 = Session::new(match_id, "ROOM-0".to_string(), 0);
    assert!(result_0.is_err(), "Capacity 0 should be rejected");
    
    let result_1 = Session::new(match_id, "ROOM-1".to_string(), 1);
    assert!(result_1.is_err(), "Capacity 1 should be rejected");
    
    let result_9 = Session::new(match_id, "ROOM-9".to_string(), 9);
    assert!(result_9.is_err(), "Capacity 9 should be rejected");
    
    println!("✓ Invalid capacities rejected");
}

// ============================================================================
// TEST: Player join and leave
// ============================================================================

#[tokio::test]
async fn test_player_join_and_leave() {
    let mut session = Session::new(
        Uuid::new_v4(),
        "ROOM-JOIN".to_string(),
        2,
    ).expect("Failed to create session");

    let player1 = PlayerId("player-1".to_string());
    let player2 = PlayerId("player-2".to_string());

    // Join first player
    session.join_player(player1.clone()).expect("Failed to join player 1");
    assert_eq!(session.players.len(), 1);
    println!("✓ Player 1 joined (1/2)");

    // Join second player
    session.join_player(player2.clone()).expect("Failed to join player 2");
    assert_eq!(session.players.len(), 2);
    println!("✓ Player 2 joined (2/2)");

    // Try to join third player (capacity)
    let player3 = PlayerId("player-3".to_string());
    let result = session.join_player(player3.clone());
    assert!(result.is_err(), "Third player should not fit");
    assert_eq!(result.unwrap_err(), SessionError::SessionFull);
    println!("✓ Third player rejected (capacity full)");

    // Try to join duplicate
    let result = session.join_player(player1.clone());
    assert!(result.is_err(), "Duplicate join should fail");
    println!("✓ Duplicate join rejected");

    // Leave player
    session.leave_player(&player1).expect("Failed to leave player 1");
    assert_eq!(session.players.len(), 1);
    println!("✓ Player 1 left (1/2)");
}

// ============================================================================
// TEST: Cannot join in non-Lobby state
// ============================================================================

#[tokio::test]
async fn test_cannot_join_in_active_state() {
    let mut session = Session::new(
        Uuid::new_v4(),
        "ROOM-ACTIVE".to_string(),
        2,
    ).expect("Failed to create session");

    let player1 = PlayerId("player-1".to_string());
    let player2 = PlayerId("player-2".to_string());

    session.join_player(player1.clone()).ok();
    session.join_player(player2.clone()).ok();

    // Transition to Ready
    session.transition(SessionState::Ready).expect("Failed to transition to Ready");
    assert_eq!(session.state, SessionState::Ready);
    println!("✓ Transitioned to Ready");

    // Try to join in Ready state
    let player3 = PlayerId("player-3".to_string());
    let result = session.join_player(player3);
    assert!(result.is_err(), "Should not be able to join in Ready state");
    assert_eq!(result.unwrap_err(), SessionError::CannotJoinInState(SessionState::Ready));
    println!("✓ Join rejected in Ready state");
}

// ============================================================================
// TEST: State transitions - valid and invalid
// ============================================================================

#[tokio::test]
async fn test_state_transitions() {
    let mut session = Session::new(
        Uuid::new_v4(),
        "ROOM-TRANSITION".to_string(),
        2,
    ).expect("Failed to create session");

    // Invalid: Lobby -> Active (must go Lobby -> Ready -> Active)
    let result = session.transition(SessionState::Active);
    assert!(result.is_err(), "Cannot go directly from Lobby to Active");
    println!("✓ Invalid transition Lobby->Active rejected");

    let player1 = PlayerId("player-1".to_string());
    let player2 = PlayerId("player-2".to_string());
    session.join_player(player1).ok();
    session.join_player(player2).ok();

    // Valid: Lobby -> Ready
    session.transition(SessionState::Ready).expect("Failed to transition to Ready");
    assert_eq!(session.state, SessionState::Ready);
    println!("✓ Valid transition Lobby->Ready");

    // Valid: Ready -> Active
    session.transition(SessionState::Active).expect("Failed to transition to Active");
    assert_eq!(session.state, SessionState::Active);
    println!("✓ Valid transition Ready->Active");

    // Valid: Active -> Reconnecting
    session.transition(SessionState::Reconnecting).expect("Failed to transition to Reconnecting");
    assert_eq!(session.state, SessionState::Reconnecting);
    println!("✓ Valid transition Active->Reconnecting");

    // Valid: Reconnecting -> Active
    session.transition(SessionState::Active).expect("Failed to transition back to Active");
    assert_eq!(session.state, SessionState::Active);
    println!("✓ Valid transition Reconnecting->Active");

    // Valid: Active -> Finished
    session.transition(SessionState::Finished).expect("Failed to transition to Finished");
    assert_eq!(session.state, SessionState::Finished);
    println!("✓ Valid transition Active->Finished");

    // Valid: Finished -> Cleaned
    session.transition(SessionState::Cleaned).expect("Failed to transition to Cleaned");
    assert_eq!(session.state, SessionState::Cleaned);
    println!("✓ Valid transition Finished->Cleaned");
}

// ============================================================================
// TEST: Ready-up logic (all players required)
// ============================================================================

#[tokio::test]
async fn test_ready_up_logic() {
    let mut session = Session::new(
        Uuid::new_v4(),
        "ROOM-READY".to_string(),
        2,
    ).expect("Failed to create session");

    // Cannot ready up with no players
    let result = session.transition(SessionState::Ready);
    assert!(result.is_err(), "Cannot ready up with no players");
    println!("✓ Cannot ready up with no players");

    // Add one player
    let player1 = PlayerId("player-1".to_string());
    session.join_player(player1).ok();

    // Cannot ready up with partial players
    let result = session.transition(SessionState::Ready);
    assert!(result.is_err(), "Cannot ready up with 1/2 players");
    println!("✓ Cannot ready up with 1/2 players");

    // Add second player
    let player2 = PlayerId("player-2".to_string());
    session.join_player(player2).ok();

    // Now can ready up
    session.transition(SessionState::Ready).expect("Should be able to ready up with all players");
    assert_eq!(session.state, SessionState::Ready);
    println!("✓ Ready up successful with all players");
}

// ============================================================================
// TEST: Reconnect token generation and validation
// ============================================================================

#[tokio::test]
async fn test_reconnect_token() {
    let player_id = PlayerId("player-reconnect".to_string());
    let token = ReconnectToken::new(player_id.clone());

    assert!(token.is_valid(), "Token should be valid immediately");
    assert!(token.token.contains("player-reconnect"), "Token should contain player ID");
    println!("✓ Reconnect token created and valid");

    // Create expired token (manually)
    let expired_token = ReconnectToken {
        token: "expired-token".to_string(),
        player_id,
        issued_at: chrono::Utc::now() - chrono::Duration::minutes(10),
        expires_at: chrono::Utc::now() - chrono::Duration::minutes(5),
    };
    assert!(!expired_token.is_valid(), "Expired token should be invalid");
    println!("✓ Expired token correctly identified as invalid");
}

// ============================================================================
// TEST: Repository - create and retrieve session
// ============================================================================

#[tokio::test]
async fn test_repository_create_and_get() {
    let repo = InMemorySessionRepository::new();
    
    let session = Session::new(
        Uuid::new_v4(),
        "ROOM-REPO".to_string(),
        2,
    ).expect("Failed to create session");
    
    let session_id = session.session_id;
    
    // Create
    let created = repo.create(session).await.expect("Failed to create session");
    assert_eq!(created.session_id, session_id);
    println!("✓ Session created in repository");
    
    // Get by ID
    let retrieved = repo.get(session_id).await.expect("Failed to get session")
        .expect("Session not found");
    assert_eq!(retrieved.session_id, session_id);
    assert_eq!(retrieved.room_code, "ROOM-REPO");
    println!("✓ Session retrieved by ID");
    
    // Get by room code
    let by_code = repo.get_by_room_code("ROOM-REPO").await.expect("Failed to get by code")
        .expect("Session not found");
    assert_eq!(by_code.session_id, session_id);
    println!("✓ Session retrieved by room code");
}

// ============================================================================
// TEST: Repository - duplicate room code rejected
// ============================================================================

#[tokio::test]
async fn test_repository_duplicate_room_code() {
    let repo = InMemorySessionRepository::new();
    
    let session1 = Session::new(
        Uuid::new_v4(),
        "ROOM-DUP".to_string(),
        2,
    ).expect("Failed to create session 1");
    
    let session2 = Session::new(
        Uuid::new_v4(),
        "ROOM-DUP".to_string(),
        2,
    ).expect("Failed to create session 2");
    
    // Create first
    repo.create(session1).await.expect("Failed to create session 1");
    println!("✓ First session created");
    
    // Try to create second with same room code
    let result = repo.create(session2).await;
    assert!(result.is_err(), "Duplicate room code should be rejected");
    println!("✓ Duplicate room code rejected");
}

// ============================================================================
// TEST: Repository - heartbeat and stale detection
// ============================================================================

#[tokio::test]
async fn test_repository_stale_detection() {
    let repo = InMemorySessionRepository::new();
    
    let session1 = Session::new(
        Uuid::new_v4(),
        "ROOM-FRESH".to_string(),
        2,
    ).expect("Failed to create session");
    
    let mut session2 = Session::new(
        Uuid::new_v4(),
        "ROOM-STALE".to_string(),
        2,
    ).expect("Failed to create session");
    
    let id1 = session1.session_id;
    let id2 = session2.session_id;
    
    // Make session2 stale
    session2.last_heartbeat = chrono::Utc::now() - chrono::Duration::seconds(40);
    
    repo.create(session1).await.ok();
    repo.create(session2).await.ok();
    
    // Update heartbeat for session1
    repo.heartbeat(id1).await.expect("Failed to heartbeat session 1");
    println!("✓ Session 1 heartbeat updated");
    
    // Find stale (threshold 30 seconds)
    let stale = repo.find_stale(30).await.expect("Failed to find stale");
    assert_eq!(stale.len(), 1, "Should find 1 stale session");
    assert_eq!(stale[0].session_id, id2);
    println!("✓ Stale session correctly identified");
}

// ============================================================================
// TEST: Repository - reconnect token storage and validation
// ============================================================================

#[tokio::test]
async fn test_repository_reconnect_tokens() {
    let repo = InMemorySessionRepository::new();
    
    let player_id = PlayerId("player-token".to_string());
    let token = ReconnectToken::new(player_id);
    let token_str = token.token.clone();
    
    // Store
    repo.store_reconnect_token(token.clone()).await.expect("Failed to store token");
    println!("✓ Reconnect token stored");
    
    // Get
    let retrieved = repo.get_reconnect_token(&token_str).await.expect("Failed to get token")
        .expect("Token not found");
    assert_eq!(retrieved.token, token_str);
    println!("✓ Reconnect token retrieved");
    
    // Validate (should succeed)
    let validated = repo.validate_reconnect_token(&token_str).await.expect("Failed to validate");
    assert_eq!(validated.token, token_str);
    println!("✓ Valid reconnect token validated");
    
    // Validate with bad token
    let result = repo.validate_reconnect_token("bad-token").await;
    assert!(result.is_err(), "Bad token should fail validation");
    println!("✓ Bad reconnect token rejected");
}

// ============================================================================
// TEST: In-Memory Session Flow - Two players join, ready up, play 100 ticks, finish
// Note: Uses InMemorySessionRepository, does NOT exercise UDP protocol or network transport
// ============================================================================

#[tokio::test]
async fn test_inmemory_two_player_session_flow() {
    let repo = InMemorySessionRepository::new();
    
    let mut session = Session::new(
        Uuid::new_v4(),
        "ROOM-E2E".to_string(),
        2,
    ).expect("Failed to create session");
    
    let session_id = session.session_id;
    let player1 = PlayerId("player-e2e-1".to_string());
    let player2 = PlayerId("player-e2e-2".to_string());
    
    // Create in repository
    session = repo.create(session).await.expect("Failed to create session");
    println!("✓ Session created");
    
    // Players join
    session.join_player(player1.clone()).expect("Player 1 join failed");
    session.join_player(player2.clone()).expect("Player 2 join failed");
    assert_eq!(session.players.len(), 2);
    println!("✓ Both players joined");
    
    // Ready up
    session.transition(SessionState::Ready).expect("Ready transition failed");
    assert_eq!(session.state, SessionState::Ready);
    repo.update(session.clone()).await.ok();
    println!("✓ Session ready");
    
    // Start active play
    session.transition(SessionState::Active).expect("Active transition failed");
    assert_eq!(session.state, SessionState::Active);
    repo.update(session.clone()).await.ok();
    println!("✓ Session active");
    
    // Simulate 100 ticks
    for _ in 0..100 {
        session.tick();
    }
    assert_eq!(session.tick_count, 100);
    repo.update(session.clone()).await.ok();
    println!("✓ 100 ticks played");
    
    // Finish
    session.transition(SessionState::Finished).expect("Finished transition failed");
    assert_eq!(session.state, SessionState::Finished);
    repo.update(session.clone()).await.ok();
    println!("✓ Session finished");
    
    // Verify persistence
    let final_session = repo.get(session_id).await.expect("Failed to get final session")
        .expect("Session not found");
    assert_eq!(final_session.tick_count, 100);
    assert_eq!(final_session.state, SessionState::Finished);
    println!("✓ E2E session verified: 2 players, 100 ticks, finished");
}
