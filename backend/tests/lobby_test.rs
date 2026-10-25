// Lobby subsystem tests (C1)
// Tests: create, join, leave, ready, timeout, concurrent joins, idempotent ready

use chrono::Utc;
use rollback_netcode_server::session::{LobbyRegistry, PlayerId, SessionError};
use std::sync::Arc;

/// Test 1: Create a lobby and verify its properties
#[tokio::test]
async fn test_create_lobby() {
    let registry = LobbyRegistry::new();

    // Create a lobby with max_players=2 and 60 second timeout
    let lobby_id = registry.create_lobby(2, 60).await.expect("should create lobby");

    // Verify lobby exists
    let lobby = registry.get_lobby(&lobby_id).await.expect("should get lobby");
    assert!(lobby.is_some(), "lobby should exist in registry");

    let lobby = lobby.unwrap();
    assert_eq!(lobby.max_players, 2, "max_players should be 2");
    assert_eq!(lobby.auto_start_after_secs, 60, "auto_start_after_secs should be 60");
    assert!(lobby.players.is_empty(), "players should be empty initially");
    assert!(lobby.ready_states.is_empty(), "ready_states should be empty initially");
}

/// Test 2: Join players to a lobby
#[tokio::test]
async fn test_join_lobby() {
    let registry = LobbyRegistry::new();
    let lobby_id = registry.create_lobby(2, 60).await.expect("should create lobby");

    let player1 = PlayerId("player1".to_string());
    let player2 = PlayerId("player2".to_string());

    // Join first player
    registry.join_lobby(&lobby_id, player1.clone()).await.expect("player1 should join");

    // Join second player
    registry.join_lobby(&lobby_id, player2.clone()).await.expect("player2 should join");

    // Verify both players in lobby
    let lobby = registry.get_lobby(&lobby_id).await.expect("should get lobby");
    let lobby = lobby.unwrap();
    assert_eq!(lobby.players.len(), 2, "should have 2 players");
    assert!(lobby.players.contains(&player1), "player1 should be in lobby");
    assert!(lobby.players.contains(&player2), "player2 should be in lobby");

    // Verify both marked as NotReady
    use rollback_netcode_server::session::ReadyState;
    assert_eq!(
        lobby.ready_states.get(&player1),
        Some(&ReadyState::NotReady),
        "player1 should be NotReady"
    );
    assert_eq!(
        lobby.ready_states.get(&player2),
        Some(&ReadyState::NotReady),
        "player2 should be NotReady"
    );

    // Try to join a third player (exceeds capacity)
    let player3 = PlayerId("player3".to_string());
    let result = registry.join_lobby(&lobby_id, player3).await;
    assert!(result.is_err(), "should reject join when full");
    assert_eq!(result.unwrap_err(), SessionError::SessionFull);
}

/// Test 3: Leave a lobby
#[tokio::test]
async fn test_leave_lobby() {
    let registry = LobbyRegistry::new();
    let lobby_id = registry.create_lobby(2, 60).await.expect("should create lobby");

    let player1 = PlayerId("player1".to_string());
    let player2 = PlayerId("player2".to_string());

    // Join both players
    registry.join_lobby(&lobby_id, player1.clone()).await.expect("player1 should join");
    registry.join_lobby(&lobby_id, player2.clone()).await.expect("player2 should join");

    // Verify both in lobby
    let lobby = registry.get_lobby(&lobby_id).await.expect("should get lobby");
    assert_eq!(lobby.unwrap().players.len(), 2);

    // Player 1 leaves
    registry.leave_lobby(&lobby_id, &player1).await.expect("player1 should leave");

    // Verify player1 removed
    let lobby = registry.get_lobby(&lobby_id).await.expect("should get lobby");
    let lobby = lobby.unwrap();
    assert_eq!(lobby.players.len(), 1, "should have 1 player after leave");
    assert!(!lobby.players.contains(&player1), "player1 should not be in lobby");
    assert!(lobby.players.contains(&player2), "player2 should still be in lobby");

    // Player 2 leaves (last player)
    registry.leave_lobby(&lobby_id, &player2).await.expect("player2 should leave");

    // Verify lobby is cleaned up (removed from registry)
    let lobby = registry.get_lobby(&lobby_id).await.expect("should query registry");
    assert!(
        lobby.is_none(),
        "empty lobby should be cleaned up and removed from registry"
    );
}

