# ADR-005: Separate Connections for Multi-Instance Reads

**Status:** Accepted  
**Date:** 2026-10-10  
**Participants:** Rollback Netcode Team

---

## Context

The rollback netcode server is designed for multi-instance deployment:
- Multiple server instances on different machines (us-east-1, us-west-2, eu-west-1)
- Each instance needs to read/write session state to shared Postgres database
- Data consistency must be maintained across instances

Question: Should session reads:
- **Option A:** Always use the same Postgres connection (affinity)
- **Option B:** Use connection pool (each query may hit different physical connection)

---

## Decision

Use **connection pool** (Option B): each query independently acquires a connection from the pool. This may hit different physical connections or even different Postgres replicas.

---

## Rationale

### Horizontal Scalability
- Load balancing: queries distributed across multiple Postgres backends
- One server instance can exhaust connection limits; pooling prevents starvation
- Enables adding Postgres replicas without code changes

### High Availability
- If one Postgres replica fails, queries route to other replicas
- Connection pool handles failover transparently
- No single point of failure per instance

### Multi-Region Deployment
- Instance in us-east-1 reads from us-east-1 Postgres replica (low latency)
- Data replication (WAL, logical replication) keeps replicas in sync
- Each region has its own pool → resilient to regional failures

### Operational Simplicity
- Standard Postgres replication (leader-follower, multi-master) handles data consistency
- No custom data sync logic needed
- Pool management is delegated to Postgres driver (sqlx)

---

## Consequences

### Positive
- Scales naturally: add more Postgres replicas → load distributed automatically
- Each instance is independent; failures don't cascade
- No affinity stickiness → better resource utilization

### Negative
- Must use Postgres replication for consistency (not just single-server)
- Potential for stale reads if replicas lag (milliseconds to seconds)
- Requires monitoring replica lag and alerting

### Neutral
- Requires proper Postgres setup (primary + standby replicas, WAL archiving)
- Operations team must manage replication lag

---

## Implementation

### Connection Pool Setup

```rust
// backend/src/main.rs

use sqlx::postgres::PgPool;

pub async fn create_postgres_pool(database_url: &str, pool_size: u32) -> Result<PgPool, Error> {
    let pool = PgPool::connect_with(
        PgConnectOptions::new()
            .from_str(database_url)?
            .ssl_mode(sqlx::postgres::PgSslMode::Prefer)
    )
    .max_connections(pool_size)
    .build()
    .await?;

    Ok(pool)
}

#[tokio::main]
async fn main() {
    let config = load_config("config.toml").await?;
    
    // Create pool: each query may hit different connection/replica
    let pool = create_postgres_pool(&config.database.postgres_url, 32).await?;
    
    // Use pool everywhere
    let session = sqlx::query_as::<_, Session>(
        "SELECT * FROM match_sessions WHERE session_id = $1"
    )
    .bind(session_id)
    .fetch_one(&pool)  // <-- May be from different replica
    .await?;
}
```

### Replica Configuration (Example)

```yaml
# Postgres Primary (writes)
primary:
  host: postgres-primary.internal
  port: 5432
  role: primary

# Postgres Replica 1 (read-only)
replica1:
  host: postgres-replica-1.internal
  port: 5432
  role: standby

# Replica 2 (read-only)
replica2:
  host: postgres-replica-2.internal
  port: 5432
  role: standby

# Connection string for read replicas
# Clients connect to any replica; primary handles writes
```

### Test: Multiple Connections See Same Data

```rust
#[tokio::test]
async fn test_multiple_connections_same_data() {
    let pool = create_postgres_pool("postgres://...", 4).await?;
    
    // Write via one connection in pool
    let user_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users (username, email) VALUES ($1, $2) RETURNING user_id"
    )
    .bind("test_user")
    .bind("test@example.com")
    .fetch_one(&pool)
    .await?;
    
    // Read via potentially different connection in pool
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;
    
    assert_eq!(user.username, "test_user");
    println!("✓ Multiple connections see same data (replication working)");
}
```

---

## Consistency Guarantees

### Strong Consistency (Within Same Connection)
```rust
// Within single connection: guaranteed to see own writes
let user_id = insert_user(&conn).await?;
let user = fetch_user(&conn, user_id).await?;  // <-- Sees the insert
```

### Eventual Consistency (Across Connections)
```rust
// Across connections in pool: replicas may lag
let user_id = insert_user(&pool).await?;      // Write to primary
let user = fetch_user(&pool, user_id).await?; // Read from replica (may be stale)
// If replica is lagging, may not see the user immediately
```

**Mitigation:** Use primary for reads after writes:
```rust
let user_id = insert_user(&primary_conn).await?;
let user = fetch_user(&primary_conn, user_id).await?;  // <-- Read from same connection
```

---

## Alternatives Considered

### 1. Affinity: Always Same Connection per Request
- **Approach:** Round-robin pool, but pin all queries for a session to same connection
- **Rejected:** Still doesn't help with failover; adds complexity

### 2. Read-Write Splitting
- **Approach:** Writes to primary, reads from replicas
- **Partially adopted:** Handled by Postgres replication (primary write, replicas read)
- **Note:** Replication lag still a concern

### 3. Single Server (No Replication)
- **Rejected:** Not highly available; single failure = downtime

---

## Monitoring & Alerts

Operations team should monitor:

1. **Replica Lag:** Alert if any replica is > 10 seconds behind primary
2. **Connection Pool:** Alert if pool exhaustion (all connections in use)
3. **Read Errors:** Catch and log instances where read fails after write

### Queries

```sql
-- Monitor replica lag (on primary)
SELECT slot_name, restart_lsn, confirmed_flush_lsn 
FROM pg_replication_slots;

-- Check replication status
SELECT * FROM pg_stat_replication;

-- Monitor connections
SELECT sum(numbackends) FROM pg_stat_database;
```

---

## Testing Strategy

### Unit Tests (In-Memory)
- Use `InMemorySessionRepository` (no pool needed)
- Fast, no dependencies

### Integration Tests (With Postgres)
- Single Postgres instance (no replicas)
- Verify pool behavior
- ⏳ Multi-replica tests pending

### Production Testing (⏳)
- Deploy to multi-region environment
- Inject replica lag and verify resilience
- Monitor error rates

---

## Related ADRs

- **ADR-001:** In-Memory vs. Postgres Session Repository (trait allows both patterns)
- **ADR-002:** Graceful Degradation When Redis Offline

---

## Future Considerations

- **Phase 2:** Implement read-write splitting explicitly (read from replicas, write to primary)
- **Phase 2:** Add circuit breaker for replica failures
- **Phase 3:** Implement geo-distributed replicas with conflict resolution

---

## Sign-Off

- **Approved by:** Rollback Netcode Team
- **Date:** 2026-10-10

---

## Appendix: Postgres Replication Setup

### Primary + Standby Setup

```bash
# On primary (Postgres 14)
psql -U postgres -c "CREATE ROLE replication WITH REPLICATION PASSWORD 'password' LOGIN;"

# In postgresql.conf on primary
wal_level = replica
max_wal_senders = 3
max_replication_slots = 3

# On standby (Postgres 14)
pg_basebackup -h primary.internal -U replication -D /var/lib/postgresql/14/main -Fp -Xs -P -R

# Verify replication
psql -U postgres -c "SELECT * FROM pg_stat_replication;" # on primary
```

See [Postgres Official Replication Docs](https://www.postgresql.org/docs/14/streaming-replication.html) for full setup.
