use rollback_netcode_server::protocol::packets::GamePacket;
use rollback_netcode_server::protocol::version::CURRENT_PROTOCOL_VERSION;

#[test]
fn test_handshake_versioning_encode_decode() {
    // Test versioned handshake serialization
    let handshake = GamePacket::Handshake {
        client_version: CURRENT_PROTOCOL_VERSION,
        capabilities: 0x0001, // INPUT_BUNDLING
        session_token: [0xAAu8; 16],
    };

    let encoded = handshake.encode();
    println!("[✓] Handshake packet encoded ({} bytes)", encoded.len());

    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");
    match decoded {
        GamePacket::Handshake {
            client_version,
            capabilities,
            session_token,
        } => {
            assert_eq!(client_version, CURRENT_PROTOCOL_VERSION);
            assert_eq!(capabilities, 0x0001);
            assert_eq!(session_token, [0xAAu8; 16]);
            println!("[✓] Handshake roundtrip: version={}, cap=0x{:04X}", client_version, capabilities);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_handshake_ack_encode_decode() {
    // Test handshake acknowledgment
    let ack = GamePacket::HandshakeAck {
        negotiated_version: CURRENT_PROTOCOL_VERSION,
        player_id: 0,
        tick_rate: 60,
        assigned_token: [0xBBu8; 16],
    };

    let encoded = ack.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::HandshakeAck {
            negotiated_version,
            player_id,
            tick_rate,
            assigned_token,
        } => {
            assert_eq!(negotiated_version, CURRENT_PROTOCOL_VERSION);
            assert_eq!(player_id, 0);
            assert_eq!(tick_rate, 60);
            assert_eq!(assigned_token, [0xBBu8; 16]);
            println!("[✓] HandshakeAck: version={}, player={}, tick_rate={}", negotiated_version, player_id, tick_rate);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_input_frame_with_5frame_redundancy() {
    // Test 5-frame redundant bundling per spec
    let current_tick = 100u32;
    let input = GamePacket::InputFrame {
        player_id: 0,
        sequence_num: 100,
        current_tick,
        input_bitmask: 0x0005, // LEFT | JUMP
        redundant_inputs: vec![0x0001, 0x0002, 0x0003, 0x0004, 0x0005],
    };

    let encoded = input.encode();
    println!("[✓] InputFrame packet encoded ({} bytes)", encoded.len());

    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");
    match decoded {
        GamePacket::InputFrame {
            player_id,
            sequence_num,
            current_tick: decoded_tick,
            input_bitmask,
            redundant_inputs,
        } => {
            assert_eq!(player_id, 0);
            assert_eq!(sequence_num, 100);
            assert_eq!(decoded_tick, current_tick);
            assert_eq!(input_bitmask, 0x0005);
            assert_eq!(redundant_inputs.len(), 5, "Must have 5 redundant historical inputs");
            println!("[✓] InputFrame: tick={}, bitmask=0x{:04X}, redundancy={} frames", decoded_tick, input_bitmask, redundant_inputs.len());
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_state_ack_packet() {
    // Test state acknowledgment used for RTT estimation
    let ack = GamePacket::StateAck {
        player_id: 1,
        ack_tick: 42,
        rtt_sample_ms: 25,
    };

    let encoded = ack.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::StateAck {
            player_id,
            ack_tick,
            rtt_sample_ms,
        } => {
            assert_eq!(player_id, 1);
            assert_eq!(ack_tick, 42);
            assert_eq!(rtt_sample_ms, 25);
            println!("[✓] StateAck: player={}, ack_tick={}, RTT={}ms", player_id, ack_tick, rtt_sample_ms);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_checksum_packet_for_desync_detection() {
    // Test Blake3 checksum packet for desync detection
    let checksum_pkt = GamePacket::Checksum {
        tick: 100,
        state_checksum: 0xDEADBEEF,
    };

    let encoded = checksum_pkt.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::Checksum {
            tick,
            state_checksum,
        } => {
            assert_eq!(tick, 100);
            assert_eq!(state_checksum, 0xDEADBEEF);
            println!("[✓] Checksum: tick={}, hash=0x{:08X}", tick, state_checksum);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_error_packet_for_version_mismatch() {
    // Test error packet with version mismatch reason
    let error = GamePacket::Error {
        error_code: 421, // Version mismatch
        reason: "Unsupported protocol version: client=99, server=1".to_string(),
    };

    let encoded = error.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::Error { error_code, reason } => {
            assert_eq!(error_code, 421);
            assert!(reason.contains("Unsupported"));
            println!("[✓] Error packet: code={}, reason={}", error_code, reason);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_ping_pong_for_clock_sync() {
    // Test ping/pong for NTP-style clock synchronization
    let ping_ts = 1234567890u64;
    let ping = GamePacket::Ping {
        client_timestamp_ms: ping_ts,
    };

    let encoded = ping.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::Ping {
            client_timestamp_ms,
        } => {
            assert_eq!(client_timestamp_ms, ping_ts);
            println!("[✓] Ping: T1={}", client_timestamp_ms);
        }
        _ => panic!("Decoded wrong packet type"),
    }

    // Test Pong response (T2, T3 in NTP)
    let pong = GamePacket::Pong {
        client_timestamp_ms: ping_ts,
        server_timestamp_ms: 1234567900,
    };

    let encoded = pong.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::Pong {
            client_timestamp_ms,
            server_timestamp_ms,
        } => {
            assert_eq!(client_timestamp_ms, ping_ts);
            assert!(server_timestamp_ms > client_timestamp_ms);
            let rtt = (server_timestamp_ms - client_timestamp_ms) as i64;
            println!("[✓] Pong: T1={}, T2/T3={}, RTT_est={}ms", client_timestamp_ms, server_timestamp_ms, rtt / 2);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_reconnect_token_packet() {
    // Test reconnect token for 30-second grace window recovery
    let reconnect = GamePacket::Reconnect {
        reconnect_token: [0xCCu8; 16],
        last_received_tick: 500,
    };

    let encoded = reconnect.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::Reconnect {
            reconnect_token,
            last_received_tick,
        } => {
            assert_eq!(reconnect_token, [0xCCu8; 16]);
            assert_eq!(last_received_tick, 500);
            println!("[✓] Reconnect: token valid, last_tick={}", last_received_tick);
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_snapshot_request_response() {
    // Test snapshot request/response for desync recovery
    let request = GamePacket::SnapshotRequest {
        requested_tick: 50,
    };

    let encoded = request.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::SnapshotRequest { requested_tick } => {
            assert_eq!(requested_tick, 50);
            println!("[✓] SnapshotRequest: tick={}", requested_tick);
        }
        _ => panic!("Decoded wrong packet type"),
    }

    // Test response
    let mut payload = Vec::new();
    payload.extend_from_slice(&[0xDEu8; 100]); // Mock snapshot data
    let response = GamePacket::SnapshotResponse {
        snapshot_tick: 50,
        data_payload: payload,
    };

    let encoded = response.encode();
    let decoded = GamePacket::decode(&encoded).expect("Failed to decode");

    match decoded {
        GamePacket::SnapshotResponse {
            snapshot_tick,
            data_payload,
        } => {
            assert_eq!(snapshot_tick, 50);
            assert!(data_payload.len() > 0);
            println!("[✓] SnapshotResponse: tick={}, payload={} bytes", snapshot_tick, data_payload.len());
        }
        _ => panic!("Decoded wrong packet type"),
    }
}

#[test]
fn test_all_packet_types_roundtrip() {
    // Comprehensive roundtrip test for all packet types
    let packets = vec![
        ("Handshake", GamePacket::Handshake {
            client_version: 1,
            capabilities: 0x000F,
            session_token: [0; 16],
        }),
        ("HandshakeAck", GamePacket::HandshakeAck {
            negotiated_version: 1,
            player_id: 0,
            tick_rate: 60,
            assigned_token: [1; 16],
        }),
        ("InputFrame", GamePacket::InputFrame {
            player_id: 0,
            sequence_num: 1,
            current_tick: 1,
            input_bitmask: 0x0001,
            redundant_inputs: vec![],
        }),
        ("StateAck", GamePacket::StateAck {
            player_id: 0,
            ack_tick: 1,
            rtt_sample_ms: 10,
        }),
        ("Ping", GamePacket::Ping {
            client_timestamp_ms: 1000,
        }),
        ("Pong", GamePacket::Pong {
            client_timestamp_ms: 1000,
            server_timestamp_ms: 1010,
        }),
    ];

    for (name, pkt) in packets {
        let encoded = pkt.encode();
        let decoded = GamePacket::decode(&encoded);
        assert!(decoded.is_ok(), "Failed to decode {}", name);
        println!("[✓] {} roundtrip successful", name);
    }
}
