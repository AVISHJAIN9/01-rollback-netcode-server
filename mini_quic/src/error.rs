//! Structured error types for the Mini-QUIC wire codec.
//!
//! Every public codec function returns one of these variants instead of
//! panicking.  The variants carry enough context to pinpoint the exact
//! failure in a protocol trace.

use core::fmt;

// ─── VarInt Errors ────────────────────────────────────────────────────────────

/// Errors produced by [`crate::varint::VarInt`] construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarIntError {
    /// The supplied `u64` value is ≥ 2^62 and cannot be represented as a QUIC
    /// VarInt.  RFC 9000 §16 imposes this upper limit.
    ValueTooLarge(u64),
}

impl fmt::Display for VarIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VarIntError::ValueTooLarge(v) => {
                write!(f, "value {v} is >= 2^62 and cannot be encoded as a QUIC VarInt")
            }
        }
    }
}

// ─── Connection-ID Errors ─────────────────────────────────────────────────────

/// Errors produced by [`crate::cid::ConnectionId`] construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CidError {
    /// The provided slice is longer than [`crate::cid::MAX_CID_SIZE`] (20 bytes).
    TooLong { provided: usize },
}

impl fmt::Display for CidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CidError::TooLong { provided } => {
                write!(
                    f,
                    "connection ID length {provided} exceeds maximum of 20 bytes"
                )
            }
        }
    }
}

// ─── Codec Errors ─────────────────────────────────────────────────────────────

/// General codec errors returned by packet and frame encode/decode operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    // ── Buffer / length errors ────────────────────────────────────────────────
    /// Not enough bytes remaining in the input buffer to complete decoding.
    UnexpectedEof {
        /// What the decoder needed.
        needed: usize,
        /// What remained in the buffer.
        available: usize,
    },
    /// The destination buffer provided to an encoder is too small.
    BufferTooSmall {
        needed: usize,
        available: usize,
    },

    // ── Header bit errors ─────────────────────────────────────────────────────
    /// The Fixed Bit (0x40) in the first byte of a packet header was zero.
    /// RFC 9000 §17.2 and §17.3 require this bit to be 1 (except in Version
    /// Negotiation packets).
    InvalidFixedBit,
    /// The Reserved Bits (0x18) in a Short-Header first byte were non-zero.
    /// RFC 9000 §17.3.1 requires these bits to be 0 after header protection
    /// has been removed.
    NonZeroReservedBits,

    // ── Value / range errors ──────────────────────────────────────────────────
    /// A VarInt value was out of the 2^62 range.
    VarIntTooLarge(u64),
    /// An unknown or unsupported packet long-header type was encountered.
    UnknownLongPacketType(u8),
    /// An unknown frame type tag was encountered.
    UnknownFrameType(u64),
    /// A connection ID length field exceeded 20 bytes.
    CidTooLong { len: usize },
    /// A STREAM frame offset VarInt was decoded but its value overflows `u64`.
    StreamOffsetOverflow,
    /// A packet number length derived from the header was 0 or > 4.
    InvalidPacketNumberLength(usize),
    /// The Version field was unexpected for the given packet type.
    UnexpectedVersion(u32),
    /// An error propagated from the VarInt subsystem during encoding.
    VarIntEncode(VarIntError),
    /// The First ACK Range exceeds the Largest Acknowledged packet number.
    AckRangeExceedsLargest {
        first_ack_range: u64,
        largest_acknowledged: u64,
    },
    /// A CRYPTO frame offset + length would exceed the maximum stream size (2^62 - 1).
    CryptoOffsetOverflow,
    /// A MAX_STREAMS limit exceeds 2^60.
    MaxStreamsTooLarge(u64),
    /// A state transition was rejected because it is invalid from the current state.
    InvalidStateTransition {
        from: &'static str,
        to: &'static str,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::UnexpectedEof { needed, available } => {
                write!(
                    f,
                    "unexpected EOF: needed {needed} bytes, only {available} available"
                )
            }
            CodecError::BufferTooSmall { needed, available } => {
                write!(
                    f,
                    "destination buffer too small: needed {needed} bytes, only {available} available"
                )
            }
            CodecError::InvalidFixedBit => {
                write!(f, "QUIC Fixed Bit (0x40) is zero in packet header")
            }
            CodecError::NonZeroReservedBits => {
                write!(
                    f,
                    "Short Header reserved bits (0x18) are non-zero after header protection removal"
                )
            }
            CodecError::VarIntTooLarge(v) => {
                write!(f, "VarInt value {v} >= 2^62")
            }
            CodecError::UnknownLongPacketType(t) => {
                write!(f, "unknown long-header packet type {t:#04x}")
            }
            CodecError::UnknownFrameType(t) => {
                write!(f, "unknown frame type {t:#x}")
            }
            CodecError::CidTooLong { len } => {
                write!(f, "connection ID length {len} exceeds 20 bytes")
            }
            CodecError::StreamOffsetOverflow => {
                write!(f, "STREAM frame offset overflows u64")
            }
            CodecError::InvalidPacketNumberLength(n) => {
                write!(f, "invalid packet number length {n}: must be 1–4")
            }
            CodecError::UnexpectedVersion(v) => {
                write!(f, "unexpected QUIC version {v:#010x}")
            }
            CodecError::VarIntEncode(e) => {
                write!(f, "VarInt encode error: {e}")
            }
            CodecError::AckRangeExceedsLargest { first_ack_range, largest_acknowledged } => {
                write!(
                    f,
                    "First ACK Range {first_ack_range} exceeds Largest Acknowledged {largest_acknowledged}"
                )
            }
            CodecError::CryptoOffsetOverflow => {
                write!(f, "CRYPTO frame offset + length overflows 2^62")
            }
            CodecError::MaxStreamsTooLarge(v) => {
                write!(f, "MAX_STREAMS value {v} exceeds 2^60")
            }
            CodecError::InvalidStateTransition { from, to } => {
                write!(f, "invalid state transition: {from} -> {to}")
            }
        }
    }
}

impl From<VarIntError> for CodecError {
    fn from(e: VarIntError) -> Self {
        CodecError::VarIntEncode(e)
    }
}
