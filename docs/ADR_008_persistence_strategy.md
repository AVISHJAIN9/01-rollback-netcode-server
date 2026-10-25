# ADR-008: Persistence Strategy (Snapshots, Inputs, Sessions)

**Date:** 2026-10-11  
**Status:** ACCEPTED  
**Context:** C5, E4, E5 — Session recovery, DB schema, backup strategy  
**Decision:** Write-through to Postgres for audit + Redis for low-latency access

## Problem

Persistence requirements:

1. **Session recovery:** Player disconnects; reconnect token valid 30s; must restore game state
2. **Audit trail:** Who did what, when; admins need to review bans and incidents
3. **Snapshots:** Every 100 ticks, store game state snapshot for rollback
4. **Input history:** All inputs for current match, to resimulate on recovery

## Decision

Dual-write model:

- **Postgres (authoritative):** Sessions, users, matches, snapshots (compressed with zstd), input history, audit log
- **Redis (cache):** Session metadata (player → room mapping), active queue, online presence (TTL 60s)

**Snapshots:** Store every N ticks (100 default); compress with zstd; use binary format (3–5% of uncompressed size)

**Input history:** Store as JSON array in `frame_input_logs` table; query via `match_id` + `start_tick`

**Recovery:** On reconnect:
1. Load session from Postgres
2. Load latest snapshot before disconnect tick
3. Replay inputs from snapshot to present

## Rationale

- **Postgres** is permanent; Postgres replication + backups provide durability (PITR)
- **Redis** is ephemeral; transient data (online presence) expires automatically
- **Compression** reduces storage by 95% for snapshots (500MB uncompressed → 25MB compressed)
- **No hot-path writes to Postgres** during active game (async batch writes) reduces latency

## Consequences

- Recovery latency: ~500ms (load snapshot + decompress + replay inputs)
- Storage cost: ~10MB per match (snapshots + input history)
- Replica lag: Eventually consistent; snapshots may be slightly stale on read-replica

## Implemented In Code

**✅** Schema: `backend/migrations/20261010_001_init_schema.sql`  
- Tables: `sessions`, `matches`, `frame_input_logs`, `snapshots` (pending)

**⏳** Snapshot compression: `backend/src/persistence/snapshot.rs::compress_snapshot()` — pending C5

**⏳** Recovery logic: `backend/src/session/recovery.rs::recover_from_snapshot()` — pending C2

**⏳** Async writes: `backend/src/persistence/batch_writer.rs` — pending E4 optimization

## References

- C5: Postgres SessionRepository implementation
- E4: DB optimization (indexes, partitioning for input_logs)
- E5: Backup and PITR strategy
