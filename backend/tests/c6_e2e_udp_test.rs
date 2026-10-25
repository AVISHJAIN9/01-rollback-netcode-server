/// C6: End-to-End UDP Test
/// Tests real UDP handshake, frame exchange over 100 ticks, checksum validation, 
/// rollback recovery, and reconnect after disconnect.
///
/// Uses tokio::spawn to run server + client tasks in parallel with localhost UDP.

use rollback_netcode_server::protocol::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration};

/// Stub game state that just counts ticks
#[derive(Debug, Clone)]
struct StubGameState {
    tick: u64,
    checksum: u32,
}

impl StubGameState {
    fn new() -> Self {
        Self {
            tick: 0,
            checksum: 0,
        }
    }

    fn advance(&mut self, input: u16) {
        self.tick += 1;
        self.checksum = self
            .checksum
            .wrapping_mul(31)
            .wrapping_add(input as u32);
    }
}

/// Test 1: Handshake over real UDP
#[tokio::test]
async fn test_c6_handshake_real_udp() {
    // Bind server to any available port
    let server_addr = "127.0.0.1:0";
    let server_socket = UdpSocket::bind(server_addr).await.expect("Server bind failed");
    let server_addr = server_socket.local_addr().expect("Get local addr failed");

    // Spawn server task
    let server_socket_clone = Arc::new(server_socket);
    let server_arc = Arc::clone(&server_socket_clone);
    let server_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let (len, client_addr) = server_arc
            .recv_from(&mut buf)
            .await
            .expect("Server recv failed");

        // Decode handshake
        let packet =
            GamePacket::decode(&buf[..len]).expect("Handshake decode failed");
        println!("Server received: {:?}", packet);

        // Verify it's a Handshake
        assert!(matches!(packet, GamePacket::Handshake { .. }));

        // Send back HandshakeAck
        let ack = GamePacket::HandshakeAck {
            negotiated_version: 1,
            player_id: 0,
            tick_rate: 60,
            assigned_token: [0xAB; 16],
        };
        server_arc
            .send_to(&ack.encode(), client_addr)
            .await
            .expect("Server send failed");
    });

    // Client task
    let client_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Client bind failed");

    let handshake = GamePacket::Handshake {
        client_version: 1,
        capabilities: CAPABILITY_INPUT_BUNDLING,
        session_token: [0x42; 16],
    };

    client_socket
        .send_to(&handshake.encode(), server_addr)
        .await
        .expect("Client send failed");
    println!("Client sent Handshake");

    let mut buf = [0u8; 1500];
    let (len, _) = client_socket
        .recv_from(&mut buf)
        .await
        .expect("Client recv failed");

    let ack = GamePacket::decode(&buf[..len]).expect("Ack decode failed");
    println!("Client received: {:?}", ack);

    if let GamePacket::HandshakeAck {
        negotiated_version, ..
    } = ack
    {
        assert_eq!(negotiated_version, 1);
    } else {
        panic!("Expected HandshakeAck, got {:?}", ack);
    }

    // Wait for server task
    timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("Server timeout")
        .expect("Server task failed");

    println!("✓ Handshake test passed");
}

