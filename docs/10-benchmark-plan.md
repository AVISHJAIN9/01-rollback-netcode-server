# 10 - Benchmark Plan & Performance Targets: Rollback Netcode Server

## 1. Benchmarking Environment [Specified]
- **Hardware Configuration**: Apple Silicon M-series (8 cores, 16GB RAM) & AWS c6i.2xlarge (8 vCPU, 16GB RAM)
- **Baseline Implementations**:
  1. Plain Lockstep Server (waits for all peer ACKs before advancing frame)
  2. Server-Only No-Prediction Architecture
- **Metrics Collected**:
  - Single-frame simulation execution time ($\mu s$)
  - Rollback resimulation cost per frame rewound ($\mu s / 	ext{frame}$)
  - Memory allocation rate (bytes / frame)
  - UDP packet processing throughput (packets / sec)

## 2. Benchmark Target Matrix [Specified]

| Metric | Target Specification | Measured Value |
| :--- | :--- | :--- |
| Single-Frame Physics Tick (64 entities) | < 50 $\mu s$ | **TO BE MEASURED** |
| 8-Frame Rollback Resimulation | < 250 $\mu s$ | **TO BE MEASURED** |
| 32-Frame Rollback Resimulation | < 950 $\mu s$ | **TO BE MEASURED** |
| State Snapshot Blake3 Hash | < 15 $\mu s$ | **TO BE MEASURED** |
| Heap Allocations per Tick | Exactly 0 Bytes | **TO BE MEASURED** |
| Max Concurrent Matches per Server Core | >= 250 matches | **TO BE MEASURED** |

### Measurement Command
```bash
cargo bench --bench simulation_bench
```
