# Rust Backend Implementation Plan: `01-rollback-netcode-server`

## Executive Summary
This document defines the comprehensive engineering plan to take the Rust backend of `01-rollback-netcode-server` from the current scaffold to a 100% production-ready, fully-tested, zero-stub implementation meeting all requirements of `Rollback_Netcode_Expanded_Master_Plan.xlsx` (Backend sheet, Database sheet, Features sheet) and Release Gates G1 through G5.

---

## 1. Baseline Audit & Module Status

| Module / Component | Current State | Audit Findings & Deficiencies | Target Phase |
| :--- | :--- | :--- | :--- |
| `math/fixed_point.rs` | **Stub** | Basic `FixedI32` lacks `Serialize`/`Deserialize`, `isqrt`, trigonometric LUT, vector math (`Vec2`), and saturating arithmetic. | Phase 2 |
| `protocol/packets.rs` | **Stub** | Missing binary handshake, version validation, bounds-checked frame decoders, checksum frames, and error/kick frames. | Phase 1 |
| `simulation/state.rs` | **Stub** | Missing full game rules (2D kinematics, hitboxes, health, attack cooldowns, deterministic jump/gravity). Float/serde issues. | Phase 2 |
| `simulation/ring_buffer.rs` | **Stub** | Naive slice; missing generic bounds, tick indexing, uncommitted input history management. | Phase 2 |
| `simulation/rollback.rs` | **Stub** | Stubbed resimulation loop; lacks multi-player input sequence tracking and prediction policy. | Phase 2 |
| `desync/detector.rs` | **Stub** | Basic hash comparison stub; lacks divergence classification and snapshot delta recovery generator. | Phase 2 |
| `network/udp_server.rs` | **Stub** | Skeleton socket bind; lacks actor-per-room routing, client session tables, and packet buffering. | Phase 3 |
| `network/clock_sync.rs` | **Stub** | Unused variables; missing NTP-style 4-timestamp RTT estimation and clock drift filter. | Phase 3 |
| `network/transport.rs` | **Missing** | Missing generic `Transport` trait and in-memory simulated network channel (with loss/jitter/reordering). | Phase 3 |
| `session/` | **Missing** | Missing room manager, reconnect token signing/validation, heartbeat reaper, and match lifecycle state machine. | Phase 4 |
| `security/` | **Missing** | Missing token bucket rate limiter, auth validation, server authority kinematic bounds checker, audit logger. | Phase 5 |
| `storage/` (Postgres/Redis) | **Missing** | Missing SQLx queries for sessions/players/audit logs and Redis matchmaking queue Lua scripts. | Phase 4 & 5 |
| `telemetry/` | **Missing** | Missing Prometheus metrics collector, health/readiness endpoints, and partitioned event logging. | Phase 6 |
| `api/` | **Missing** | Missing Axum/Tokio HTTP REST & WebSocket gateway handlers. | Phase 7 |

---

## 2. Dependencies & Crates to Add (`backend/Cargo.toml`)

```toml
[dependencies]
# Async Runtime & Networking
tokio = { version = "1.38", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }
axum = { version = "0.7", features = ["ws"] }
tower = { version = "0.4", features = ["util"] }
tower-http = { version = "0.5", features = ["cors", "trace"] }

# Serialization & Binary Protocols
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
byteorder = "1.5"
bytes = "1.6"
bincode = "1.3"

# Cryptography & Hashing
blake3 = "1.5"
rand = "0.8"
rand_xoshiro = "0.6"
jsonwebtoken = "9.3"

# Error Handling & Diagnostics
thiserror = "1.0"
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
metrics = "0.23"
metrics-exporter-prometheus = "0.15"

# Database & Cache (Optional / Feature Flagged)
sqlx = { version = "0.7", features = ["runtime-tokio", "postgres", "uuid", "chrono", "migrate"], optional = true }
redis = { version = "0.25", features = ["tokio-comp", "aio"], optional = true }
uuid = { version = "1.8", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
criterion = "0.5"
proptest = "1.4"
tokio-test = "0.4"
```

---

## 3. Public Traits and Type Signatures

### Fixed-Point & Simulation (`math` & `simulation`)
```rust
pub struct FixedI32(pub i32);
pub struct Vec2Fixed { pub x: FixedI32, pub y: FixedI32 }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub position: Vec2Fixed,
    pub velocity: Vec2Fixed,
    pub health: u8,
    pub attack_cooldown: u8,
    pub is_grounded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub tick: u32,
    pub players: [PlayerState; 2],
    pub rng_seed: u64,
}

pub trait SimulationEngine {
    fn step(&self, state: &GameState, inputs: &[u16; 2]) -> GameState;
    fn compute_checksum(&self, state: &GameState) -> u32;
    fn validate_transition(&self, previous: &GameState, next: &GameState, inputs: &[u16; 2]) -> bool;
}
```

