# Implementation Plan: C6–D8 (Security & Release, Final Scope Closure)

**Baseline**: 128 passing tests (C5 complete). **Target**: ~153 passing tests (25 new).
**Scope**: 9 tasks, 1 commit per task, no Docker, all tests with `ALLOW_SKIP_INTEGRATION=1`.
**Evidence**: Raw `cargo test` output saved to `docs/evidence/<task-id>.txt` per task.

---

## Task Ordering & Dependencies

Tasks are **strictly sequential** (one commit per task); later tasks may depend on earlier infrastructure.

1. **C6: End-to-End UDP Test** → establishes real UDP handshake + frame exchange + rollback cycle (foundation for D1–D8).
2. **D1: Replay Attack Defense** → tests seq number validation in protocol layer (independent, uses C6 infra).
3. **D2: Desync Attack Detection** → tests checksum-based forged state detection (independent, uses simulation).
4. **D3: Session Hijacking Defense** → tests token expiry + validation (independent, uses session repo).
5. **D4: DoS Mitigation (Rate Limiting)** → tests handshake rate limiting by IP (independent, uses UDP server).
6. **D5: Orphan Cleanup E2E** → tests 40-second stale session cleanup on real repo (independent).
7. **D6: Protocol Version Negotiation** → tests v0.9 and v2.0 rejection (independent).
8. **D7: Release Build Verification** → tests `cargo build --release` succeeds (independent).
9. **D8: M1 Integration Test** → full lifecycle (lobby → ready → active → disconnect → reconnect → cleanup) (depends on C1–C5, C6).

---

## C6: End-to-End UDP Test (5 tests)

**Goal**: Establish real UDP localhost server + 2 client connections; run 100 ticks of deterministic rollback; verify matching checksums.

**Design Decision**: Use `tokio::spawn` to run server + 2 client tasks in parallel; client tasks send `InputFrame` packets with predictable input sequences (LEFT, RIGHT, JUMP); server echoes `Checksum` packets; clients verify checksums match. No actual game state collision needed (stub `GameState` advancing tick counter is sufficient).

**Files to Create/Modify**:
- `backend/tests/c6_e2e_udp_test.rs` — 5 integration tests for UDP E2E (handshake, frame exchange, checksum sync, rollback cycle, tick 100 convergence).
- `backend/src/network/udp_server.rs` — (modify if needed) ensure `run()` emits `Checksum` packets after each tick.

**Steps**:
1. Create a test helper that spawns a UDP server on localhost:7777 bound to a `FakeClock` (or `Utc::now()`).
2. Create two client tasks that connect via UDP handshake, exchange 100 `InputFrame` packets with seq 1–100, and collect `Checksum` responses.
3. Assert both clients' final checksums are equal after 100 ticks.
4. Add a test for rollback: after tick 50, inject a replay (old seq num) and verify server rejects it.
5. Add a test for frame loss recovery: drop a frame, verify 5-frame redundancy reconstruction.

**Verification**: `cargo test --test c6_e2e_udp_test --lib -- --nocapture 2>&1 | tee docs/evidence/c6_e2e_udp.txt`
- Expected: **5 passing** tests (handshake, frame exchange 100 ticks, checksum match, rollback rejection, redundancy recovery).
- Exit: 0. Output saved to `docs/evidence/c6_e2e_udp.txt`.

**Notes**:
- Use `tokio::time::timeout(Duration::from_secs(10), ...)` to prevent hangs.
- Stub game logic (e.g., just count ticks or increment position by input bitmask).
- No real multiplayer game state needed; determinism is proven by existing simulation tests.

---

## D1: Replay Attack Defense (3 tests)

**Goal**: Verify `InputFrame` seq numbers are monotonically increasing and server rejects seq ≤ last_acked_seq.

**Design Decision**: Refactor `transport::protocol.rs` (or create new `transport::sequence.rs`) with a `SequenceValidator` trait. In-memory impl tracks `last_acked_seq` per player; test sends seq 100, 101, 50 (replay), 102 and verifies only 100, 101, 102 are accepted.

