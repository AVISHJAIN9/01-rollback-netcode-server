# Implementation Plan: Rollback Netcode Server (Production Grade)

## 1. Executive Summary & Audit of Repository State

This document establishes the master implementation blueprint for **`01-rollback-netcode-server`**, taking the project from its initial scaffold to a complete, fully tested, production-ready system. Every deliverable defined in **`Rollback_Netcode_Expanded_Master_Plan.xlsx`** (90 work items across Frontend, Backend, AI/ML, Database, and Features) and documented in `docs/01` through `docs/13` is mapped to concrete source modules and governed by Release Gates **G1 through G8**.

### Audit Summary:
| Workstream | Total Items | Current Status | Key Missing / Stubbed Elements |
| :--- | :---: | :--- | :--- |
| **Backend (Rust)** | 18 | Scaffolded (Stubs with `todo!()`) | Service boundaries, full tick loop, complete transport abstraction (UDP + WebSocket fallback), NAT traversal, session allocation, reconnect tokens, rate limiting, moderation APIs, metrics/tracing, replay & spectator modes. |
| **Frontend (Next.js/TS)** | 13 | Partially Implemented | App shell routing, state store isolation, connection setup screen, match lobby, admin dashboard, settings/remapping/accessibility, client telemetry, Playwright E2E test suite. |
| **AI/ML (Python)** | 12 | Basic Evaluation Scripts | Unified reproducible feature pipeline, 6 full models with baselines (Jitter, Input Delay, Rollback Risk, Capacity Forecasting, Matchmaking, Cheat Detection), MLOps registry with rollback, explainability, safety guardrails & kill switch. |
| **Database (Postgres/Redis)** | 14 | Initial Schema | Partitioned telemetry, append-only audit log constraints, Redis presence, atomic matchmaking queues, rate-limit counters, automated backup/PITR drill scripts, retention policies. |
| **Features & DevOps** | 33 | Scaffolded | Complete CI/CD security scanning, reproducible `make setup && make test && make dev`, threat model & ADR documentation (`docs/adr/`), chaos test harness, security fuzzing, incident runbooks, version compatibility matrix. |

---

## 2. Complete Gap Analysis & Deliverables Mapping

Every row from `Rollback_Netcode_Expanded_Master_Plan.xlsx` is mapped below to target source files and verification methods:

### 2.1 Backend Workstream (18 Rows)
| Row ID | Area | Deliverable | Priority | Phase | Target File(s) | Verification / Test Method |
| :--- | :--- | :--- | :---: | :--- | :--- | :--- |
| **BE-01** | Architecture | Service Boundaries | P0 | Foundation | `backend/src/lib.rs`, `backend/src/gateway/`, `backend/src/session/` | Architecture dependency test |
| **BE-02** | Networking | Protocol | P0 | Foundation | `backend/src/protocol/packets.rs`, `shared/protocol/packets.ts` | Handshake & versioning unit tests |
| **BE-03** | Networking | Transport Abstraction | P0 | MVP | `backend/src/network/transport.rs`, `backend/src/network/udp.rs`, `backend/src/network/ws.rs` | Socket round-trip latency tests |
| **BE-04** | Networking | Tick Loop | P0 | Core | `backend/src/simulation/tick_loop.rs` | Monotonic clock & deadline benchmark |
| **BE-05** | Rollback | Snapshot System | P0 | Core | `backend/src/simulation/ring_buffer.rs`, `shared/simulation/ring_buffer.ts` | 128-frame ring buffer stress test |
| **BE-06** | Rollback | Input History | P0 | Core | `backend/src/simulation/input_queue.rs` | Out-of-order & duplicate input test |
| **BE-07** | Rollback | Resimulation | P0 | Core | `backend/src/simulation/rollback.rs`, `shared/simulation/rollback.ts` | Rollback correctness test against linear pass |
| **BE-08** | Sync | Checksums & Desync Recovery | P0 | Core | `backend/src/desync/detector.rs`, `backend/src/desync/recovery.rs` | Checksum divergence & recovery test |
| **BE-09** | Matchmaking | Match Queue | P1 | MVP | `backend/src/matchmaking/queue.rs`, `server/src/matchmaking/` | Region/MMR bucket queue test |
| **BE-10** | Matchmaking | Session Allocation | P0 | MVP | `backend/src/matchmaking/session.rs` | Room allocation & lifecycle test |
| **BE-11** | Reliability | Reconnect | P1 | Beta | `backend/src/session/reconnect.rs` | Disconnect and token recovery test |
| **BE-12** | Security | Authentication | P0 | Security | `backend/src/security/auth.rs` | JWT verification, expiry & rotation test |
| **BE-13** | Security | Authorization | P0 | Security | `backend/src/security/rbac.rs` | Role & resource RBAC test suite |
| **BE-14** | Security | Rate Limiting | P0 | Security | `backend/src/security/rate_limit.rs` | Token bucket & sliding window load test |
| **BE-15** | Anti-cheat | Server Authority | P0 | Security | `backend/src/simulation/authority.rs` | Impossible kinematic transition rejection |
| **BE-16** | Admin | Moderation APIs | P1 | Beta | `backend/src/admin/moderation.rs` | Kick/ban/terminate API tests |
| **BE-17** | Observability | Metrics/Tracing | P0 | Hardening | `backend/src/telemetry/metrics.rs` | Prometheus / OTLP metrics endpoint test |
| **BE-18** | Testing | Load & Soak Testing | P0 | Hardening | `benchmarks/benches/simulation_bench.rs`, `benchmarks/harness/load_generator.py` | 1,000 concurrent room load soak test |