/// Test 4: Players ready and auto-start logic
#[tokio::test]
async fn test_ready_all() {
    let registry = LobbyRegistry::new();
    let lobby_id = registry.create_lobby(2, 60).await.expect("should create lobby");

    let player1 = PlayerId("player1".to_string());
    let player2 = PlayerId("player2".to_string());

    // Join both players
    registry.join_lobby(&lobby_id, player1.clone()).await.expect("player1 should join");
    registry.join_lobby(&lobby_id, player2.clone()).await.expect("player2 should join");

    // Initially, all_ready should be false
    let lobby = registry.get_lobby(&lobby_id).await.unwrap().unwrap();
    assert!(!lobby.all_ready(), "not all players ready initially");

    // Mark player 1 as ready
    registry.set_player_ready(&lobby_id, &player1).await.expect("player1 should set ready");

    // Still not all ready
    let lobby = registry.get_lobby(&lobby_id).await.unwrap().unwrap();
    assert!(!lobby.all_ready(), "not all players ready after player1 ready");

    // Mark player 2 as ready
    registry.set_player_ready(&lobby_id, &player2).await.expect("player2 should set ready");

    // Now all ready
    let lobby = registry.get_lobby(&lobby_id).await.unwrap().unwrap();
    assert!(lobby.all_ready(), "all players should be ready");

    // Check can_transition_to_match_ready (should be true now)
    let can_transition = registry.can_transition_to_match_ready(&lobby_id).await.expect("should check transition");
    assert!(can_transition, "should be able to transition to MatchReady");
}

/// Test 5: Auto-start timeout logic
#[tokio::test]
async fn test_ready_timeout() {
    let registry = LobbyRegistry::new();

    // Create a lobby with a very short timeout (1 second)
    let lobby_id = registry.create_lobby(2, 1).await.expect("should create lobby");

    let player1 = PlayerId("player1".to_string());

    // Join one player
    registry.join_lobby(&lobby_id, player1.clone()).await.expect("player1 should join");

    // Lobby should not be ready (only 1 player, timeout not fired yet)
    let lobby = registry.get_lobby(&lobby_id).await.unwrap().unwrap();
    assert!(!lobby.all_ready(), "not all players ready");
    assert!(
        !lobby.can_transition_to_match_ready(),
        "should not transition (no timeout yet)"
    );

    // Simulate time passing by creating a lobby with past created_at
    // We'll verify the timeout logic works correctly
    let past_time = Utc::now() - chrono::Duration::seconds(10);
    let old_lobby = rollback_netcode_server::session::Lobby::new_with_created_at(2, 1, past_time)
        .expect("should create lobby with past time");

    // This lobby has no players but timeout has fired
    assert!(
        old_lobby.should_auto_start(),
        "timeout should have fired for old lobby"
    );

    // A newly created lobby should not have timeout yet
    let new_lobby = rollback_netcode_server::session::Lobby::new(2, 60).expect("should create new lobby");
    assert!(
        !new_lobby.should_auto_start(),
        "timeout should not have fired for new lobby"
    );
}

