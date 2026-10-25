# ADR 007: Serialization and Wire Protocol Encoding Format

## Status
Accepted

## Context
Standard JSON or bloated protobuf serialization introduces significant packet overhead and CPU parsing latency on 60 Hz realtime UDP connections. To fit within standard MTUs (< 1200 bytes) without packet fragmentation and minimize deserialization overhead, a tight binary wire format is essential.

## Decision
1. **Realtime Datagrams**: Compact custom big-endian / network-byte-order binary encoding with explicit packet headers:
   - `Magic`: 2 bytes (`0x5242` / 'RB')
   - `Version`: 1 byte (`0x01`)
   - `MsgType`: 1 byte (Handshake, InputFrame, Checksum, Snapshot, etc.)
   - `Payload`: Explicit length-bounded, zero-copy parseable byte slice with bounds checking.
2. **Deterministic Checksum**: Blake3 / FNV-1a 32-bit state hashes computed over canonical byte layout.
3. **Control & Admin API**: JSON over HTTP/REST and WebSockets for session creation, admin moderation, and telemetry queries.

## Consequences
- Input datagrams are under 40 bytes per packet including redundant historical inputs.
- Microsecond decode latency on the hot path.
- Malformed datagrams are rejected immediately without CPU exhaustion or memory allocations.
