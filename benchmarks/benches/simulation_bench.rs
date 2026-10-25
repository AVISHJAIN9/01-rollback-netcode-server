use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rollback_netcode_server::simulation::*;

fn bench_simulation(c: &mut Criterion) {
    let state = GameState::default();
    c.bench_function("simulation_step", |b| {
        b.iter(|| black_box(&state).step(black_box(&[INPUT_RIGHT, INPUT_LEFT])))
    });

    let mut ring = SnapshotRing::<128>::new();
    c.bench_function("snapshot_save", |b| {
        b.iter(|| ring.save(black_box(state.clone())))
    });

    ring.save(state.clone());
    c.bench_function("snapshot_restore", |b| {
        b.iter(|| ring.restore(black_box(0)))
    });

    let depths = [1, 2, 4, 8, 12];
    for &depth in &depths {
        c.bench_function(&format!("resimulation_depth_{}", depth), |b| {
            b.iter(|| {
                let mut engine = RollbackEngine::default();
                for t in 1..=depth {
                    engine.advance_local_tick(0, INPUT_RIGHT);
                }
                // Inject late input causing full rollback of `depth` frames
                let _ = engine.handle_remote_input(1, 1, INPUT_LEFT);
            })
        });
    }
}

criterion_group!(benches, bench_simulation);
criterion_main!(benches);
