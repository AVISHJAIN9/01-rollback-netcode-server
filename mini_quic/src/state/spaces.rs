//! Packet Number Spaces — RFC 9000 §12.3.
//!
//! QUIC uses three independent packet number spaces:
//! - **Initial**         — Used before handshake keys are established.
//! - **Handshake**       — Used during TLS handshake.
//! - **ApplicationData** — Used for 1-RTT protected traffic.
//!
//! Each space has:
//! - Its own monotonically increasing next-packet-number counter.
//! - Its own largest-received packet number (for ACK generation).
//! - A queue of sent-but-unacknowledged packet numbers (for loss detection).
//! - A discarded flag: once an epoch is completed, its state and keys are wiped.

use alloc::vec::Vec;
use zeroize::Zeroizing;

// ─── Epoch ────────────────────────────────────────────────────────────────────

/// The three packet number space epochs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Epoch {
    Initial,
    Handshake,
    ApplicationData,
}

impl core::fmt::Display for Epoch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Epoch::Initial => write!(f, "Initial"),
            Epoch::Handshake => write!(f, "Handshake"),
            Epoch::ApplicationData => write!(f, "ApplicationData"),
        }
    }
}

// ─── SentPacket ───────────────────────────────────────────────────────────────

/// A record of a packet sent but not yet acknowledged.
#[derive(Debug, Clone)]
pub struct SentPacket {
    /// The packet number.
    pub packet_number: u64,
    /// Monotonic timestamp (nanoseconds) at which this packet was sent.
    pub sent_time_ns: u64,
    /// Whether this packet contained ack-eliciting frames (STREAM, CRYPTO, …).
    pub ack_eliciting: bool,
    /// The serialised packet bytes, retained for potential retransmission.
    pub bytes: Vec<u8>,
}

// ─── PacketSpace ─────────────────────────────────────────────────────────────

/// State for a single QUIC packet number space (epoch).
#[derive(Debug)]
pub struct PacketSpace {
    /// Which epoch this space belongs to.
    pub epoch: Epoch,
    /// The next packet number to use when sending.  Starts at 0, increments
    /// by 1 for each packet sent regardless of retransmission.
    pub next_packet_number: u64,
    /// The largest packet number received from the peer in this space.
    /// `None` if no packet has been received yet.
    pub largest_received: Option<u64>,
    /// Packets sent but not yet acknowledged (ordered by packet number).
    pub unacked: Vec<SentPacket>,
    /// The raw keying material (key + IV + HP key) for this space.  Zeroized
    /// on `discard`.
    pub keying_material: Option<Zeroizing<Vec<u8>>>,
    /// Whether this space has been discarded (keys wiped, state cleared).
    pub discarded: bool,
}

impl PacketSpace {
    /// Create a new, empty packet space for `epoch`.
    pub fn new(epoch: Epoch) -> Self {
        PacketSpace {
            epoch,
            next_packet_number: 0,
            largest_received: None,
            unacked: Vec::new(),
            keying_material: None,
            discarded: false,
        }
    }

    /// Assign keying material to this space (called when keys become available).
    pub fn set_keying_material(&mut self, material: Vec<u8>) {
        self.keying_material = Some(Zeroizing::new(material));
    }

    /// Allocate and return the next packet number, advancing the counter.
    ///
    /// Returns `None` if this space has been discarded.
    pub fn next_pn(&mut self) -> Option<u64> {
        if self.discarded {
            return None;
        }
        let pn = self.next_packet_number;
        self.next_packet_number += 1;
        Some(pn)
    }

    /// Record that a packet was received with `packet_number`.
    ///
    /// Updates `largest_received` if this is the highest seen so far.
    pub fn on_packet_received(&mut self, packet_number: u64) {
        match self.largest_received {
            None => self.largest_received = Some(packet_number),
            Some(prev) if packet_number > prev => {
                self.largest_received = Some(packet_number);
            }
            _ => {}
        }
    }

    /// Record a sent packet in the unacknowledged queue.
    pub fn on_packet_sent(&mut self, packet: SentPacket) {
        self.unacked.push(packet);
    }

    /// Process an ACK covering `acked_packets` and remove them from the
    /// unacknowledged queue.
    ///
    /// Returns the set of packet numbers that were newly acknowledged.
    pub fn on_ack_received(&mut self, acked_packets: &[u64]) -> Vec<u64> {
        let mut newly_acked = Vec::new();
        self.unacked.retain(|sent| {
            if acked_packets.contains(&sent.packet_number) {
                newly_acked.push(sent.packet_number);
                false // remove from unacked
            } else {
                true // keep
            }
        });
        newly_acked
    }