### 2.2 Frontend Workstream (13 Rows)
| Row ID | Area | Deliverable | Priority | Phase | Target File(s) | Verification / Test Method |
| :--- | :--- | :--- | :---: | :--- | :--- | :--- |
| **FE-01** | Architecture | App Shell & Routing | P0 | Foundation | `frontend/app/layout.tsx`, `frontend/app/error.tsx`, `frontend/app/components/` | Route navigation & error boundary test |
| **FE-02** | Architecture | State Management | P0 | Foundation | `frontend/app/stores/uiStore.ts`, `networkStore.ts`, `simStore.ts` | Isolated store mutation tests |
| **FE-03** | UX | Connection Setup | P0 | MVP | `frontend/app/connect/page.tsx` | Validation UI test |
| **FE-04** | UX | Match Lobby | P0 | MVP | `frontend/app/lobby/page.tsx` | Create/join/ready lifecycle test |
| **FE-05** | Gameplay | Input Capture | P0 | MVP | `frontend/app/components/InputHandler.ts` | Keyboard & Gamepad sampling test |
| **FE-06** | Gameplay | Prediction State | P0 | Core | `frontend/app/arena/page.tsx`, `shared/simulation/rollback.ts` | Prediction reconciliation test |
| **FE-07** | Gameplay | Rollback Debug Overlay | P1 | Debug | `frontend/app/components/RollbackDebugOverlay.tsx` | Live HUD inspection test |
| **FE-08** | HUD | Network Health | P1 | MVP | `frontend/app/components/NetworkHealthHud.tsx` | RTT, jitter, loss & rollback HUD test |
| **FE-09** | Settings | Controls & Accessibility | P1 | Beta | `frontend/app/settings/page.tsx` | Key remapping persistence test |
| **FE-10** | Performance | Render Optimization | P0 | Hardening | `frontend/app/components/ArenaCanvas.tsx` | Frame pacing & zero-allocation render test |
| **FE-11** | Security | Client Validation | P0 | Security | `frontend/app/arena/page.tsx` | Client assertion non-trust test |
| **FE-12** | Observability | Client Telemetry | P1 | Beta | `frontend/app/telemetry/clientTelemetry.ts` | Structured event streaming test |
| **FE-13** | Testing | E2E Client Flow | P0 | Hardening | `frontend/e2e/match_flow.spec.ts` | Playwright connect -> lobby -> match test |

