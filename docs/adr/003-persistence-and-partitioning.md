# ADR 003: Relational Persistence, Telemetry Partitioning & Redis Caching

## Status
Accepted

## Context
The game server produces high-frequency input streams (60 events/sec per player) and requires low-latency matchmaking pools (< 5ms queue operations) alongside durable match records and immutable security audit logs.

## Decision
1. **PostgreSQL 16**: Relational storage for users, matches, sessions, and append-only audit logs. High-volume `frame_input_logs` and telemetry events are partitioned by month.
2. **Redis 7.2**: In-memory sorted sets for low-latency MMR matchmaking queues, ephemeral room leases (TTL 5 minutes), presence tracking, and distributed rate-limiting counters.
3. **Audit Log Immutability**: Database constraints enforce append-only rules for non-superuser database roles.

## Consequences
- Hot-path matchmaking queries resolve in $O(\log N)$ in memory.
- Telemetry table bloat is managed via partition pruning.
