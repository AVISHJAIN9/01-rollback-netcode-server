# Backend: Rollback Netcode Engine (Rust)

## Architecture Overview
The backend is a high-performance, asynchronous, server-authoritative game server built in Rust with Tokio. It provides deterministic physics simulation, snapshot circular buffers, rollback resimulation, and redundant UDP packet processing.

## Submodules
- `src/math/`: Q16.16 Fixed-point arithmetic (`FixedI32`) ensuring bit-exact determinism across CPU architectures.
- `src/protocol/`: Binary UDP packet serializers and 5-frame redundant input bundling.
- `src/simulation/`: 2D arena kinematic simulation, AABB collisions, and `RingBuffer<WorldState, 128>`.
- `src/network/`: Tokio asynchronous UDP socket event loop and NTP clock offset estimator.
- `src/desync/`: Blake3 state hash verification and forensic diff engine.

## Building & Testing
```bash
cargo build
cargo test
cargo bench
```