### 2.3 AI/ML Workstream (12 Rows)
| Row ID | Area | Deliverable | Priority | Phase | Target File(s) | Verification / Test Method |
| :--- | :--- | :--- | :---: | :--- | :--- | :--- |
| **AI-01** | Data | Feature Engineering Pipeline | P1 | Research | `aiml/data/feature_pipeline.py` | Leakage-free window transformation test |
| **AI-02** | Model | Network Quality Prediction | P1 | Research | `aiml/models/network_quality_model.py` | Eval vs static baseline on jitter trace |
| **AI-03** | Model | Adaptive Input Delay | P1 | Experimental | `aiml/models/adaptive_delay_model.py` | Bounded dynamic frame delay eval |
| **AI-04** | Model | Rollback-Risk Prediction | P1 | Experimental | `aiml/models/rollback_risk_model.py` | High-rollback session classification |
| **AI-05** | Model | Capacity Forecasting | P2 | Beta | `aiml/models/capacity_forecaster.py` | Time-series match volume forecast |
| **AI-06** | Model | Matchmaking Optimization | P2 | Research | `aiml/models/matchmaking_optimizer.py` | Latency/MMR pairing simulation |
| **AI-07** | Anomaly | Cheat/Anomaly Detection | P0 | Experimental | `aiml/models/isolation_forest_cheat.py` | F1 score vs baseline (>0.94 target) |
| **AI-08** | Explainability | Model Explanations | P2 | Beta | `aiml/eval/explainability.py`, `frontend/app/ai-insights/` | Feature importance & driver extraction |
| **AI-09** | MLOps | Experiment Tracking | P1 | Research | `aiml/mlops/experiment_tracker.py` | Model versioning & metric logging |
| **AI-10** | MLOps | Model Registry | P2 | Beta | `aiml/mlops/model_registry.py` | Promotion & <5min rollback drill |
| **AI-11** | Safety | ML Guardrails & Kill Switch | P0 | Experimental | `aiml/safety/guardrails.py` | Bounds, fallback & kill switch test |
| **AI-12** | Model Card | Comprehensive Model Cards | P0 | Foundation | `aiml/MODEL_CARD.md` | Complete model cards per specification |

### 2.4 Database Workstream (14 Rows)
| Row ID | Area | Deliverable | Priority | Phase | Target File(s) | Verification / Test Method |
| :--- | :--- | :--- | :---: | :--- | :--- | :--- |
| **DB-01** | Architecture | Data Model | P0 | Foundation | `database/schema.sql`, `database/migrations/` | Schema validation |
| **DB-02** | PostgreSQL | Users Schema | P0 | MVP | `database/migrations/002_users.sql` | User CRUD & credential hash tests |
| **DB-03** | PostgreSQL | Matches Schema | P0 | MVP | `database/migrations/003_matches.sql` | Match participant & score integrity |
| **DB-04** | PostgreSQL | Sessions Schema | P0 | MVP | `database/migrations/004_sessions.sql` | Room lifecycle & heartbeat test |
| **DB-05** | PostgreSQL | Audit Logs Schema | P0 | Security | `database/migrations/005_audit_logs.sql` | Append-only security log verification |
| **DB-06** | Redis | Presence | P0 | MVP | `backend/src/storage/redis_presence.rs` | TTL expiration test |
| **DB-07** | Redis | Match Queues | P0 | MVP | `backend/src/storage/redis_queues.rs` | Atomic FIFO & sorted set test |
| **DB-08** | Redis | Rate-Limit Counters | P0 | Security | `backend/src/storage/redis_ratelimit.rs` | Atomic distributed counter test |
| **DB-09** | Performance | Composite Indexes | P0 | Hardening | `database/migrations/006_indexes.sql` | Query execution plan inspection |
| **DB-10** | Performance | Telemetry Partitioning | P1 | Scale | `database/migrations/007_partitioning.sql` | Range partitioning benchmark |
| **DB-11** | Reliability | Automated Backups & PITR | P0 | Production | `database/scripts/backup_pitr.sh` | Backup & restore drill verification |
| **DB-12** | Reliability | Replication / Failover | P1 | Production | `database/scripts/failover_test.sh` | Read replica failover simulation |
| **DB-13** | Privacy | Retention & Deletion | P0 | Security | `database/scripts/retention_cleanup.sh` | Automatic purge test |
| **DB-14** | Security | Encryption & Secret Mgmt | P0 | Security | `database/scripts/verify_encryption.sh` | Zero plaintext secret audit |

