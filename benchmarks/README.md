# Benchmarks & Performance Harness

## Available Benchmarks
1. `benches/simulation_bench.rs`: Criterion microbenchmarks measuring single-frame physics steps, Blake3 state hashing, and multi-frame rollback resimulation.
2. `harness/load_generator.py`: High-concurrency synthetic UDP client load generator simulating 1,000 concurrent player sessions under network latency and packet loss.

## Execution
```bash
cargo bench --bench simulation_bench
python harness/load_generator.py --clients 500 --rtt 60 --loss 0.05
```
