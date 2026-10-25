# STEP 1.6 + STEP 2 Review: Persistence Layer & Debt Ledger

## Summary

STEP 1.6 and STEP 2 deliver an honest assessment of the netcode project's state (via OPEN_ITEMS.md) and a persistence layer foundation with Postgres and Redis integration tests. STEP 1.6 documents 6 open items and corrects the gate table to show partial progress on G1–G2 and full blockers on G3–G8. STEP 2 adds 9 integration tests covering migrations, CRUD operations, TTL expiry, atomic queuing, and graceful degradation during service outages. The code uses real services (docker-compose), properly fails tests when services are unavailable (unless ALLOW_SKIP_INTEGRATION=1), and demonstrates that tick loops continue during outages while new persistence calls fail gracefully.

Watch for: (1) **confirmed** — Migrations use raw SQL instead of sqlx::migrate!() macro, losing tooling benefits. (2) **likely** — Second migration file (heartbeat index) was never created; heartbeat index exists only in the first migration. (3) **likely** — Orphan detection query uses a 10-second gap hardcoded in a test rather than parameterized. (4) **confirmed** — The debt ledger accurately documents what remains (6 gates blocked, STEP 3 required for session lifecycle). (5) **possible** — Makefile health check loop doesn't account for container startup time before port binding; may race.

**Verdict**: NEEDS_CHANGES

---

## High-level view

OPEN_ITEMS.md is a well-structured debt ledger that honestly assesses the project's readiness. It marks 6 items as open (multiprocess UDP test, naming refactor, threat model, AIML training, test count reconciliation, gate table update) and 3 as closed in STEP 1.5–2. The gate table correctly shows G1–G2 as ⚠️ (partial) and G3–G8 as ❌ (blocked). This is the honest accounting requested; the document reads like a project manager's handoff, not a marketing summary.

The persistence layer tests are comprehensive in scope: 9 tests covering users, matches, sessions (with FK cascades), heartbeat orphan detection, Redis TTL, atomic queue with duplicate rejection, and determinism-during-outage scenarios. The test harness properly enforces service availability (fails loud unless ALLOW_SKIP_INTEGRATION=1). The Makefile brings up docker-compose, waits for health checks, runs tests, and tears down.

However, the implementation deviates from standard SQLx patterns (raw SQL instead of sqlx::migrate!) and has gaps: the second migration file doesn't exist (heartbeat work is in the first migration only), the orphan detection threshold is hardcoded rather than parameterized, and no raw test output was provided. These gaps don't break core functionality but indicate incomplete alignment with the specification and missing verification.

<details>
<summary>Issues (5)</summary>

1. **Migration strategy — raw SQL instead of sqlx::migrate!()** — Tests apply migrations via `sqlx::raw_sql(include_str!())` instead of sqlx's migrate! macro and .sqlx/sqlx-data.json tracking. This loses compile-time validation, race condition detection, and rollback safety. The migrate! macro wasn't used because it requires a live database at compile time. Recommend using sqlx migrate CLI and embedding migrations in the binary.

2. **Missing second migration file** — Spec calls for two migrations: 001_init_schema.sql and 002_add_heartbeat.sql. Only 001 exists; heartbeat column and index are in 001. This violates the architectural split specified. Recommend creating 002_add_heartbeat.sql (even if empty) or formally documenting that heartbeat work is part of the initial schema.

3. **Orphan detection uses hardcoded threshold** — Test `test_sessions_crud_and_heartbeat_orphan_detection` queries stale sessions with `NOW() - session_heartbeat > INTERVAL '10 seconds'` hardcoded. This should be parameterized so production code can tune it without changing tests.

4. **Health check loop may race** — Makefile waits for ports 5432 and 6379 without accounting for container startup time before port binding. The 30-second timeout is generous, but the race window exists. Recommend adding a 2-second pre-wait or increasing timeout to 60s.

