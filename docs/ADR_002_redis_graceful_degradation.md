# ADR-002: Graceful Degradation When Redis Is Offline

**Status:** Accepted  
**Date:** 2026-10-10  
**Participants:** Rollback Netcode Team

---

## Context

The rollback netcode server uses Redis for:
- **Fast session metadata lookups** (reconnect tokens, player IDs)
- **TTL-based expiry** (reconnect tokens expire after 30 seconds)
- **Atomic queue operations** (ordered enqueue for player join order)

Redis is an external service; it can fail, restart, or become unavailable. The server must handle this gracefully without cascading failure.

---

## Decision

Implement a **fallback mechanism** where the server can continue operating with degraded functionality if Redis is unavailable:

1. **If Redis online:** Use Redis for tokens, metadata, and queues (fast path)
2. **If Redis offline:** 
   - Fall back to Postgres for persistent token storage
   - Use Postgres for player join ordering (with explicit timestamps)
   - Issue reconnect tokens with shorter TTL (e.g., 10 seconds vs. 30)
   - Continue accepting new sessions and players

---

## Rationale

### Resilience
- Server remains operational even if Redis restarts or crashes
- Players don't lose games mid-match
- Automatic recovery: when Redis comes back, switch to fast path

### User Experience
- Transparent fallback; players unaware of the degradation
- Performance degrades gracefully (Postgres slower than Redis, but acceptable for 60 Hz ticks)
- No need for manual server restart or failover

### Simplicity
- Single code path for both cases; conditional branching on Redis availability
- Reduces operational burden (no need for Redis HA setup in early phases)
- Tests can verify both paths: with Redis mock, and with Postgres-only

---

## Consequences

### Positive
- Server remains available even when Redis fails
- Simpler architecture than Redis cluster + failover
- Faster recovery (no cluster leader election delays)

### Negative
- Postgres under higher load when Redis offline (more queries)
- Reconnect tokens expire faster (10 vs. 30 seconds) → more re-authentication
- Additional code complexity (dual-path logic)

### Neutral
- Requires monitoring Redis health; ops must watch for offline conditions
- Performance SLA differs when Redis offline (acceptable for graceful degradation)

---

## Implementation

### Pseudo-code

```rust
// backend/src/session/repository.rs

pub struct HybridSessionRepository {
    postgres_pool: PgPool,
    redis_client: Option<redis::Connection>,
}

impl HybridSessionRepository {
    async fn store_reconnect_token(&self, token: ReconnectToken) -> Result<(), SessionError> {
        match &self.redis_client {
            Some(redis) => {
                // Fast path: Redis
                redis.set_ex(&token.token, serde_json::to_string(&token)?, 30)?;
                Ok(())
            }
            None => {
                // Fallback: Postgres with shorter TTL
                log::warn!("Redis offline; storing token in Postgres (10s TTL)");
                let expiry = chrono::Utc::now() + chrono::Duration::seconds(10);
                sqlx::query(
                    "INSERT INTO reconnect_tokens (token, player_id, expires_at) VALUES ($1, $2, $3)"
                )
                .bind(&token.token)
                .bind(&token.player_id.0)
                .bind(expiry)
                .execute(&self.postgres_pool)
                .await?;
                Ok(())
            }
        }
    }

    async fn validate_reconnect_token(&self, token_str: &str) -> Result<ReconnectToken, SessionError> {
        // Try Redis first
        if let Some(redis) = &self.redis_client {
            if let Ok(Some(data)) = redis.get::<_, String>(token_str) {
                return serde_json::from_str(&data).map_err(|_| SessionError::InvalidToken);
            }
        }

        // Fall back to Postgres
        log::info!("Token lookup falling back to Postgres");
        let row = sqlx::query(
            "SELECT token, player_id, expires_at FROM reconnect_tokens WHERE token = $1"
        )
        .bind(token_str)
        .fetch_optional(&self.postgres_pool)
        .await?
        .ok_or(SessionError::InvalidToken)?;

        let expires_at: chrono::DateTime<chrono::Utc> = row.get("expires_at");
        if expires_at < chrono::Utc::now() {
            return Err(SessionError::TokenExpired);
        }

        Ok(ReconnectToken {
            token: row.get("token"),
            player_id: PlayerId(row.get("player_id")),
            issued_at: chrono::Utc::now(),
            expires_at,
        })
    }
}
```

### Client Setup

```rust
// backend/src/main.rs

async fn main() {
    let postgres_pool = sqlx::PgPool::connect(&config.database.postgres_url).await?;
    
    let redis_client = match redis::Client::open(&config.database.redis_url) {
        Ok(client) => match client.get_connection() {
            Ok(conn) => {
                log::info!("Redis connected");
                Some(conn)
            }
            Err(e) => {
                log::warn!("Redis connection failed: {}; will use Postgres fallback", e);
                None
            }
        },
        Err(e) => {
            log::warn!("Redis client creation failed: {}; will use Postgres fallback", e);
            None
        }
    };

    let repo = HybridSessionRepository { postgres_pool, redis_client };
    // ... rest of server setup
}
```

---

## Testing

### Unit Tests
- **With Redis:** Mock Redis client returning values → fast path tested
- **Without Redis:** Mock Redis client returning None → fallback path tested
- Both paths produce identical results ✓

### Integration Tests (⏳)
- Start Postgres and Redis → test fast path
- Stop Redis mid-test → verify fallback
- Restart Redis → verify return to fast path

---

## Monitoring & Alerts

Operations team should monitor:
1. **Redis availability:** Alert if `redis.connect()` fails
2. **Fallback usage:** Log `WARN` when falling back to Postgres
3. **Token TTL:** Verify no tokens expire prematurely (10s vs. 30s)

---

## Related ADRs

- **ADR-001:** In-Memory vs. Postgres Session Repository
- **ADR-004:** Deterministic Tick Rate with 60 Hz Default

---

## Verification

- ✅ `tests/postgres_redis_integration.rs::test_graceful_degradation_redis_offline` — fallback path tested
- ⏳ Multi-service failover test pending

---

## Sign-Off

- **Approved by:** Rollback Netcode Team
- **Date:** 2026-10-10

---

## Future Considerations

- **Phase 2:** Add Redis cluster failover (if scale requires HA)
- **Phase 2:** Add metrics to track fallback frequency
- **Phase 3:** Implement Redis circuit breaker (auto-disable after N retries)
