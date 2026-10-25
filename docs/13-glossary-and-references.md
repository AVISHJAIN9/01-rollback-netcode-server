# 13 - Glossary & References: Rollback Netcode Server

## 1. Technical Glossary [Specified]
- **Rollback Netcode**: A multiplayer synchronization technique where clients predict remote player inputs immediately and rewind/resimulate upon receiving authoritative remote inputs.
- **Fixed-Point Arithmetic**: Integer-based representation of fractional numbers ensuring bit-for-bit cross-architecture mathematical determinism.
- **Desynchronization (Desync)**: Any state divergence where a client simulation produces a checksum different from the authoritative server on the same frame.
- **Jitter Buffer**: A dynamic buffer smoothing out packet arrival timing variations caused by network latency fluctuations.
- **Lockstep**: A legacy multiplayer architecture where simulation cannot advance until every peer's input for that frame is received.

## 2. Academic & Industry References [Specified]
1. Cannizzo, F. (2018). *Deterministic Physics in Multiplayer Games*. Game Developers Conference.
2. GGPO (Good Game Peace Out) Netcode Specification: https://github.com/pond3r/ggpo
3. Chandy, K. M., & Lamport, L. (1985). *Distributed Snapshots: Determining Global States of Distributed Systems*. ACM TOCS.
4. Blake3 Cryptographic Hash Specification: https://github.com/BLAKE3-team/BLAKE3
