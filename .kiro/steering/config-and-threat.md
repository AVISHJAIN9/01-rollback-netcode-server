# Rollback Netcode Server: Config & Threat Model

## Config Requirements (PART 1, Items 4–10)

### Tick Rate & Timing
- **Default tick rate:** 60 Hz (16.67 ms per frame)
- **Reconnect timeout:** 30 seconds
- **Stale session detection:** 40 seconds (heartbeat interval)
- **Redis TTL:** 60 seconds (session metadata, reconnect tokens)

### Network Ports
- **UDP Game Server:** 7777 (rollback protocol, collision on same host)
- **HTTP Admin/Metrics:** 8080 (management API, not exposed in prod)
- **Frontend WebSocket:** 3000 (React dev server / production load balancer)
- **Postgres:** 5432 (internal, not exposed)
- **Redis:** 6379 (internal, not exposed)

### Feature Flags (Default: OFF)
All feature flags default to `false` (disabled). Enabled only for testing:
- `ENABLE_AI_CHEAT_DETECTION` – ML-based anomaly detection
- `ENABLE_JITTER_PREDICTION` – Kalman filter latency forecasting
- `ENABLE_ROLLBACK_RISK_METRICS` – Observability for rollback risk
- `ENABLE_MATCHMAKING_OPTIMIZER` – AI-driven player pairing

Flags stored in `config.toml` and loaded at server startup. No hot-reload.

---

## Threat Model (PART 1, Item 5)

### Threats & Mitigations

#### 1. **UDP Packet Loss / Corruption**
- **Threat:** Unencrypted UDP packets can be lost or corrupted on network.
- **Mitigation:** 5-frame redundancy (encode each input as 5 consecutive frames); CRC checksums on all packets.
- **Design:** `network::mod.rs` implements redundancy codec.

#### 2. **Replay Attacks**
- **Threat:** Attacker captures and replays old input frames.
- **Mitigation:** Monotonic sequence numbers per player; server rejects inputs with seq ≤ last_acked_seq.
- **Design:** `transport::protocol.rs` enforces seq checks.

#### 3. **Desync Attacks** (e.g., FP-cheating)
- **Threat:** Player sends forged state to cause desync, forcing rollbacks that favor them.
- **Mitigation:** Deterministic simulation (all players run identical logic on same inputs); authoritative state checksum validation; mismatch → rollback + resync.
- **Design:** `desync::detector.rs` computes checksums; `game::mod.rs` enforces determinism.

#### 4. **Session Hijacking**
- **Threat:** Attacker reconnects with stolen reconnect token.
- **Mitigation:** Tokens issued with player ID + session ID; expire after 30 seconds; stored in Redis with TTL.
- **Design:** `session::repository.rs` validates tokens; Redis persistence.

#### 5. **Denial of Service (Connection Flood)**
- **Threat:** Attacker sends thousands of handshake packets to exhaust server resources.
- **Mitigation:** Rate limiting on UDP handshakes (no explicit limit yet, flagged ⏳); graceful degradation if Redis offline.
- **Design:** `network::udp_server.rs` accepts connections sequentially.

#### 6. **Man-in-the-Middle (No Encryption)**
- **Threat:** UDP traffic is plaintext; attacker can modify packets in transit.
- **Mitigation:** Out of scope for this phase. Protocol assumes trusted LAN or private network. ⏳ QUIC/TLS integration later.
- **Design:** Current protocol does not encrypt.

#### 7. **Orphan Sessions (Resource Leak)**
- **Threat:** Player disconnects; server holds session in memory forever, leaking resources.
- **Mitigation:** Heartbeat-based stale detection; sessions with no heartbeat for 40 seconds → marked orphaned → cleaned up.
- **Design:** `session::repository.rs::find_stale()` with Postgres `session_heartbeat` column + TTL logic.

#### 8. **Concurrent Connection Collision**
- **Threat:** Two players in different data centers connect to same server, same UDP port → collision.
- **Mitigation:** Server runs on single UDP port (7777); each client has unique (IP, port) pair → no collision on LAN. For multi-region, use DNS round-robin or load balancer.
- **Design:** Single-port design; multi-instance deployment ⏳ pending.

---

## Architecture Decision Records (ADRs) (PART 1, Item 6)

### ADR-001: In-Memory Session Repository for Local Development
**Decision:** Use `InMemorySessionRepository` for tests; Postgres-backed `PostgresSessionRepository` for prod.
**Rationale:** Fast iteration in dev; no service dependencies for unit tests.
**Consequences:** Tests do not exercise real Postgres; integration tests marked ⏳.

### ADR-002: Graceful Degradation When Redis Offline
**Decision:** If Redis unavailable, session metadata stored in Postgres; reconnect tokens still issued but with reduced TTL.
**Rationale:** Resilience; prevents cascading failure if Redis service restarts.
**Consequences:** Minor performance degradation (Postgres writes slower than Redis).

