use proptest::prelude::*;
use rollback_netcode_server::protocol::*;

#[test]
fn test_magic_and_header_validation() {
    let bad_magic = vec![0x00, 0x00, 0x01, 0x01];
    assert_eq!(
        GamePacket::decode(&bad_magic),
        Err(ProtocolError::InvalidMagic(0x0000))
    );

    let too_short = vec![0x52, 0x42];
    assert_eq!(
        GamePacket::decode(&too_short),
        Err(ProtocolError::BufferTooShort {
            expected: 4,
            actual: 2
        })
    );
}

#[test]
fn test_version_negotiation_matrix() {
    let negotiator = VersionNegotiator::default();
    
    // Version 1.0 is the only supported version (hardcoded in PROTOCOL_VERSION)
    assert_eq!(negotiator.negotiate(1), Ok(1), "Version 1 should be accepted");
    
    // Edge cases: rejection
    assert!(negotiator.negotiate(0).is_err(), "Version 0 should be rejected");
    assert!(negotiator.negotiate(2).is_err(), "Version 2 should be rejected (future)");
    assert!(negotiator.negotiate(99).is_err(), "Version 99 should be rejected (far future)");
    
    // Boundary: max u8 version
    assert!(negotiator.negotiate(255).is_err(), "Version 255 should be rejected");
    
    // Idempotency: multiple calls should be consistent
    for _ in 0..3 {
        assert_eq!(negotiator.negotiate(1), Ok(1));
        assert!(negotiator.negotiate(2).is_err());
    }
}

#[test]
fn test_oversized_packet_rejection() {
    let oversized = vec![0u8; MAX_PACKET_BYTES + 10];
    assert_eq!(
        GamePacket::decode(&oversized),
        Err(ProtocolError::PayloadLengthExceeded {
            max_allowed: MAX_PACKET_BYTES,
            actual: MAX_PACKET_BYTES + 10,
        })
    );
}

#[test]
fn test_handshake_roundtrip() {
    let packet = GamePacket::Handshake {
        client_version: 1,
        capabilities: CAPABILITY_INPUT_BUNDLING | CAPABILITY_SPECTATOR,
        session_token: [0x42; 16],
    };
    let encoded = packet.encode();
    let decoded = GamePacket::decode(&encoded).expect("Decode failed");
    assert_eq!(packet, decoded);
}

#[test]
fn test_input_frame_roundtrip_with_redundancy() {
    let packet = GamePacket::InputFrame {
        player_id: 1,
        sequence_num: 1042,
        current_tick: 500,
        input_bitmask: 0b0000_0011,
        redundant_inputs: vec![3, 3, 2, 2, 0],
    };
    let encoded = packet.encode();
    let decoded = GamePacket::decode(&encoded).expect("Decode failed");
    assert_eq!(packet, decoded);
}

#[test]
fn test_checksum_roundtrip() {
    let packet = GamePacket::Checksum {
        tick: 100_000,
        state_checksum: 0x8A697184,
    };
    let encoded = packet.encode();
    let decoded = GamePacket::decode(&encoded).expect("Decode failed");
    assert_eq!(packet, decoded);
}

#[test]
fn test_error_and_kick_roundtrip() {
    let error_pkt = GamePacket::Error {
        error_code: 404,
        reason: "Match Room Not Found".to_string(),
    };
    let encoded = error_pkt.encode();
    let decoded = GamePacket::decode(&encoded).expect("Decode failed");
    assert_eq!(error_pkt, decoded);

    let kick_pkt = GamePacket::Kick { reason_code: 9 };
    let encoded = kick_pkt.encode();
    let decoded = GamePacket::decode(&encoded).expect("Decode failed");
    assert_eq!(kick_pkt, decoded);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000)
    ))]
    #[test]
    fn proptest_input_frame_arbitrary(
        player_id in 0u8..2,
        sequence_num in 0u32..1_000_000,
        current_tick in 0u32..1_000_000,
        input_bitmask in 0u16..=0xFFFF,
        redundant in prop::collection::vec(0u16..=0xFFFF, 0..=7)
    ) {
        let pkt = GamePacket::InputFrame {
            player_id,
            sequence_num,
            current_tick,
            input_bitmask,
            redundant_inputs: redundant,
        };
        let bytes = pkt.encode();
        let decoded = GamePacket::decode(&bytes).expect("Proptest decode failed");
        prop_assert_eq!(pkt, decoded);
    }

    #[test]
    fn proptest_fuzz_random_bytes_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..=1250)) {
        // Must never panic on arbitrary byte sequences
        let _ = GamePacket::decode(&bytes);
    }
}
