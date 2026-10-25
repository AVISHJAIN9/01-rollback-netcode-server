# ADR 008: Snapshot Retention Strategy (Full Ring Buffer vs Delta Resync)

## Status
Accepted

## Context
Rollback resimulation requires instant restoration of past game states when late or mispredicted inputs arrive. During normal gameplay, rollback depth is typically 1 to 8 frames (rarely up to 15 frames). However, upon complete client desynchronization or reconnection, a full state restoration is needed.

## Decision
1. **Local Ring Buffer (128 ticks)**: The simulation maintains a circular ring buffer of 128 full state snapshots. Every tick snapshot is stored in uncompressed fixed-size memory (~256 bytes per state), allowing $O(1)$ instantaneous clone and restore for rollbacks.
2. **Periodic Golden Checkpoints**: Every 60 ticks (1 second), an authoritative state checksum is broadcasted for divergence detection.
3. **Desync / Reconnect Recovery**: If a client diverges beyond the 128-tick window or fails checksum verification, the server transmits a full authoritative snapshot packet followed by current pending inputs to force-resynchronize the client.

## Consequences
- Fixed, predictable memory overhead (~32 KB per session).
- Zero compression CPU overhead during high-speed 60Hz tick simulation.
- Guaranteed bit-for-bit state recovery under packet loss or late arrival.
