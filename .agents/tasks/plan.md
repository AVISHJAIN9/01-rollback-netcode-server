# Implementation Plan: STEP 1.6 + STEP 2 (Final Deliverable)

**Status**: Ready for implementation  
**Scope**: 
1. STEP 1.6 — Debt ledger documentation with corrected gate table
2. STEP 2 — Full Postgres + Redis persistence with integration tests

**Test count reconciliation**: 37 passing (per backend/tests count) + 2 ignored = 39 total before STEP 2  
After STEP 2: 37 + 11 new integration tests = 48 total tests passing

---

## STEP 1.6 — Debt Ledger & Gate Correction (Docs Only)

### 1. Create docs/OPEN_ITEMS.md with explicit debt tracking
   - Six numbered items, each marked ❌ OPEN
   - Include: multiprocess UDP test, drop simulator rename, G1 audit, G6 evidence, test-count clarification, gate status
   - Structure: What's needed | Current state | Blocking reason | Defer to
   - Corrected gate table: G1 ⚠️, G2 ⚠️, G3–G8 ❌, G6 ⚠️
   - Test reconciliation table showing 39 total (37 passing + 2 ignored)
   - Files: `docs/OPEN_ITEMS.md` (750–900 words)
   - Verify: File exists, contains all 6 items, gate table is readable, no typos

### 2. Commit docs/OPEN_ITEMS.md with clear message
   - Commit message: "docs(STEP 1.6): debt ledger with corrected gates and test-count reconciliation"
   - Files: `docs/OPEN_ITEMS.md`
   - Verify: `git log --oneline -1` shows the message; file is staged and committed

---

## STEP 2 — Postgres + Redis Persistence Layer

### 3. Verify Cargo.toml dependencies
   - Check sqlx has features: `runtime-tokio-native-tls`, `postgres`, `uuid`, **`migrate`**
   - Check redis crate is present with async support (already "0.23")
   - Add if missing: `sqlx = { version = "0.7", features = ["runtime-tokio-native-tls", "postgres", "uuid", "migrate"] }`
   - Also verify tokio and chrono are present (required by integration tests)
   - Files: `backend/Cargo.toml`
   - Verify: `cargo check` compiles successfully

### 4. Create backend/migrations/ directory and SQL migration files
   - Create `backend/migrations/20261010_001_init_schema.sql` — already exists, verify it has:
     - `users` table: user_id (UUID PK), username, email (UNIQUE), created_at, indexes on email
     - `matches` table: match_id (UUID PK), created_by_user_id (FK → users, CASCADE), status (CHECK IN PENDING/ACTIVE/FINISHED), created_at, indexes on status and created_by
     - `match_sessions` table: session_id (UUID PK), match_id (FK → matches, CASCADE), room_code (UNIQUE), status (CHECK IN ACTIVE/FINISHED), player_count, tick_rate, server_region, session_heartbeat, started_at, ended_at, created_at, indexes on match_id, status, and heartbeat-WHERE-ACTIVE
     - `frame_input_logs` table: id (BIGSERIAL), session_id (FK → match_sessions, CASCADE), player_id, frame_index, input_bitmask, dx_q16, dy_q16, received_at, PK (id, received_at), indexes on session+frame
     - `desync_incidents` table: incident_id (UUID PK), session_id (FK → match_sessions, CASCADE), frame_index, server_checksum, client_checksum, diff_payload (JSONB), ai_root_cause, created_at, indexes on session_id
   - All migrations must be idempotent (use CREATE TABLE IF NOT EXISTS)
   - Files: `backend/migrations/20261010_001_init_schema.sql`
   - Verify: `cargo sqlx migrate info` runs without error (requires DATABASE_URL set to valid Postgres)

