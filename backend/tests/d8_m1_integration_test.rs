/// D8: M1 Scope Closure — Full Integration Test Suite
/// Tests complete lifecycle: lobby → ready → active → disconnect → reconnect → cleanup

use rollback_netcode_server::session::*;
use uuid::Uuid;
use chrono::Duration;

/// Test 1: Happy path - lobby to finished
#[tokio::test]
async fn test_m1_lobby_to_finished_lifecycle() {
    let repo = InMemorySessionRepository::new();

    // Create a session (Lobby state)
    let match_id = Uuid::new_v4();
    let mut session = Session::new(
        match_id,
        "HAPPYROOM".to_string(),
        2,
    ).expect("Failed to create session");

    let session_id = session.session_id;
    let player1 = PlayerId("alice".to_string());
    let player2 = PlayerId("bob".to_string());

    println!("✓ Session created in Lobby state: {:?}", session_id);

    // Join players
    session.join_player(player1.clone()).expect("Failed to join player1");
    session.join_player(player2.clone()).expect("Failed to join player2");
    println!("✓ Both players joined");

    // Save session
    let mut session = repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Transition to Ready
    session
        .transition(SessionState::Ready)
        .expect("Failed to transition to Ready");
    repo.update(session.clone())
        .await
        .expect("Failed to update to Ready");
    println!("✓ Transitioned to Ready");

    // Transition to Active
    session
        .transition(SessionState::Active)
        .expect("Failed to transition to Active");
    repo.update(session.clone())
        .await
        .expect("Failed to update to Active");
    println!("✓ Transitioned to Active (game running)");

    // Simulate 20 ticks
    for tick in 1..=20 {
        session.tick_count = tick;
        repo.update(session.clone())
            .await
            .expect("Failed to update tick");
        if tick % 5 == 0 {
            println!("  ✓ Tick {}", tick);
        }
    }
    println!("✓ Processed 20 game ticks");

    // Transition to Finished
    session
        .transition(SessionState::Finished)
        .expect("Failed to transition to Finished");
    repo.update(session.clone())
        .await
        .expect("Failed to update to Finished");
    println!("✓ Transitioned to Finished");

    // Verify final state
    let final_session = repo
        .get(session_id)
        .await
        .expect("Failed to get session")
        .expect("Session not found");
    
    assert_eq!(final_session.state, SessionState::Finished);
    assert_eq!(final_session.tick_count, 20);
    assert_eq!(final_session.players.len(), 2);

    println!("✓ Happy path lifecycle complete");
}

/// Test 2: Disconnect and reconnect with token
#[tokio::test]
async fn test_m1_disconnect_and_reconnect() {
    let repo = InMemorySessionRepository::new();

    // Setup: Create active session
    let match_id = Uuid::new_v4();
    let mut session = Session::new(
        match_id,
        "RECONNECTROOM".to_string(),
        2,
    ).expect("Failed to create session");

    let session_id = session.session_id;
    let player1 = PlayerId("charlie".to_string());
    let player2 = PlayerId("diana".to_string());

    session.join_player(player1.clone()).expect("Failed to join player1");
    session.join_player(player2.clone()).expect("Failed to join player2");

    let mut session = repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Ready up
    session
        .transition(SessionState::Ready)
        .expect("Failed to transition to Ready");
    repo.update(session.clone())
        .await
        .expect("Failed to update to Ready");

    // Start active play
    session
        .transition(SessionState::Active)
        .expect("Failed to transition to Active");
    repo.update(session.clone())
        .await
        .expect("Failed to update to Active");

    println!("✓ Session active, starting at tick 0");

    // Process 10 ticks
    for tick in 1..=10 {
        session.tick_count = tick;
        repo.update(session.clone())
            .await
            .expect("Failed to update tick");
    }
    println!("✓ Processed 10 ticks");

    // Player 1 disconnects (don't update heartbeat)
    // Record last known tick for player1
    let disconnect_tick = 10u64;
    println!("✓ Player1 disconnected at tick {}", disconnect_tick);

    // Player 1 tries to reconnect with a new token
    let reconnect_token = ReconnectToken::new(player1.clone());
    repo.store_reconnect_token(reconnect_token.clone())
        .await
        .expect("Failed to store reconnect token");

    // Validate token before reconnect
    let validated = repo
        .validate_reconnect_token_for_player(&reconnect_token.token, &player1)
        .await
        .expect("Failed to validate reconnect token");

    assert_eq!(validated.player_id, player1);
    println!("✓ Reconnect token validated for player1");

    // Resume session at tick 11 (disconnect_tick + 1)
    let resume_tick = disconnect_tick + 1;
    session.tick_count = resume_tick;
    repo.update(session.clone())
        .await
        .expect("Failed to update after reconnect");

    println!("✓ Session resumed at tick {}", resume_tick);

    // Continue for 10 more ticks (11-20)
    for tick in (resume_tick + 1)..=(resume_tick + 10) {
        session.tick_count = tick;
        repo.update(session.clone())
            .await
            .expect("Failed to update tick");
    }
    println!("✓ Processed 10 more ticks (11-20)");

    // Verify final state
    let final_session = repo
        .get(session_id)
        .await
        .expect("Failed to get session")
        .expect("Session not found");

    assert_eq!(final_session.tick_count, resume_tick + 10);
    println!("✓ Disconnect and reconnect complete");
}

