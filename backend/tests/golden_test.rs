use rollback_netcode_server::simulation::*;

#[test]
fn test_golden_replay_milestones() {
    let mut state = GameState::new(0x42);

    // Frame 1
    state = state.step(&[INPUT_RIGHT, INPUT_LEFT]);
    let h1 = state.compute_checksum();
    assert_eq!(h1, 0xDEE6_DE86);

    // Frame 10 with jumps & attacks
    for _ in 2..=10 {
        state = state.step(&[INPUT_RIGHT | INPUT_JUMP, INPUT_LEFT | INPUT_ATTACK]);
    }
    let h10 = state.compute_checksum();
    assert_eq!(h10, 0xFBD4_F18D);

    // Frame 60 (1 second mark)
    for _ in 11..=60 {
        state = state.step(&[INPUT_ATTACK, INPUT_BLOCK]);
    }
    let h60 = state.compute_checksum();
    assert_eq!(h60, 0x35A9_8AD9);
}