### 2.5 Features, DevOps, QA & Operations Workstream (33 Rows)
| Row ID | Area | Deliverable | Priority | Phase | Target File(s) | Verification / Test Method |
| :--- | :--- | :--- | :---: | :--- | :--- | :--- |
| **FT-01** | Core | Account & Identity | P0 | MVP | `server/src/auth/`, `frontend/app/login/` | Auth flow test |
| **FT-02** | Core | Create Match | P0 | MVP | `server/src/matchmaking/` | Room creation test |
| **FT-03** | Core | Join Match | P0 | MVP | `server/src/matchmaking/` | Capacity & invite code test |
| **FT-04** | Core | Rollback Engine | P0 | Core | `backend/src/simulation/rollback.rs` | Resimulation test suite |
| **FT-05** | Core | Deterministic Sim | P0 | Core | `tests/test_determinism.py` | 1,000,000 frame determinism test |
| **FT-06** | Core | Desync Recovery | P0 | Core | `backend/src/desync/` | State resync packet injection test |
| **FT-07** | Network | Latency Monitor | P1 | MVP | `frontend/app/components/NetworkHealthHud.tsx` | Live HUD metric update test |
| **FT-08** | Network | Adaptive Networking | P1 | Beta | `backend/src/network/adaptive.rs` | Policy benchmark under jitter |
| **FT-09** | Network | NAT Traversal | P1 | Beta | `backend/src/network/nat.rs` | STUN / ICE / Relay strategy doc & test |
| **FT-10** | Network | Region Selection | P1 | MVP | `frontend/app/connect/page.tsx` | Ping-based auto region picker test |
| **FT-11** | Security | Anti-Cheat | P0 | Security | `backend/src/security/anticheat.rs` | Telemetry anomaly flag test |
| **FT-12** | Security | DDoS Resilience | P0 | Production | `infra/k8s/rate-limit.yaml`, `backend/src/security/ddos.rs` | SYN/UDP flood resilience test |
| **FT-13** | Reliability | Reconnect | P1 | Beta | `server/src/session/reconnect.ts` | Session recovery within 30s window |
| **FT-14** | Reliability | Graceful Degradation | P0 | Production | `backend/src/circuit_breaker.rs` | Sim survival during DB outage |
| **FT-15** | Operations | Admin Dashboard | P1 | Beta | `frontend/app/admin/page.tsx` | Session termination & ban UI test |
| **FT-16** | Operations | Observability Dashboard | P0 | Production | `infra/grafana/dashboard.json` | SLO & p95 latency dashboard |
| **FT-17** | Testing | Chaos Testing | P0 | Hardening | `tests/chaos_network_test.py` | 30% drop, 50ms jitter test |
| **FT-18** | Testing | Determinism Suite | P0 | Core | `tests/golden_determinism_test.rs` | Golden input replay suite |
| **FT-19** | Testing | Security Suite | P0 | Security | `tests/security_fuzz_test.py` | Malformed packet fuzzing suite |
| **FT-20** | Product | Replay System | P2 | Beta | `backend/src/replay/`, `frontend/app/replay/` | Input playback matching 100% |
| **FT-21** | Product | Spectator Mode | P2 | Beta | `server/src/spectator/` | Read-only zero-mutation observer |
| **FT-22** | Product | Client API / SDK | P1 | Beta | `sdk/typescript/`, `sdk/rust/` | SDK smoke integration test |
| **FT-23** | Product | Feature Flags | P0 | Production | `backend/src/config/flags.rs` | Dynamic toggle without redeploy |
| **FT-24** | Product | Version Compatibility | P0 | Production | `backend/src/protocol/version.rs` | Compatibility matrix test |
| **FT-25** | Architecture | Threat Model | P0 | Foundation | `docs/threat-model.md` | Asset & trust boundary analysis |
| **FT-26** | Architecture | ADRs | P0 | Foundation | `docs/adr/001-transport.md` through `004` | Recorded architectural decisions |
| **FT-27** | DevOps | CI/CD | P0 | Foundation | `.github/workflows/ci.yml` | Lint, test, audit, scan pipeline |
| **FT-28** | DevOps | Containers | P0 | Foundation | `Makefile`, `infra/docker-compose.yml` | `make setup && make test && make dev` |
| **FT-29** | DevOps | Staging / Production | P0 | Production | `infra/k8s/` | Reversible deployment manifests |
| **FT-30** | Documentation | Engineering Docs | P0 | Beta | `docs/`, `README.md` | Quickstart & runbooks |
| **FT-31** | Privacy | Data Lifecycle | P1 | Production | `docs/data-lifecycle.md` | Consent, export & deletion workflows |
| **FT-32** | Performance | E2E Profiling | P0 | Hardening | `benchmarks/profiling_report.md` | p95 latency budget validation |
| **FT-33** | Recovery | Incident Playbooks | P0 | Production | `docs/playbooks/` | Outage & rollback runbooks |

---

## 3. Ordered Execution Plan by Release Gate (G1 to G8)