**Files to Create/Modify**:
- `backend/src/protocol/sequence.rs` — new module with `SequenceValidator` trait, in-memory impl, and `validate_seq(player_id, seq_num) -> Result<bool, ...>`.
- `backend/tests/d1_replay_defense_test.rs` — 3 tests: valid sequence acceptance, replay rejection, reused sequence rejection.

**Steps**:
1. Add `SequenceValidator` trait with methods: `new()`, `validate_seq(&mut self, player_id: u8, seq: u32) -> Result<bool, ...>`.
2. Implement in-memory validator storing `last_acked: HashMap<u8, u32>`.
3. Test 1: Send seq 1, 2, 3 → all accepted.
4. Test 2: Send seq 1, 2, 1 (replay) → 1, 2 accepted, replay rejected with `SequenceError::Replay`.
5. Test 3: Send seq 100, 100 (reused) → first accepted, second rejected.

**Verification**: `cargo test --test d1_replay_defense_test -- --nocapture 2>&1 | tee docs/evidence/d1_replay_defense.txt`
- Expected: **3 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- No UDP needed; unit tests with in-memory validator sufficient.
- Sequence numbers are u32; wrap-around (after 2^32) is a later concern (out of M1 scope).

---

## D2: Desync Attack Detection (3 tests)

**Goal**: Verify `DesyncDetector` computes authoritative checksums per frame and rejects forged client checksums.

**Design Decision**: Enhance `desync::detector.rs` with:
- `record_authoritative_hash(frame: u64, hash: u32)` — server records after simulation.
- `verify_client_hash(frame: u64, client_hash: u32) -> Result<bool, ...>` — client sends hash; server checks.
- On mismatch: trigger rollback (already tested in `simulation_determinism_test.rs`).

**Files to Create/Modify**:
- `backend/src/desync/detector.rs` — (modify if needed) ensure `verify_client_hash()` returns clear error on mismatch; add state mutation tracking for forensics (optional for D2).
- `backend/tests/d2_desync_detection_test.rs` — 3 tests: correct checksum acceptance, forged checksum rejection, rollback trigger on mismatch.

**Steps**:
1. Test 1: Simulate 10 frames with known inputs; compute authoritative hashes after each tick (using `blake3` or existing hash function); client sends matching hashes; all pass.
2. Test 2: Client sends wrong hash at frame 5; detector rejects with `DesyncError::ChecksumMismatch`.
3. Test 3: On detection, rollback engine re-simulates from frame 0 with client inputs and verifies new hash matches authoritative. (This is already tested; reuse `test_forced_desync_detection_and_authoritative_recovery` pattern.)

**Verification**: `cargo test --test d2_desync_detection_test -- --nocapture 2>&1 | tee docs/evidence/d2_desync_detection.txt`
- Expected: **3 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- Leverage existing `simulation_determinism_test.rs` for determinism proof.
- Focus on detector logic, not game state mutation (already covered).

---

## D3: Session Hijacking Defense (3 tests)

**Goal**: Verify reconnect tokens are bound to (player_id, session_id) and rejected if expired or player_id mismatch.

**Design Decision**: Enhance `session::repository.rs`:
- `ReconnectToken` already has `player_id`, `issued_at`, `expires_at`, and `is_valid()`.
- `validate_reconnect_token(token_str: &str, expected_player_id: PlayerId) -> Result<ReconnectToken, SessionError>` — checks token exists, is valid, and player_id matches.
- Test 1: Valid token within 30s window → accepted.
- Test 2: Expired token (> 30s) → rejected with `TokenExpired`.
- Test 3: Correct token but wrong player_id → rejected with `PlayerMismatch`.

**Files to Create/Modify**:
- `backend/src/session/repository.rs` — (modify) add `validate_reconnect_token()` signature if not present; ensure error types distinguish between expiry, not found, and player mismatch.
- `backend/tests/d3_session_hijack_test.rs` — 3 tests: valid token acceptance, expired token rejection, player mismatch rejection.

**Steps**:
1. Test 1: Create session, issue token to player1 at T0, verify within 30s.
2. Test 2: Create session, issue token, advance fake clock by 31s, verify `TokenExpired`.
3. Test 3: Create session, issue token for player1, try to validate with player2, verify `PlayerMismatch`.

