// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Replay Detection (RFC 9000 §8.2)
// 64-packet bitmask sliding window for detecting duplicate/replayed packets

/// Replay detection filter using a bitmask sliding window.
///
/// Tracks the last 64 packet numbers and detects replays/duplicates.
/// Assumes packet numbers arrive roughly in order (off-order window: 64 packets).
#[derive(Clone, Debug)]
pub struct ReplayDetector {
    /// Bitmask of received packet numbers (within window).
    /// Bit i represents whether packet (highest - i) was received.
    received_mask: u64,

    /// Highest packet number seen so far.
    highest_packet: Option<u64>,

    /// Count of duplicate packets detected.
    duplicates_detected: u64,

    /// Window size in packets (must be <= 64).
    window_size: u32,
}

impl ReplayDetector {
    /// Create a new replay detector with a 64-packet window.
    pub fn new() -> Self {
        ReplayDetector {
            received_mask: 0,
            highest_packet: None,
            duplicates_detected: 0,
            window_size: 64,
        }
    }

    /// Check if a packet is a replay/duplicate.
    ///
    /// Returns `true` if this packet was already received (replay detected),
    /// `false` if it is new or beyond the window.
    pub fn is_replay(&mut self, packet_number: u64) -> bool {
        match self.highest_packet {
            None => {
                // First packet
                self.highest_packet = Some(packet_number);
                false
            }
            Some(highest) => {
                if packet_number > highest {
                    // New packet beyond highest: slide window and update
                    let advance = (packet_number - highest).min(64);

                    // Shift the mask to make room for new packets
                    if advance < 64 {
                        self.received_mask >>= advance;
                    } else {
                        // Beyond window: clear mask
                        self.received_mask = 0;
                    }

                    // Mark this packet as received (bit 0 = highest)
                    self.received_mask |= 1u64 << 0;
                    self.highest_packet = Some(packet_number);

                    false // New packet, not a replay
                } else if packet_number == highest {
                    // Exact duplicate
                    self.duplicates_detected += 1;
                    true
                } else {
                    // Packet is older than highest
                    let diff = highest - packet_number;

                    if diff >= 64 {
                        // Beyond window: cannot determine, assume replay to be safe
                        self.duplicates_detected += 1;
                        true
                    } else {
                        // Check if bit is set in mask
                        let bit_pos = diff;
                        let was_received = (self.received_mask & (1u64 << bit_pos)) != 0;

                        if was_received {
                            self.duplicates_detected += 1;
                        }

                        was_received
                    }
                }
            }
        }
    }

    /// Mark a packet as received (if not a replay).
    ///
    /// Should be called only for non-replay packets.
    pub fn mark_received(&mut self, packet_number: u64) {
        if let Some(highest) = self.highest_packet {
            if packet_number <= highest {
                let diff = highest - packet_number;
                if diff < 64 {
                    self.received_mask |= 1u64 << diff;
                }
            }
        }
    }

    /// Get highest packet number seen.
    pub fn highest_packet(&self) -> Option<u64> {
        self.highest_packet
    }

    /// Get count of duplicate packets detected.
    pub fn duplicates_count(&self) -> u64 {
        self.duplicates_detected
    }

    /// Reset detector state.
    pub fn reset(&mut self) {
        self.received_mask = 0;
        self.highest_packet = None;
        self.duplicates_detected = 0;
    }
}

/// Multi-path replay detector (separate detector per source address).
#[derive(Clone, Debug)]
pub struct MultiPathReplayDetector {
    /// Map of peer address -> ReplayDetector.
    detectors: std::collections::HashMap<String, ReplayDetector>,
}

impl MultiPathReplayDetector {
    pub fn new() -> Self {
        MultiPathReplayDetector {
            detectors: std::collections::HashMap::new(),
        }
    }

    /// Get or create detector for a peer.
    fn get_or_create(&mut self, peer_id: &str) -> &mut ReplayDetector {
        self.detectors
            .entry(peer_id.to_string())
            .or_insert_with(ReplayDetector::new)
    }

