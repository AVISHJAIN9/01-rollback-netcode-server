// Shared test suite for SessionRepository implementations
use rollback_netcode_server::session::{InMemorySessionRepository, SessionRepository, Session, PlayerId, ReconnectToken};
use rollback_netcode_server::session::postgres_repo::PostgresSessionRepository;
use uuid::Uuid;

async fn run_crud_test<R: SessionRepository + 'static>(repo: &R) {
    let match_id = Uuid::new_v4();
    let room_code = format!("room_{}", Uuid::new_v4());
    let session = Session::new(match_id, room_code.clone(), 2)
        .expect("Failed to create Session");
    let session_id = session.session_id;

    // CREATE
    let created = repo.create(session.clone())
        .await
        .expect("Failed to create session");
    assert_eq!(created.session_id, session_id, "Session ID should match");

    // GET
    let result = repo.get(session_id)
        .await
        .expect("Failed to get session");
    assert!(result.is_some(), "Session should exist");
    if let Some(s) = result {
        assert_eq!(s.room_code, room_code, "Room code should match");
    }

    // DELETE
    repo.delete(session_id)
        .await
        .expect("Failed to delete session");

    let result = repo.get(session_id)
        .await
        .expect("Failed to get session after delete");
    assert!(result.is_none(), "Session should be deleted");
}

async fn run_room_code_test<R: SessionRepository + 'static>(repo: &R) {
    let match_id = Uuid::new_v4();
    let room_code = format!("room_{}", Uuid::new_v4());
    let session = Session::new(match_id, room_code.clone(), 2)
        .expect("Failed to create Session");

    repo.create(session)
        .await
        .expect("Failed to create session");

    // GET_BY_ROOM_CODE
    let result = repo.get_by_room_code(&room_code)
        .await
        .expect("Failed to get by room code");
    assert!(result.is_some(), "Session should exist by room code");
}

async fn run_heartbeat_test<R: SessionRepository + 'static>(repo: &R) {
    let match_id = Uuid::new_v4();
    let room_code = format!("room_{}", Uuid::new_v4());
    let session = Session::new(match_id, room_code, 2)
        .expect("Failed to create Session");
    let session_id = session.session_id;

    repo.create(session)
        .await
        .expect("Failed to create session");

    // HEARTBEAT should not fail
    repo.heartbeat(session_id)
        .await
        .expect("Failed to record heartbeat");

    // Session should still exist
    let result = repo.get(session_id)
        .await
        .expect("Failed to get session after heartbeat");
    assert!(result.is_some(), "Session should exist after heartbeat");
}

#[tokio::test]
async fn test_inmemory_crud() {
    let repo = InMemorySessionRepository::new();
    run_crud_test(&repo).await;
}

#[tokio::test]
async fn test_inmemory_room_code() {
    let repo = InMemorySessionRepository::new();
    run_room_code_test(&repo).await;
}

#[tokio::test]
async fn test_inmemory_heartbeat() {
    let repo = InMemorySessionRepository::new();
    run_heartbeat_test(&repo).await;
}

#[tokio::test]
async fn test_postgres_crud() {
    let repo = PostgresSessionRepository::new();
    // Placeholder: just ensure methods are callable
    let match_id = Uuid::new_v4();
    let room_code = format!("room_{}", Uuid::new_v4());
    let session = Session::new(match_id, room_code, 2)
        .expect("Failed to create Session");
    
    let _ = repo.create(session).await;
    let _ = repo.get(Uuid::new_v4()).await;
    let _ = repo.delete(Uuid::new_v4()).await;
}

#[tokio::test]
async fn test_postgres_room_code() {
    let repo = PostgresSessionRepository::new();
    // Placeholder: just ensure methods are callable
    let _ = repo.get_by_room_code("test_room").await;
}

#[tokio::test]
async fn test_postgres_heartbeat() {
    let repo = PostgresSessionRepository::new();
    // Placeholder: just ensure methods are callable
    let _ = repo.heartbeat(Uuid::new_v4()).await;
}
