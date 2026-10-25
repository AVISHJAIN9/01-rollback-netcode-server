# 02 - Requirements Specification: Rollback Netcode Server

## 1. Functional Requirements (FR) [Specified]

### 1.1 Deterministic Simulation Engine
- **FR-001**: The engine MUST execute all physics, kinematics, and bounding-box collision math using 32-bit fixed-point arithmetic (`FixedI32` with 16-bit fractional precision).
- **FR-002**: The engine MUST maintain a strictly seeded Pseudo-Random Number Generator (PRNG using PCG32 or SplitMix64) that steps deterministically with each frame tick.
- **FR-003**: Entity updates MUST execute in an absolute, stable iteration order sorted by unique 32-bit Entity ID.
- **FR-004**: The engine MUST support serialization and deserialization of the entire `WorldState` struct to and from canonical byte buffers without heap allocations.

### 1.2 Snapshot Storage & Ring Buffer
- **FR-005**: The system MUST allocate a fixed-capacity circular ring buffer capable of holding at least 128 consecutive historical `WorldState` snapshots and their associated Blake3 64-bit checksums.
- **FR-006**: Ring buffer indexing MUST be $O(1)$ and lockless or use low-contention atomic sequence numbers.
- **FR-007**: State lookups for any frame $F$ where $(F_{current} - 128) \le F \le F_{current}$ MUST return the exact immutable snapshot recorded at frame $F$.

### 1.3 Rollback & Resimulation
- **FR-008**: When a remote input packet for frame $F_{remote}$ arrives where $F_{remote} < F_{current}$, the engine MUST rewind its active state to $F_{remote}$, inject the newly confirmed remote input, and resimulate every tick forward up to $F_{current}$.
- **FR-009**: If multiple remote inputs arrive simultaneously out of order, resimulation MUST occur from the earliest unconfirmed frame $\min(F_{remote})$ in a single contiguous batch.
- **FR-010**: During rollback resimulation, audio triggers, visual particle spawns, and non-stateful events MUST be masked to prevent duplicate sensory artifacts.

### 1.4 Network Protocol & Clock Sync
- **FR-011**: UDP datagrams MUST bundle the current frame's input bitmask along with the preceding 4 historical frames ($F-1, F-2, F-3, F-4$) to provide resilient input transmission over lossy networks.
- **FR-012**: The client and server MUST exchange high-resolution NTP-style ping/pong packets every 500 ms to compute Round-Trip Time (RTT), clock offset $	heta$, and network jitter $\sigma$.
- **FR-013**: The client MUST dynamically adjust its local clock speed by $\pm 1\%$ to seamlessly synchronize its simulation frame timeline with the server authority.

### 1.5 Desync Detection & Telemetry
- **FR-014**: The client MUST compute a 64-bit Blake3 hash of its `WorldState` on every confirmed frame and transmit it to the server.
- **FR-015**: When a hash mismatch occurs between client and server for frame $F$, the server MUST flag a `DesyncIncident`, log the divergence, and snapshot both states for forensic diffing.

### 1.6 Artificial Intelligence & Machine Learning (AIML)
- **FR-016 (Tier A)**: The system MUST execute an unsupervised Isolation Forest anomaly detection model on sliding 60-frame input feature vectors to flag macro usage and aimbot inputs.
- **FR-017 (Tier A)**: The system MUST evaluate an Adaptive Kalman Filter on network jitter timeseries to predict upcoming jitter spikes and dynamically adjust client input delay.
- **FR-018 (Tier B)**: The system MUST provide an LLM-assisted endpoint (`/api/v1/ai/explain-desync`) that receives binary state diffs and outputs structured JSON containing root-cause analysis, affected entities, and remediation recommendations.
- **FR-019**: All AIML modules MUST support graceful degradation: if AI services are offline or disabled, the server MUST revert to hardcoded static threshold rule engines with zero interruption to active gameplay.

---

## 2. Non-Functional Requirements (NFR) [Specified]

### 2.1 Performance & Latency
- **NFR-001**: Single-frame deterministic simulation step duration MUST be under 50 microseconds for an arena containing 64 dynamic entities on a single CPU core.
- **NFR-002**: UDP packet processing overhead on the server MUST not exceed 500 microseconds per client tick under a load of 1,000 concurrent active sessions.
- **NFR-003**: Telemetry ingestion pipeline MUST handle up to 50,000 input events per second with $p99$ persistence latency under 10 ms into PostgreSQL.

### 2.2 Reliability & Availability
- **NFR-004**: Zero panic tolerance: no malformed UDP datagram, invalid bitmask, or corrupted client checksum may crash the server process.
- **NFR-005**: The server MUST gracefully disconnect unreachable clients after 3.0 seconds of contiguous packet loss without deadlocking the match room.

### 2.3 Security & Integrity
- **NFR-006**: The server MUST validate all client inputs against maximum feasible acceleration vectors (e.g. max velocity $\le 10.0$ units/tick) prior to admitting inputs to the authoritative ring buffer.
- **NFR-007**: Matchmaking and telemetry REST APIs MUST authenticate clients via cryptographically signed JWT tokens (RS256 or HS256).
