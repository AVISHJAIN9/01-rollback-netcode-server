// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Production-Grade Core (RFC 9000 & RFC 9002)
// Autonomous Transport Engine with Loss Recovery, Congestion Control, Flow Control, and Path Management

pub mod wire_interface;
pub mod recovery;
pub mod congestion;
pub mod stream;
pub mod path;
pub mod runtime;
pub mod security;

pub use wire_interface::*;
pub use recovery::{rtt::RttEstimator, loss::LossDetector};
pub use congestion::{
    CongestionController, CongestionEvent, CongestionState, CongestionTelemetry, NewReno, Cubic,
    TokenBucketPacer, PriorityPacer, BdpEstimator,
};
pub use stream::id::{StreamIdAllocator, StreamRole, StreamDir};
pub use stream::buffer::{StreamReceiveBuffer, StreamSendBuffer, BufferError};
pub use stream::flow_control::{StreamFlowControl, StreamFlowControlManager, FlowControlError};
pub use path::PathManager;
pub use runtime::udp::{BufferPool, UdpEventLoop, PacketHandler};
pub use security::anti_amplification::{AntiAmplificationDefense, AntiAmplificationManager};
pub use security::replay::{ReplayDetector, MultiPathReplayDetector};

/// Mini-QUIC Transport Engine: Integrated core transport layer.
///
/// This module provides a complete, production-grade QUIC transport engine
/// implementing RFC 9000 (QUIC Protocol) and RFC 9002 (QUIC Loss Detection and Congestion Control).
///
/// # Features
///
/// ## Loss Recovery (RFC 9002 §5-7)
/// - RTT Estimation: Smoothed RTT (SRTT), RTT Variation (RTTVAR), Minimum RTT
/// - Probe Timeout (PTO): With exponential backoff for consecutive losses
/// - Loss Detection: Packet-threshold (3 packets) and time-threshold (9/8 RTT) methods
/// - Duplicate ACK tracking for fast recovery
///
/// ## Congestion Control (RFC 9002 §7)
/// - Pluggable `CongestionController` trait for algorithm modularity
/// - NewReno: Slow Start, Congestion Avoidance, Fast Recovery
/// - CUBIC: RFC-compliant cubic window growth with TCP-friendly mode
/// - Bandwidth-Delay Product (BDP) estimator for probing
/// - Token-bucket pacing with microsecond precision
/// - Multi-priority packet pacer for frame prioritization
/// - ECN (Explicit Congestion Notification) signal reaction
///
/// ## Streams & Flow Control (RFC 9000 §2-4)
/// - Stream ID allocator for Client/Server, Bidirectional/Unidirectional spaces
/// - Stream receive buffer: Out-of-order reassembly with sparse interval tracking
/// - Stream send buffer: Chunked transmission with flow-control credit consumption
/// - Connection-level `MAX_DATA` and stream-level `MAX_STREAM_DATA` credit managers
/// - Auto-tuning flow control windows (consumer-driven advertisement)
/// - Stream state lifecycle: Open, LocalFinished, RemoteFinished, Closed, ResetSent, ResetReceived
///
/// ## Connection Migration & Path Management (RFC 9000 §9)
/// - Multi-path tracking: Path state, RTT, MTU discovery, packet counters
/// - PATH_CHALLENGE/PATH_RESPONSE generation and validation
/// - Connection migration triggers and path validation logic
/// - Idle path cleanup and multi-path metrics
///
/// ## Async Socket Runtime & Zero-Copy IO
/// - Tokio-based async UDP event loop with batch send/receive scaffolding
/// - Recycled buffer pool for zero-allocation packet ingest
/// - UDP datagram MTU discovery (1200-byte Initial floor)
/// - Packet size limits and fragmentation handling
///
/// ## Protocol Hardening & Failure Defenses
/// - Anti-amplification defense: 3x received byte quota on unvalidated paths
/// - Replay filter: 64-packet bitmask sliding window for duplicate detection
/// - Resource exhaustion mitigations: Concurrent stream limits, idle connection reaping
/// - Transport Error & Crypto Error code taxonomy (RFC 9000 §20)
///
/// # Example Usage
///
/// ```ignore
/// use mini_quic_engine_core::*;
///
/// // Create RTT estimator
/// let mut rtt_est = RttEstimator::new(Duration::from_millis(25));
/// rtt_est.update_sample(Duration::from_millis(100));
///
/// // Create NewReno congestion controller
/// let mut cc = NewReno::new(1200);
/// cc.on_init(1200);
///
/// // Create loss detector
/// let mut loss_detector = LossDetector::new();
/// loss_detector.on_packet_sent(0, Instant::now(), 1200, false);
///
/// // Create stream ID allocator (client side)
/// let mut stream_allocator = StreamIdAllocator::new(StreamRole::Client, 100, 50);
/// let stream_id = stream_allocator.allocate(StreamDir::Bidirectional).unwrap();
///
/// // Create flow control manager
/// let mut flow_ctrl = StreamFlowControlManager::new(1000000, 100000);
/// let stream_fc = flow_ctrl.get_or_create(stream_id);
///
/// // Create path manager
/// let mut path_mgr = PathManager::new("127.0.0.1:1234".parse().unwrap(), 1200);
///
/// // Create anti-amplification defense
/// let mut anti_amp = AntiAmplificationDefense::new();
/// anti_amp.on_bytes_received(100);
/// assert!(anti_amp.can_send(300));
///
/// // Create replay detector
/// let mut replay_detector = ReplayDetector::new();
/// assert!(!replay_detector.is_replay(0));
/// assert!(replay_detector.is_replay(0)); // Duplicate
/// ```
///
/// # Architecture Notes
///
/// - **Modularity**: Each transport subsystem (RTT, loss detection, congestion control,
///   flow control, path management) is independently testable and replaceable.
/// - **Zero-Copy**: The buffer pool enables packet reception without heap allocation.
/// - **RFC Compliance**: All algorithms implement RFC 9000 and RFC 9002 verbatim,
///   with explicit equation numbers in comments.
/// - **Production Ready**: Full telemetry counters, comprehensive error handling,
///   and extensive test coverage.
///
/// # Security Considerations
///
/// - Anti-amplification limits prevent reflection attacks on unvalidated paths.
/// - Replay detection catches duplicate or out-of-order packets.
/// - Flow control prevents sender/receiver resource exhaustion.
/// - Connection migration validation ensures path legitimacy.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_integration_rtt_and_loss() {
        let mut rtt_est = RttEstimator::new(Duration::from_millis(25));
        let mut loss_detector = LossDetector::new();
        let now = std::time::Instant::now();

        // Simulate packet send and ACK
        loss_detector.on_packet_sent(0, now, 1200, false);
        rtt_est.update_sample(Duration::from_millis(100));

        let (acked, largest) = loss_detector.on_ack_received(0, 0, Duration::from_millis(25));
        assert_eq!(acked.len(), 1);
        assert_eq!(largest, Some(0));
    }

    #[test]
    fn test_integration_stream_flow_control() {
        let mut allocator = StreamIdAllocator::new(StreamRole::Client, 100, 50);
        let mut flow_ctrl = StreamFlowControlManager::new(1000000, 100000);

        let stream_id = allocator.allocate(StreamDir::Bidirectional).unwrap();
        let stream_fc = flow_ctrl.get_or_create(stream_id);

        assert!(stream_fc.can_send(50000));
        assert!(!stream_fc.can_send(100001));
    }

    #[test]
    fn test_integration_anti_amplification() {
        let mut defense = AntiAmplificationDefense::new();

        defense.on_bytes_received(1000);
        assert!(defense.can_send(3000));
        assert!(!defense.can_send(3001));

        defense.on_bytes_sent(2000);
        assert_eq!(defense.available_quota(), 1000);

        defense.validate_path();
        assert!(defense.can_send(1000000));
    }

    #[test]
    fn test_integration_replay_detection() {
        let mut replay = ReplayDetector::new();
        let mut mgr = MultiPathReplayDetector::new();

        assert!(!replay.is_replay(0));
        assert!(replay.is_replay(0));

        assert!(!mgr.is_replay("peer1", 0));
        assert!(mgr.is_replay("peer1", 0));
        assert_eq!(mgr.total_duplicates(), 1);
    }

    #[test]
    fn test_integration_congestion_control_newreno() {
        let mut cc = NewReno::new(1200);
        cc.on_init(1200);

        assert!(cc.can_send(0));
        assert!(cc.can_send(6000));
        assert!(!cc.can_send(12000));

        cc.on_ack(1200, Duration::from_millis(50));
        assert_eq!(cc.congestion_window(), 12000 + 1200);
    }

    #[test]
    fn test_integration_cubic() {
        let mut cc = Cubic::new(1200);
        cc.on_init(1200);

        let initial_window = cc.congestion_window();
        cc.on_ack(1200, Duration::from_millis(50));
        assert!(cc.congestion_window() > initial_window);
    }

    #[test]
    fn test_integration_token_bucket_pacer() {
        let mut pacer = TokenBucketPacer::new(10000, 100_000.0);
        let now = std::time::Instant::now();

        let wait = pacer.send(500, now);
        assert_eq!(wait, Duration::from_secs(0));
        assert_eq!(pacer.bytes_paced(), 500);
    }
}
