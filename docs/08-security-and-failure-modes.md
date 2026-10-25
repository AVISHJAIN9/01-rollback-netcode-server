# 08 - Security & Failure Modes: Rollback Netcode Server

## 1. Threat Model & Mitigations [Specified]

| Threat | Attack Vector | System Defense Mechanism |
| :--- | :--- | :--- |
| **Input Forgery / Teleportation** | Modified client sends impossible coordinate deltas | Server-authoritative physics: client only sends input bitmasks; server calculates all positions |
| **Speed Hacking / Clock Manipulation** | Client accelerates local tick rate to generate excessive frames | NTP clock sync rate limits: server rejects inputs $> 4$ frames ahead of server clock |
| **UDP Replay / Packet Injection** | Malicious actor captures and replays valid input packets | Monotonically increasing frame indices + 32-bit ephemeral session tokens |
| **Denial of Service (Flooding)** | UDP packet flooding targeted at game socket | Token-bucket rate limiting per IP + early drop of invalid session tokens |
| **Desync Exploitation** | Client intentionally induces desync to force state resets | Instant desync detection: server logs incident and kicks client if divergence persists |

## 2. Failure Modes & Graceful Degradation [Specified]
1. **Severe Packet Loss (> 40%)**:
   - The 5-frame redundant input bundling recovers transient drops. If loss exceeds recovery capability, the engine pauses forward prediction and displays a "Network Lag Reconnecting" banner.
2. **PostgreSQL Telemetry Outage**:
   - The server buffers input logs in an in-memory queue. If Postgres becomes unavailable, active gameplay continues uninterrupted; analytics logs are drained asynchronously upon database recovery.
3. **AI Service Failure**:
   - If the Isolation Forest or LLM endpoints time out, the server falls back to static hardcoded thresholds without blocking any simulation ticks.