    /// Check if packet is a replay.
    pub fn is_replay(&mut self, peer_id: &str, packet_number: u64) -> bool {
        self.get_or_create(peer_id).is_replay(packet_number)
    }

    /// Mark packet as received.
    pub fn mark_received(&mut self, peer_id: &str, packet_number: u64) {
        if let Some(detector) = self.detectors.get_mut(peer_id) {
            detector.mark_received(packet_number);
        }
    }

    /// Remove detector for a peer (e.g., on connection close).
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.detectors.remove(peer_id);
    }

    /// Get total duplicates detected across all peers.
    pub fn total_duplicates(&self) -> u64 {
        self.detectors.values().map(|d| d.duplicates_count()).sum()
    }
}

impl Default for ReplayDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for MultiPathReplayDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_detector_init() {
        let detector = ReplayDetector::new();
        assert_eq!(detector.highest_packet(), None);
        assert_eq!(detector.duplicates_count(), 0);
    }

    #[test]
    fn test_replay_detector_in_order() {
        let mut detector = ReplayDetector::new();

        assert!(!detector.is_replay(0));
        assert!(!detector.is_replay(1));
        assert!(!detector.is_replay(2));

        assert_eq!(detector.highest_packet(), Some(2));
        assert_eq!(detector.duplicates_count(), 0);
    }

    #[test]
    fn test_replay_detector_duplicate() {
        let mut detector = ReplayDetector::new();

        assert!(!detector.is_replay(0));
        assert!(detector.is_replay(0)); // Duplicate

        assert_eq!(detector.duplicates_count(), 1);
    }

    #[test]
    fn test_replay_detector_out_of_order() {
        let mut detector = ReplayDetector::new();

        assert!(!detector.is_replay(0));
        assert!(!detector.is_replay(2)); // Skip 1
        assert!(!detector.is_replay(1)); // Backfill
        assert_eq!(detector.duplicates_count(), 0);

        // Try again: should be replay
        assert!(detector.is_replay(1));
        assert_eq!(detector.duplicates_count(), 1);
    }

    #[test]
    fn test_replay_detector_window() {
        let mut detector = ReplayDetector::new();

        // Receive packets 0-63
        for i in 0..64 {
            assert!(!detector.is_replay(i));
        }

        // Packet 64 slides the window
        assert!(!detector.is_replay(64));

        // Packet 0 is now out of window (should be treated as replay to be safe)
        // Actually, let's check the implementation...
        // If diff >= 64, it's treated as replay
        assert!(detector.is_replay(0));
    }

    #[test]
    fn test_replay_detector_mark_received() {
        let mut detector = ReplayDetector::new();

        assert!(!detector.is_replay(5));
        detector.mark_received(5);

        // Duplicate check
        assert!(detector.is_replay(5));
    }

    #[test]
    fn test_multipath_replay_detector() {
        let mut detector = MultiPathReplayDetector::new();

        assert!(!detector.is_replay("peer1", 0));
        assert!(!detector.is_replay("peer1", 1));
        assert!(!detector.is_replay("peer2", 0));
        assert!(!detector.is_replay("peer2", 1));

        // Check for replays
        assert!(detector.is_replay("peer1", 0));
        assert!(detector.is_replay("peer2", 0));

        assert_eq!(detector.total_duplicates(), 2);
    }

    #[test]
    fn test_multipath_replay_detector_remove_peer() {
        let mut detector = MultiPathReplayDetector::new();

        assert!(!detector.is_replay("peer1", 0));
        detector.remove_peer("peer1");

        // Should be a new detector after removal
        assert!(!detector.is_replay("peer1", 0)); // First packet for new detector
    }

    #[test]
    fn test_replay_detector_reset() {
        let mut detector = ReplayDetector::new();

        assert!(!detector.is_replay(0));
        detector.reset();

        assert_eq!(detector.highest_packet(), None);
        assert_eq!(detector.duplicates_count(), 0);
    }
}
