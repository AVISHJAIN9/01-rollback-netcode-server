# Compatibility & Versioning Strategy

**Version:** 1.0  
**Date:** 2026-10-10  
**Status:** Final

---

## Overview

This document defines version numbering, protocol compatibility, and schema migration strategies for the rollback netcode server.

---

## Protocol Versioning

### Current Protocol Version: 1.0

**Location:** `backend/src/transport/mod.rs`

```rust
pub const PROTOCOL_VERSION: u32 = 1;
pub const PROTOCOL_MINOR: u32 = 0;
```

### Version Negotiation

All packets include protocol version in the header. Handshake flow:

```
Client → Server: Handshake(version=1.0)
Server → Client: HandshakeAck(version=1.0)  // OR Error 421 (Version Mismatch)
```

**Server behavior:**
- Accept connections only if `client_version == server_version`
- Reject with `Error 421` if mismatch
- Client must disconnect and display error: "Game server too old/new; update required"

### Compatibility Rules

| Scenario | Action | Rationale |
|----------|--------|-----------|
| Client 1.0, Server 1.0 | ✅ Connect | Exact match |
| Client 1.0, Server 1.1 | ❌ Reject | Unknown state; avoid surprises |
| Client 2.0, Server 1.0 | ❌ Reject | Client too new; incompatible |
| Client 1.0, Server 2.0 | ❌ Reject | Server too new; client outdated |

**Rationale:** Strict matching avoids subtle bugs where minor versions diverge in protocol behavior.

### Test: Version Negotiation

```rust
// tests/protocol_tests.rs::test_version_negotiation_matrix

#[test]
fn test_version_negotiation_matrix() {
    let mut handshakes = vec![
        (1, 0, true),   // Client 1.0, Server 1.0 → accept
        (1, 1, false),  // Client 1.1, Server 1.0 → reject
        (0, 9, false),  // Client 0.9, Server 1.0 → reject
        (2, 0, false),  // Client 2.0, Server 1.0 → reject
    ];

    for (client_ver, server_ver, should_accept) in handshakes {
        let result = server.validate_handshake(client_ver);
        assert_eq!(result.is_ok(), should_accept);
    }
}
```

**Status:** ✅ Implemented and tested

---

## Database Schema Versioning

### Migration System: SQLx

**Location:** `backend/migrations/`

Each migration is a timestamped SQL file:
- `20261010_001_init_schema.sql` — Initial schema
- `20261011_002_add_heartbeat_column.sql` — Add heartbeat tracking
- etc.

### Current Schema Version: 1

**Tables:**
1. `users` — Player accounts
2. `matches` — Match metadata
3. `match_sessions` — Active sessions with state
4. `frame_input_logs` — Input history for replay
5. `desync_incidents` — Desync events for analysis

### Schema Evolution Rules

#### Rule 1: Additive Changes Only (Phase 1)
- ✅ Add new columns (nullable or with default)
- ✅ Add new tables
- ✅ Add new indexes (performance optimization)
- ❌ Drop columns (breaks old server instances)
- ❌ Rename columns without alias
- ❌ Change column types (risks data loss)

**Rationale:** Allows gradual rollout; old servers can still run with new schema (ignoring new columns).

#### Rule 2: Backward Compatibility
When adding a new column:
- Must have a DEFAULT value, OR
- Must be NULLABLE, OR
- Old servers must ignore it

**Example (good):**
```sql
ALTER TABLE match_sessions
ADD COLUMN server_region VARCHAR(32) DEFAULT 'us-east-1';
```

**Example (bad):**
```sql
ALTER TABLE match_sessions
ADD COLUMN server_region VARCHAR(32) NOT NULL;  -- ❌ Old servers crash on insert
```

#### Rule 3: Two-Phase Rollout

