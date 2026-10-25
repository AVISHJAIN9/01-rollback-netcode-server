# ADR 006: Asynchronous Runtime and Threading Architecture

## Status
Accepted

## Context
The game server must concurrently manage:
1. High-frequency realtime UDP packet processing (thousands of datagrams per second per session).
2. Fixed-timestep 60Hz tick execution and compute-heavy rollback resimulation.
3. Database persistence, Redis heartbeat/matchmaking operations, and REST/WebSocket client connections.

Blocking async worker threads with rollback compute can cause network jitter and missed tick deadlines.

## Decision
1. **Tokio Multi-Thread Runtime**: Use `tokio::runtime::Builder::new_multi_thread()` with work-stealing for I/O tasks (UDP intake, TCP/WS framing, DB queries, Redis).
2. **Actor-per-Room Session Runtime**: Each active match room is spawned as an isolated Tokio task or dedicated worker loop communicating via bounded `tokio::sync::mpsc` channels with strict backpressure (64 packets max queue).
3. **Pure CPU Simulation Isolation**: Simulation step and rollback resimulation routines are pure synchronous functions executed directly inside the room task, maintaining strictly zero allocation loops in the hot path.

## Consequences
- Network I/O and match simulation are fully decoupled.
- Overrun in one room does not stall packet processing for other rooms.
- High memory locality and predictable cache utilization.