**Verification**: `cargo test --test d3_session_hijack_test -- --nocapture 2>&1 | tee docs/evidence/d3_session_hijack_defense.txt`
- Expected: **3 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- Reuse `reconnect_test.rs` patterns (fake clock, token creation).
- No UDP needed; session repo unit tests sufficient.

---

## D4: DoS Mitigation (Rate Limiting) (2 tests)

**Goal**: Verify UDP server rate-limits handshakes by IP address (e.g., max 10 per second).

**Design Decision**: Add `RateLimiter` struct to `network::udp_server.rs`:
- Track `(SocketAddr) -> Vec<Instant>` of handshake times.
- On each handshake, check if last 1 second has ≥ 10 handshakes; if so, reject with `RateLimitExceeded`.
- Clean up old entries every N handshakes.

**Files to Create/Modify**:
- `backend/src/network/rate_limiter.rs` — new module with `RateLimiter` struct and `check_handshake(peer: SocketAddr) -> Result<(), RateLimitError>`.
- `backend/src/network/udp_server.rs` — (modify) integrate `RateLimiter` into handshake path.
- `backend/tests/d4_dos_mitigation_test.rs` — 2 tests: burst rejection, rate limit recovery.

**Steps**:
1. Test 1: Send 5 handshakes rapidly from same IP → all accepted. Send 6–15 → rejected with `RateLimitExceeded`.
2. Test 2: Send 10 handshakes, wait 1.1s, send 10 more → all accepted (window has rolled).

**Verification**: `cargo test --test d4_dos_mitigation_test -- --nocapture 2>&1 | tee docs/evidence/d4_dos_mitigation.txt`
- Expected: **2 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- Simple sliding window (in-memory, not persistence); sufficient for M1.
- Use `tokio::time::Instant` for monotonic timing.

---

## D5: Orphan Cleanup E2E (2 tests)

**Goal**: Verify `session::cleanup::OrphanCleanupTask` finds stale sessions (no heartbeat ≥ 40s) on real PostgresSessionRepository.

**Design Decision**: Leverage existing `C3: Orphan cleanup on fake clock` test. For D5, extend to use real Postgres:
- `find_stale(40)` from repo.
- Verify orphan marked for cleanup.
- (No actual deletion in M1; just detection, as per threat model.)

**Files to Create/Modify**:
- `backend/src/session/cleanup.rs` — (modify if needed) ensure `OrphanCleanupTask` is public; add `run_once(&self, repo: &dyn SessionRepository) -> Result<Vec<Session>, ...>` helper for testing.
- `backend/tests/d5_orphan_cleanup_e2e_test.rs` — 2 tests: find stale sessions on PostgresSessionRepository, verify no stale on InMemorySessionRepository after heartbeat.

**Steps**:
1. Test 1: Create session on Postgres, don't heartbeat for 41s (fake clock), run cleanup, verify found.
2. Test 2: Create session on Postgres, heartbeat every 30s, run cleanup after 45s (3 heartbeats), verify not found (all within 40s).

**Verification**: `cargo test --test d5_orphan_cleanup_e2e_test -- --nocapture 2>&1 | tee docs/evidence/d5_orphan_cleanup_e2e.txt`
- Expected: **2 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- Requires Postgres (but ALLOW_SKIP_INTEGRATION=1 will skip if unavailable; mark test with `#[tokio::test]` and check for DB connection in test body).
- Alternatively, use InMemorySessionRepository + fake clock for both tests (simpler, still proves cleanup logic).

---

## D6: Protocol Version Negotiation (3 tests)

**Goal**: Verify `VersionNegotiator` accepts v1.0 and rejects v0.9 and v2.0 with clear error.

**Design Decision**: Enhance `protocol::version.rs`:
- `CURRENT_PROTOCOL_VERSION = 1` (already defined).
- `MIN_SUPPORTED_PROTOCOL_VERSION = 1`, `MAX_SUPPORTED_PROTOCOL_VERSION = 1`.
- `VersionNegotiator::negotiate(client_version: u8) -> Result<u8, VersionError>` — returns negotiated version or `VersionError::UnsupportedVersion`.

