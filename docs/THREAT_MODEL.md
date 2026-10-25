# Rollback Netcode Server: Threat Model

**Date:** 2026-10-10  
**Version:** 1.0  
**Status:** Final

---

## Executive Summary

This document outlines eight primary threats to the rollback netcode system and the mitigations implemented. The system assumes a **trusted LAN environment** and does not provide encryption; QUIC/TLS integration is deferred to Phase 2.

---

## Threat Analysis

### 1. **UDP Packet Loss & Corruption**

**Threat:**
- Unencrypted UDP packets can be lost on the network or corrupted during transmission.
- Missing packets cause input lag or rollback failures.

**Severity:** HIGH (affects core gameplay)

**Mitigation:**
- 5-frame input redundancy: each InputFrame encoded as 5 consecutive frames at source
- Server reconstructs missing frames if >= 1 copy received
- CRC32 checksums on all packet types; corrupted packets discarded

**Design Location:** `network::mod.rs` (redundancy codec), `transport::protocol.rs` (CRC validation)

**Verification:** `tests/chaos_network_channel.rs` — redundancy survives 30% loss; `tests/integration_2client_udp.rs` — handshake and frame exchange verified

---

### 2. **Replay Attacks**

**Threat:**
- Attacker captures old InputFrame packets and replays them later.
- Server accepts replayed inputs → players run wrong moves → desync.

**Severity:** HIGH (game-breaking cheating vector)

**Mitigation:**
- Monotonic sequence numbers per player (0, 1, 2, ..., wrapping at u32 max).
- Server only accepts inputs with `seq > last_acked_seq`.
- Sequence number included in InputFrame header and validated before execution.

**Design Location:** `transport::protocol.rs` — InputFrame seq field validation

**Verification:** `tests/transport_protocol_test.rs::test_input_frame_roundtrip_with_redundancy` — seq numbers encoded/decoded correctly

---

### 3. **Desync Attacks** (e.g., False-Positive Cheating)

**Threat:**
- Player A sends forged game state that differs from Player B's state.
- Server detects mismatch but Player A claims Player B cheated → unfair rollback.

**Severity:** CRITICAL (breaks integrity)

**Mitigation:**
- **Deterministic simulation:** All players run identical simulation logic on identical inputs.
- **Authoritative checksums:** Server computes state hash after each frame; players echo checksum in StateAck.
- **Mismatch detection:** Server compares checksums; if mismatch → authoritative rollback + resync.
- **AI guardrails:** ML model (if enabled) flags anomalous state patterns before they propagate.

**Design Location:**
- `game::mod.rs` — tick() enforces determinism
- `desync::detector.rs` — checksum computation and mismatch detection
- `aiml/models/isolation_forest_cheat.py` — ML anomaly detection ⏳

**Verification:**
- `tests/simulation_determinism_test.rs::test_forced_desync_detection_and_authoritative_recovery` — desync detected and recovered
- `tests/integration_2client_udp.rs::test_protocol_convergence_100_ticks` — 100-tick convergence verified

---

### 4. **Session Hijacking**

**Threat:**
- Attacker steals a reconnect token and uses it to reconnect as a legitimate player.
- Server grants the attacker access to the player's session state.

**Severity:** HIGH (account takeover equivalent)

**Mitigation:**
- Reconnect tokens include player ID + session ID + issued timestamp.
- Tokens expire after 30 seconds (configurable via `reconnect_timeout_secs`).
- Tokens stored in Redis with TTL; lookup fails after expiry.
- Token format includes random nonce (UUID); brute-force infeasible.

**Design Location:** `session::mod.rs::ReconnectToken`, `session::repository.rs::validate_reconnect_token()`

**Verification:** `tests/session_state_machine_test.rs::test_reconnect_token` — token expiry validated

---

### 5. **Denial of Service (Connection Flood)**

**Threat:**
- Attacker sends thousands of handshake packets from different IPs to exhaust server resources.
- Server runs out of memory or CPU handling invalid handshakes.

**Severity:** MEDIUM (affects availability, not confidentiality)

**Mitigation:**
- **Current:** Server processes handshakes sequentially in single thread; invalid handshakes rejected immediately.
- **Future:** Rate limiting per IP (⏳ Phase 2); SYN cookies or challenge-response; connection pooling limits.

**Design Location:** `network::udp_server.rs` — sequential processing

**Note:** Explicit rate limiting not implemented yet. Flagged as ⏳.

---

### 6. **Man-in-the-Middle (No Encryption)**

**Threat:**
- Attacker on the network intercepts and modifies unencrypted UDP packets.
- Attacker injects forged inputs or state to cause cheating.

**Severity:** CRITICAL (but out of scope for Phase 1)

**Mitigation:**
- **Out of scope for Phase 1:** Assumes trusted LAN or VPN.
- **Deferred to Phase 2:** QUIC/TLS encryption with certificate pinning.

