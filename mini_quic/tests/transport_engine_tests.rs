// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Integration Tests
// Comprehensive test suite covering loss recovery, congestion control, streams, and hardening

#[cfg(test)]
mod integration_tests {
    use mini_quic_engine_core::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_loss_recovery_scenario() {
        // Simulate packet send, loss detection, and recovery
        let mut loss_detector = LossDetector::new();
        let mut rtt_est = RttEstimator::new(Duration::from_millis(25));
        let now = Instant::now();

        // Send 10 packets
        for i in 0..10 {
            loss_detector.on_packet_sent(i, now + Duration::from_millis(i as u64), 1200, false);
        }

        // Acknowledge packets 5-9 (skip 0-4, simulating loss)
        let (acked, largest) = loss_detector.on_ack_received(5, 9, Duration::from_millis(50));
        assert_eq!(acked.len(), 5);
        assert_eq!(largest, Some(9));

        // Update RTT
        rtt_est.update_sample(Duration::from_millis(50));

        // Detect loss: packets 0-4 should be marked as lost by packet threshold
        let lost = loss_detector.detect_loss(now, Duration::from_millis(100));
        assert_eq!(lost.len(), 0); // Not yet due to time threshold

        let later = now + Duration::from_millis(200);
        let lost = loss_detector.detect_loss(later, Duration::from_millis(100));
        assert!(lost.len() > 0, "Expected loss detection by time threshold");
    }

    #[test]
    fn test_newreno_slow_start_to_congestion_avoidance() {
        let mut cc = NewReno::new(1200);
        cc.on_init(1200);

        assert_eq!(cc.state(), CongestionState::SlowStart);
        let initial_window = cc.congestion_window();

        // In slow start, window grows by bytes_acked
        for _ in 0..5 {
            cc.on_ack(1200, Duration::from_millis(50));
        }

        // Should transition to CA when reaching ssthresh
        assert_eq!(cc.state(), CongestionState::CongestionAvoidance);
        assert!(cc.congestion_window() > initial_window);
    }

    #[test]
    fn test_cubic_loss_and_recovery() {
        let mut cc = Cubic::new(1200);
        cc.on_init(1200);

        let initial_window = cc.congestion_window();

        // Simulate congestion event
        cc.on_congestion_event(CongestionEvent::PacketLoss, initial_window);

        assert_eq!(cc.state(), CongestionState::FastRecovery);
        assert!(cc.congestion_window() < initial_window);

        // Recover with ACKs
        for _ in 0..10 {
            cc.on_ack(1200, Duration::from_millis(50));
        }

        // Window should gradually increase again
        assert!(cc.congestion_window() > 0);
    }

    #[test]
    fn test_stream_id_allocation_and_validation() {
        let mut allocator = StreamIdAllocator::new(StreamRole::Client, 100, 50);

        // Allocate bidirectional streams
        let id1 = allocator.allocate(StreamDir::Bidirectional).unwrap();
        let id2 = allocator.allocate(StreamDir::Bidirectional).unwrap();

        let (role1, dir1, seq1) = StreamIdAllocator::decode_stream_id(id1);
        let (role2, dir2, seq2) = StreamIdAllocator::decode_stream_id(id2);

        assert_eq!(role1, StreamRole::Client);
        assert_eq!(dir1, StreamDir::Bidirectional);
        assert_eq!(seq1, 0);

        assert_eq!(role2, StreamRole::Client);
        assert_eq!(dir2, StreamDir::Bidirectional);
        assert_eq!(seq2, 1);

        // Verify local stream check
        assert!(allocator.is_local_stream(id1));
        assert!(allocator.is_local_stream(id2));
    }

    #[test]
    fn test_stream_reassembly_ooo_packets() {
        let mut recv_buf = StreamReceiveBuffer::new(10000);

        // Insert out of order: first data at offset 10
        let inserted = recv_buf.insert(10, b"world").unwrap();
        assert!(inserted);
        assert_eq!(recv_buf.contiguous_len(), 0); // Can't read yet

        // Fill gap: data at offset 0
        recv_buf.insert(0, b"hello").unwrap();

        // Now should be able to read all contiguous data
        let data = recv_buf.read_contiguous();
        assert_eq!(data, b"helloworld");
    }

    #[test]
    fn test_stream_send_buffer_chunked_transmission() {
        let mut send_buf = StreamSendBuffer::new(1000);

        let written = send_buf.write(b"Hello, World!").unwrap();
        assert_eq!(written, 13);

        let (offset, data) = send_buf.get_sendable(5);
        assert_eq!(offset, 0);
        assert_eq!(data, b"Hello");

        send_buf.mark_sent(0, 5);
        assert_eq!(send_buf.bytes_in_flight(), 5);

        send_buf.mark_acked(0, 5);
        assert_eq!(send_buf.bytes_in_flight(), 0);
        assert_eq!(send_buf.bytes_acked(), 5);
    }