**Files to Create/Modify**:
- `backend/src/protocol/version.rs` — (modify if needed) ensure bounds checks; add test-friendly `negotiate()` method if missing.
- `backend/tests/d6_protocol_version_test.rs` — 3 tests: v1.0 accepted, v0.9 rejected, v2.0 rejected.

**Steps**:
1. Test 1: Client sends Handshake with version=1 → VersionNegotiator returns 1.
2. Test 2: Client sends Handshake with version=0 → VersionNegotiator rejects with `UnsupportedVersion("0.x not supported")`.
3. Test 3: Client sends Handshake with version=2 → VersionNegotiator rejects with `UnsupportedVersion("2.x not supported")`.

**Verification**: `cargo test --test d6_protocol_version_test -- --nocapture 2>&1 | tee docs/evidence/d6_protocol_version.txt`
- Expected: **3 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- Reuse patterns from `protocol_tests.rs` (encode/decode roundtrip).
- No UDP needed; unit tests on version negotiator sufficient.

---

## D7: Release Build Verification (1 test)

**Goal**: Verify `cargo build --release` succeeds and binary exists.

**Design Decision**: Simple shell-based integration test that invokes `cargo build --release` and checks exit code.

**Files to Create/Modify**:
- `backend/tests/d7_release_build_test.rs` — 1 test: run build, check exit code and binary existence.

**Steps**:
1. Test 1: Invoke `cargo build --release` in subprocess; assert exit code 0; assert binary exists at `target/release/rollback-netcode-server` (or appropriate binary name).

**Verification**: `cargo test --test d7_release_build_test -- --nocapture 2>&1 | tee docs/evidence/d7_release_build.txt`
- Expected: **1 passing** test.
- Exit: 0. Output saved.

**Notes**:
- This test runs `cargo build --release` from within a test; may be slow (5–30s). Acceptable for M1 final verification.
- Alternatively, skip via `#[ignore]` and run manually `cargo build --release` to save CI time.

---

## D8: M1 Scope Closure (2+ tests)

**Goal**: Full lifecycle integration test: lobby → ready → active → input exchange → disconnect → reconnect → cleanup.

**Design Decision**: Combine C1–C5 and C6 patterns:
1. Create lobby, join 2 players, mark ready.
2. Transition to Active.
3. Exchange InputFrame packets over UDP (via C6 server) for 20 ticks.
4. Simulate disconnect (close client socket or timeout).
5. Reconnect with token from earlier session.
6. Verify resumed at correct tick with matching checksums.
7. Cleanup: run orphan sweeper, verify session not found.

**Files to Create/Modify**:
- `backend/tests/d8_m1_integration_test.rs` — 2+ tests: full lifecycle (happy path), disconnect + reconnect, cleanup after finish.

**Steps**:
1. Test 1 (Happy Path):
   - Create lobby for 2 players.
   - Join player1, player2.
   - Both ready → transition to Active.
   - Spawn UDP server + 2 client tasks.
   - Exchange 20 InputFrame packets; verify tick counter and checksums match.
   - Close clients gracefully.
   - Run cleanup; verify session marked Finished (or transitioned appropriately).

2. Test 2 (Disconnect + Reconnect):
   - Create lobby, join, ready, transition to Active.
   - Send 10 InputFrames.
   - Player1 disconnects (no heartbeat for 41s on fake clock).
   - Cleanup marks session orphaned.
   - Player1 reconnects with reconnect token.
   - Verify resumed at tick 11 (not 1) with matching checksums.
   - Complete remaining 10 ticks (ticks 11–20).

3. Test 3 (Optional, for margin):
   - Verify desync detection during active play (bonus test).

**Verification**: `cargo test --test d8_m1_integration_test -- --nocapture 2>&1 | tee docs/evidence/d8_m1_integration.txt`
- Expected: **2–3 passing** tests.
- Exit: 0. Output saved.