### 5. Enhance postgres_redis_integration.rs tests to cover all requirements
   - File already exists with 7 tests; verify and document each:
     a. **test_migration_from_empty_schema** — Applies migrations, verifies all 5 tables exist (users, matches, match_sessions, frame_input_logs, desync_incidents)
     b. **test_users_crud_and_constraints** — CREATE user, READ via separate query, UPDATE username, DELETE, verify email UNIQUE constraint, verify username NOT NULL
     c. **test_matches_crud_and_cascade** — CREATE match (FK to user), READ, UPDATE status, verify status enum constraint, verify CASCADE delete (delete user → match gone)
     d. **test_sessions_crud_and_heartbeat_orphan_detection** — CREATE session (FK to match), READ, CREATE two sessions, UPDATE one heartbeat to NOW (fresh), stale one to NOW()-30s, query finds only stale session, verify via separate query
     e. **test_redis_ttl_expiry** — SET key with 1s TTL, assert EXISTS=1 immediately, sleep 1.1s, assert EXISTS=0
     f. **test_redis_ordered_queue_atomic_enqueue** — Lua script that: checks SISMEMBER, only RPUSH+SADD if not present, returns 1 on success, 0 on duplicate. Enqueue A/B/C, verify FIFO order, attempt duplicate A (return 0), verify queue still has 3
     g. **test_determinism_during_postgres_outage** — 10 ticks with Postgres, 20 ticks simulated outage (checksums still deterministic, persist fails gracefully), 10 ticks recovery. Verify all 40 checksums match expected values.
   - Two graceful-degradation tests also present (test_graceful_degradation_postgres_offline, test_graceful_degradation_redis_offline)
   - All tests respect `ALLOW_SKIP_INTEGRATION` env var: if unset, test FAILS if services unavailable; if "1", test SKIPS
   - Files: `backend/tests/postgres_redis_integration.rs` (already complete per STEP 2 delivery)
   - Verify: Run with `ALLOW_SKIP_INTEGRATION=1 cargo test --test postgres_redis_integration -- --nocapture --test-threads=1` → all 9 tests pass

### 6. Update Makefile test-integration target
   - Target must:
     - `docker-compose -f infra/docker-compose.yml up -d postgres redis` (start services)
     - Wait for health checks: use `pg_isready -h localhost -U postgres` in loop (timeout 60s)
     - Wait for Redis: use `redis-cli -h localhost ping` in loop (timeout 60s)
     - Run: `cd backend && ALLOW_SKIP_INTEGRATION=0 cargo test --test postgres_redis_integration -- --nocapture --test-threads=1`
     - Print `docker compose ps` before (services started) and after (still running)
     - `docker-compose -f infra/docker-compose.yml down` (teardown, but do NOT delete volumes)
     - On any test failure, ensure services are torn down before exit
   - Files: `Makefile` (replace/update `test-integration` target)
   - Verify: `make test-integration` runs without manual docker setup, all tests pass, services are cleaned up

### 7. Verify full test suite and generate evidence output
   - Run: `cargo test -p rollback-netcode-server --lib --bins` to count Rust tests
   - Run: `python3 tests/test_determinism.py tests/test_golden_determinism.py tests/test_rollback.py tests/test_multiplayer_session.py tests/test_security_fuzzing.py tests/test_chaos_network.py aiml/eval/evaluate_all_models.py tests/test_e2e_flow.py tests/test_sdk_smoke.py` to verify all Python tests
   - Count final test totals per file/crate and create reconciliation table
   - Files: No new files; verification only
   - Verify: `make test` completes without error; count shown in stdout

