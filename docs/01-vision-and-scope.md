# 01 - Vision & Scope: Rollback Netcode Server

## 1. Executive Summary [Specified]
In multiplayer real-time network gaming, latency is the defining constraint of player experience. Traditional lockstep architectures introduce severe input lag equal to the Round-Trip Time (RTT) of the slowest peer, while basic server-authoritative client-side prediction systems suffer from jarring teleportation artifacts and non-deterministic state divergence when remote inputs arrive late.

`rollback-netcode-server` provides a state-of-the-art, deterministic, server-authoritative multiplayer netcode engine and client runtime designed for competitive 2D arena combat. The engine combines deterministic fixed-point physics (Q16.16), a snapshot circular ring buffer (128 frames), rollback resimulation (rewinding up to 128 frames in sub-millisecond time), packet redundancy over UDP, continuous Blake3 frame state checksumming for sub-frame desync detection, and an integrated Machine Learning subsystem featuring real-time input anomaly detection (cheat detection), adaptive jitter prediction, and an LLM-powered desync diagnostic assistant.

## 2. Core Value Proposition & Problem Statement [Specified]
- **Zero Perceived Input Lag**: Local player inputs are executed immediately on the local frame without waiting for server or remote player confirmation.
- **Strict Server Authority**: Unlike peer-to-peer rollback engines (e.g. GGPO) where malicious peers can forge state, our architecture is server-authoritative. The server maintains the true master simulation, validates input feasibility, executes rollback resimulation upon receiving late client inputs, and acts as the final arbiter.
- **Guaranteed Determinism**: All arithmetic uses fixed-point math (`FixedI32`), avoiding IEEE 754 floating-point non-determinism across disparate CPU architectures (x86_64, ARM64, WASM).
- **Sub-Frame Desync Diagnostics**: Every frame produces a 64-bit Blake3 checksum over canonical state memory. Any client-server divergence triggers an immediate binary differential dump and automated root-cause analysis.
- **Built-in AI Operations**: Autonomous cheat detection on raw input sequences and predictive jitter modeling to dynamically calibrate the local input delay window.

## 3. System Boundaries & Target Performance [Specified]
| Metric | Specification Target | Status |
| :--- | :--- | :--- |
| Simulation Tick Rate | 60.0 Hz (16.666 ms fixed interval) | [Specified] |
| Max Supported Rollback Window | 128 frames (2.13 seconds) | [Specified] |
| Rollback Resimulation Overhead (8 frames) | < 0.25 ms on modern x86_64 / ARM64 | **TO BE MEASURED** |
| Rollback Resimulation Overhead (64 frames) | < 1.80 ms on modern x86_64 / ARM64 | **TO BE MEASURED** |
| State Snapshot Size | <= 512 bytes per frame (uncompressed) | [Specified] |
| Determinism Verification | 1,000,000 continuous frames zero divergence | [Specified] |
| Telemetry Ingestion Throughput | >= 25,000 events/sec | **TO BE MEASURED** |
| Cheat Detection Inference Latency | < 5.0 ms per 60-frame window | **TO BE MEASURED** |
| Jitter Predictor Inference Latency | < 0.1 ms per packet step | **TO BE MEASURED** |

## 4. User Personas [Specified]
1. **Competitive Esports Players**: Require responsive < 1 frame perceived input latency and zero teleportation artifacts under typical network jitter (< 60 ms).
2. **Game Systems Engineers**: Need full visibility into snapshot ring buffers, rollback frequencies, clock drift, and deterministic frame logs.
3. **Game Anti-Cheat Operations**: Require telemetry on input rates, impossibility metrics, and automated anomaly scoring.