/// Test 6: Auto-start when all players ready
#[tokio::test]
async fn test_auto_start() {
    let registry = LobbyRegistry::new();

    // Create a lobby with a very long timeout
    let lobby_id = registry.create_lobby(2, 1000).await.expect("should create lobby");

    let player1 = PlayerId("player1".to_string());
    let player2 = PlayerId("player2".to_string());

    // Join both players
    registry.join_lobby(&lobby_id, player1.clone()).await.expect("player1 should join");
    registry.join_lobby(&lobby_id, player2.clone()).await.expect("player2 should join");

    // Timeout should not have fired yet
    let lobby = registry.get_lobby(&lobby_id).await.unwrap().unwrap();
    assert!(!lobby.should_auto_start(), "timeout should not fire yet");

    // But we can transition because all are ready (all_ready checks if count == max_players)
    // Actually, they're not ready yet. Let's mark them ready.
    drop(lobby);

    registry.set_player_ready(&lobby_id, &player1).await.expect("player1 should set ready");
    registry.set_player_ready(&lobby_id, &player2).await.expect("player2 should set ready");

    // Now can_transition_to_match_ready should be true (all_ready returns true)
    let can_transition = registry
        .can_transition_to_match_ready(&lobby_id)
        .await
        .expect("should check transition");
    assert!(
        can_transition,
        "should transition to MatchReady (all ready, not timeout)"
    );
}

/// Test 7: Concurrent joins
#[tokio::test]
async fn test_concurrent_joins() {
    let registry = Arc::new(LobbyRegistry::new());

    // Create a lobby with max 4 players
    let lobby_id = registry.create_lobby(4, 60).await.expect("should create lobby");

    // Spawn N concurrent tasks to join
    let mut handles = vec![];

    for i in 0..4 {
        let registry_clone = Arc::clone(&registry);
        let lobby_id_clone = lobby_id.clone();

        let handle = tokio::spawn(async move {
            let player = PlayerId(format!("player{}", i));
            registry_clone.join_lobby(&lobby_id_clone, player).await
        });

        handles.push(handle);
    }

    // Wait for all joins to complete
    for handle in handles {
        let result = handle.await.expect("join task should complete");
        assert!(result.is_ok(), "join should succeed");
    }

    // Verify all 4 players in lobby
    let lobby = registry.get_lobby(&lobby_id).await.expect("should get lobby");
    let lobby = lobby.unwrap();
    assert_eq!(lobby.players.len(), 4, "should have 4 players");

    // Try to join a 5th concurrently (should fail)
    let registry_clone = Arc::clone(&registry);
    let lobby_id_clone = lobby_id.clone();

    let handle = tokio::spawn(async move {
        let player = PlayerId("player5".to_string());
        registry_clone.join_lobby(&lobby_id_clone, player).await
    });

    let result = handle.await.expect("task should complete");
    assert!(result.is_err(), "5th join should fail");
    assert_eq!(result.unwrap_err(), SessionError::SessionFull);
}

/// Test 8: Idempotent ready
#[tokio::test]
async fn test_idempotent_ready() {
    let registry = LobbyRegistry::new();
    let lobby_id = registry.create_lobby(2, 60).await.expect("should create lobby");

    let player1 = PlayerId("player1".to_string());
    let player2 = PlayerId("player2".to_string());

    // Join both players
    registry.join_lobby(&lobby_id, player1.clone()).await.expect("player1 should join");
    registry.join_lobby(&lobby_id, player2.clone()).await.expect("player2 should join");

    // Set player1 ready
    registry.set_player_ready(&lobby_id, &player1).await.expect("player1 should set ready");

    // Try to set player1 ready again (should error, not be idempotent)
    let result = registry.set_player_ready(&lobby_id, &player1).await;
    assert!(result.is_err(), "setting ready twice should error");

    // Verify state unchanged
    let lobby = registry.get_lobby(&lobby_id).await.unwrap().unwrap();
    use rollback_netcode_server::session::ReadyState;
    assert_eq!(
        lobby.ready_states.get(&player1),
        Some(&ReadyState::Ready),
        "player1 should still be Ready"
    );
    assert!(!lobby.all_ready(), "not all players ready");

    // Try to set ready on a player not in lobby (should error)
    let player3 = PlayerId("player3".to_string());
    let result = registry.set_player_ready(&lobby_id, &player3).await;
    assert!(result.is_err(), "should reject ready for player not in lobby");
    assert_eq!(
        result.unwrap_err(),
        SessionError::PlayerNotInSession("player3".to_string())
    );
}