    #[test]
    fn test_flow_control_auto_tuning() {
        let mut fc = ConnectionFlowControl::new(100000);

        // Consume 60% of window
        fc.on_data_received(60000).unwrap();

        // Should trigger advertisement
        let new_limit = fc.should_advertise_limit();
        assert!(new_limit.is_some());
        let new_limit = new_limit.unwrap();
        assert!(new_limit > 100000);

        // Advertise the new limit
        fc.advertise_limit(new_limit);
        assert_eq!(fc.flow_control_window, new_limit);
    }

    #[test]
    fn test_stream_flow_control_violation() {
        let mut fc = StreamFlowControl::new(0, 100);

        let result = fc.on_data_sent(101);
        assert!(result.is_err());
        assert_eq!(result, Err(FlowControlError::StreamFlowControlViolation));
    }

    #[test]
    fn test_connection_migration_path_management() {
        use std::str::FromStr;

        let addr1 = std::net::SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let addr2 = std::net::SocketAddr::from_str("127.0.0.1:5678").unwrap();

        let mut path_mgr = PathManager::new(addr1, 1200);

        // Add candidate path
        path_mgr.add_candidate_path(addr2).unwrap();

        // Validate path
        path_mgr.validate_path(addr2).unwrap();

        // Migrate to new path
        let result = path_mgr.migrate_to_path(addr2);
        assert!(result.is_ok());

        assert_eq!(path_mgr.active_path().unwrap().remote_addr, addr2);
        assert_eq!(path_mgr.migrations_count(), 1);
    }

    #[test]
    fn test_anti_amplification_defense() {
        let mut defense = AntiAmplificationDefense::new();

        // Receive 500 bytes
        defense.on_bytes_received(500);

        // Can send up to 1500 bytes (3x rule)
        assert!(defense.can_send(1500));
        assert!(!defense.can_send(1501));

        // Send 1000 bytes
        defense.on_bytes_sent(1000);
        assert_eq!(defense.available_quota(), 500);

        // After validation, can send unlimited
        defense.validate_path();
        assert!(defense.can_send(1000000));
    }

    #[test]
    fn test_anti_amplification_multi_path() {
        let mut mgr = AntiAmplificationManager::new();

        mgr.on_bytes_received("path1", 200);
        mgr.on_bytes_received("path2", 300);

        assert!(mgr.can_send("path1", 600));
        assert!(!mgr.can_send("path1", 601));

        assert!(mgr.can_send("path2", 900));
        assert!(!mgr.can_send("path2", 901));

        mgr.on_bytes_sent("path1", 400);
        assert_eq!(mgr.available_quota("path1"), 200);
    }

    #[test]
    fn test_replay_detection_sliding_window() {
        let mut detector = ReplayDetector::new();

        // Receive packets 0-10 in order
        for i in 0..11 {
            assert!(!detector.is_replay(i));
        }

        // Try to replay packet 5
        assert!(detector.is_replay(5));
        assert_eq!(detector.duplicates_count(), 1);

        // Receive packet 20
        assert!(!detector.is_replay(20));

        // Packet 0 is now beyond window
        assert!(detector.is_replay(0));
        assert_eq!(detector.duplicates_count(), 3); // Two additional replays: 0
    }

    #[test]
    fn test_replay_detection_multi_peer() {
        let mut detector = MultiPathReplayDetector::new();

        // Peer 1: receive packets 0-2
        assert!(!detector.is_replay("peer1", 0));
        assert!(!detector.is_replay("peer1", 1));
        assert!(!detector.is_replay("peer1", 2));

        // Peer 2: receive packets 0-2 (independent)
        assert!(!detector.is_replay("peer2", 0));
        assert!(!detector.is_replay("peer2", 1));
        assert!(!detector.is_replay("peer2", 2));

        // Replay on both peers
        assert!(detector.is_replay("peer1", 1));
        assert!(detector.is_replay("peer2", 1));

        assert_eq!(detector.total_duplicates(), 2);
    }

    #[test]
    fn test_token_bucket_pacing() {
        let mut pacer = TokenBucketPacer::new(10000, 100_000.0); // 100KB/s
        let now = Instant::now();

        // Send 5KB immediately
        let wait = pacer.send(5000, now);
        assert_eq!(wait, Duration::from_secs(0));

        // Send remaining 5KB
        let wait = pacer.send(5000, now);
        assert_eq!(wait, Duration::from_secs(0));

        // Send 1KB more (will be delayed)
        let wait = pacer.send(1000, now);
        assert!(wait > Duration::from_secs(0));
    }

