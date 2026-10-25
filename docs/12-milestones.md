# 12 - 8-Week Implementation Milestones: Rollback Netcode Server

## Week 1: Deterministic Physics & Fixed-Point Core [Specified]
- Implement `FixedI32` math primitives and geometric AABB collision structures.
- Implement 2D arena kinematic update loop.
- Pass `TEST-DET-01` (1M step determinism test).

## Week 2: Snapshot Ring Buffer & Serializer [Specified]
- Implement zero-allocation `RingBuffer<WorldState, 128>`.
- Implement SIMD-accelerated Blake3 state hashing.
- Benchmark snapshot copy times.

## Week 3: Rollback Resimulation Engine [Specified]
- Implement input history buffer and late input injection.
- Implement rewind-and-resimulate loop.
- Pass `TEST-ROL-01`.

## Week 4: UDP Protocol & Clock Synchronization [Specified]
- Build Tokio asynchronous UDP event loop.
- Implement redundant input bundling protocol.
- Implement NTP clock offset estimator and jitter tracking.

## Week 5: Telemetry Pipeline & Database Storage [Specified]
- Configure PostgreSQL schema with partitioned input logs.
- Set up Redis matchmaking queue and lobby manager.
- Implement WebSocket telemetry streaming.

## Week 6: Frontend 3D Landing & Live Dashboard [Specified]
- Build Next.js 14 frontend with TailwindCSS.
- Create 3D hero sequence with Lenis smooth scroll and GSAP.
- Build live SVG ring buffer visualizer and WebGL arena client.

## Week 7: AIML Cheat Detector & Jitter Predictor [Specified]
- Train and package Isolation Forest cheat detection model.
- Implement 1D Kalman Filter jitter delay regulator.
- Write `aiml/MODEL_CARD.md` and evaluation benchmarks.

## Week 8: LLM Desync Explainer, Benchmarks & Hardening [Specified]
- Integrate Tier B LLM Desync Explainer with schema validation.
- Run load tests comparing against lockstep baseline.
- Package Docker images and Kubernetes manifests.