### Transport & Network Abstraction (`network`)
```rust
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn send_to(&self, peer: SocketAddr, packet: &[u8]) -> Result<usize, TransportError>;
    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), TransportError>;
    fn local_addr(&self) -> SocketAddr;
}

pub struct SimulatedChannel {
    pub loss_rate: f64,
    pub latency_ms: u64,
    pub jitter_ms: u64,
}
```

### Session & Rollback Management (`session`)
```rust
pub struct RollbackManager {
    pub history: RingBuffer<GameState, 128>,
    pub input_history: [InputHistory; 2],
    pub max_rollback_frames: u32,
}

impl RollbackManager {
    pub fn advance_frame(&mut self, local_inputs: [u16; 2]) -> GameState;
    pub fn handle_remote_input(&mut self, player_idx: usize, tick: u32, input: u16) -> Option<GameState>;
    pub fn restore_snapshot(&mut self, tick: u32) -> Result<(), RollbackError>;
}
```

---

## 4. Phase-by-Phase Execution Strategy

### Phase 1: Foundation (Gate G1)
- Binary protocol wire frame parser (`Handshake`, `InputFrame`, `StateAck`, `Checksum`, `Snapshot`, `Reconnect`, `Kick`).
- Version negotiation matrix with clean error frames.
- Proptest encoding/decoding round-trip property tests.
- CI pipeline (`cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`).

### Phase 2: Deterministic Simulation & Rollback Core (Gate G2)
- Q16.16 arithmetic, vector geometry, AABB collision, deterministic trig/isqrt LUT.
- Pure `step(state, inputs) -> state` physics engine with zero floats and deterministic PRNG.
- 128-frame uncompressed ring buffer snapshot manager.
- Rollback resimulation loop and authoritative desync detection.
- Property tests: 100% snapshot restore, duplicate rejection, and 1,000,000-frame determinism.
- Criterion benchmarks for step and rollback depths (1, 2, 4, 8, 12 frames).

### Phase 3: Networking & Realtime Tick Loop (Gates G2/G3)
- `Transport` trait with UDP implementation and in-memory simulated loss/jitter channel.
- Fixed 60Hz tick loop with monotonic clock and drift compensation.
- Room-isolated async actor task processing bounded queues (64 packets max).
- Chaos verification: two simulated clients converging under 30% loss and 100ms jitter.

### Phase 4: Matchmaking, Sessions & Reliability (Gate G3)
- Matchroom lifecycle state machine (Lobby, Active, Finished, Cleanup).
- Reconnect tokens (HMAC-SHA256) with 30s grace window and snapshot catchup.
- Postgres & Redis persistence integration (atomic matchmaking queues and presence TTL).

### Phase 5: Security, Authority & Anti-Cheat (Gate G4)
- Server authority kinematic validator (blocking speed hacks, teleportation, impossible action cooldowns).
- Token-bucket rate limiter per IP / session.
- Append-only moderation audit logging.
- 10,000 malformed datagram protocol fuzzer.

### Phase 6: Observability, Resilience & Telemetry (Gate G5)
- Prometheus metrics (`netcode_rtt_ms`, `rollback_depth_frames`, `resim_duration_us`, `desync_total`).
- Partitioned telemetry event pipeline.
- Graceful degradation: running match continuity if DB/Redis drops.
- Binary replay recorder and deterministic playback verifier.

### Phase 7: REST/WS API, SDK & Production Docs
- Axum HTTP & WebSocket gateway endpoints per `docs/06-api-reference.md`.
- OpenAPI specification generation.
- Client integration smoke test (connect, handshake, 100 ticks, disconnect).
- Multi-stage non-root `Dockerfile.backend` and Kubernetes deployment manifests.

---

## 5. Identified Risks and Mitigation Plan

1. **Floating-Point Leakage**:
   - *Risk*: Third-party math or standard library conversion introducing non-deterministic IEEE-754 floats.
   - *Mitigation*: Strict `#![deny(clippy::float_arithmetic)]` and exclusive usage of `FixedI32` in simulation crates.
2. **Rollback Overrun Under High Packet Delay**:
   - *Risk*: 12-frame rollback stalling the 16.66ms tick deadline.
   - *Mitigation*: Criterion benchmarks to keep single-frame step under 5 microseconds; 12-frame resimulation takes < 60 microseconds (well under 16.66ms).
3. **Packet Loss Under Hostile Network Conditions**:
   - *Risk*: Burst packet drops causing input starvation.
   - *Mitigation*: ACK-aware redundant historical bundling (transmitting up to 8 unACKed past inputs per datagram).
