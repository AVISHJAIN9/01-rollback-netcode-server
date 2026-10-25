//! ACK Frame codec — RFC 9000 §19.3.
//!
//! # Wire Layout
//!
//! ```text
//! Type (VarInt)               0x02 or 0x03
//! Largest Acknowledged (VarInt)
//! ACK Delay (VarInt)
//! ACK Range Count (VarInt)    number of additional ACK Range blocks
//! First ACK Range (VarInt)    packets Largest..=Largest-First (all acked)
//! ACK Range [0..count] {
//!   Gap (VarInt)              packets skipped (not acked)
//!   ACK Range Length (VarInt) additional acked run
//! }
//! [Only when type == 0x03]:
//! ECT(0) Count (VarInt)
//! ECT(1) Count (VarInt)
//! ECN-CE Count (VarInt)
//! ```
//!
//! Each "Gap" represents `gap + 2` unacknowledged packets.
//! Each "ACK Range Length" represents `ack_range_length + 1` acknowledged packets.

use alloc::vec::Vec;

use crate::{error::CodecError, varint::VarInt};

// ─── AckRange ────────────────────────────────────────────────────────────────

/// One additional ACK range block following the First ACK Range.
///
/// On the wire:
/// - `gap`:             number of unacknowledged packets between this range
///                      and the previous range, encoded as `gap - 2`.
/// - `ack_range_len`:   the count of contiguous acknowledged packets in this
///                      range minus 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckRange {
    /// Raw Gap VarInt value as decoded from the wire (= actual_gap - 2).
    pub gap: u64,
    /// Raw ACK Range Length VarInt value (= actual_count - 1).
    pub ack_range_len: u64,
}

// ─── EcnCounts ───────────────────────────────────────────────────────────────

/// Optional ECN codepoint counts carried by type-0x03 ACK frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcnCounts {
    pub ect0_count: u64,
    pub ect1_count: u64,
    pub ecn_ce_count: u64,
}

// ─── AckFrame ────────────────────────────────────────────────────────────────

/// A decoded QUIC ACK frame (type 0x02 or 0x03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckFrame {
    /// The highest packet number being acknowledged.
    pub largest_acknowledged: u64,
    /// The ACK delay encoded as a scaled VarInt value (raw wire value;
    /// caller must apply the ACK delay exponent to convert to microseconds).
    pub ack_delay: u64,
    /// The first contiguous run of acknowledged packets immediately below
    /// `largest_acknowledged` (raw wire value; actual count = first_ack_range + 1).
    pub first_ack_range: u64,
    /// Additional ACK range blocks (one per non-contiguous acknowledged run).
    pub additional_ranges: Vec<AckRange>,
    /// Optional ECN counts present when the frame type was 0x03.
    pub ecn_counts: Option<EcnCounts>,
}

impl AckFrame {
    // ── Encoding ─────────────────────────────────────────────────────────────

    /// Encode this `AckFrame` into `dst`.
    ///
    /// The frame type byte (`0x02` or `0x03`) is included as the first VarInt.
    /// Returns the total bytes written.
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        let mut pos = 0;

        // ── Frame type ────────────────────────────────────────────────────────
        let type_val: u64 = if self.ecn_counts.is_some() { 0x03 } else { 0x02 };
        pos += VarInt::from_u64(type_val)?.encode(&mut dst[pos..])?;

        // ── Largest Acknowledged ──────────────────────────────────────────────
        pos += VarInt::from_u64(self.largest_acknowledged)?.encode(&mut dst[pos..])?;

        // ── ACK Delay ─────────────────────────────────────────────────────────
        pos += VarInt::from_u64(self.ack_delay)?.encode(&mut dst[pos..])?;

        // ── ACK Range Count ───────────────────────────────────────────────────
        pos += VarInt::from_u64(self.additional_ranges.len() as u64)?
            .encode(&mut dst[pos..])?;

        // ── First ACK Range ───────────────────────────────────────────────────
        pos += VarInt::from_u64(self.first_ack_range)?.encode(&mut dst[pos..])?;

