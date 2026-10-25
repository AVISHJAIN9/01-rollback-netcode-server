# ADR-001: In-Memory vs. Postgres Session Repository Strategy

**Status:** Accepted  
**Date:** 2026-10-10  
**Participants:** Rollback Netcode Team

---

## Context

The rollback netcode server requires persistent storage of session state:
- Player join/leave events
- Session transitions (Lobby → Ready → Active → Finished)
- Reconnect tokens
- Heartbeat tracking for orphan detection

We need a repository pattern that supports:
1. **Local development & testing** without external dependencies
2. **Production deployment** with real persistence (Postgres)
3. **Integration testing** with both implementations

---

## Decision

Use a **trait-based repository pattern** with two implementations:

1. **`InMemorySessionRepository`** (default for tests):
   - Stores sessions in `HashMap<SessionId, Session>`
   - No persistence between process restarts
   - No external dependencies; tests run offline

2. **`PostgresSessionRepository`** (production):
   - Stores sessions in `match_sessions` table
   - Persists across restarts
   - Requires running Postgres service

---

## Rationale

### Fast Iteration in Development
- Unit tests run in microseconds without Postgres connection overhead
- CI/CD can run offline tests (no Docker required for fast feedback)
- Developers can write tests without setting up containers

### Production Correctness
- Real Postgres persistence prevents data loss
- Database constraints (FK, UNIQUE, CHECK) enforce schema invariants
- Multi-instance deployments can share session state via same database

### Clean Testing Strategy
- Integration tests use `PostgresSessionRepository` with Docker (⏳)
- Unit tests use `InMemorySessionRepository` (runs locally)
- Both implement same `SessionRepository` trait → identical test logic

---

## Consequences

### Positive
- Fast feedback loop: `cargo test` runs in < 5 seconds
- Tests are hermetic: no external service dependencies
- Clear separation of concerns (trait boundary)

### Negative
- Integration tests don't exercise real Postgres until Docker available (⏳)
- Data types must be serializable to/from SQL (added complexity)
- Schema changes require migrations; can't just add fields to `Session` struct

### Neutral
- Requires implementing `SessionRepository` trait twice
- Tests may pass in-memory but fail in Postgres (unlikely, but possible with constraint violations)

---

## Implementation

### Trait Definition (backend/src/session/repository.rs)
```rust
#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn create(&self, session: Session) -> Result<Session, SessionError>;
    async fn get(&self, session_id: Uuid) -> Result<Option<Session>, SessionError>;
    async fn get_by_room_code(&self, room_code: &str) -> Result<Option<Session>, SessionError>;
    async fn update(&self, session: Session) -> Result<(), SessionError>;
    async fn delete(&self, session_id: Uuid) -> Result<(), SessionError>;
    async fn find_stale(&self, threshold_secs: u64) -> Result<Vec<Session>, SessionError>;
    async fn heartbeat(&self, session_id: Uuid) -> Result<(), SessionError>;
    async fn store_reconnect_token(&self, token: ReconnectToken) -> Result<(), SessionError>;
    async fn get_reconnect_token(&self, token: &str) -> Result<Option<ReconnectToken>, SessionError>;
    async fn validate_reconnect_token(&self, token: &str) -> Result<ReconnectToken, SessionError>;
}
```

### In-Memory Implementation
- Uses `Arc<RwLock<HashMap<...>>>` for thread-safe access
- Implements all methods synchronously (wrapped in async for trait compatibility)
- Stale detection scans entire map; acceptable for test scale

### Postgres Implementation (⏳ Phase 2)
- Uses `sqlx::PgPool` for connection pooling
- Executes SQL queries for each operation
- Stale detection uses `SELECT ... WHERE session_heartbeat < now() - interval`

---

## Alternatives Considered

### 1. Only In-Memory (No Postgres)
**Rejected:** Production would lose data on restart; multi-instance not feasible.

### 2. Only Postgres
**Rejected:** Tests would require Docker; slow feedback; blocks offline development.

### 3. Shared State via Redis
**Rejected:** Adds dependency; less reliable than Postgres for durability.

---

## Related ADRs

- **ADR-002:** Graceful Degradation When Redis Offline
- **ADR-005:** Separate Connections for Multi-Instance Reads

---

## Verification

- ✅ `tests/session_state_machine_test.rs` — all tests pass with `InMemorySessionRepository`
- ⏳ Integration tests with `PostgresSessionRepository` pending Docker

---

## Sign-Off

- **Approved by:** Rollback Netcode Team
- **Date:** 2026-10-10