### 8. Document test reconciliation in OPEN_ITEMS.md update (if necessary)
   - Ensure Item #5 in OPEN_ITEMS.md reflects accurate counts
   - If test count changed from 39, explain why in a new reconciliation note
   - Files: `docs/OPEN_ITEMS.md` (update Item #5 if needed)
   - Verify: Count matches actual test output

### 9. Commit Postgres + Redis integration tests
   - Commit message: "feat(STEP 2): Postgres + Redis persistence integration tests with graceful degradation"
   - Files: `backend/tests/postgres_redis_integration.rs`, `backend/Cargo.toml`, `Makefile`
   - Verify: `git log --oneline -1` shows the message; `make test-integration` passes (or gracefully skips if services unavailable)

### 10. Verify determinism and no regressions
   - Run: `cargo test --lib` (all unit tests in lib.rs modules) → must pass
   - Run: `cargo test --test '*'` (all integration tests) → must pass or skip (if docker-compose unavailable)
   - Verify no panics, no changes to existing passing tests
   - Files: No changes
   - Verify: `cargo test` output shows no new failures

---

## Test Reconciliation Table

| File | Tests | Ignored | Type |
|------|-------|---------|------|
| fuzz_decoder.rs | 1 | 0 | Protocol |
| golden_test.rs | 1 | 0 | Determinism |
| integration_2client_udp.rs | 4 | 0 | Transport |
| module_isolation_test.rs | 1 | 0 | Module |
| multiprocess_chaos_test.rs | 2 | 2 | Network (placeholder) |
| postgres_redis_integration.rs | 9 | 0 | Persistence |
| protocol_tests.rs | 9 | 0 | Protocol |
| simulation_determinism_test.rs | 6 | 0 | Simulation |
| transport_protocol_test.rs | 10 | 0 | Transport |
| chaos_network_channel.rs | 3 | 0 | Network |
| **TOTAL** | **46** | **2** | — |

**Before STEP 2**: 37 passing + 2 ignored = 39 total  
**After STEP 2**: 46 passing + 2 ignored = 48 total (9 new integration tests)

---

## Gate Status Table (Updated for STEP 2)

| Gate | Status | Summary | Blocker |
|------|--------|---------|---------|
| **G1** | ⚠️ PARTIAL | Protocol ✅, Checksums ✅; Threat Model ❌, ADRs ❌, CI/CD ❌, Docker ❌ | OPEN_ITEMS#3 |
| **G2** | ⚠️ PARTIAL | Determinism ✅ (1M steps), In-process chaos ✅; True multiprocess ❌ | OPEN_ITEMS#1 |
| **G3** | ❌ BLOCKED | Session lifecycle not implemented | STEP 3 required |
| **G4** | ❌ BLOCKED | Auth/rate-limiting not implemented | STEP 3+ required |
| **G5** | ⚠️ PARTIAL | Postgres/Redis integration ✅; Real chaos network ❌ | OPEN_ITEMS#1 |
| **G6** | ⚠️ PARTIAL | Models complete ✅; Evaluation output not captured | OPEN_ITEMS#4 |
| **G7** | ❌ BLOCKED | E2E multiplayer not implemented | STEP 3 required |
| **G8** | ❌ BLOCKED | Production readiness audit pending | Future |

---

## Key Decisions

1. **Migrations are sqlx-based** — Uses `backend/migrations/` directory with `sqlx migrate` CLI integration. No separate versioning tool needed.
2. **Lua script for atomic queue** — Redis SET operations alone cannot prevent duplicates in a distributed setting; Lua script ensures atomicity (check + push + add-to-set = single atomic op).
3. **Graceful degradation with fail-loud** — Tests FAIL if services are unavailable UNLESS `ALLOW_SKIP_INTEGRATION=1` env var is set. This prevents silent passes in CI.
4. **Separate connection per test** — Each test gets its own pool/connection from docker-compose. No connection pooling tuning (out of scope for integration tests).
5. **Determinism during outage** — Checksums are computed deterministically regardless of persistence. A real outage (Postgres down) causes persist calls to fail, but the game loop keeps ticking. On recovery, persist resumes.

---

## Deliverables Summary

✅ **STEP 1.6 output**: `docs/OPEN_ITEMS.md` + commit
✅ **STEP 2 output**: 
   - 9 integration tests (postgres + redis + graceful degradation)
   - Enhanced Makefile `test-integration` target
   - sqlx migrations in `backend/migrations/`
   - Cargo.toml with `migrate` feature
   - Full test suite reconciliation (48 tests total)

✅ **Proof**: Raw command output from `make test-integration` showing all tests passing with services actually running
✅ **Gate status**: Corrected table with accurate assessment of what's blocked vs. partially complete

---

## Execution Order

1. Create and commit STEP 1.6 docs (OPEN_ITEMS.md) — **1 commit**
2. Update Cargo.toml with migrate feature and verify compilation — **Inline, no separate commit**
3. Verify backend/migrations/ exists with correct schema — **Already present, verify only**
4. Verify postgres_redis_integration.rs tests are complete — **Already present, verify only**
5. Update Makefile test-integration target — **Inline in final commit**
6. Run full test suite and generate reconciliation table — **Verification only**
7. Final commit: STEP 2 integration tests + Makefile — **1 commit**

**Total commits**: 2 (one for STEP 1.6, one for STEP 2)

