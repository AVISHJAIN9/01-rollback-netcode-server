# ADR 005: Fixed-Point Numeric Type for Cross-Platform Determinism

## Status
Accepted

## Context
Floating point operations (`f32`, `f64`) across different CPU architectures (x86_64 vs ARM64/Apple Silicon) and compiler optimization levels yield subtle non-deterministic divergence due to FMA (Fused Multiply-Add) instructions, extended precision registers, and rounding mode differences. A single sub-epsilon difference causes desynchronization in lockstep/rollback simulations.

## Decision
We implement a dedicated `FixedI32` Q16.16 signed fixed-point type (16 bits integer, 16 bits fractional):
- Zero floating-point instructions in simulation, state, and physics.
- Integer arithmetic (`wrapping_add`, `wrapping_sub`, 64-bit intermediate multiplication and division).
- Integer square root (`isqrt`) and deterministic trigonometric sine/cosine lookup tables (256-entry quadrant LUT).
- Explicit `serde::Serialize` and `serde::Deserialize` implementations storing exact 32-bit raw integer payloads.

## Consequences
- Guaranteed bit-identical simulation results across Apple Silicon, x86_64 Linux servers, and WASM/browser clients.
- Arithmetic operations execute in 1–2 clock cycles.
- Dynamic range is bounded to [-32,768.0, +32,767.99998] with precision of ~0.000015 (1/65536), which is ideal for 2D arena game kinematics.