### 🔹 Gate G1: Foundation & Threat Model (Week 1)
- **Goal**: Establish project toolchain, CI/CD, container environments, protocol versioning, threat modeling, and ADRs.
- **Tasks**:
  1. Fix `Dashboard` sheet formulas in `Rollback_Netcode_Expanded_Master_Plan.xlsx` (Row formulas corrected to dynamically evaluate all 90 items).
  2. Implement protocol versioning in `shared/protocol/packets.ts` and `backend/src/protocol/`.
  3. Create ADRs in `docs/adr/` (`001-transport-layer.md`, `002-state-authority.md`, `003-persistence.md`, `004-ml-guardrails.md`).
  4. Write `docs/threat-model.md` mapping assets, attack surface, and trust boundaries.
  5. Setup `Makefile` with `make setup`, `make test`, `make dev`, `make clean`.
  6. Configure GitHub Actions CI workflow with build, test, `cargo clippy`, `npm run lint`, and security audit.
- **Exit Criteria**: `make setup && make test` succeeds from clean repository; threat model approved; G1 marked **Passed** in spreadsheet.

---

### 🔹 Gate G2: Core Netcode & Deterministic Physics (Week 2)
- **Goal**: Build and verify the deterministic physics simulation, snapshot ring buffer, input history, rollback resimulation, and desync recovery.
- **Tasks**:
  1. Complete `FixedI32`, `Vec2Fixed`, and trigonometry lookups in `backend/src/math/` and `shared/math/`.
  2. Complete `SnapshotRingBuffer` (128 frames) with zero allocations.
  3. Implement `RollbackEngine` in Rust and TypeScript with late input handling and fast-forward resimulation.
  4. Implement continuous Blake3 state hashing and differential desync detection.
  5. Build automated determinism test suite (`tests/test_determinism.py` & `tests/golden_determinism_test.rs`) running 1,000,000 continuous frames.
  6. Implement rollback accuracy verification test comparing rewound execution with linear single pass.
- **Exit Criteria**: 1M frame determinism verified (0 bit differences); rollback resimulation matches 100%; G2 marked **Passed** in spreadsheet.

---

### 🔹 Gate G3: Multiplayer Transport & Session Lifecycle (Week 3)
- **Goal**: Implement server-authoritative multiplayer transport, matchmaking queues, lobby management, reconnect tokens, and spectator mode.
- **Tasks**:
  1. Build multi-transport abstraction: low-latency UDP socket with WebSocket browser fallback.
  2. Implement NTP 4-probe clock synchronization estimating RTT, jitter, and clock skew.
  3. Build Redis-backed matchmaking queue with region latency and MMR clustering.
  4. Implement match lobby (create room, join code, ready state, player slots).
  5. Implement session reconnect tokens with 30-second grace recovery window.
  6. Implement read-only Spectator Mode and match Replay recording system.
- **Exit Criteria**: Two browser clients can join lobby, launch match, and exchange 60 Hz input streams with live rollback prediction; G3 marked **Passed** in spreadsheet.

---

### 🔹 Gate G4: Security, Server Authority & Governance (Week 4)
- **Goal**: Implement token authentication, RBAC authorization, rate limiting, anti-cheat validation, moderation APIs, and protocol fuzzing.
- **Tasks**:
  1. Implement JWT token validation with expiry, rotation, and revocation list.
  2. Implement RBAC authorization (Player vs Spectator vs Admin roles).
  3. Implement distributed rate limiting (Token Bucket & Sliding Window) on API and UDP packet ingress.
  4. Implement server-authoritative kinematics validation (rejecting impossible acceleration or speed hacking).
  5. Implement Moderation APIs (Kick, Ban, Terminate Session) with append-only database audit logs.
  6. Run security suite & protocol fuzzer (`tests/security_fuzz_test.py`) with zero panics.
- **Exit Criteria**: 0 critical security findings; 0 plaintext secrets; malformed packets rejected cleanly; G4 marked **Passed** in spreadsheet.

---

### 🔹 Gate G5: Database, Reliability & Chaos Hardening (Week 5)
- **Goal**: Implement full PostgreSQL partitioned schema, Redis caching, backup/PITR drill, and chaos network tests.
- **Tasks**:
  1. Execute PostgreSQL migrations: `users`, `matches`, `sessions`, partitioned `telemetry`, and `audit_logs`.
  2. Configure Redis presence keys, atomic match queues, and rate-limit counters.
  3. Implement automated backup & Point-in-Time Recovery (PITR) script (`database/scripts/backup_pitr.sh`).
  4. Implement data retention and privacy deletion worker (`database/scripts/retention_cleanup.sh`).
  5. Build network chaos test harness injecting 30% packet loss, 50ms latency, and 30ms jitter.
  6. Execute load and soak benchmark test simulating 1,000 concurrent rooms.
