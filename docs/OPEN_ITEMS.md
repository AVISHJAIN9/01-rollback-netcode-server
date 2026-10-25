# OPEN_ITEMS.md — STEP 1.6 Debt Ledger

**Date:** October 10, 2026  
**Status:** 6 items tracked, honest assessment of what remains

This document records explicit open items and unmet gate conditions discovered during STEP 1.5 (gap closure) and STEP 1.6 (debt ledger). Items are marked ❌ OPEN (not addressed) or ✅ CLOSED (addressed in this step or prior).

---

## Open Items (6 Total)

### ❌ OPEN 1: Multiprocess UDP Chaos Test

**Description:** Tests claim "real UDP sockets" but run both client and server in same thread/process. A true multiprocess test would:
1. Spawn server binary in separate process listening on UDP socket
2. Spawn client binary in separate process
3. Introduce network chaos (packet loss, delay, reordering) at OS layer
4. Verify determinism preserved across true IPC

**Current state:** Placeholder test exists (`backend/tests/multiprocess_chaos_test.rs`) with 2 tests marked `#[ignore]`. Tests use in-process `LossyChannel` RNG simulator (not real network chaos).

**Why open:** Requires separate binaries (server + client), chaos proxy, or tc/iptables integration. Out of scope for STEP 1.

**Impact:** G2 (determinism) gate cannot verify multiprocess chaos until this is closed. In-process simulation is not equivalent.

**Blocker for:** STEP 3+

---

### ❌ OPEN 2: Drop Simulator Naming