    /// Discard this packet space: wipe keying material and clear the
    /// unacknowledged queue.  Called when the epoch completes (e.g., when
    /// Handshake keys are confirmed the Initial space is discarded).
    pub fn discard(&mut self) {
        // Zeroizing<Vec<u8>> zeroes memory on drop.
        drop(self.keying_material.take());
        // Clear unacked packets (their bytes may contain sensitive data).
        self.unacked.clear();
        self.discarded = true;
    }

    /// Return `true` if this space has been discarded.
    pub fn is_discarded(&self) -> bool {
        self.discarded
    }

    /// Return the number of unacknowledged packets.
    pub fn unacked_count(&self) -> usize {
        self.unacked.len()
    }
}

// ─── PacketSpaces ─────────────────────────────────────────────────────────────

/// Holds all three packet number spaces for a connection.
#[derive(Debug)]
pub struct PacketSpaces {
    pub initial: PacketSpace,
    pub handshake: PacketSpace,
    pub application_data: PacketSpace,
}

impl PacketSpaces {
    /// Create a fresh set of packet spaces.
    pub fn new() -> Self {
        PacketSpaces {
            initial: PacketSpace::new(Epoch::Initial),
            handshake: PacketSpace::new(Epoch::Handshake),
            application_data: PacketSpace::new(Epoch::ApplicationData),
        }
    }

    /// Return a mutable reference to the space for `epoch`.
    pub fn get_mut(&mut self, epoch: Epoch) -> &mut PacketSpace {
        match epoch {
            Epoch::Initial => &mut self.initial,
            Epoch::Handshake => &mut self.handshake,
            Epoch::ApplicationData => &mut self.application_data,
        }
    }

    /// Return a shared reference to the space for `epoch`.
    pub fn get(&self, epoch: Epoch) -> &PacketSpace {
        match epoch {
            Epoch::Initial => &self.initial,
            Epoch::Handshake => &self.handshake,
            Epoch::ApplicationData => &self.application_data,
        }
    }

    /// Discard the Initial epoch (called when Handshake keys are confirmed).
    pub fn discard_initial(&mut self) {
        self.initial.discard();
    }

    /// Discard the Handshake epoch (called when 1-RTT keys are confirmed and
    /// the handshake is complete).
    pub fn discard_handshake(&mut self) {
        self.handshake.discard();
    }
}

impl Default for PacketSpaces {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_pn_increments_monotonically() {
        let mut space = PacketSpace::new(Epoch::Initial);
        assert_eq!(space.next_pn(), Some(0));
        assert_eq!(space.next_pn(), Some(1));
        assert_eq!(space.next_pn(), Some(2));
    }

    #[test]
    fn next_pn_returns_none_after_discard() {
        let mut space = PacketSpace::new(Epoch::Handshake);
        space.next_pn();
        space.discard();
        assert_eq!(space.next_pn(), None);
    }

    #[test]
    fn largest_received_tracks_maximum() {
        let mut space = PacketSpace::new(Epoch::ApplicationData);
        space.on_packet_received(5);
        assert_eq!(space.largest_received, Some(5));
        space.on_packet_received(3); // older — must not regress
        assert_eq!(space.largest_received, Some(5));
        space.on_packet_received(10);
        assert_eq!(space.largest_received, Some(10));
    }

    #[test]
    fn ack_received_removes_packets() {
        let mut space = PacketSpace::new(Epoch::Initial);
        for pn in 0..5 {
            space.on_packet_sent(SentPacket {
                packet_number: pn,
                sent_time_ns: 0,
                ack_eliciting: true,
                bytes: alloc::vec![],
            });
        }
        assert_eq!(space.unacked_count(), 5);

        let acked = space.on_ack_received(&[0, 2, 4]);
        assert_eq!(acked.len(), 3);
        assert!(acked.contains(&0));
        assert!(acked.contains(&2));
        assert!(acked.contains(&4));
        assert_eq!(space.unacked_count(), 2);
    }

    #[test]
    fn discard_clears_unacked_and_keys() {
        let mut space = PacketSpace::new(Epoch::Handshake);
        space.set_keying_material(alloc::vec![0xAAu8; 96]);
        space.on_packet_sent(SentPacket {
            packet_number: 1,
            sent_time_ns: 0,
            ack_eliciting: true,
            bytes: alloc::vec![],
        });

        space.discard();
        assert!(space.is_discarded());
        assert!(space.keying_material.is_none());
        assert_eq!(space.unacked_count(), 0);
    }

    #[test]
    fn packet_spaces_discard_initial_leaves_others() {
        let mut spaces = PacketSpaces::new();
        spaces.discard_initial();

        assert!(spaces.initial.is_discarded());
        assert!(!spaces.handshake.is_discarded());
        assert!(!spaces.application_data.is_discarded());
    }
}
