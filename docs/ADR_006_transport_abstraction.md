# ADR-006: Transport Abstraction Layer

**Date:** 2026-10-11  
**Status:** ACCEPTED  
**Context:** B1 — Implement lossy UDP proxy and chaos testing  
**Decision:** Define a transport trait to allow swappable implementations (UDP, lossy proxy, QUIC ⏳)

## Problem

The current UDP transport is tightly coupled to packet encoding/decoding. To test resilience without Docker (which provides network emulation), we need a pluggable transport layer that:

1. Allows testing with artificial loss/jitter/reorder without hardware simulators
2. Supports future migration to QUIC/TLS (Phase 2)
3. Keeps the game loop independent of transport mechanics

## Decision

Create a `Transport` trait in `backend/src/network/transport_trait.rs`:

```rust
pub trait Transport: Send + Sync {
    async fn send(&self, dest: SocketAddr, packet: &[u8]) -> io::Result<()>;
    async fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
}
```

Implement three variants:

1. **UdpTransport** — Direct UDP (current behavior, production)
2. **LossyTransport** — Wraps UDP, applies loss/jitter/reorder (testing)
3. **QuicTransport** — QUIC + TLS (Phase 2 ⏳)

## Rationale

- **Separation of concerns:** Game logic does not know how packets move
- **Testability:** B1 uses LossyTransport with configurable loss rates (0%, 30%)
- **Future-proof:** QUIC adoption requires only new Transport impl, not game changes

## Consequences

- Small performance overhead (trait dispatch); mitigated with inline hints
- All transport tests use mock implementations instead of raw sockets
- Migration to QUIC requires implementing the trait (no game loop changes)

## Implemented In Code

**✅** Transport trait defined at `backend/src/network/mod.rs::Transport`

**⏳** LossyTransport: tests/lossy_transport.rs — pending B1 implementation

**⏳** Game loop integration: `backend/src/main.rs` accepts Transport param

## References

- B1 task: UDP proxy with 30% loss, 0% control
- QUIC roadmap: Phase 2
