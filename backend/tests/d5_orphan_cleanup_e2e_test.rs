/// D5: Orphan Session Cleanup E2E
/// Tests stale session detection and cleanup using real session repository

use rollback_netcode_server::session::*;
use uuid::Uuid;
use chrono::{Utc, Duration};

/// Test 1: Stale sessions are detected (no heartbeat for 41+ seconds)
#[tokio::test]
async fn test_stale_sessions_detected() {
    let repo = InMemorySessionRepository::new();

    // Create a session
    let match_id = Uuid::new_v4();
    let session = Session::new(
        match_id,
        "TESTROOM1".to_string(),
        2,
    ).expect("Session creation failed");

    let session_id = session.session_id;
    let created_session = repo
        .create(session)
        .await
        .expect("Failed to create session");

    println!("✓ Session created: {:?}", session_id);

    // Manually set the last_heartbeat to 41 seconds in the past
    // by updating the session with an old heartbeat
    let mut stale_session = repo
        .get(session_id)
        .await
        .expect("Failed to get session")
        .expect("Session not found");
    
    stale_session.last_heartbeat = Utc::now() - Duration::seconds(41);
    repo.update(stale_session)
        .await
        .expect("Failed to update session");

    println!("✓ Set heartbeat to 41 seconds in the past");

    // Find stale sessions (threshold: 40 seconds)
    let stale_sessions = repo
        .find_stale(40)
        .await
        .expect("Failed to find stale sessions");

    assert!(
        stale_sessions.iter().any(|s| s.session_id == session_id),
        "Stale session should be detected"
    );
    println!("✓ Stale session correctly detected by find_stale(40)");
}

/// Test 2: Active sessions are not detected as stale
#[tokio::test]
async fn test_active_sessions_preserved() {
    let repo = InMemorySessionRepository::new();

    // Create multiple sessions
    let mut session_ids = Vec::new();
    for i in 0..3 {
        let match_id = Uuid::new_v4();
        let session = Session::new(
            match_id,
            format!("TESTROOM{}", i),
            2,
        ).expect("Session creation failed");

        let created = repo
            .create(session)
            .await
            .expect("Failed to create session");
        
        session_ids.push(created.session_id);
    }

    println!("✓ Created {} active sessions", session_ids.len());

    // Heartbeat each session to keep it fresh
    for &sid in &session_ids {
        repo.heartbeat(sid)
            .await
            .expect("Failed to heartbeat");
    }

    println!("✓ Heartbeated all sessions");

    // Find stale sessions (should be empty for fresh sessions)
    let stale_sessions = repo
        .find_stale(40)
        .await
        .expect("Failed to find stale sessions");

    let stale_count = stale_sessions
        .iter()
        .filter(|s| session_ids.contains(&s.session_id))
        .count();

    assert_eq!(
        stale_count, 0,
        "Recently heartbeated sessions should not be marked stale"
    );
    println!("✓ Active sessions correctly not detected as stale");
}

/// Test 3: Mix of stale and active sessions
#[tokio::test]
async fn test_mixed_stale_and_active() {
    let repo = InMemorySessionRepository::new();

    // Create 3 sessions
    let mut session_ids = Vec::new();
    for i in 0..3 {
        let match_id = Uuid::new_v4();
        let session = Session::new(
            match_id,
            format!("MIXROOM{}", i),
            2,
        ).expect("Session creation failed");

        let created = repo
            .create(session)
            .await
            .expect("Failed to create session");
        
        session_ids.push(created.session_id);
    }

    println!("✓ Created 3 sessions");

    // Make first session stale (no heartbeat)
    let mut stale_session = repo
        .get(session_ids[0])
        .await
        .expect("Failed to get session")
        .expect("Session not found");
    stale_session.last_heartbeat = Utc::now() - Duration::seconds(50);
    repo.update(stale_session)
        .await
        .expect("Failed to update session");

    // Heartbeat the other two to keep them fresh
    for &sid in &session_ids[1..] {
        repo.heartbeat(sid)
            .await
            .expect("Failed to heartbeat");
    }

    println!("✓ Session 0 stale, sessions 1-2 fresh");

    // Find stale sessions
    let stale_sessions = repo
        .find_stale(40)
        .await
        .expect("Failed to find stale sessions");

    assert!(
        stale_sessions.iter().any(|s| s.session_id == session_ids[0]),
        "First session should be detected as stale"
    );
    assert!(
        !stale_sessions.iter().any(|s| s.session_id == session_ids[1]),
        "Second session should not be stale"
    );
    assert!(
        !stale_sessions.iter().any(|s| s.session_id == session_ids[2]),
        "Third session should not be stale"
    );

    println!("✓ Correctly identified 1 stale and 2 active sessions");
}

/// Test 4: Heartbeat refresh prevents stale marking
#[tokio::test]
async fn test_heartbeat_refresh_prevents_stale() {
    let repo = InMemorySessionRepository::new();

    // Create a session
    let match_id = Uuid::new_v4();
    let session = Session::new(
        match_id,
        "REFRESHROOM".to_string(),
        2,
    ).expect("Session creation failed");

    let session_id = session.session_id;
    repo.create(session)
        .await
        .expect("Failed to create session");

    // Make it old
    let mut old_session = repo
        .get(session_id)
        .await
        .expect("Failed to get session")
        .expect("Session not found");
    old_session.last_heartbeat = Utc::now() - Duration::seconds(50);
    repo.update(old_session)
        .await
        .expect("Failed to update session");

    println!("✓ Session set to stale (50s old)");

    // Verify it's stale
    let stale_before = repo
        .find_stale(40)
        .await
        .expect("Failed to find stale");
    assert!(stale_before.iter().any(|s| s.session_id == session_id));
    println!("✓ Confirmed session is stale");

    // Heartbeat to refresh
    repo.heartbeat(session_id)
        .await
        .expect("Failed to heartbeat");

    println!("✓ Heartbeat sent");

    // Verify it's no longer stale
    let stale_after = repo
        .find_stale(40)
        .await
        .expect("Failed to find stale");
    assert!(
        !stale_after.iter().any(|s| s.session_id == session_id),
        "Session should no longer be stale after heartbeat"
    );

    println!("✓ Session correctly no longer marked as stale after heartbeat");
}
