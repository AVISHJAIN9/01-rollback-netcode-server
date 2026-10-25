// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Loss Detection (RFC 9002 §6)
// Packet number space tracking, ACK range processing, and loss threshold detection

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Sent packet for loss tracking and RTT sampling.
#[derive(Clone, Debug)]
pub struct SentPacket {
    /// Packet number in the packet number space.
    pub packet_number: u64,

    /// Time packet was sent.
    pub sent_time: Instant,

    /// Number of bytes in this packet (including UDP header for congestion control).
    pub size: usize,

    /// Whether this packet is in-flight (not yet acknowledged or declared lost).
    pub in_flight: bool,

    /// Whether this packet was sent before a PTO timer fired.
    pub is_pto_probe: bool,

    /// Lost packet declaration time (if set, packet is lost).
    pub lost_time: Option<Instant>,
}

/// Loss Detector implementing RFC 9002 §6 packet threshold and time threshold algorithms.
#[derive(Clone, Debug)]
pub struct LossDetector {
    /// Map of packet numbers to sent packets in this packet number space.
    /// Entries are removed when packets are acked or declared lost.
    sent_packets: BTreeMap<u64, SentPacket>,

    /// Largest packet number acknowledged so far.
    largest_acked: Option<u64>,

    /// Count of duplicate ACKs received (same largest_acked value).
    duplicate_ack_count: u32,

    /// Threshold for packet-number-based loss detection.
    /// RFC 9002 suggests 3 packets.
    packet_threshold: u64,

    /// Multiplier for time-based loss detection.
    /// RFC 9002 suggests 9/8 (1.125x SRTT).
    time_threshold_multiplier: f64,
}

impl LossDetector {
    /// Create a new loss detector.
    pub fn new() -> Self {
        LossDetector {
            sent_packets: BTreeMap::new(),
            largest_acked: None,
            duplicate_ack_count: 0,
            packet_threshold: 3,
            time_threshold_multiplier: 9.0 / 8.0,
        }
    }

    /// Register a sent packet for tracking.
    pub fn on_packet_sent(
        &mut self,
        packet_number: u64,
        sent_time: Instant,
        size: usize,
        is_pto_probe: bool,
    ) {
        self.sent_packets.insert(
            packet_number,
            SentPacket {
                packet_number,
                sent_time,
                size,
                in_flight: true,
                is_pto_probe,
                lost_time: None,
            },
        );
    }

    /// Process an ACK frame covering the given range of packet numbers.
    ///
    /// Returns a tuple of (newly_acked_packets, largest_acked_packet).
    pub fn on_ack_received(
        &mut self,
        ack_range_start: u64,
        ack_range_end: u64,
        _ack_delay: Duration,
    ) -> (Vec<SentPacket>, Option<u64>) {
        let mut newly_acked = Vec::new();

        // Validate ACK range
        if ack_range_start > ack_range_end {
            return (newly_acked, None);
        }

        // Update largest_acked
        let prev_largest = self.largest_acked;
        let new_largest = Some(ack_range_end);

        // Check for duplicate ACK
        if prev_largest == new_largest {
            self.duplicate_ack_count += 1;
        } else {
            self.duplicate_ack_count = 0;
            self.largest_acked = new_largest;
        }

        // Mark packets in range as acknowledged
        for pn in ack_range_start..=ack_range_end {
            if let Some(mut sent_pkt) = self.sent_packets.remove(&pn) {
                sent_pkt.in_flight = false;
                newly_acked.push(sent_pkt);
            }
        }

        (newly_acked, new_largest)
    }

    /// Detect lost packets using packet-threshold and time-threshold methods.
    ///
    /// RFC 9002 §6.1: Packets are lost if:
    /// 1. Packet threshold: packet_number <= largest_acked - packet_threshold, or
    /// 2. Time threshold: time_sent + (SRTT * 9/8) < now
    pub fn detect_loss(&mut self, now: Instant, srtt: Duration) -> Vec<SentPacket> {
        let mut lost_packets = Vec::new();

        let time_threshold = srtt.saturating_mul(9) / 8;

        // Iterate over in-flight packets
        let pns_to_check: Vec<u64> = self
            .sent_packets
            .iter()
            .filter(|(_, pkt)| pkt.in_flight && pkt.lost_time.is_none())
            .map(|(pn, _)| *pn)
            .collect();

        for pn in pns_to_check {
            if let Some(sent_pkt) = self.sent_packets.get_mut(&pn) {
                let is_packet_threshold_lost = self.largest_acked.map_or(false, |largest| {
                    (largest as i64 - pn as i64) >= self.packet_threshold as i64
                });

                let is_time_threshold_lost = now.duration_since(sent_pkt.sent_time) > time_threshold;

                if is_packet_threshold_lost || is_time_threshold_lost {
                    sent_pkt.lost_time = Some(now);
                    sent_pkt.in_flight = false;
                }
            }
        }

        // Collect lost packets
        let pns_to_remove: Vec<u64> = self.sent_packets.iter()
            .filter(|(_, pkt)| pkt.lost_time.is_some() && !pkt.in_flight)
            .map(|(pn, _)| *pn)
            .collect();
        
        for pn in pns_to_remove {
            if let Some(p) = self.sent_packets.remove(&pn) {
                lost_packets.push(p);
            }
        }

        lost_packets
    }