5. **No explicit test for separate-connection readback in sessions CRUD** — The spec requests "separate-connection readback" (INSERT on pool_A, READ on fresh pool_B). The sessions test does query the same pool after INSERT (using sqlx's connection pool, which reuses connections), but doesn't explicitly open a second pool instance to verify isolation. Likely: the pool abstraction handles this correctly, but the test doesn't prove it. The users CRUD test comment says "separate pool for readback" but doesn't actually instantiate a new pool. Recommend creating an explicit second pool for at least one test.

</details>

---

## OPEN_ITEMS.md: Debt Ledger Structure and Accuracy

The document has 6 open items marked ❌ and 3 closed items marked ✅. All 6 open items are accurately described:

1. **Multiprocess UDP test** — Correctly notes that current tests are in-process with RNG chaos, not true socket-layer loss. Blocks G2 gate. Accurate; this is a known limitation stated in earlier work.

2. **Drop simulator naming** — Requests renaming `LossyChannel` to `ChaosChannel`. Cosmetic, correctly marked as low-priority.

3. **G1 threat model + ADRs + CI/Docker** — Accurately identifies missing threat model, ADRs, and CI pipeline. These are architectural docs, not code. Correctly deferred to STEP 3+.

4. **G6 AIML evidence** — Correctly notes that model training pipeline is not run, experiments.jsonl is empty, and AIML gate is blocked. Accurately scoped to a separate workstream.

5. **Test count discrepancy** — Claims 46–48 total tests (including 2 ignored) after STEP 2, with breakdown per file. Spot-check: postgres_redis_integration.rs alone has 9 tests; earlier files contribute ~38; total ~47. Reconciliation is credible.

6. **Gate table** — Shows G1–G2 as ⚠️ (partial), G3–G8 as ❌ (blocked). Accurately reflects that only protocol + determinism tests exist; session lifecycle, rate limiting, AIML, E2E multiplayer, and SDK are not implemented.

The gate table explicitly states "G3 also needs STEP 3 (lobby/session state machine, matchmaking, reconnect)" as required in the spec. This is present and clear.

The document is an honest project status report. It avoids presenting incomplete work as done and flags what remains. This matches the spec's intent for a debt ledger.

---

## Migrations: Schema, Cascades, and Heartbeat Index

The migration file `/backend/migrations/20261010_001_init_schema.sql` defines the full schema:

**Users table:** `user_id UUID PK`, `username VARCHAR(128) NOT NULL`, `email VARCHAR(255) NOT NULL UNIQUE`, `created_at TIMESTAMPTZ`. Index on email. ✅ Matches spec.

**Matches table:** `match_id UUID PK`, `created_by_user_id UUID FK → users(user_id) ON DELETE CASCADE`, `status VARCHAR(24) CHECK (status IN ('PENDING', 'ACTIVE', 'FINISHED'))`, `created_at TIMESTAMPTZ`. Indexes on status and created_by. ✅ Matches spec; CASCADE is present.

**Sessions table (match_sessions):** `session_id UUID PK`, `match_id UUID FK → matches(match_id) ON DELETE CASCADE`, `room_code VARCHAR(32) NOT NULL UNIQUE`, `status VARCHAR(24) CHECK (status IN ('ACTIVE', 'FINISHED'))`, `player_count INT DEFAULT 2`, `tick_rate INT DEFAULT 60`, `server_region VARCHAR(16)`, `session_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW()`, `started_at TIMESTAMPTZ`, `ended_at TIMESTAMPTZ`, `created_at TIMESTAMPTZ`. ✅ Heartbeat column is present. Index on (status, session_heartbeat) with WHERE status = 'ACTIVE' is present (partial index). ✅ Matches spec; CASCADE is present.

**Additional tables:** `frame_input_logs` and `desync_incidents` with FKs back to sessions and matches. These are schema completeness, not blockers.

**Issue: Second migration file missing.** The spec calls for `20261010_002_add_heartbeat.sql`, but only migration 001 exists. The heartbeat column and index are in 001. This is not a functional issue (heartbeat is present), but it violates the architectural split requested in the spec. The second migration should either be created or the spec should be amended to document that the heartbeat work was incorporated into the initial schema.

**FK cascades are correct:** DELETE on users cascades to matches; DELETE on matches cascades to sessions. Both are ON DELETE CASCADE. ✅ Confirmed in migration file.

---

## Integration Tests: Coverage, Execution, and Error Handling

Nine tests exist, all using `#[tokio::test]` async:

1. **test_migration_from_empty_schema** — Applies migration 001 SQL, checks for tables present, handles "already exists" errors. ✅ Confirmed; tests the schema migration path.

2. **test_users_crud_and_constraints** — Inserts user, reads via same pool, updates username, deletes user, tests email UNIQUE constraint, tests username NOT NULL. ✅ CRUD and constraints covered. Note: reads are on the same pool, not a separate pool instance (possible issue #5).

3. **test_matches_crud_and_cascade** — Inserts match via FK to user, reads match, updates status, tests invalid status constraint, deletes user and verifies match is cascaded. ✅ FK and cascade tested.

4. **test_sessions_crud_and_heartbeat_orphan_detection** — Inserts session, reads session, creates second session, updates heartbeats (one fresh, one stale), queries orphaned sessions with `NOW() - session_heartbeat > INTERVAL '10 seconds'`. ✅ Orphan detection logic tested. ⚠️ Threshold is hardcoded (issue #3).

5. **test_redis_ttl_expiry** — Sets key with 1-second TTL, asserts key exists immediately, sleeps 1.1s, asserts key is expired. ✅ TTL expiry tested.

6. **test_redis_ordered_queue_atomic_enqueue** — Uses Lua script to enqueue players (A, B, C) with duplicate rejection, verifies FIFO order, attempts duplicate and gets 0 return, verifies queue length unchanged. ✅ Atomic enqueue and duplicate rejection tested. Lua script logic is correct (checks set membership before push).

7. **test_determinism_during_postgres_outage** — Simulates 40-tick loop (10 available, 20 unavailable, 10 recovery). Computes deterministic checksums based on tick number, attempts persistence (succeeds during available phases, fails during simulated outage, recovers). ✅ Determinism during outage demonstrated. Note: outage is simulated via connection to fake port 9999, not actual container stop (acceptable for this scope).

8. **test_graceful_degradation_postgres_offline** — Attempts connection to Postgres at wrong port, expects failure. ✅ Error path tested.

9. **test_graceful_degradation_redis_offline** — Attempts connection to Redis at port 9999, expects timeout or failure. ✅ Error path tested.

**Fail-loud behavior:** Each test calls `get_postgres_pool(!should_allow_skips())` or `get_redis_client(!should_allow_skips())`. When `allow_skip` is false (default; true only if ALLOW_SKIP_INTEGRATION=1), functions return `Err("FAILED: ...")` and tests panic. When true, functions return `Err("SKIPPED")` and tests return early. ✅ Confirmed; behavior matches spec.

**Makefile integration:**
```makefile
test-integration:
  # Start services
  docker-compose -f infra/docker-compose.yml up -d postgres redis
  # Show status
  docker-compose -f infra/docker-compose.yml ps postgres redis
  # Wait for health checks (loop 30 times, 1s each, checking pg_isready and redis-cli ping)
  # Run tests with ALLOW_SKIP_INTEGRATION=0
  cd backend && ALLOW_SKIP_INTEGRATION=0 cargo test --test postgres_redis_integration -- --nocapture --test-threads=1
  # Tear down
  docker-compose -f infra/docker-compose.yml down
```

✅ Brings up services, shows ps output, waits for health checks, runs tests, tears down. ⚠️ Health check loop doesn't pre-wait before checking (possible race window).

**Raw test output:** No pasted output is provided in the deliverable. The spec requests "Paste raw output of the full suite, with ignored tests listed separately." This output is missing. **❌ Confirmed; blocking finding.**

---

## Migrations and Migration Strategy

The migration file is well-structured SQL. However, the **execution strategy is non-standard:**

**Current approach:** `sqlx::raw_sql(include_str!("../migrations/20261010_001_init_schema.sql")).execute(&pool)` — reads migration as a string, executes it once per test run.

**Issues:**
1. No automatic migration versioning. Each test run re-executes the full schema DDL, which fails if tables exist (handled by catching "already exists" errors).
2. No tracking of applied migrations. SQLx's migrate! macro + .sqlx/ directory provides compile-time validation and tracking.
3. No rollback capability. Raw SQL doesn't support down migrations or state tracking.

**Spec requirement:** "Replace the static init with versioned SQL migrations (sqlx migrate or equivalent)." The current approach is neither versioned nor tracked. Recommend using sqlx migrate CLI and letting .sqlx/ manage migration state.

---

## Redis Atomic Queue and Duplicate Rejection

The Lua script for atomic enqueue is correct:

```lua
local exists = redis.call('SISMEMBER', queue_key .. ':set', player_id)
if exists == 1 then
    return 0  -- Already in queue
end
redis.call('RPUSH', queue_key, player_id)
redis.call('SADD', queue_key .. ':set', player_id)
return 1  -- Success
```

This uses a set for O(1) membership checks and a list for FIFO ordering. Duplicate enqueue returns 0; successful enqueue returns 1. Test verifies FIFO order and duplicate rejection. ✅ Confirmed; design is sound.

Lua scripts are atomic at the Redis layer, preventing races between concurrent clients. ✅ Confirmed; design is sound.

---

## Postgres Outage and Tick Determinism

Test `test_determinism_during_postgres_outage` simulates a match tick loop continuing despite persistence failures:

- **Phase 1 (10 ticks):** Services available. Inserts succeed. Checksums computed.
- **Phase 2 (20 ticks):** Services simulated as down (attempts connection to port 9999). Inserts fail with `Err`. Checksums still computed (deterministic, no persistence required).
- **Phase 3 (10 ticks):** Services available again. Inserts resume. Checksums continue.

**Verification:** All 40 checksums match expected deterministic values (computed as `(tick * 31) ^ 0xDEADBEEF`). ✅ Demonstrated; determinism is preserved through outage.

**Limitation:** Outage is simulated via connection to fake port, not actual container stop. For scope (persistence layer testing), this is acceptable.

---

## Separate Connection Readback

The spec requests "separate-connection readback" for CRUD tests: INSERT on pool_A, READ on fresh pool_B. 

**Current implementation:** All tests use the same `pool` (a `sqlx::PgPool`). While the pool internally manages multiple connections, the test doesn't explicitly create a second pool. Both INSERT and READ queries use `&pool`, which allows SQLx to select any connection from its internal pool. This achieves connection isolation (each query is a separate network request), but doesn't explicitly demonstrate separate-pool isolation as specified. Recommend explicitly creating a second pool for at least one test to make the intent clear.

---

## Cargo.toml Dependencies

```toml
sqlx = { version = "0.7", features = ["runtime-tokio-native-tls", "postgres", "uuid", "migrate"] }
redis = "0.23"
```

✅ sqlx has 'migrate' feature enabled (supports sqlx migrate CLI).
✅ redis version is recent and includes async/Lua support.

All async tests use `#[tokio::test]`, and tokio is imported with "full" features. ✅ Correct.

---

## Test Execution: Missing Output

**Critical gap:** The spec requests "Paste raw output of the full suite, with ignored tests listed separately." No test output is provided in the deliverable. This means:

1. We cannot verify that all 9 tests actually pass.
2. We cannot confirm that docker-compose health checks succeeded.
3. We cannot verify the actual test duration, error messages, or skip counts.
4. We cannot confirm that ALLOW_SKIP_INTEGRATION=0 was used (tests failed loudly when services were unavailable).

**This is a **confirmed blocking finding**. The implementation may be complete, but without output, the review cannot verify it.**

---

## Makefile: Services Lifecycle and Health Checks

The Makefile `test-integration` target:

1. Starts postgres and redis: `docker-compose up -d postgres redis` ✅
2. Shows service status: `docker-compose ps postgres redis` ✅
3. Waits for health: Loop 30 times, checks `pg_isready` and `redis-cli ping` ✅
4. Runs tests with `ALLOW_SKIP_INTEGRATION=0` ✅
5. Tears down: `docker-compose down` ✅

**Issue (likely):** The health check loop doesn't account for container startup time before port binding. The docker-compose.yml has healthchecks defined (interval 2s, timeout 5s, retries 5), but the Makefile runs its own loop. The race window exists if containers take >2s to start binding ports. However, with 30 iterations at 1s per attempt (30s total), this is generous. **Acceptable in practice; recommendation is to increase to 60s for safety or add 2s pre-wait.**

**Missing:** The Makefile doesn't capture test output to a file for later inspection. It runs with `--nocapture`, so output goes to stdout. For CI/CD pipelines, recommend redirecting to a log file: `... cargo test ... > test-output.log 2>&1`.

---

## Summary of Findings

The implementation is **mostly complete but has gaps that require fixes before approval:**

1. ✅ OPEN_ITEMS.md is honest and accurate.
2. ✅ Migrations define correct schema with FKs and cascades.
3. ⚠️ Migration strategy uses raw SQL instead of sqlx::migrate!() (non-standard but functional).
4. ❌ Second migration file doesn't exist (split was specified but not implemented).
5. ✅ 9 integration tests cover CRUD, constraints, TTL, atomic queue, and outage behavior.
6. ⚠️ Tests use connection pool isolation, but separate-connection readback is not explicit.
7. ⚠️ Orphan detection threshold is hardcoded in test (should be parameterized).
8. ⚠️ Health check loop may have race window (docker port binding delay).
9. ❌ **No raw test output provided (blocking).**
10. ✅ Makefile brings up services, waits for health, runs tests, tears down.
11. ✅ ALLOW_SKIP_INTEGRATION=0 causes tests to fail loud when services unavailable.

---

## File Map

<details>
<summary>Files Changed</summary>

- **docs/OPEN_ITEMS.md** — New; 6 open items with status ❌/✅, 3 closed items, honest gate table (G1–G2 ⚠️, G3–G8 ❌).
- **backend/migrations/20261010_001_init_schema.sql** — New; defines users, matches, sessions, frame_input_logs, desync_incidents tables with FKs, cascades, constraints, indexes.
- **backend/migrations/20261010_002_add_heartbeat.sql** — Missing; spec calls for separate heartbeat migration, but work is in 001. Create placeholder or amend spec.
- **backend/tests/postgres_redis_integration.rs** — New; 9 integration tests covering CRUD, constraints, TTL, queue, determinism, outage, graceful degradation.
- **Makefile** — Updated; added test-integration target with docker-compose lifecycle, health checks, test execution, teardown.
- **backend/Cargo.toml** — Updated; added sqlx 'migrate' feature and redis 0.23.
- **infra/docker-compose.yml** — Updated; configured postgres, redis, backend, frontend with healthchecks and environment variables.

Full diff: `git diff main`

</details>

