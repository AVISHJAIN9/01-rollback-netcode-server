#[test]
fn test_module_boundary_isolation() {
    // Verify that math and simulation can be compiled and used in complete isolation
    use rollback_netcode_server::math::FixedI32;
    use rollback_netcode_server::protocol::GamePacket;
    use rollback_netcode_server::simulation::state::GameState;

    let v = FixedI32::from_int(10);
    assert_eq!(v.to_int(), 10);

    let state = GameState::default();
    assert_eq!(state.tick, 0);

    let pkt = GamePacket::Ping {
        client_timestamp_ms: 12345,
    };
    assert_eq!(pkt.encode().len(), 12);
}
