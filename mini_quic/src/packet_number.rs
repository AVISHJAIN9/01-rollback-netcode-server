//! Packet Number encoding, truncation, and full-number reconstruction.
//!
//! # Background (RFC 9000 §17.1 and Appendix A)
//!
//! QUIC packet numbers are 62-bit unsigned integers.  On the wire they are
//! truncated to 1–4 bytes to save space.  The receiver must reconstruct the
//! full packet number from the truncated value using a sliding window centered
//! on the largest previously acknowledged packet number.
//!
//! ## Encoding (truncation)
//!
//! The encoder picks the *minimum* byte width such that the window
//! `[full_pn - 2^(nbits-1), full_pn + 2^(nbits-1))` contains the full
//! packet number and does not overlap any previously acknowledged numbers.
//!
//! ## Decoding (reconstruction) — RFC 9000 Appendix A
//!
//! Given:
//! - `largest_pn`: the largest packet number successfully processed so far.
//! - `truncated_pn`: the truncated value read from the wire.
//! - `pn_nbits`: the number of *bits* in the truncated value (8, 16, 24, 32).
//!
//! The algorithm computes three candidate values and picks the one closest to
//! `expected_pn = largest_pn + 1`.

use crate::error::CodecError;

// ─── Truncation (Encoding) ────────────────────────────────────────────────────

/// Compute the minimum byte-width truncated packet number for `full_pn`,
/// given that `largest_acked` is the most recently acknowledged packet number.
///
/// Returns `(truncated_pn, num_bytes)` where `num_bytes` is 1, 2, 3, or 4.
///
/// The selection criterion: the *window* `[full_pn − half, full_pn + half)`
/// must not contain any packets already acknowledged (i.e., must be larger than
/// the acknowledgement range).  We satisfy this by requiring:
/// `full_pn - largest_acked < 2^(nbits - 1)`.
pub fn encode_packet_number(full_pn: u64, largest_acked: u64) -> (u32, usize) {
    // The difference between the current and the largest acknowledged tells us
    // how many distinct values the receiver must distinguish.
    // We need 2 × difference + 1 values to fit unambiguously.
    let required_range = if full_pn >= largest_acked {
        // Normal case: full_pn is ahead.  The window must cover at least
        // 2 × (full_pn − largest_acked + 1) so that the receiver can tell
        // this packet number apart from everything it has already seen.
        (full_pn - largest_acked).saturating_add(1)
    } else {
        // full_pn < largest_acked: should not happen in a well-behaved sender,
        // but we handle it gracefully by using the maximum width.
        u64::MAX
    };

    // Determine how many bytes we need to represent `required_range` distinct
    // values in the half-window.
    let num_bytes = if required_range < (1 << 7) {
        1 // 8 bits, half-window = 128
    } else if required_range < (1 << 15) {
        2 // 16 bits, half-window = 32768
    } else if required_range < (1 << 23) {
        3 // 24 bits, half-window = 8_388_608
    } else {
        4 // 32 bits
    };

    let nbits = num_bytes * 8;
    let mask = (1u64 << nbits) - 1;
    let truncated = (full_pn & mask) as u32;
    (truncated, num_bytes)
}

/// Write a truncated packet number `trunc_pn` of `num_bytes` bytes into `dst`
/// in big-endian order.
///
/// # Errors
/// Returns [`CodecError::BufferTooSmall`] if `dst.len() < num_bytes`.
/// Returns [`CodecError::InvalidPacketNumberLength`] if `num_bytes` is 0 or > 4.
pub fn write_packet_number(
    dst: &mut [u8],
    trunc_pn: u32,
    num_bytes: usize,
) -> Result<(), CodecError> {
    if num_bytes == 0 || num_bytes > 4 {
        return Err(CodecError::InvalidPacketNumberLength(num_bytes));
    }
    if dst.len() < num_bytes {
        return Err(CodecError::BufferTooSmall {
            needed: num_bytes,
            available: dst.len(),
        });
    }
    // Write the least-significant `num_bytes` bytes of `trunc_pn` in big-endian.
    for i in 0..num_bytes {
        let shift = (num_bytes - 1 - i) * 8;
        dst[i] = (trunc_pn >> shift) as u8;
    }
    Ok(())
}

/// Read a big-endian truncated packet number of `num_bytes` bytes from `src`.
///
/// # Errors
/// Returns [`CodecError::UnexpectedEof`] if `src.len() < num_bytes`.
/// Returns [`CodecError::InvalidPacketNumberLength`] if `num_bytes` is 0 or > 4.
pub fn read_packet_number(src: &[u8], num_bytes: usize) -> Result<u32, CodecError> {
    if num_bytes == 0 || num_bytes > 4 {
        return Err(CodecError::InvalidPacketNumberLength(num_bytes));
    }
    if src.len() < num_bytes {
        return Err(CodecError::UnexpectedEof {
            needed: num_bytes,
            available: src.len(),
        });
    }
    let mut val = 0u32;
    for i in 0..num_bytes {
        val = (val << 8) | (src[i] as u32);
    }
    Ok(val)
}

// ─── Full Reconstruction (Decoding) — RFC 9000 Appendix A ────────────────────

