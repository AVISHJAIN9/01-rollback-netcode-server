# 03 - System Architecture: Rollback Netcode Server

## 1. System Topology Overview [Specified]

```mermaid
graph TD
    subgraph Client Space
        CP[Client Prediction & Renderer]
        CRB[Client 128-Frame Ring Buffer]
        CROLL[Client Rollback Controller]
        CUDP[Client UDP Socket]
        CP --> CRB
        CROLL --> CRB
        CUDP --> CROLL
    end

    subgraph Network Pipe
        UDP_CONN[Lossy UDP Transport (Bundled Inputs)]
        WS_CONN[WebSocket Telemetry & Diagnostics]
    end

    subgraph Server Space
        SUDP[Server UDP Event Loop]
        SRB[Authoritative Ring Buffer]
        SROLL[Authoritative Rollback & Sim]
        SDESYNC[Blake3 Desync Detector]
        SUDP --> SROLL
        SROLL --> SRB
        SRB --> SDESYNC
    end

    subgraph Storage & Cache
        PG[(PostgreSQL 16: Input Logs & Desyncs)]
        REDIS[(Redis 7.2: Matchmaking & Lobbies)]
    end

    subgraph AIML Engine
        ISO_FOREST[Tier A: Isolation Forest Cheat Detector]
        KALMAN[Tier A: Jitter & Delay Predictor]
        LLM_EXP[Tier B: LLM Desync Explainer]
    end

    CUDP <--> UDP_CONN <--> SUDP
    CP <--> WS_CONN <--> SUDP
    SROLL --> PG
    SUDP --> REDIS
    SDESYNC --> PG
    SDESYNC --> LLM_EXP
    SUDP --> ISO_FOREST
    CUDP --> KALMAN
```

## 2. Component Breakdown [Specified]

### 2.1 Backend Core Engine (Rust)
- **Fixed-Point Physics Kernel (`src/math/fixed_point.rs`)**: Provides bit-exact integer mathematics (`FixedI32`). Eliminates floating-point non-determinism across platforms.
- **State Ring Buffer (`src/simulation/ring_buffer.rs`)**: Statically allocated 128-slot circular memory buffer. Stores raw uncompressed `WorldState` snapshots and 64-bit Blake3 checksums.
- **Rollback Resimulation Controller (`src/simulation/rollback.rs`)**: Coordinates state rewind, input queue injection, and deterministic multi-tick fast-forwarding.
- **UDP Network Manager (`src/network/udp_server.rs`)**: Asynchronous Tokio-based UDP socket handler parsing custom bitpacked datagrams with historical frame bundling.
- **Clock Synchronizer (`src/network/clock_sync.rs`)**: NTP-style latency probe calculator estimating clock skew, one-way network transit time, and jitter variance.
- **Desync Detector (`src/desync/detector.rs`)**: Compares client-reported checksums against authoritative server snapshots. Generates binary differential dumps on divergence.

### 2.2 Frontend Client & Visualization (Next.js 14 + TypeScript)
- **Hero Landing Page (`app/page.tsx`)**: 3D interactive hero with GSAP ScrollTrigger and Lenis smooth momentum scrolling.
- **Live Netcode Dashboard (`app/dashboard/page.tsx`)**: Real-time visualization of snapshot ring buffer occupancy, frame rollback frequency, jitter gauges, and network RTT.
- **2D Arena Canvas (`app/components/ArenaCanvas.tsx`)**: WebGL/Canvas2D deterministic client rendering predicted local entities and server ghost positions.
- **AI Diagnostics Studio (`app/ai-insights/page.tsx`)**: Interactive cheat detection scatter plots, Kalman jitter tracking, and natural language desync diff reports.

### 2.3 Storage Layer
- **PostgreSQL 16**: Relational schema storing full session input logs (for deterministic replay playback), match metadata, and binary desync incident reports.
- **Redis 7.2**: In-memory sorted sets for low-latency MMR matchmaking pools, room state leases, and ephemeral session tokens.
