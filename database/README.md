# Database & Storage Layer: Rollback Netcode Server

## Technology Stack
- **PostgreSQL 16**: Relational storage for match sessions, monthly partitioned input logs (for deterministic match replay), and desync incident forensics.
- **Redis 7.2**: In-memory sorted sets for low-latency matchmaking pools and ephemeral room state.

## Migrations
Execute migration scripts via:
```bash
psql -h localhost -U postgres -d rollback_db -f schema.sql
```
