# ADR 002: Server Authority and Fixed-Point (Q16.16) Determinism

## Status
Accepted

## Context
Multiplayer state desynchronization occurs when different clients compute slightly diverging coordinates, velocities, or collision outcomes. Standard IEEE 754 floating-point operations vary across CPU architectures, compilers, and JavaScript JIT engines. Furthermore, client-reported positions enable teleportation and speed-hack vulnerabilities.

## Decision
1. **Arithmetic Representation**: All kinematic math uses signed 32-bit fixed-point numbers with 16 fractional bits (`FixedI32`).
2. **Strict Server Authority**: Clients only transmit raw input bitmasks and analog stick deltas; the server alone calculates authoritative `WorldState` snapshots and collision outcomes.
3. **Continuous State Checksumming**: Continuous FNV-1a/Blake3 state hashes are computed every frame to detect sub-frame divergence immediately.

## Consequences
- Bit-exact mathematical determinism across x86_64, ARM64, and WebAssembly/V8 engines.
- Complete mitigation of position-spoofing and speed-hacking exploits.