    /// Get next packet number for this packet number space.
    pub fn next_packet_number(&self) -> u64 {
        self.sent_packets.keys().next_back().copied().unwrap_or(0) + 1
    }

    /// Get the largest acknowledged packet number.
    pub fn largest_acked(&self) -> Option<u64> {
        self.largest_acked
    }

    /// Get in-flight packets.
    pub fn in_flight_packets(&self) -> Vec<&SentPacket> {
        self.sent_packets
            .values()
            .filter(|pkt| pkt.in_flight)
            .collect()
    }

    /// Get total bytes in flight.
    pub fn bytes_in_flight(&self) -> usize {
        self.in_flight_packets().iter().map(|pkt| pkt.size).sum()
    }

    /// Remove acknowledged packets older than a threshold to free memory.
    pub fn remove_acked_before(&mut self, threshold_time: Instant) {
        self.sent_packets.retain(|_, pkt| {
            pkt.in_flight || pkt.sent_time > threshold_time || pkt.lost_time.is_some()
        });
    }

    /// Get packet threshold value.
    pub fn packet_threshold(&self) -> u64 {
        self.packet_threshold
    }

    /// Set packet threshold (default 3).
    pub fn set_packet_threshold(&mut self, threshold: u64) {
        self.packet_threshold = threshold;
    }

    /// Get count of in-flight packets.
    pub fn in_flight_count(&self) -> usize {
        self.sent_packets.values().filter(|pkt| pkt.in_flight).count()
    }
}

impl Default for LossDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loss_detector_initialization() {
        let detector = LossDetector::new();
        assert_eq!(detector.largest_acked(), None);
        assert_eq!(detector.in_flight_count(), 0);
    }

    #[test]
    fn test_send_and_ack() {
        let mut detector = LossDetector::new();
        let now = Instant::now();

        detector.on_packet_sent(0, now, 1200, false);
        detector.on_packet_sent(1, now, 1200, false);

        let (acked, largest) = detector.on_ack_received(0, 1, Duration::from_millis(10));
        assert_eq!(acked.len(), 2);
        assert_eq!(largest, Some(1));
        assert_eq!(detector.in_flight_count(), 0);
    }

    #[test]
    fn test_packet_threshold_loss_detection() {
        let mut detector = LossDetector::new();
        let now = Instant::now();

        // Send packets 0-5
        for i in 0..6 {
            detector.on_packet_sent(i, now, 1200, false);
        }

        // Acknowledge only packets 3-5
        let (acked, _) = detector.on_ack_received(3, 5, Duration::from_millis(10));
        assert_eq!(acked.len(), 3);

        // Detect loss: packets 0-2 should be marked as lost (0 < 3-3, 1 < 3-3, 2 < 3-3)
        let lost = detector.detect_loss(now, Duration::from_millis(100));
        assert_eq!(lost.len(), 0); // Not lost by time threshold yet

        // But if we check after time passes
        let later = now + Duration::from_millis(1000);
        let lost = detector.detect_loss(later, Duration::from_millis(100));
        assert_eq!(lost.len(), 3);
    }

    #[test]
    fn test_time_threshold_loss_detection() {
        let mut detector = LossDetector::new();
        let now = Instant::now();

        detector.on_packet_sent(0, now, 1200, false);
        detector.on_packet_sent(1, now + Duration::from_millis(10), 1200, false);

        let (acked, _) = detector.on_ack_received(1, 1, Duration::from_millis(10));
        assert_eq!(acked.len(), 1);

        // Packet 0 is in-flight and will be lost by time threshold
        let later = now + Duration::from_millis(200);
        let lost = detector.detect_loss(later, Duration::from_millis(100));
        assert_eq!(lost.len(), 1);
    }

    #[test]
    fn test_bytes_in_flight() {
        let mut detector = LossDetector::new();
        let now = Instant::now();

        detector.on_packet_sent(0, now, 500, false);
        detector.on_packet_sent(1, now, 1000, false);

        assert_eq!(detector.bytes_in_flight(), 1500);

        let (_, _) = detector.on_ack_received(0, 0, Duration::from_millis(10));
        assert_eq!(detector.bytes_in_flight(), 1000);
    }

    #[test]
    fn test_duplicate_ack_tracking() {
        let mut detector = LossDetector::new();
        let now = Instant::now();

        detector.on_packet_sent(0, now, 1200, false);
        detector.on_packet_sent(1, now, 1200, false);

        let (_, _) = detector.on_ack_received(0, 0, Duration::from_millis(10));
        assert_eq!(detector.duplicate_ack_count, 0);

        let (_, _) = detector.on_ack_received(0, 0, Duration::from_millis(10));
        assert_eq!(detector.duplicate_ack_count, 1);

        let (_, _) = detector.on_ack_received(0, 1, Duration::from_millis(10));
        assert_eq!(detector.duplicate_ack_count, 0);
    }
}