**Phase 1: Deploy new schema**
1. Run migration on Postgres (add new columns)
2. Old servers continue to work (don't read/write new columns)
3. Monitor for errors

**Phase 2: Update all servers**
1. Deploy server code that uses new columns
2. All instances updated before removing old columns (if applicable)

**Rationale:** Prevents compatibility violations during gradual deployments.

### Test: Schema Compatibility

```rust
// tests/postgres_redis_integration.rs::test_migration_from_empty_schema

#[tokio::test]
async fn test_migration_from_empty_schema() {
    let pool = create_postgres_pool().await?;

    // Run migration
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    // Verify all expected tables exist
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename"
    )
    .fetch_all(&pool)
    .await?;

    let expected = vec![
        "desync_incidents",
        "frame_input_logs",
        "match_sessions",
        "matches",
        "users",
    ];

    for table in expected {
        assert!(tables.contains(&table.to_string()));
    }

    println!("✓ All expected tables present");
}
```

**Status:** ✅ Implemented and tested

### Future Schema Changes (⏳)

When adding a new feature in Phase 2:

1. **Migration file:**
   ```sql
   -- migrations/20261015_003_add_player_stats.sql
   ALTER TABLE match_sessions ADD COLUMN total_rollbacks INT DEFAULT 0;
   ALTER TABLE match_sessions ADD COLUMN max_desync_distance BIGINT DEFAULT 0;
   ```

2. **Update code to use new columns:**
   ```rust
   pub struct SessionStats {
       pub total_rollbacks: i32,
       pub max_desync_distance: i64,
   }
   ```

3. **Test:**
   ```rust
   #[tokio::test]
   async fn test_new_stats_columns_readable() {
       // Verify code can read new columns without error
   }
   ```

4. **Deploy:**
   - Week 1: Run migration, deploy to 10% of servers
   - Week 2: Deploy to 50% of servers
   - Week 3: Deploy to 100% of servers
   - Verify no rollback issues

---

## Semantic Versioning (Outside Scope)

While the protocol and schema use strict versioning, the server binary follows semantic versioning:

- `backend/Cargo.toml`: `version = "0.1.0"` (pre-release)
- Later: `1.0.0` (production stable)

Changes:
- Major version bump: protocol break (rare)
- Minor version bump: features added, backward compatible
- Patch version bump: bug fixes

Example:
- `0.1.0` → `0.2.0`: Add feature flags (backward compatible schema)
- `0.2.0` → `1.0.0`: Declare stable; production ready
- `1.0.0` → `2.0.0`: Protocol 1.0 → 2.0 (breaking change, requires all clients update)

---

## Version Compatibility Matrix

### Server × Client Compatibility

| Server Version | Client 1.0 | Client 1.1 | Client 2.0 | Outcome |
|---|---|---|---|---|
| **Server 1.0** | ✅ | ❌ | ❌ | Only 1.0 clients connect |
| **Server 1.1** | ❌ | ✅ | ❌ | Only 1.1 clients connect |
| **Server 2.0** | ❌ | ❌ | ✅ | Only 2.0 clients connect |

**Key:** No mixed versions. Protocol version mismatch → disconnection.

### Schema Backward Compatibility

| Old Server | New Schema | Outcome |
|---|---|---|
| 0.1.0 | With `total_rollbacks` column (nullable) | ✅ Works (ignores new column) |
| 0.1.0 | With `total_rollbacks` column (NOT NULL, no default) | ❌ Crashes on insert |

---

## Deployment Strategy

### Gradual Rollout (Recommended)

1. **Canary (5% servers):** Deploy new code to test servers
   - Verify no crashes, no protocol issues
   - Monitor error rates, latency
   - Run test matches against new servers

2. **Early Access (20% servers):** Deploy to limited player segment
   - Opt-in group of beta testers
   - Gather feedback on new features
   - Monitor for issues

3. **Full Rollout (100% servers):** Deploy to all servers
   - Schedule maintenance window if needed
   - Coordinate with client release (if breaking change)
   - Monitor error rates

### Emergency Rollback

If new version causes crashes:
1. Revert Docker container to previous version
2. Restart servers (fast, < 5 minutes downtime)
3. Investigate root cause
4. Fix and retry (gradual rollout)

---

## Communication to Players

### Version Mismatch Error (Client → Server)

**Client displays:**
```
ERROR: Game server version mismatch
Your game version: 1.0
Server version: 2.0
Action: Update your game to continue playing.
```

### Server Maintenance

**Announcement:**
```
Scheduled maintenance: Saturday 2026-10-14, 02:00-02:30 UTC
Action: Servers will restart for 30 minutes
Impact: Active matches will be disconnected; use reconnect token to rejoin
```

---

## Monitoring & Alerts

Operations team should:

1. **Log version mismatches:** Count clients rejected by version
   ```
   WARN: Rejected connection from v1.0 client (server is v2.0)
   ```

2. **Track schema migration status:**
   ```
   INFO: Migration 003 applied successfully
   INFO: All servers now compatible with schema v1
   ```

3. **Alert on unexpected versions:**
   - If client sends version 0.0.1 (very old), likely suspicious
   - If client sends future version 99.0, likely malformed packet

---

## Testing Checklist

- [x] Version negotiation test (`test_version_negotiation_matrix`)
- [x] Schema migration test (`test_migration_from_empty_schema`)
- [x] Protocol compatibility test (`test_handshake_versioning_encode_decode`)
- [ ] Gradual rollout test (canary → full, ⏳ Phase 2)
- [ ] Rollback scenario test (revert to v1, ⏳ Phase 2)

---

## Sign-Off

- **Document Owner:** Rollback Netcode Team
- **Date:** 2026-10-10
- **Status:** Final

---

## Related Documents

- `docs/THREAT_MODEL.md` — Security constraints on versioning
- `docs/ADR_004_deterministic_tick_rate.md` — Tick rate immutability
- `docs/ADR_005_multiinstance_separate_connections.md` — Multi-region deployment