- **Exit Criteria**: Chaos test maintains playable simulation; backup restore drill succeeds; G5 marked **Passed** in spreadsheet.

---

### 🔹 Gate G6: AI/ML Subsystem & Guardrails (Week 6)
- **Goal**: Complete the 6 AI models with baselines, MLOps registry, explainability studio, and ML safety guardrails.
- **Tasks**:
  1. Build leak-free feature engineering pipeline (`aiml/data/feature_pipeline.py`).
  2. Build & evaluate **Tier A Isolation Forest Cheat Detector** against fixed threshold baseline (Target F1 > 0.94).
  3. Build & evaluate **Tier A Adaptive Kalman Jitter Predictor** against EWMA baseline (Target MAE < 0.35 frames).
  4. Build **Tier A Rollback Risk Predictor** and **Capacity Demand Forecaster**.
  5. Build **Tier B LLM Desync Explainer** with rigid schema validation.
  6. Implement MLOps experiment tracking, model registry with <5min rollback, and ML kill-switch guardrails.
  7. Update `aiml/MODEL_CARD.md` with empirical evaluation benchmarks.
- **Exit Criteria**: Tier A models beat non-AI baselines; guardrails prevent model errors from affecting simulation correctness; G6 marked **Passed** in spreadsheet.

---

### 🔹 Gate G7: Frontend UI/UX, Observability & E2E Testing (Week 7)
- **Goal**: Polish Next.js frontend with live dashboard, admin UI, settings/remapping, client telemetry, and Playwright E2E tests.
- **Tasks**:
  1. Build connection setup screen (`frontend/app/connect/page.tsx`).
  2. Build match lobby screen (`frontend/app/lobby/page.tsx`).
  3. Build Admin Moderation Dashboard (`frontend/app/admin/page.tsx`).
  4. Build Settings screen with controls remapping and accessibility persistence (`frontend/app/settings/page.tsx`).
  5. Build live Network Health HUD and Rollback Debug Overlay.
  6. Implement client structured telemetry streamer.
  7. Write Playwright E2E test suite covering connect -> lobby -> match -> disconnect -> reconnect.
- **Exit Criteria**: E2E Playwright test suite green; 60 FPS stable canvas rendering; G7 marked **Passed** in spreadsheet.

---

### 🔹 Gate G8: Production Readiness, Runbooks & Release (Week 8)
- **Goal**: Finalize documentation, Kubernetes deployment manifests, incident playbooks, client SDKs, and release verification.
- **Tasks**:
  1. Package Kubernetes manifests with health probes, NodePort/HostPort UDP routing, and horizontal pod autoscaling.
  2. Write Incident Playbooks (`docs/playbooks/outage-recovery.md`, `rollback-procedure.md`).
  3. Generate TypeScript and Rust Client SDKs with integration smoke tests (`sdk/typescript/`, `sdk/rust/`).
  4. Write Client/Server Compatibility Matrix and Data Lifecycle policies.
  5. Verify `make setup && make test && make dev` from clean clone.
  6. Record all gate evidence and KPI metrics in `Rollback_Netcode_Expanded_Master_Plan.xlsx`.
- **Exit Criteria**: All 8 release gates marked Passed with linked evidence; zero TODOs or stubs in codebase; G8 marked **Passed** in spreadsheet.

---

## 4. Architecture & Directory Blueprint

