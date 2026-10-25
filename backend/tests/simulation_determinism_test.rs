use proptest::prelude::*;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use rollback_netcode_server::math::FixedI32;
use rollback_netcode_server::simulation::*;

#[test]
fn test_100_percent_snapshot_restore() {
    let mut ring = SnapshotRing::<64>::new();
    let mut state = GameState::new(0xCAFE);

    let mut saved_states = Vec::new();
    for _ in 1..=50 {
        state = state.step(&[INPUT_RIGHT, INPUT_LEFT]);
        ring.save(state.clone());
        saved_states.push(state.clone());
    }

    // Verify every single retained tick restores bit-for-bit with identical checksum
    for state in &saved_states {
        let restored = ring.restore(state.tick).expect("State must be present");
        assert_eq!(&restored, state);
        assert_eq!(restored.compute_checksum(), state.compute_checksum());
    }
}

#[test]
fn test_duplicate_input_deduplication() {
    let mut history = InputHistory::new();
    assert_eq!(history.insert_input(1, 0b0001), Ok(true));
    // Duplicate insertion must return Ok(false) and not corrupt state
    assert_eq!(history.insert_input(1, 0b0001), Ok(false));
    assert_eq!(history.get_input(1), Some(0b0001));
    assert_eq!(history.confirmed_tick, 1);
}

#[test]
fn test_1000_run_randomized_rollback_determinism() {
    // Fixed reference 100-frame input sequence
    let mut rng = StdRng::seed_from_u64(0x12345678);
    let mut reference_inputs = Vec::new();
    for _ in 1..=100 {
        let i0 = (rng.gen_range(0..16)) as u16;
        let i1 = (rng.gen_range(0..16)) as u16;
        reference_inputs.push([i0, i1]);
    }

    // Compute ground truth linear execution
    let mut ground_truth = GameState::new(0x9999);
    for inputs in &reference_inputs {
        ground_truth = ground_truth.step(inputs);
    }
    let expected_hash = ground_truth.compute_checksum();

    // Run 1,000 simulations with randomized late-input rollback delivery schedules (jitter 0..=5 frames)
    for run in 0..1000 {
        let mut sim_rng = StdRng::seed_from_u64(run as u64 + 1);
        let mut engine = RollbackEngine::new_with_initial_state(GameState::new(0x9999), RepeatLastInputPolicy);

        // Schedule delivery of each remote input at (tick + delay)
        let mut scheduled_deliveries: Vec<Vec<(u32, u16)>> = vec![Vec::new(); 120];
        for tick in 1..=100 {
            let delay = sim_rng.gen_range(0..=5); // Jitter delay within MAX_ROLLBACK
            let arrival_tick = tick + delay;
            scheduled_deliveries[arrival_tick as usize].push((tick, reference_inputs[(tick - 1) as usize][1]));
        }

        for tick in 1..=100 {
            let [i0, _] = reference_inputs[(tick - 1) as usize];
            engine.advance_local_tick(0, i0);

            // Deliver any remote packets scheduled to arrive at this tick
            for &(remote_tick, remote_input) in &scheduled_deliveries[tick as usize] {
                let _ = engine.handle_remote_input(1, remote_tick, remote_input);
            }
        }

        // Flush in-flight delayed packets (arriving at ticks 101..=105)
        for arrival_tick in 101..120 {
            for &(remote_tick, remote_input) in &scheduled_deliveries[arrival_tick] {
                let _ = engine.handle_remote_input(1, remote_tick, remote_input);
            }
        }

        let final_state = engine.current_state();
        assert_eq!(
            final_state.compute_checksum(),
            expected_hash,
            "Run {} diverged from ground truth!",
            run
        );
    }
}

#[test]
fn test_two_peer_convergence_under_network_delay() {
    let mut peer_a = RollbackEngine::default();
    let mut peer_b = RollbackEngine::default();

    let mut sim_rng = StdRng::seed_from_u64(0xBEEF);
    let mut p0_inputs = Vec::new();
    let mut p1_inputs = Vec::new();

    for _ in 1..=60 {
        p0_inputs.push((sim_rng.gen_range(0..16)) as u16);
        p1_inputs.push((sim_rng.gen_range(0..16)) as u16);
    }

    // Advance peer A and peer B with 2-frame cross-delivery delay
    for t in 1..=60 {
        let i0 = p0_inputs[(t - 1) as usize];
        let i1 = p1_inputs[(t - 1) as usize];

        peer_a.advance_local_tick(0, i0);
        peer_b.advance_local_tick(1, i1);

        if t > 2 {
            let delayed_t = t - 2;
            let _ = peer_a.handle_remote_input(1, delayed_t, p1_inputs[(delayed_t - 1) as usize]);
            let _ = peer_b.handle_remote_input(0, delayed_t, p0_inputs[(delayed_t - 1) as usize]);
        }
    }

    // Flush remaining inputs
    for t in 59..=60 {
        let _ = peer_a.handle_remote_input(1, t, p1_inputs[(t - 1) as usize]);
        let _ = peer_b.handle_remote_input(0, t, p0_inputs[(t - 1) as usize]);
    }

    assert_eq!(
        peer_a.current_state().compute_checksum(),
        peer_b.current_state().compute_checksum(),
        "Peer A and Peer B failed to converge to identical state hash!"
    );
}

#[test]
fn test_forced_desync_detection_and_authoritative_recovery() {
    let mut local_engine = RollbackEngine::default();
    for _ in 0..10 {
        local_engine.advance_local_tick(0, INPUT_RIGHT);
    }

    let authoritative_state = local_engine.current_state();
    let auth_hash = authoritative_state.compute_checksum();

    // Corrupt local player health to simulate memory corruption or cheat
    let mut corrupted_state = authoritative_state.clone();
    corrupted_state.players[0].health = 42;
    let corrupted_hash = corrupted_state.compute_checksum();

    assert_ne!(auth_hash, corrupted_hash);

    let classification = DivergenceDetector::classify(
        authoritative_state.tick,
        corrupted_hash,
        auth_hash,
        Some([INPUT_RIGHT, 0]),
        Some([INPUT_RIGHT, 0]),
    );

    match classification {
        DivergenceClassification::SimulationMismatch { .. } => {
            // Expected classification
        }
        other => panic!("Expected SimulationMismatch, got {:?}", other),
    }

    // Force Authoritative State Resync Recovery
    local_engine.force_resync_state(authoritative_state.clone());
    assert_eq!(local_engine.current_state().compute_checksum(), auth_hash);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn proptest_simulation_invariants(inputs in prop::collection::vec(0u16..=0x1F, 50)) {
        let mut state = GameState::default();
        for &input in &inputs {
            state = state.step(&[input, input]);
            // Invariants:
            // 1. Health is always in range [0, 100]
            prop_assert!(state.players[0].health <= 100);
            prop_assert!(state.players[1].health <= 100);
            // 2. Positions stay within arena boundaries
            prop_assert!(state.players[0].position.x.raw() >= 0);
            prop_assert!(state.players[0].position.x.raw() <= FixedI32::from_int(ARENA_WIDTH_INT).raw());
            prop_assert!(state.players[0].position.y.raw() >= 0);
        }
    }
}