**Notes**:
- This is the final integration test; complexity is acceptable.
- Reuse helpers from C1–C6 (lobby creation, UDP server, fake clock).
- If time/context runs low, reduce to 2 tests (happy path + disconnect).

---

## Verification Template (All Tasks)

For each task `C6`, `D1`, ..., `D8`:

```bash
cd backend

# Run test, save output
ALLOW_SKIP_INTEGRATION=1 cargo test --test <task>_* -- --nocapture 2>&1 | tee docs/evidence/<task>.txt

# Verify exit code
echo "Exit: $?"

# Count passing tests
grep "test result:" docs/evidence/<task>.txt | grep "passed"
```

**Expected Cumulative**:
- C6: 5 tests
- D1: 3 tests
- D2: 3 tests
- D3: 3 tests
- D4: 2 tests
- D5: 2 tests
- D6: 3 tests
- D7: 1 test
- D8: 2+ tests
- **Total**: 24–25 new tests → 152–153 total passing.

---

## Commit Strategy

Each task is one atomic commit:
```
C6: E2E UDP rollback protocol test (5 tests)
D1: Replay attack defense (3 tests)
D2: Desync attack detection (3 tests)
D3: Session hijacking defense (3 tests)
D4: DoS mitigation – rate limiting (2 tests)
D5: Orphan cleanup E2E (2 tests)
D6: Protocol version negotiation (3 tests)
D7: Release build verification (1 test)
D8: M1 scope closure (full integration test suite, 2+ tests)
```

Final commit (after all tests pass):
```
M1: Final verification – 153 tests passing, all evidence files recorded.
```

---

## Known Constraints & Assumptions

1. **No Docker required**: All tests use `ALLOW_SKIP_INTEGRATION=1` and stub external services (Postgres optional, Redis optional).
2. **Determinism**: Existing `simulation_determinism_test.rs` proves game engine determinism; security tests focus on protocol and session management.
3. **Placeholder implementations OK**: Each test may use simple stub game state (counter ticks, not complex physics).
4. **No rollback engine rewrites**: All tests use existing `RollbackEngine` and simulation code.
5. **UDP on localhost only**: No multi-host or firewall testing; single machine assumptions.
6. **Token expiry on fake clock**: Tests use `chrono::Utc::now()` or injected fake clock; wall-clock time is acceptable for timeouts (±1s margin).

---

## Risk Mitigations

| Risk | Mitigation |
|------|-----------|
| UDP server hangs (C6) | Use `tokio::time::timeout(10s)` on all client/server tasks. |
| Flaky timing tests (D4, D5) | Use fake clock where possible; otherwise add ±100ms margin to asserts. |
| Rate limiter thrashing (D4) | Simple in-memory sliding window; no persistence needed. |
| Postgres unavailable (D5) | Test can gracefully skip or use in-memory repo; mark with `#[ignore]` if needed. |
| Release build slow (D7) | Run once locally, optionally skip in CI (use `#[ignore]`). |
| Integration test complexity (D8) | Break into 2 simpler tests (happy path + disconnect); extend if time permits. |

---

## Summary

- **Total tasks**: 9 (C6, D1–D8)
- **Total tests**: 24–25 new (5+3+3+3+2+2+3+1+2–3)
- **Baseline**: 128 passing → **Target**: 152–153 passing
- **Files to create**: 9 test files (c6_e2e_udp_test.rs, d1_replay_defense_test.rs, ..., d8_m1_integration_test.rs)
- **Files to modify**: ~6 source files (protocol/sequence.rs, desync/detector.rs, session/repository.rs, network/rate_limiter.rs, network/udp_server.rs, session/cleanup.rs)
- **Evidence files**: 9 (docs/evidence/{c6_e2e_udp, d1_replay_defense, d2_desync_detection, d3_session_hijack_defense, d4_dos_mitigation, d5_orphan_cleanup_e2e, d6_protocol_version, d7_release_build, d8_m1_integration}.txt)
- **Commit strategy**: 1 per task (9 commits total)
- **No Docker required**: All tests pass with `ALLOW_SKIP_INTEGRATION=1 cargo test --all`