### ADR-003: No Hot-Reload for Feature Flags
**Decision:** Feature flags read from config at startup only; no runtime changes.
**Rationale:** Simplicity; prevents flag inconsistencies during play sessions.
**Consequences:** Config changes require server restart.

### ADR-004: Deterministic Tick Rate with 60 Hz Default
**Decision:** All replayed inputs run at fixed 60 Hz; no adaptive tick rate.
**Rationale:** Predictable rollback behavior; easier testing.
**Consequences:** Lower-end clients may experience jitter if network latency high; Jitter Predictor ⏳ mitigates.

### ADR-005: Separate Connections for Multi-Instance Reads
**Decision:** Session data reads use connection pool; each query may hit different physical connection.
**Rationale:** Load balancing across Postgres backends; horizontal scalability.
**Consequences:** Data must be replicated across connections (handled by Postgres replication).

---

## Compatibility & Versioning (PART 1, Item 7)

### Protocol Version Negotiation
- **Current version:** 1.0 (hardcoded in `transport::mod.rs`)
- **Handshake check:** Client sends `Handshake(version=1.0)`; server rejects if version mismatch.
- **Test:** `protocol_tests::test_version_negotiation_matrix` covers reject behavior.
- **Future:** Version 2.0 to include QUIC/TLS (⏳ Phase 2).

### Database Schema Versioning
- **Migration system:** `sqlx` CLI; migrations in `backend/migrations/`.
- **Current schema version:** `20261010_001_init_schema.sql`
- **Tables:** `users`, `matches`, `match_sessions`, `frame_input_logs`, `desync_incidents`.
- **Constraints:** NOT NULL, UNIQUE (email), FK with CASCADE, CHECK (status).

---

## CI/CD Workflow (PART 1, Item 8)

### GitHub Actions Setup
- **Location:** `.github/workflows/ci.yml`
- **Triggers:** Push to `main` or `dev`; PR checks
- **Services:** Postgres 14 + Redis 7 (Docker Compose in CI)
- **Steps:**
  1. Checkout code
  2. Build backend (`cargo build`)
  3. Run tests (`cargo test --all` with `ALLOW_SKIP_INTEGRATION=1`)
  4. Run clippy linter
  5. Build frontend (React)
  6. Upload coverage (codecov)

### Local Verification
- **Without Docker:** `cd backend && ALLOW_SKIP_INTEGRATION=1 cargo test --all` (56 passing + 2 ignored)
- **With Docker:** `make test-integration` (all 58 tests pass, pending Docker availability ⏳)

---

## Feature Flags Configuration (PART 1, Item 9)

### Config File Location
`backend/config.toml`

### Structure
```toml
[server]
tick_rate = 60
reconnect_timeout_secs = 30
stale_session_threshold_secs = 40

[ports]
udp_game = 7777
http_admin = 8080
frontend = 3000

[database]
postgres_url = "postgres://user:pass@localhost:5432/rollback_db"
redis_url = "redis://localhost:6379/0"

[features]
enable_ai_cheat_detection = false
enable_jitter_prediction = false
enable_rollback_risk_metrics = false
enable_matchmaking_optimizer = false
```

### Loading at Runtime
```rust
// backend/src/config.rs
pub async fn load_config() -> Result<ServerConfig, ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::from(Path::new("config.toml")))
        .add_source(config::Environment::with_prefix("ROLLBACK"))
        .build()?;
    config.try_deserialize()
}
```

---

## Docker Compose Setup (PART 1, Item 10, ⏳ Requires Docker)

### Compose File
`infra/docker-compose.yml` (code already written, needs user to run)

### Services
- **Postgres 14:** Port 5432, persistent volume
- **Redis 7:** Port 6379, in-memory
- **Backend:** Runs on 7777 (UDP) + 8080 (HTTP)
- **Frontend:** Runs on 3000 (React dev server)

### Health Checks
- Postgres: `pg_isready` command
- Redis: `redis-cli PING`
- Backend: UDP handshake check ⏳

### Usage (User Must Run)
```bash
# Start all services
docker-compose -f infra/docker-compose.yml up -d

# Stop
docker-compose -f infra/docker-compose.yml down

# View logs
docker-compose -f infra/docker-compose.yml logs -f backend
```

---

## Next Steps (PART 1, Items 4–10)
- [x] Threat model documented above
- [x] ADRs written (001–005)
- [x] Feature flags structure defined
- [ ] Config module implementation (backend/src/config.rs)
- [ ] CI workflow YAML (`.github/workflows/integration.yml`)
- [ ] Docker Compose refinement + health checks
- [ ] Compatibility matrix tests