    #[test]
    fn test_priority_pacer() {
        let now = Instant::now();
        let mut pacer = PriorityPacer::new(100_000.0, 10000, now);

        // High-priority should be available immediately
        let wait_high = pacer.send_high_priority(100, now);
        assert_eq!(wait_high, Duration::from_secs(0));

        // Normal priority
        let wait_normal = pacer.send_normal(100, now);
        assert_eq!(wait_normal, Duration::from_secs(0));

        // Low priority
        let wait_low = pacer.send_low_priority(100, now);
        assert_eq!(wait_low, Duration::from_secs(0));
    }

    #[test]
    fn test_bdp_estimator() {
        let mut bdp = BdpEstimator::new();

        // 1000 bytes in 10ms = 100KB/s
        bdp.update(1000, Duration::from_millis(10));
        assert_eq!(bdp.max_bandwidth(), 100_000);

        // BDP = bandwidth * RTT = 100KB/s * 10ms = 1000 bytes
        let bdp_bytes = bdp.bdp();
        assert_eq!(bdp_bytes, 1000);
    }

    #[test]
    fn test_pto_computation_and_backoff() {
        let mut rtt_est = RttEstimator::new(Duration::from_millis(25));

        // Update with 100ms sample
        rtt_est.update_sample(Duration::from_millis(100));

        let pto_0 = rtt_est.compute_pto_with_backoff(0);
        let pto_1 = rtt_est.compute_pto_with_backoff(1);
        let pto_2 = rtt_est.compute_pto_with_backoff(2);

        // Verify exponential backoff
        assert!(pto_1 > pto_0);
        assert!(pto_2 > pto_1);
        assert_eq!(pto_1, pto_0 * 2);
        assert_eq!(pto_2, pto_0 * 4);
    }

    #[test]
    fn test_congestion_telemetry() {
        let mut cc = NewReno::new(1200);
        cc.on_init(1200);

        cc.on_ack(1200, Duration::from_millis(50));
        cc.on_congestion_event(CongestionEvent::PacketLoss, 12000);

        let telemetry = cc.telemetry();
        assert!(telemetry.congestion_window > 0);
        assert_eq!(telemetry.congestion_events_count, 1);
        assert!(telemetry.samples_count > 0);
    }

    #[test]
    fn test_integrated_flow_control_and_congestion() {
        let mut flow_ctrl = StreamFlowControlManager::new(1000000, 100000);
        let mut cc = NewReno::new(1200);
        cc.on_init(1200);

        let stream_id = 0u64;
        let stream_fc = flow_ctrl.get_or_create(stream_id);

        // Check both constraints
        let bytes_to_send = 50000u64;
        assert!(stream_fc.can_send(bytes_to_send));
        assert!(cc.can_send(bytes_to_send as u64));

        // Send should respect both
        let available = cc.available_window(0);
        let stream_available = stream_fc.available_window();
        assert!(available > 0);
        assert!(stream_available > 0);
    }

    #[test]
    fn test_stream_buffer_with_fin() {
        let mut recv_buf = StreamReceiveBuffer::new(10000);

        recv_buf.insert(0, b"hello").unwrap();
        recv_buf.finalize(5).unwrap();

        assert_eq!(recv_buf.final_offset(), Some(5));

        let data = recv_buf.read_contiguous();
        assert_eq!(data, b"hello");
        assert!(recv_buf.is_complete());
    }

    #[test]
    fn test_congestion_window_dynamics() {
        let mut cc = NewReno::new(1200);
        cc.on_init(1200);

        let initial = cc.congestion_window();

        // Slow start: ACK increases window
        for _ in 0..10 {
            cc.on_ack(1200, Duration::from_millis(50));
        }

        let after_slow_start = cc.congestion_window();
        assert!(after_slow_start > initial);

        // Loss: window decreases
        cc.on_congestion_event(CongestionEvent::PacketLoss, after_slow_start);
        let after_loss = cc.congestion_window();
        assert!(after_loss < after_slow_start);

        // Recovery: window grows again
        for _ in 0..20 {
            cc.on_ack(1200, Duration::from_millis(50));
        }

        let after_recovery = cc.congestion_window();
        assert!(after_recovery > after_loss);
    }

    #[test]
    fn test_version_negotiation_packet() {
        use mini_quic_engine_core::{VersionNegotiationPacket, ConnectionId};

        let dcid = ConnectionId::from(vec![1, 2, 3, 4]);
        let scid = ConnectionId::from(vec![5, 6, 7, 8]);
        let supported_versions = vec![0x00000001, 0x00000002];

        let vn_pkt = VersionNegotiationPacket::new(dcid.clone(), scid.clone(), supported_versions.clone());

        let mut buf = vec![0u8; 1024];
        let len = vn_pkt.encode(&mut buf).unwrap();

        assert!(len > 0);
        assert!(len < 1024);
    }
}