```
01-rollback-netcode-server/
├── PLAN.md                               # This master execution plan
├── Makefile                              # make setup, make test, make dev, make clean
├── README.md                             # Comprehensive technical master documentation
├── Rollback_Netcode_Expanded_Master_Plan.xlsx # Master roadmap, workstreams & release gates
│
├── backend/                              # Rust Server-Authoritative Core Engine
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── main.rs
│   │   ├── math/                         # Q16.16 FixedI32, Vec2Fixed, Trig
│   │   ├── physics/                      # AABB collision & kinematics
│   │   ├── protocol/                     # Bitpacked UDP packets & versioning
│   │   ├── simulation/                   # Tick loop, RingBuffer<128>, RollbackEngine
│   │   ├── network/                      # UDP + WebSocket transport & ClockSync
│   │   ├── matchmaking/                  # Queue, session allocation, lifecycle
│   │   ├── session/                      # Reconnect tokens & state management
│   │   ├── security/                     # JWT auth, RBAC, rate limiting, anti-cheat
│   │   ├── desync/                       # Blake3 hashing & desync recovery
│   │   ├── admin/                        # Moderation APIs (kick, ban, terminate)
│   │   ├── replay/                       # Deterministic match replay recorder
│   │   ├── telemetry/                    # Metrics & OTLP tracing
│   │   └── config/                       # Feature flags & version compatibility
│
├── shared/                               # Isomorphic Core Engine (TypeScript)
│   ├── math/fixed_point.ts
│   ├── physics/aabb.ts
│   ├── protocol/packets.ts
│   └── simulation/                       # state.ts, ring_buffer.ts, rollback.ts
│
├── server/                               # Node.js / TypeScript WebSocket & Matchmaking Service
│   ├── src/
│   │   ├── server.ts
│   │   ├── matchmaking/
│   │   ├── session/
│   │   └── telemetry/
│
├── frontend/                             # Next.js 14 Web Visualizer & Arena Client
│   ├── app/
│   │   ├── layout.tsx
│   │   ├── page.tsx                      # 3D Hero Landing
│   │   ├── arena/page.tsx                # Playable 2D Arena Canvas
│   │   ├── lobby/page.tsx                # Multiplayer Match Lobby
│   │   ├── connect/page.tsx              # Region & Server Connection Setup
│   │   ├── dashboard/page.tsx            # Live Telemetry & Ring Buffer Visualizer
│   │   ├── ai-insights/page.tsx          # AIML Studio & Explainer
│   │   ├── admin/page.tsx                # Admin Moderation Dashboard
│   │   ├── settings/page.tsx             # Controls Remapping & Accessibility
│   │   └── api/                          # Next.js Serverless Route Handlers
│   ├── components/                       # UI HUD, Overlay, Navbar, Canvas
│   ├── stores/                           # Isolated UI, network, sim state stores
│   └── e2e/                              # Playwright E2E test suite
│
├── aiml/                                 # AI/ML Subsystem
│   ├── MODEL_CARD.md                     # Formal model cards
│   ├── requirements.txt
│   ├── data/                             # Feature engineering pipeline
│   ├── baselines/                        # Non-AI comparison baselines
│   ├── models/                           # 6 ML models (Cheat, Jitter, Delay, Risk, Capacity, Match)
│   ├── eval/                             # Evaluation & benchmark scripts
│   ├── mlops/                            # Experiment tracker & model registry
│   └── safety/                           # ML guardrails & kill switch
│
├── database/                             # Database & Persistence Layer
│   ├── schema.sql
│   ├── migrations/                       # 001 through 007 (Users, Matches, Telemetry Partitions, Indexes)
│   └── scripts/                          # Backup PITR, retention cleanup, encryption audit
│
├── sdk/                                  # Client SDKs
│   ├── typescript/
│   └── rust/
│
├── tests/                                # Determinism, Rollback, Chaos & Security Tests
│   ├── test_determinism.py               # 1M frame determinism verification
│   ├── test_rollback.py                  # Rollback accuracy verification
│   ├── chaos_network_test.py             # Network loss & jitter injection
│   └── security_fuzz_test.py             # Malformed datagram fuzzing
│
├── benchmarks/                           # Criterion benches & Load Harnesses
│   ├── benches/simulation_bench.rs
│   └── harness/load_generator.py
│
├── infra/                                # Containers & Orchestration
│   ├── Dockerfile.backend
│   ├── Dockerfile.frontend
│   ├── docker-compose.yml
│   └── k8s/                              # Deployment, Service, StatefulSet
│
└── docs/                                 # Complete Engineering Documentation & ADRs
    ├── 01-vision-and-scope.md through 13-glossary-and-references.md
    ├── threat-model.md
    ├── data-lifecycle.md
    ├── adr/                              # Architecture Decision Records (001 to 004)
    └── playbooks/                        # Outage recovery & rollback runbooks
```

---

## 5. Next Steps & Approval Gate

Upon your review and approval of this plan:
1. We will begin implementation strictly at **Gate G1 (Foundation & Threat Model)**.
2. Maintain green CI and working tests at each commit.
3. Record progress and benchmark evidence in `Rollback_Netcode_Expanded_Master_Plan.xlsx` gate by gate until **G8** is complete.