**Design Location:** None (to be added in Phase 2)

**Consequence:** System is **NOT safe for public internet** without encryption.

---

### 7. **Orphan Sessions (Resource Leak)**

**Threat:**
- Player crashes or loses connection without gracefully disconnecting.
- Server holds session in memory indefinitely → memory leak → eventual server OOM.

**Severity:** MEDIUM (slow attack, high impact if unmitigated)

**Mitigation:**
- **Heartbeat mechanism:** Players send heartbeat pings every 5–10 seconds during active play.
- **Stale detection:** Server scans for sessions with `last_heartbeat < now - 40 seconds`.
- **Cleanup:** Stale sessions marked `Cleaned` and removed from repository.
- **Database TTL:** Postgres `session_heartbeat` column; Redis reconnect tokens have 60-second TTL.

**Design Location:**
- `session::mod.rs::Session::tick()` — heartbeat counter
- `session::repository.rs::find_stale()` — stale detection
- `backend/migrations/20261010_001_init_schema.sql` — heartbeat columns

**Verification:** `tests/postgres_redis_integration.rs::test_sessions_crud_and_heartbeat_orphan_detection` — stale sessions detected; `tests/session_state_machine_test.rs::test_repository_stale_detection` — in-memory stale detection

---

### 8. **Concurrent Connection Collision (Same Port)**

**Threat:**
- Two clients on the same LAN connect to server on same port, causing packet confusion.
- Server cannot distinguish between them → packets routed incorrectly.

**Severity:** LOW (requires same IP + port, unlikely in practice)

**Mitigation:**
- **Single UDP port design:** Server binds to single port (7777); each client identified by `(source_ip, source_port)` tuple.
- **LAN assumption:** On LAN, each client has unique IP or ephemeral port; collision negligible.
- **Multi-region:** Use DNS round-robin or load balancer to distribute across multiple servers on different ports/IPs.

**Design Location:** `network::udp_server.rs::UdpServer::bind()`

**Consequence:** System scales to ~100–1000 concurrent sessions per single-port server on LAN; multi-region deployment ⏳.

---

## Summary Table

| Threat | Severity | Mitigation | Status | Implemented in Code | Verification |
|--------|----------|-----------|--------|---------------------|--------------|
| UDP Loss/Corruption | HIGH | 5-frame redundancy, CRC | ✅ Implemented | ✅ network::mod.rs, transport::protocol.rs | chaos_network_channel tests |
| Replay Attacks | HIGH | Seq numbers, ACK tracking | ✅ Implemented | ✅ transport::protocol.rs::InputFrame | transport_protocol tests |
| Desync Attacks | CRITICAL | Determinism, checksums, ML | ✅ Core; ⏳ ML | ✅ Core (game::mod.rs, desync::detector.rs); ⏳ ML | simulation_determinism tests |
| Session Hijacking | HIGH | Token expiry, Redis TTL | ✅ Implemented | ✅ session::mod.rs, session::repository.rs | session_state_machine tests |
| DoS (Connection Flood) | MEDIUM | Sequential processing | ⏳ Rate limiting | ⏳ D2 (rate limiting trait) | Monitoring only |
| MITM (No Encryption) | CRITICAL | Trusted LAN assumption | ⏳ Phase 2 | ❌ (QUIC/TLS deferred) | Out of scope |
| Orphan Sessions | MEDIUM | Heartbeat + stale detection | ✅ Implemented | ✅ session::mod.rs, session::repository.rs, schema.sql | postgres_redis_integration tests |
| Concurrent Collision | LOW | Single port, unique (IP, port) | ✅ By design | ✅ network::udp_server.rs | Multi-region ⏳ |

---

## Assumptions & Limitations

1. **Trusted LAN:** System assumes attackers are NOT on the network. For public internet, add encryption (QUIC/TLS).
2. **Deterministic Simulation:** Assumes all logic is stateless and deterministic (no RNG in core game loop).
3. **No Secrets in Packets:** Plaintext UDP means no confidentiality. IPs, player IDs, state checksums are visible.
4. **Single-Threaded UDP:** Server processes handshakes sequentially; no built-in rate limiting yet (⏳).

---

## Recommendations for Production

1. **Add QUIC/TLS encryption** (Phase 2).
2. **Implement rate limiting** per IP / per session (Phase 2).
3. **Add DDoS monitoring** and alerting (Phase 3).
4. **Enable AI cheat detection** for large-scale deployments (Phase 2).
5. **Deploy in trusted VPC** or behind firewall.
6. **Monitor orphan session cleanup** logs for anomalies.

---

## Review & Sign-Off

- **Author:** Rollback Netcode Team
- **Reviewed:** 2026-10-10
- **Approved:** Pending security audit

---

**Next:** Security audit by external firm (⏳ Phase 3)
