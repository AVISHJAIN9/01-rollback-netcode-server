//! QUIC Variable-Length Integer encoding and decoding — RFC 9000 §16.
//!
//! # Wire Format
//!
//! The two most-significant bits of the first byte determine the total length
//! and the number of value bits:
//!
//! ```text
//! Prefix  Length  Value bits  Range
//! 0b00    1 byte       6      0 .. 63
//! 0b01    2 bytes     14      64 .. 16_383
//! 0b10    4 bytes     30      16_384 .. 1_073_741_823
//! 0b11    8 bytes     62      1_073_741_824 .. 4_611_686_018_427_387_903
//! ```
//!
//! Any value ≥ 2^62 (`VARINT_MAX`) is a protocol violation.

use crate::error::{CodecError, VarIntError};

/// The maximum value encodable in a QUIC VarInt (2^62 − 1).
pub const VARINT_MAX: u64 = (1 << 62) - 1;

/// A QUIC Variable-Length Integer.
///
/// Internally stores the decoded `u64` value.  The wire encoding is always
/// chosen to be the smallest representation that fits the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarInt(u64);

impl VarInt {
    // ── Construction ─────────────────────────────────────────────────────────

    /// Construct a `VarInt` from a `u64`, returning an error when
    /// `val >= 2^62`.
    ///
    /// # Errors
    /// Returns [`VarIntError::ValueTooLarge`] when `val > VARINT_MAX`.
    #[inline]
    pub fn from_u64(val: u64) -> Result<Self, VarIntError> {
        if val > VARINT_MAX {
            return Err(VarIntError::ValueTooLarge(val));
        }
        Ok(VarInt(val))
    }

    /// Construct a `VarInt` from a value known at compile time to be in range.
    ///
    /// # Panics
    /// Panics (in debug builds) if `val > VARINT_MAX`.
    #[inline]
    pub const fn from_u64_unchecked(val: u64) -> Self {
        // debug_assert is evaluated only in debug builds.
        debug_assert!(val <= VARINT_MAX);
        VarInt(val)
    }

    // ── Value accessors ───────────────────────────────────────────────────────

    /// Return the inner `u64` value.
    #[inline]
    pub const fn into_inner(self) -> u64 {
        self.0
    }

    // ── Encoding ─────────────────────────────────────────────────────────────

    /// Return the number of bytes this `VarInt` requires on the wire.
    #[inline]
    pub const fn encoded_len(self) -> usize {
        match self.0 {
            0..=0x3F => 1,
            0x40..=0x3FFF => 2,
            0x4000..=0x3FFF_FFFF => 4,
            _ => 8,
        }
    }