/// Reconstruct the full 62-bit packet number from a truncated wire value.
///
/// # Parameters
/// - `largest_pn`: the largest packet number successfully processed so far.
/// - `truncated_pn`: the low-order bits read from the wire.
/// - `pn_nbits`: the number of bits in `truncated_pn` (typically 8, 16, 24, or 32).
///
/// # Algorithm (RFC 9000 Appendix A)
///
/// ```text
/// expected_pn = largest_pn + 1
/// pn_win      = 1 << pn_nbits
/// pn_hwin     = pn_win / 2
/// pn_mask     = pn_win - 1
/// candidate   = (expected_pn & !pn_mask) | truncated_pn
///
/// if candidate <= expected_pn - pn_hwin AND candidate + pn_win <= 2^62 - 1:
///     candidate += pn_win   // too far behind: shift forward one window
/// elif candidate > expected_pn + pn_hwin AND candidate >= pn_win:
///     candidate -= pn_win   // too far ahead: shift backward one window
/// ```
pub fn decode_packet_number(largest_pn: u64, truncated_pn: u64, pn_nbits: usize) -> u64 {
    debug_assert!(pn_nbits > 0 && pn_nbits <= 62);

    let expected_pn: u64 = largest_pn.wrapping_add(1);
    let pn_win: u64 = 1u64 << pn_nbits;
    let pn_hwin: u64 = pn_win >> 1;
    let pn_mask: u64 = pn_win - 1;

    // Base candidate: align the high bits to expected_pn and fill in the low
    // bits from the wire value.
    let candidate = (expected_pn & !pn_mask) | (truncated_pn & pn_mask);

    // Determine which direction (if any) to shift the candidate.
    //
    // Case 1: candidate is too far behind expected_pn (more than half a window
    // behind).  Shift forward by one window, unless that would overflow 2^62.
    if candidate <= expected_pn.saturating_sub(pn_hwin) {
        // Check that adding pn_win does not overflow the 62-bit space.
        let max_varint = (1u64 << 62) - 1;
        if let Some(shifted) = candidate.checked_add(pn_win) {
            if shifted <= max_varint {
                return shifted;
            }
        }
        // Overflow would occur: return the original candidate.
        return candidate;
    }

    // Case 2: candidate is too far ahead of expected_pn (more than half a
    // window ahead) AND subtracting one window would not underflow.
    if candidate > expected_pn.saturating_add(pn_hwin) && candidate >= pn_win {
        return candidate - pn_win;
    }

    candidate
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_small_packet_numbers() {
        // When largest_acked == 0 and full_pn == 1, difference = 1 < 128.
        let (trunc, nbytes) = encode_packet_number(1, 0);
        assert_eq!(nbytes, 1);
        assert_eq!(trunc, 1);
    }

    #[test]
    fn encode_needs_two_bytes() {
        // difference = 200 >= 128, needs 2 bytes.
        let (trunc, nbytes) = encode_packet_number(200, 0);
        assert_eq!(nbytes, 2);
        assert_eq!(trunc, 200);
    }

    #[test]
    fn reconstruct_simple() {
        // largest=0, truncated=1 (8 bits) → expected=1 → candidate=1.
        let full = decode_packet_number(0, 1, 8);
        assert_eq!(full, 1);
    }

    #[test]
    fn reconstruct_wraps_forward() {
        // largest=0xFE (254), truncated=0 (8 bits).
        // expected = 255, candidate = (255 & !0xFF) | 0 = 0.
        // 0 <= 255 - 128 = 127: shift forward → 256.
        let full = decode_packet_number(254, 0, 8);
        assert_eq!(full, 256);
    }

    #[test]
    fn reconstruct_wraps_backward() {
        // largest=0x1FF (511), truncated=0xFF (255), 8-bit window.
        // expected = 512 = 0x200.
        // candidate = (0x200 & !0xFF) | 0xFF = 0x2FF = 767.
        // 767 > 512 + 128 = 640, and 767 >= 256: shift backward → 767 - 256 = 511.
        let full = decode_packet_number(0x1FF, 0xFF, 8);
        assert_eq!(full, 511);
    }

    #[test]
    fn reconstruct_16bit() {
        // 16-bit window, large packet numbers.
        let largest = 0x1_0000u64; // 65536
        let truncated = 0x0005u64; // truncated to 16 bits
        // expected = 65537, candidate = (65537 & !0xFFFF) | 5 = 0x10000 | 5 = 65541.
        // 65541 is close to 65537, within the half-window (32768).
        let full = decode_packet_number(largest, truncated, 16);
        assert_eq!(full, 0x1_0005);
    }

    #[test]
    fn write_and_read_roundtrip() {
        for num_bytes in 1..=4usize {
            let pn: u32 = 0x01_02_03_04 & ((1u32 << (num_bytes * 8)) - 1);
            let mut buf = [0u8; 4];
            write_packet_number(&mut buf, pn, num_bytes).unwrap();
            let read = read_packet_number(&buf, num_bytes).unwrap();
            assert_eq!(read, pn, "roundtrip failed for {num_bytes} bytes");
        }
    }
}
