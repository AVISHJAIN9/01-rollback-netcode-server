# Performance Benchmark Results: `01-rollback-netcode-server`

## Hardware Environment
- **Platform**: Apple Silicon (M-series, aarch64 darwin)
- **Rust Toolchain**: `rustc 1.98.1 (48a229cea 2026-09-01)` / `cargo 1.98.1`
- **Compiler Flags**: `--release` (opt-level = 3)
- **Benchmark Suite**: Criterion.rs v0.5.1

---

## 1. Criterion Benchmark Execution Results

| Operation | Mean Latency | 95% Confidence Interval | Operations / Second |
| :--- | :--- | :--- | :--- |
| **`simulation_step`** (1 tick pure physics) | **6.92 ns** | [6.78 ns – 7.12 ns] | **144,483,615 ops/sec** |
| **`snapshot_save`** (128-tick ring save) | **334.56 ns** | [326.54 ns – 351.29 ns] | **2,989,000 ops/sec** |
| **`snapshot_restore`** (128-tick ring lookup) | **2.51 ns** | [2.34 ns – 2.70 ns] | **398,247,000 ops/sec** |
| **`resimulation_depth_1`** (1-frame rollback) | **1.28 µs** | [1.25 µs – 1.33 µs] | **775,193 ops/sec** |
| **`resimulation_depth_2`** (2-frame rollback) | **2.44 µs** | [2.24 µs – 2.70 µs] | **408,496 ops/sec** |
| **`resimulation_depth_4`** (4-frame rollback) | **4.72 µs** | [4.41 µs – 5.08 µs] | **211,476 ops/sec** |
| **`resimulation_depth_8`** (8-frame rollback) | **9.12 µs** | [8.26 µs – 10.06 µs] | **109,649 ops/sec** |
| **`resimulation_depth_12`** (12-frame rollback) | **14.90 µs** | [13.73 µs – 16.16 µs] | **67,091 ops/sec** |

---

## 2. Frame Budget Analysis (60 Hz Game Loop)

- **Target Tick Deadline**: **16.66 ms** ($16,666.66\text{ }\mu\text{s}$)
- **Single-Frame Step Budget**: $6.92\text{ ns} = 0.00041\%$ of frame budget.
- **Worst-Case 12-Frame Resimulation**: $14.90\text{ }\mu\text{s} = 0.089\%$ of frame budget.
- **Verdict**: **FITS WITH ENORMOUS HEADROOM** (> 99.9% of frame time remains available for network I/O, matchmaking, and audio/graphics rendering).