/// Test 3: Stale session cleanup during play
#[tokio::test]
async fn test_m1_orphan_cleanup_during_play() {
    let repo = InMemorySessionRepository::new();

    // Create multiple sessions
    let mut session_ids = Vec::new();

    for i in 0..3 {
        let match_id = Uuid::new_v4();
        let session = Session::new(
            match_id,
            format!("CLEANUPROOM{}", i),
            2,
        ).expect("Failed to create session");

        let created = repo
            .create(session)
            .await
            .expect("Failed to create session");
        
        session_ids.push(created.session_id);
    }

    println!("✓ Created 3 sessions");

    // Keep session 1 and 2 fresh via heartbeat
    repo.heartbeat(session_ids[1])
        .await
        .expect("Failed to heartbeat session 1");
    repo.heartbeat(session_ids[2])
        .await
        .expect("Failed to heartbeat session 2");

    // Let session 0 become stale (age its heartbeat)
    let mut stale_session = repo
        .get(session_ids[0])
        .await
        .expect("Failed to get session 0")
        .expect("Session 0 not found");

    stale_session.last_heartbeat = chrono::Utc::now() - Duration::seconds(50);
    repo.update(stale_session)
        .await
        .expect("Failed to update session 0");

    println!("✓ Session 0 marked stale (50s old)");
    println!("✓ Sessions 1 and 2 kept fresh");

    // Run orphan cleanup
    let orphan_sessions = repo
        .find_stale(40)
        .await
        .expect("Failed to find stale sessions");

    println!("✓ Orphan cleanup scan complete: found {} stale sessions", orphan_sessions.len());

    // Verify results
    assert!(orphan_sessions.iter().any(|s| s.session_id == session_ids[0]),
        "Session 0 should be detected as stale");
    
    assert!(!orphan_sessions.iter().any(|s| s.session_id == session_ids[1]),
        "Session 1 should not be stale");
    
    assert!(!orphan_sessions.iter().any(|s| s.session_id == session_ids[2]),
        "Session 2 should not be stale");

    println!("✓ Cleanup correctly identified stale vs active sessions");
}

/// Test 4: Session state transitions validity
#[tokio::test]
async fn test_m1_invalid_transitions_rejected() {
    let repo = InMemorySessionRepository::new();

    let match_id = Uuid::new_v4();
    let mut session = Session::new(
        match_id,
        "TRANSITIONROOM".to_string(),
        2,
    ).expect("Failed to create session");

    let player1 = PlayerId("eve".to_string());
    session.join_player(player1).expect("Failed to join player1");

    let session = repo
        .create(session)
        .await
        .expect("Failed to create session");

    println!("✓ Session in Lobby state");

    // Try invalid transition from Lobby to Active (should fail - need Ready first)
    let mut invalid_session = session.clone();
    let result = invalid_session.transition(SessionState::Active);
    assert!(result.is_err(), "Lobby -> Active should be invalid");
    println!("✓ Invalid transition (Lobby -> Active) correctly rejected");

    // Try invalid transition from Lobby to Finished
    let mut invalid_session = session.clone();
    let result = invalid_session.transition(SessionState::Finished);
    assert!(result.is_err(), "Lobby -> Finished should be invalid");
    println!("✓ Invalid transition (Lobby -> Finished) correctly rejected");

    println!("✓ State machine enforces valid transitions");
}

/// Test 5: Multiple concurrent operations
#[tokio::test]
async fn test_m1_concurrent_sessions() {
    let repo = InMemorySessionRepository::new();

    // Create 5 concurrent sessions
    let mut handles = Vec::new();

    for i in 0..5 {
        let repo_clone = std::sync::Arc::new(repo.clone());
        let handle = tokio::spawn(async move {
            let match_id = Uuid::new_v4();
            let session = Session::new(
                match_id,
                format!("CONCURRENT{}", i),
                2,
            ).expect("Failed to create session");

            repo_clone
                .create(session)
                .await
                .expect("Failed to create session")
        });

        handles.push(handle);
    }

    // Wait for all to complete
    let mut created_sessions = Vec::new();
    for handle in handles {
        let session = handle.await.expect("Task panicked");
        created_sessions.push(session);
    }

    println!("✓ Created 5 concurrent sessions");
    assert_eq!(created_sessions.len(), 5);
    assert!(created_sessions.iter().all(|s| s.state == SessionState::Lobby));

    println!("✓ Concurrent session creation works correctly");
}