    /// Encode this `VarInt` into `dst`, writing `encoded_len()` bytes.
    ///
    /// Returns the number of bytes written on success.
    ///
    /// # Errors
    /// Returns [`CodecError::BufferTooSmall`] when `dst` is shorter than
    /// `encoded_len()`.
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        let val = self.0;
        match val {
            // ── 1-byte: prefix 0b00, value in bits [5:0] ──────────────────
            0..=0x3F => {
                if dst.is_empty() {
                    return Err(CodecError::BufferTooSmall {
                        needed: 1,
                        available: 0,
                    });
                }
                // Prefix 0b00 is 0, so value fits directly.
                dst[0] = val as u8;
                Ok(1)
            }

            // ── 2-byte: prefix 0b01, value in bits [13:0] ─────────────────
            0x40..=0x3FFF => {
                if dst.len() < 2 {
                    return Err(CodecError::BufferTooSmall {
                        needed: 2,
                        available: dst.len(),
                    });
                }
                // Set prefix 0b01 in the two MSBs of the first byte.
                let encoded = val | 0x4000; // 0b01 << 14
                dst[0] = (encoded >> 8) as u8;
                dst[1] = (encoded & 0xFF) as u8;
                Ok(2)
            }

            // ── 4-byte: prefix 0b10, value in bits [29:0] ─────────────────
            0x4000..=0x3FFF_FFFF => {
                if dst.len() < 4 {
                    return Err(CodecError::BufferTooSmall {
                        needed: 4,
                        available: dst.len(),
                    });
                }
                let encoded = val | 0x8000_0000; // 0b10 << 30
                dst[0] = (encoded >> 24) as u8;
                dst[1] = (encoded >> 16) as u8;
                dst[2] = (encoded >> 8) as u8;
                dst[3] = (encoded & 0xFF) as u8;
                Ok(4)
            }

            // ── 8-byte: prefix 0b11, value in bits [61:0] ─────────────────
            _ => {
                if dst.len() < 8 {
                    return Err(CodecError::BufferTooSmall {
                        needed: 8,
                        available: dst.len(),
                    });
                }
                let encoded = val | 0xC000_0000_0000_0000u64; // 0b11 << 62
                dst[0] = (encoded >> 56) as u8;
                dst[1] = (encoded >> 48) as u8;
                dst[2] = (encoded >> 40) as u8;
                dst[3] = (encoded >> 32) as u8;
                dst[4] = (encoded >> 24) as u8;
                dst[5] = (encoded >> 16) as u8;
                dst[6] = (encoded >> 8) as u8;
                dst[7] = (encoded & 0xFF) as u8;
                Ok(8)
            }
        }
    }

    // ── Decoding ─────────────────────────────────────────────────────────────

    /// Decode a `VarInt` from the front of `src`.
    ///
    /// Returns `(VarInt, bytes_consumed)` on success.
    ///
    /// # Errors
    /// Returns [`CodecError::UnexpectedEof`] when `src` is too short for the
    /// length indicated by the prefix bits.
    pub fn decode(src: &[u8]) -> Result<(Self, usize), CodecError> {
        // Need at least 1 byte to read the prefix.
        if src.is_empty() {
            return Err(CodecError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }

        // Extract the 2-bit prefix from the most-significant bits of the first
        // byte.
        let prefix = src[0] >> 6;

        match prefix {
            // ── 1-byte: strip prefix, remaining 6 bits are the value ───────
            0b00 => {
                let val = (src[0] & 0x3F) as u64;
                Ok((VarInt(val), 1))
            }

            // ── 2-byte: 14 value bits spanning bytes [0:1] ─────────────────
            0b01 => {
                if src.len() < 2 {
                    return Err(CodecError::UnexpectedEof {
                        needed: 2,
                        available: src.len(),
                    });
                }
                let val = (((src[0] & 0x3F) as u64) << 8) | (src[1] as u64);
                Ok((VarInt(val), 2))
            }

            // ── 4-byte: 30 value bits spanning bytes [0:3] ─────────────────
            0b10 => {
                if src.len() < 4 {
                    return Err(CodecError::UnexpectedEof {
                        needed: 4,
                        available: src.len(),
                    });
                }
                let val = (((src[0] & 0x3F) as u64) << 24)
                    | ((src[1] as u64) << 16)
                    | ((src[2] as u64) << 8)
                    | (src[3] as u64);
                Ok((VarInt(val), 4))
            }

            // ── 8-byte: 62 value bits spanning bytes [0:7] ─────────────────
            0b11 => {
                if src.len() < 8 {
                    return Err(CodecError::UnexpectedEof {
                        needed: 8,
                        available: src.len(),
                    });
                }
                let val = (((src[0] & 0x3F) as u64) << 56)
                    | ((src[1] as u64) << 48)
                    | ((src[2] as u64) << 40)
                    | ((src[3] as u64) << 32)
                    | ((src[4] as u64) << 24)
                    | ((src[5] as u64) << 16)
                    | ((src[6] as u64) << 8)
                    | (src[7] as u64);
                Ok((VarInt(val), 8))
            }

            // Unreachable: a 2-bit value can only be 0b00..=0b11.
            _ => unreachable!("2-bit prefix cannot exceed 0b11"),
        }
    }
}

// ── Convenience conversions ────────────────────────────────────────────────────

impl TryFrom<u64> for VarInt {
    type Error = VarIntError;
    #[inline]
    fn try_from(val: u64) -> Result<Self, VarIntError> {
        VarInt::from_u64(val)
    }
}

