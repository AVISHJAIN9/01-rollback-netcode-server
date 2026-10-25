# ADR-007: Server-Authoritative State and Validation

**Date:** 2026-10-11  
**Status:** ACCEPTED  
**Context:** D1, D4 — Auth/authz integration, server authority validation  
**Decision:** Server is the sole authority on game state; clients submit inputs and receive state corrections

## Problem

Clients have predicted state (optimistic); the server has authoritative state. Conflicts arise:

1. Client sends input with seq=42; server accepted seq=41 → out-of-order detection needed
2. Server rolls back to tick 10; client was at tick 15 → clients must resync
3. ML model suggests Player A is cheating; server must validate before accepting the score

## Decision

Three-tier validation model:

1. **Handshake (Auth/authz):** Client identity + session token verified before any game input
2. **Input Validation (D4):** Each InputFrame checked for:
   - Valid sequence number (> last_acked)
   - Valid replay state (not a repeat of already-executed frame)
   - Bounds checking (input values in legal range, e.g., 0–360 for angle)
3. **State Authority:** Only server state after `Replayed from [X]` tick is considered canonical; clients echo checksums for verification

## Rationale

- **No client-authoritative data:** Score, position, inventory never trust client claims
- **Deterministic sim + checksums:** Server detects divergence immediately
- **Replay safety:** Monotonic seq + ack ensures no input is executed twice
- **ML as advisory:** Model flags suspicious patterns → reviewed before action (never auto-ban)

## Consequences

- Clients must implement backpressure when out-of-sequence
- Input replay history stored per session (memory cost)
- Server load increases with player count (all validation on one machine ⏳ horizontal scaling Phase 2)

## Implemented In Code

**✅** Auth/authz validation: `backend/src/auth/mod.rs`, `backend/src/authz/mod.rs` (14 tests)

**✅** Input frame seq validation: `backend/src/transport/protocol.rs::InputFrame` with seq field

**⏳** Bounds checking per input type: `backend/src/game/mod.rs::execute_input()` — pending D4

**⏳** ML advisory + review: `backend/src/admin/mod.rs::review_incident()` — pending D5

## References

- D1: Auth/authz modules wired into handshake
- D4: Server authority validation tests
- D5: Anti-cheat audit trail (no auto-ban)