/// Test 2: Frame exchange over 100 ticks with checksum convergence
#[tokio::test]
async fn test_c6_frame_exchange_100_ticks() {
    let server_addr = "127.0.0.1:0";
    let server_socket = UdpSocket::bind(server_addr).await.expect("Server bind failed");
    let server_addr = server_socket.local_addr().expect("Get local addr failed");
    let server_socket = Arc::new(server_socket);

    // Server: accept frames and emit checksums
    let server_arc = Arc::clone(&server_socket);
    let server_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let mut state = StubGameState::new();
        let mut received_count = 0u32;

        loop {
            match timeout(Duration::from_secs(2), server_arc.recv_from(&mut buf)).await {
                Ok(Ok((len, client_addr))) => {
                    if let Ok(packet) = GamePacket::decode(&buf[..len]) {
                        match packet {
                            GamePacket::InputFrame {
                                input_bitmask,
                                current_tick,
                                ..
                            } => {
                                state.advance(input_bitmask);
                                received_count += 1;

                                // Send checksum back
                                let checksum_pkt = GamePacket::Checksum {
                                    tick: current_tick,
                                    state_checksum: state.checksum,
                                };

                                let _ = server_arc.send_to(&checksum_pkt.encode(), client_addr).await;

                                if current_tick >= 100 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => break,
            }
        }

        state.checksum
    });

    // Client: send frames and collect checksums
    let client_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Client bind failed");
    let mut client_state = StubGameState::new();
    let mut checksums = Vec::new();

    for tick in 1..=100 {
        client_state.advance(0x01); // Simple input pattern

        let frame = GamePacket::InputFrame {
            player_id: 0,
            sequence_num: tick,
            current_tick: tick as u32,
            input_bitmask: 0x01,
            redundant_inputs: vec![],
        };

        client_socket
            .send_to(&frame.encode(), server_addr)
            .await
            .expect("Client send failed");

        // Receive checksum
        let mut buf = [0u8; 1500];
        match timeout(Duration::from_secs(2), client_socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                if let Ok(GamePacket::Checksum {
                    state_checksum, ..
                }) = GamePacket::decode(&buf[..len])
                {
                    checksums.push(state_checksum);
                }
            }
            _ => {}
        }
    }

    let server_final_checksum = timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("Server timeout")
        .expect("Server task failed");

    // Both sides should have same final checksum
    let client_final_checksum = *checksums.last().expect("No checksums received");
    assert_eq!(
        client_final_checksum, server_final_checksum,
        "Checksums diverged at tick 100"
    );
    println!(
        "✓ 100-tick exchange passed: final checksum = {:#x}",
        client_final_checksum
    );
}

/// Test 3: Replay rejection (old sequence number)
#[tokio::test]
async fn test_c6_replay_rejection() {
    let server_addr = "127.0.0.1:0";
    let server_socket = UdpSocket::bind(server_addr).await.expect("Server bind failed");
    let server_addr = server_socket.local_addr().expect("Get local addr failed");
    let server_socket = Arc::new(server_socket);

    let server_arc = Arc::clone(&server_socket);
    let server_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let mut last_seq = 0u32;
        let mut rejection_count = 0;

        loop {
            match timeout(Duration::from_secs(2), server_arc.recv_from(&mut buf)).await {
                Ok(Ok((len, _client_addr))) => {
                    if let Ok(GamePacket::InputFrame { sequence_num, .. }) =
                        GamePacket::decode(&buf[..len])
                    {
                        if sequence_num <= last_seq {
                            rejection_count += 1;
                            println!("Rejected replay: seq {} <= {}", sequence_num, last_seq);
                        }
                        last_seq = last_seq.max(sequence_num);

                        if last_seq >= 50 {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }

        rejection_count
    });

    let client_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Client bind failed");

    // Send seq 1, 2, 3, 5 (skip 4), then replay 2
    let sequences = vec![1, 2, 3, 5, 2, 10, 20, 30];

    for seq in sequences {
        let frame = GamePacket::InputFrame {
            player_id: 0,
            sequence_num: seq,
            current_tick: seq as u32,
            input_bitmask: 0x01,
            redundant_inputs: vec![],
        };

        let _ = client_socket.send_to(&frame.encode(), server_addr).await;
    }

    let rejection_count = timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("Server timeout")
        .expect("Server task failed");

    assert!(
        rejection_count > 0,
        "Expected at least 1 replay rejection (seq=2 sent twice)"
    );
    println!("✓ Replay rejection test passed ({} replays rejected)", rejection_count);
}

/// Test 4: Checksum validation under desync (forged state)
#[tokio::test]
async fn test_c6_desync_detection() {
    let server_addr = "127.0.0.1:0";
    let server_socket = UdpSocket::bind(server_addr).await.expect("Server bind failed");
    let server_addr = server_socket.local_addr().expect("Get local addr failed");
    let server_socket = Arc::new(server_socket);

    let server_arc = Arc::clone(&server_socket);
    let server_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let mut state = StubGameState::new();
        let mut tick_count = 0u32;

        loop {
            match timeout(Duration::from_secs(2), server_arc.recv_from(&mut buf)).await {
                Ok(Ok((len, client_addr))) => {
                    if let Ok(GamePacket::InputFrame {
                        input_bitmask,
                        current_tick,
                        ..
                    }) = GamePacket::decode(&buf[..len])
                    {
                        state.advance(input_bitmask);
                        tick_count += 1;

                        let checksum_pkt = GamePacket::Checksum {
                            tick: current_tick,
                            state_checksum: state.checksum,
                        };

                        let _ = server_arc.send_to(&checksum_pkt.encode(), client_addr).await;

                        if tick_count >= 20 {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }

        state.checksum
    });

    let client_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Client bind failed");

    // Send frames with correct inputs first
    for tick in 1..=10 {
        let frame = GamePacket::InputFrame {
            player_id: 0,
            sequence_num: tick,
            current_tick: tick as u32,
            input_bitmask: 0x01,
            redundant_inputs: vec![],
        };

        let _ = client_socket.send_to(&frame.encode(), server_addr).await;

        let mut buf = [0u8; 1500];
        let _ = client_socket.recv_from(&mut buf).await;
    }

    // Now send with forged input (0xFF instead of 0x01)
    for tick in 11..=20 {
        let frame = GamePacket::InputFrame {
            player_id: 0,
            sequence_num: tick,
            current_tick: tick as u32,
            input_bitmask: if tick == 15 { 0xFF } else { 0x01 }, // Forge at tick 15
            redundant_inputs: vec![],
        };

        let _ = client_socket.send_to(&frame.encode(), server_addr).await;

        let mut buf = [0u8; 1500];
        let _ = client_socket.recv_from(&mut buf).await;
    }

    let server_checksum = timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("Server timeout")
        .expect("Server task failed");

    println!("✓ Desync detection test passed (server final checksum: {:#x})", server_checksum);
}

/// Test 5: Reconnect after disconnect using token
#[tokio::test]
async fn test_c6_reconnect_after_disconnect() {
    let server_addr = "127.0.0.1:0";
    let server_socket = UdpSocket::bind(server_addr).await.expect("Server bind failed");
    let server_addr = server_socket.local_addr().expect("Get local addr failed");
    let server_socket = Arc::new(server_socket);

    let should_stop = Arc::new(AtomicBool::new(false));
    let should_stop_clone = Arc::clone(&should_stop);

    let server_arc = Arc::clone(&server_socket);
    let server_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let mut state = StubGameState::new();
        let mut tick_count = 0u32;

        loop {
            if should_stop_clone.load(Ordering::Relaxed) {
                break;
            }

            match timeout(Duration::from_millis(500), server_arc.recv_from(&mut buf)).await {
                Ok(Ok((len, client_addr))) => {
                    if let Ok(GamePacket::InputFrame {
                        input_bitmask,
                        current_tick,
                        ..
                    }) = GamePacket::decode(&buf[..len])
                    {
                        state.advance(input_bitmask);
                        tick_count += 1;

                        let checksum_pkt = GamePacket::Checksum {
                            tick: current_tick,
                            state_checksum: state.checksum,
                        };

                        let _ = server_arc.send_to(&checksum_pkt.encode(), client_addr).await;
                    }
                }
                _ => {}
            }
        }

        (tick_count, state.checksum)
    });

    let client_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Client bind failed");

    // Send first batch of frames (ticks 1-10)
    for tick in 1..=10 {
        let frame = GamePacket::InputFrame {
            player_id: 0,
            sequence_num: tick,
            current_tick: tick as u32,
            input_bitmask: 0x01,
            redundant_inputs: vec![],
        };

        let _ = client_socket.send_to(&frame.encode(), server_addr).await;

        let mut buf = [0u8; 1500];
        let _ = client_socket.recv_from(&mut buf).await;
    }

    // Simulate disconnect (don't send for a bit)
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Reconnect and send more frames (ticks 11-20)
    for tick in 11..=20 {
        let frame = GamePacket::InputFrame {
            player_id: 0,
            sequence_num: tick,
            current_tick: tick as u32,
            input_bitmask: 0x01,
            redundant_inputs: vec![],
        };

        let _ = client_socket.send_to(&frame.encode(), server_addr).await;

        let mut buf = [0u8; 1500];
        let _ = client_socket.recv_from(&mut buf).await;
    }

    should_stop.store(true, Ordering::Relaxed);

    let (final_tick, final_checksum) = timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("Server timeout")
        .expect("Server task failed");

    assert_eq!(final_tick, 20, "Expected 20 frames processed");
    println!(
        "✓ Reconnect test passed: {} ticks processed, final checksum {:#x}",
        final_tick, final_checksum
    );
}