        // ── Additional ACK Range Blocks ───────────────────────────────────────
        for range in &self.additional_ranges {
            pos += VarInt::from_u64(range.gap)?.encode(&mut dst[pos..])?;
            pos += VarInt::from_u64(range.ack_range_len)?.encode(&mut dst[pos..])?;
        }

        // ── ECN Counts (type 0x03 only) ───────────────────────────────────────
        if let Some(ecn) = &self.ecn_counts {
            pos += VarInt::from_u64(ecn.ect0_count)?.encode(&mut dst[pos..])?;
            pos += VarInt::from_u64(ecn.ect1_count)?.encode(&mut dst[pos..])?;
            pos += VarInt::from_u64(ecn.ecn_ce_count)?.encode(&mut dst[pos..])?;
        }

        Ok(pos)
    }

    /// Return the number of bytes this frame requires on the wire.
    pub fn encoded_len(&self) -> usize {
        let mut len = 0;
        let type_val: u64 = if self.ecn_counts.is_some() { 0x03 } else { 0x02 };
        len += varint_len(type_val);
        len += varint_len(self.largest_acknowledged);
        len += varint_len(self.ack_delay);
        len += varint_len(self.additional_ranges.len() as u64);
        len += varint_len(self.first_ack_range);
        for r in &self.additional_ranges {
            len += varint_len(r.gap);
            len += varint_len(r.ack_range_len);
        }
        if let Some(ecn) = &self.ecn_counts {
            len += varint_len(ecn.ect0_count);
            len += varint_len(ecn.ect1_count);
            len += varint_len(ecn.ecn_ce_count);
        }
        len
    }

    // ── Decoding ─────────────────────────────────────────────────────────────

    /// Decode an `AckFrame` from `src`.
    ///
    /// `src` must start **after** the frame type byte.  Pass `frame_type` (the
    /// already-decoded raw `u64` type value) so the decoder knows whether ECN
    /// counts are present.
    ///
    /// Returns `(AckFrame, bytes_consumed)`.
    pub fn decode(frame_type: u64, src: &[u8]) -> Result<(Self, usize), CodecError> {
        // Validate type
        if frame_type != 0x02 && frame_type != 0x03 {
            return Err(CodecError::UnknownFrameType(frame_type));
        }
        let has_ecn = frame_type == 0x03;
        let mut pos = 0;

        // ── Largest Acknowledged ──────────────────────────────────────────────
        let (la_vi, n) = VarInt::decode(&src[pos..])?;
        pos += n;
        let largest_acknowledged = la_vi.into_inner();

        // ── ACK Delay ─────────────────────────────────────────────────────────
        let (delay_vi, n) = VarInt::decode(&src[pos..])?;
        pos += n;
        let ack_delay = delay_vi.into_inner();

        // ── ACK Range Count ───────────────────────────────────────────────────
        let (count_vi, n) = VarInt::decode(&src[pos..])?;
        pos += n;
        let range_count = count_vi.into_inner() as usize;

        // ── First ACK Range ───────────────────────────────────────────────────
        let (first_vi, n) = VarInt::decode(&src[pos..])?;
        pos += n;
        let first_ack_range = first_vi.into_inner();

        // Sanity: first_ack_range <= largest_acknowledged (can't ack more
        // packets than exist up to largest_acknowledged).
        if first_ack_range > largest_acknowledged {
            return Err(CodecError::AckRangeExceedsLargest {
                first_ack_range,
                largest_acknowledged,
            });
        }

        // ── Additional ACK Range Blocks ───────────────────────────────────────
        let mut additional_ranges = Vec::with_capacity(range_count);
        for _ in 0..range_count {
            let (gap_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            let (alen_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            additional_ranges.push(AckRange {
                gap: gap_vi.into_inner(),
                ack_range_len: alen_vi.into_inner(),
            });
        }

        // ── ECN Counts ────────────────────────────────────────────────────────
        let ecn_counts = if has_ecn {
            let (ect0_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            let (ect1_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            let (ce_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            Some(EcnCounts {
                ect0_count: ect0_vi.into_inner(),
                ect1_count: ect1_vi.into_inner(),
                ecn_ce_count: ce_vi.into_inner(),
            })
        } else {
            None
        };

        Ok((
            AckFrame {
                largest_acknowledged,
                ack_delay,
                first_ack_range,
                additional_ranges,
                ecn_counts,
            },
            pos,
        ))
    }
}

/// Return the VarInt-encoded byte length of `val` without allocation.
fn varint_len(val: u64) -> usize {
    match val {
        0..=0x3F => 1,
        0x40..=0x3FFF => 2,
        0x4000..=0x3FFF_FFFF => 4,
        _ => 8,
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(frame: &AckFrame) {
        let capacity = frame.encoded_len();
        let mut buf = alloc::vec![0u8; capacity];
        let written = frame.encode(&mut buf).unwrap();
        assert_eq!(written, capacity, "encoded_len mismatch");

        // Skip the type byte that encode() prepended before calling decode().
        // decode() expects src AFTER the type byte.
        let type_val: u64 = if frame.ecn_counts.is_some() { 0x03 } else { 0x02 };
        // Find where the type byte ends.
        let type_varint_len = varint_len(type_val);
        let (decoded, consumed) =
            AckFrame::decode(type_val, &buf[type_varint_len..written]).unwrap();
        assert_eq!(consumed, written - type_varint_len);
        assert_eq!(&decoded, frame);
    }

    #[test]
    fn ack_no_ranges_no_ecn() {
        round_trip(&AckFrame {
            largest_acknowledged: 100,
            ack_delay: 250,
            first_ack_range: 5,
            additional_ranges: alloc::vec![],
            ecn_counts: None,
        });
    }

    #[test]
    fn ack_with_multiple_ranges() {
        round_trip(&AckFrame {
            largest_acknowledged: 500,
            ack_delay: 1000,
            first_ack_range: 10,
            additional_ranges: alloc::vec![
                AckRange { gap: 2, ack_range_len: 5 },
                AckRange { gap: 1, ack_range_len: 3 },
            ],
            ecn_counts: None,
        });
    }

    #[test]
    fn ack_with_ecn_counts() {
        round_trip(&AckFrame {
            largest_acknowledged: 200,
            ack_delay: 50,
            first_ack_range: 0,
            additional_ranges: alloc::vec![],
            ecn_counts: Some(EcnCounts {
                ect0_count: 100,
                ect1_count: 0,
                ecn_ce_count: 5,
            }),
        });
    }

    #[test]
    fn ack_first_range_exceeds_largest_rejected() {
        let result = AckFrame::decode(0x02, &{
            // Encode: largest=10, delay=0, count=0, first=15 (> largest)
            let mut buf = [0u8; 32];
            let mut pos = 0;
            pos += VarInt::from_u64(10).unwrap().encode(&mut buf[pos..]).unwrap(); // largest
            pos += VarInt::from_u64(0).unwrap().encode(&mut buf[pos..]).unwrap();  // delay
            pos += VarInt::from_u64(0).unwrap().encode(&mut buf[pos..]).unwrap();  // count
            pos += VarInt::from_u64(15).unwrap().encode(&mut buf[pos..]).unwrap(); // first > largest
            let _ = pos;
            buf[..10].to_vec()
        });
        assert!(
            matches!(result, Err(CodecError::AckRangeExceedsLargest { .. })),
            "expected AckRangeExceedsLargest, got {:?}", result
        );
    }

    #[test]
    fn ack_decode_empty_src_returns_eof() {
        assert!(matches!(
            AckFrame::decode(0x02, &[]),
            Err(CodecError::UnexpectedEof { .. })
        ));
    }

    #[test]
    fn ack_unknown_type_rejected() {
        let result = AckFrame::decode(0x04, &[]);
        assert!(matches!(result, Err(CodecError::UnknownFrameType(0x04))));
    }
}