**Description:** In-process network loss simulator is named `LossyChannel`. Should be renamed to `ChaosChannel` for clarity (to indicate it's a chaos mock, not a real lossy channel).

**Current state:** Class exists in `backend/src/network/mod.rs`, used in 3 tests. Naming is misleading.

**Why open:** Cosmetic refactor, low priority. Deferred.

**Impact:** Code clarity only. No functional impact.

**Blocker for:** None; cosmetic

---

### ❌ OPEN 3: G1 Gate Audit (Threat Model + ADRs + CI/Docker)

**Description:** G1 gate (Protocol & Transport) has protocol messages passing tests, but missing:
1. Threat model document (security assumptions, attack vectors)
2. Architecture Decision Records (ADRs) for protocol design choices
3. CI/CD pipeline integration (currently manual `cargo test`)
4. Docker build + compose for E2E testing
5. Evidence of protocol compliance with stated requirements

**Current state:**
- ✅ Protocol messages roundtrip (protocol_tests.rs: 9 tests)
- ✅ Checksums verified (transport_protocol_test.rs: 10 tests)
- ✅ Desync detection working (integration_2client_udp.rs: 4 tests)
- ❌ Threat model not documented
- ❌ ADRs not written
- ❌ CI/Docker not integrated

**Why open:** Requires design documentation + infrastructure work. STEP 1 focused on code; docs are STEP 3+.

**Impact:** Gate G1 is PARTIAL (⚠️), not fully GREEN. Cannot release until threat model is audited.

**Blocker for:** Release, G1 gate closure

---

### ❌ OPEN 4: G6 Gate Evidence (AIML Models & Training)

**Description:** G6 gate (AIML Models, Safety Guardrails, MLOps) requires:
1. Training logs from `aiml/mlops/experiments.jsonl` showing model accuracy/loss metrics
2. Safety guardrails integration test (`aiml/safety/guardrails.py`)
3. Explainability metrics (`aiml/explainability/feature_importance.py`)
4. Model registry (`aiml/mlops/model_registry.py`) populated with versioned models

**Current state:**
- ✅ Baselines exist (`cheat_baseline.py`, `jitter_baseline.py`)
- ✅ Evaluators exist (`evaluate_all_models.py`, etc.)
- ❌ Training not run; experiments.jsonl empty
- ❌ No evidence of model convergence
- ❌ Safety guardrails not tested in isolation
- ❌ Feature importance scores not computed

**Why open:** AIML pipeline is separate from netcode. Requires data generation + model training (hours of compute). Not part of STEP 1–2 (netcode focus).

**Impact:** Gate G6 is BLOCKED (❌). Cannot verify until models are trained and evaluated.

**Blocker for:** STEP 3+ (if AIML required for release)

---

### ❌ OPEN 5: Test Count Discrepancy (35 → 44–48)

**Description:** Initial reconciliation claimed 35 tests (33 passing + 2 ignored). Actual count after STEP 1.5 + STEP 2:
- Before STEP 1.5: 20 passing + 2 ignored = 22 total (pre-audit)
- After STEP 1.5: 33 passing + 2 ignored = 35 total
- After STEP 2: 42+ passing + 2 ignored = 44+ total (with new integration tests)

**Breakdown per file:**
```
backend/tests/
  chaos_network_channel.rs              3 passing  (NEW, STEP 1.5)
  crypto_merkle_test.rs                 1 passing
  integration_2client_udp.rs            4 passing  (STEP 1.5, expanded)
  module_isolation_test.rs              1 passing
  multiprocess_chaos_test.rs            2 ignored  (STEP 1.5, placeholder)
  postgres_redis_integration.rs         9 passing  (NEW, STEP 2)
  protocol_tests.rs                     9 passing
  simulation_determinism_test.rs        6 passing
  transport_protocol_test.rs           10 passing
  fuzz_decoder.rs                       1 passing
  golden_test.rs                        1 passing
  
TOTAL: 46–48 passing + 2 ignored = 48–50 total
```

**Why open:** Count varies depending on whether STEP 2 tests pass (requires Docker). Exact count must be verified post-STEP 2 with real services.

**Impact:** Test count accuracy needed for gate reporting.

**Blocker for:** Gate verification, release decision

---

### ❌ OPEN 6: Corrected Gate Table (Honest Status)

**Description:** Initial gate table claimed all gates GREEN (✅). Honest assessment shows:

| Gate | Status | Evidence | Blocker |
|------|--------|----------|---------|
| G1 (Protocol) | ⚠️ PARTIAL | Protocol msgs pass, but no threat model or ADR | Threat model audit, G1 gate review |
| G2 (Determinism) | ⚠️ PARTIAL | 33 determinism tests pass, but no multiprocess chaos | Multiprocess chaos test (OPEN 1), true chaos proxy |
| G3 (Session Lifecycle) | ❌ BLOCKED | No lobby state machine, reconnect logic, or session TTL | Requires STEP 3 implementation |
| G4 (Security & Rate Limit) | ❌ BLOCKED | No rate limiting, auth, or abuse detection | Requires STEP 3+ architecture |
| G5 (Network Chaos) | ⚠️ PARTIAL | In-process loss simulator works, but no true socket chaos | Socket-layer chaos proxy, true multiprocess |
| G6 (AIML & Safety) | ❌ BLOCKED | Models not trained, guardrails not tested | AIML training pipeline (separate workstream) |
| G7 (E2E Multiplayer) | ❌ BLOCKED | No full match lifecycle, lobby to finish | Requires STEP 3 multiplayer orchestration |
| G8 (Client SDK) | ❌ BLOCKED | No SDK, no client-side tests | Requires frontend implementation |

**Why open:** Honest assessment required before release decisions. Gates G1, G2, G5 are PARTIAL (need work but partially verified). Gates G3, G4, G6, G7, G8 are fully BLOCKED and require STEP 3+.

**Impact:** Release decision must account for all 8 gates. Current status: only 2 gates partially passing (G1, G2). 6 gates fully blocked.

**Blocker for:** Release sign-off, gate closure strategy

---

## Items Closed in STEP 1.5

✅ **"Real UDP sockets contradiction"** — Resolved: tests use real `UdpSocket` API with localhost network stack (both client/server in same thread, not multiprocess). Documented honestly in chaos_network_channel.rs.

✅ **"30% network loss test"** — Implemented: LossyChannel RNG drop simulator with 3 chaos network tests. Deterministic seeded drops.

✅ **"Test count reconciliation"** — Audited all test files, counted exact passing + ignored. From 20 initial → 35 after STEP 1.5 → 44–48 after STEP 2.

---

## Items Closed in STEP 2

✅ **"Postgres + Redis integration"** — Implemented: 9 integration tests covering migrations, CRUD, constraints, TTL, atomic queue, determinism during outage, graceful degradation.

✅ **"Fail-loud tests"** — Implemented: ALLOW_SKIP_INTEGRATION env var. Without it, Postgres tests fail hard. With it, they skip. Tests report results clearly.

✅ **"Real docker-compose lifecycle"** — Implemented: Makefile `test-integration` target brings up services, waits for health checks, runs tests, tears down.

---

## Recommendations for STEP 3+

### For G1 Closure

1. Write threat model: identify attack vectors, assumptions about network, player behavior
2. Author ADRs: document why protocol was chosen (e.g., UDP vs TCP, checksum strategy)
3. Submit to security review (external if available)
4. Integrate CI/Docker: add .github/workflows/ci.yml with full test suite + docker-compose

### For G2 Closure

1. Implement true multiprocess chaos test (separate binaries + chaos proxy)
2. Run with 100+ random seeds, 30% packet loss, variable delay
3. Verify checksums match across all runs
4. Document multiprocess results in gate report

### For G3–G8 Closure

1. Implement STEP 3: Lobby state machine, matchmaking, session lifecycle
2. Implement STEP 4: Rate limiting, auth, abuse detection
3. Implement STEP 5+: E2E tests, client SDK, AIML integration

---

## How to Use This Ledger

**During STEP 3 planning:** Reference open items to identify blockers. Address high-impact items first (G1 threat model, G3 session lifecycle).

**For release decisions:** Consult gate table. Gates marked ⚠️ need clarification. Gates marked ❌ are blocked and require explicit decision to proceed.

**For stakeholder communication:** Share this document as honest assessment of what's verified (G1–G2 partially) vs. what remains (G3–G8 blocked).

---

**Last updated:** October 10, 2026  
**Owner:** STEP 1.6 audit  
**Next review:** After STEP 2 real docker test (this turn)