impl TryFrom<usize> for VarInt {
    type Error = VarIntError;
    #[inline]
    fn try_from(val: usize) -> Result<Self, VarIntError> {
        VarInt::from_u64(val as u64)
    }
}

impl From<VarInt> for u64 {
    #[inline]
    fn from(v: VarInt) -> u64 {
        v.0
    }
}

impl core::fmt::Display for VarInt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: encode then decode and assert round-trip equality.
    fn round_trip(val: u64) {
        let vi = VarInt::from_u64(val).expect("value in range");
        let expected_len = vi.encoded_len();
        let mut buf = [0u8; 8];
        let written = vi.encode(&mut buf[..]).expect("encode succeeds");
        assert_eq!(written, expected_len, "written != encoded_len for {val}");
        let (decoded, consumed) = VarInt::decode(&buf[..written]).expect("decode succeeds");
        assert_eq!(consumed, expected_len, "consumed != encoded_len for {val}");
        assert_eq!(decoded.into_inner(), val, "round-trip mismatch for {val}");
    }

    #[test]
    fn round_trip_boundaries() {
        // 1-byte boundaries
        round_trip(0);
        round_trip(1);
        round_trip(0x3F);

        // 2-byte boundaries
        round_trip(0x40);
        round_trip(0x3FFF);

        // 4-byte boundaries
        round_trip(0x4000);
        round_trip(0x3FFF_FFFF);

        // 8-byte boundaries
        round_trip(0x4000_0000);
        round_trip(VARINT_MAX);
    }

    #[test]
    fn encoded_lengths() {
        assert_eq!(VarInt::from_u64(0).unwrap().encoded_len(), 1);
        assert_eq!(VarInt::from_u64(0x3F).unwrap().encoded_len(), 1);
        assert_eq!(VarInt::from_u64(0x40).unwrap().encoded_len(), 2);
        assert_eq!(VarInt::from_u64(0x3FFF).unwrap().encoded_len(), 2);
        assert_eq!(VarInt::from_u64(0x4000).unwrap().encoded_len(), 4);
        assert_eq!(VarInt::from_u64(0x3FFF_FFFF).unwrap().encoded_len(), 4);
        assert_eq!(VarInt::from_u64(0x4000_0000).unwrap().encoded_len(), 8);
        assert_eq!(VarInt::from_u64(VARINT_MAX).unwrap().encoded_len(), 8);
    }

    #[test]
    fn value_too_large_rejected() {
        let too_large = VARINT_MAX + 1;
        assert_eq!(
            VarInt::from_u64(too_large),
            Err(VarIntError::ValueTooLarge(too_large))
        );
        assert_eq!(
            VarInt::from_u64(u64::MAX),
            Err(VarIntError::ValueTooLarge(u64::MAX))
        );
    }

    #[test]
    fn decode_empty_buffer_returns_eof() {
        assert_eq!(
            VarInt::decode(&[]),
            Err(CodecError::UnexpectedEof {
                needed: 1,
                available: 0
            })
        );
    }

    #[test]
    fn decode_truncated_2byte_returns_eof() {
        // First byte has prefix 0b01 → 2-byte encoding, but only 1 byte given.
        let buf = [0x40u8]; // 0b01_000000 → 2-byte, value portion = 0
        assert_eq!(
            VarInt::decode(&buf),
            Err(CodecError::UnexpectedEof {
                needed: 2,
                available: 1
            })
        );
    }

    #[test]
    fn specific_wire_encodings() {
        // RFC 9000 §A.1: 37 encodes as single byte 0x25.
        let vi = VarInt::from_u64(37).unwrap();
        let mut buf = [0u8; 1];
        vi.encode(&mut buf).unwrap();
        assert_eq!(buf[0], 0x25);

        // Value 15293 encodes as 0x7bbd (2 bytes).
        let vi = VarInt::from_u64(15293).unwrap();
        let mut buf = [0u8; 2];
        vi.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x7b, 0xbd]);

        // Value 494878333 encodes as 0x9d7f3e7d (4 bytes).
        let vi = VarInt::from_u64(494_878_333).unwrap();
        let mut buf = [0u8; 4];
        vi.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x9d, 0x7f, 0x3e, 0x7d]);
    }
}
