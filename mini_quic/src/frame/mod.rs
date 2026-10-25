//! QUIC Frame Type Registry — RFC 9000 §19.
//!
//! Every frame type defined in the RFC is represented as a variant of
//! [`FrameType`].  The `Stream` variant covers the eight STREAM frame type
//! codes `0x08..=0x0F` (the low 3 bits carry OFF/LEN/FIN flags).

pub mod stream;
pub mod ack;
pub mod crypto;

use crate::{error::CodecError, varint::VarInt};

/// The complete set of QUIC frame types.
///
/// Discriminant values match the RFC 9000 §19 type identifiers.  The
/// `Stream(u8)` variant holds the full type byte so that the OFF/LEN/FIN
/// flag bits are preserved for the stream codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameType {
    /// PADDING frame (§19.1) — type `0x00`.
    Padding,
    /// PING frame (§19.2) — type `0x01`.
    Ping,
    /// ACK frame (§19.3) — type `0x02`.
    Ack,
    /// ACK frame with ECN counts (§19.3) — type `0x03`.
    AckEcn,
    /// RESET_STREAM frame (§19.4) — type `0x04`.
    ResetStream,
    /// STOP_SENDING frame (§19.5) — type `0x05`.
    StopSending,
    /// CRYPTO frame (§19.6) — type `0x06`.
    Crypto,
    /// NEW_TOKEN frame (§19.7) — type `0x07`.
    NewToken,
    /// STREAM frame (§19.8) — types `0x08`–`0x0F`.
    ///
    /// The inner `u8` carries the full type byte so that the three flag bits
    /// (OFF=0x04, LEN=0x02, FIN=0x01) are accessible by the stream codec.
    Stream(u8),
    /// MAX_DATA frame (§19.9) — type `0x10`.
    MaxData,
    /// MAX_STREAM_DATA frame (§19.10) — type `0x11`.
    MaxStreamData,
    /// MAX_STREAMS frame for bidirectional streams (§19.11) — type `0x12`.
    MaxStreamsBidi,
    /// MAX_STREAMS frame for unidirectional streams (§19.11) — type `0x13`.
    MaxStreamsUni,
    /// DATA_BLOCKED frame (§19.12) — type `0x14`.
    DataBlocked,
    /// STREAM_DATA_BLOCKED frame (§19.13) — type `0x15`.
    StreamDataBlocked,
    /// STREAMS_BLOCKED frame for bidirectional streams (§19.14) — type `0x16`.
    StreamsBlockedBidi,
    /// STREAMS_BLOCKED frame for unidirectional streams (§19.14) — type `0x17`.
    StreamsBlockedUni,
    /// NEW_CONNECTION_ID frame (§19.15) — type `0x18`.
    NewConnectionId,
    /// RETIRE_CONNECTION_ID frame (§19.16) — type `0x19`.
    RetireConnectionId,
    /// PATH_CHALLENGE frame (§19.17) — type `0x1A`.
    PathChallenge,
    /// PATH_RESPONSE frame (§19.18) — type `0x1B`.
    PathResponse,
    /// CONNECTION_CLOSE frame carrying a transport error (§19.19) — type `0x1C`.
    ConnectionCloseTransport,
    /// CONNECTION_CLOSE frame carrying an application error (§19.19) — type `0x1D`.
    ConnectionCloseApp,
    /// HANDSHAKE_DONE frame (§19.20) — type `0x1E`.
    HandshakeDone,
}

impl FrameType {
    /// Decode a [`FrameType`] from a `VarInt` type identifier.
    ///
    /// # Errors
    /// Returns [`CodecError::UnknownFrameType`] for any value not defined in
    /// RFC 9000.
    pub fn from_varint(v: VarInt) -> Result<Self, CodecError> {
        let raw = v.into_inner();
        match raw {
            0x00 => Ok(FrameType::Padding),
            0x01 => Ok(FrameType::Ping),
            0x02 => Ok(FrameType::Ack),
            0x03 => Ok(FrameType::AckEcn),
            0x04 => Ok(FrameType::ResetStream),
            0x05 => Ok(FrameType::StopSending),
            0x06 => Ok(FrameType::Crypto),
            0x07 => Ok(FrameType::NewToken),
            // STREAM frames: 0x08..=0x0F (OFF | LEN | FIN flag bits in low 3 bits)
            0x08..=0x0F => Ok(FrameType::Stream(raw as u8)),
            0x10 => Ok(FrameType::MaxData),
            0x11 => Ok(FrameType::MaxStreamData),
            0x12 => Ok(FrameType::MaxStreamsBidi),
            0x13 => Ok(FrameType::MaxStreamsUni),
            0x14 => Ok(FrameType::DataBlocked),
            0x15 => Ok(FrameType::StreamDataBlocked),
            0x16 => Ok(FrameType::StreamsBlockedBidi),
            0x17 => Ok(FrameType::StreamsBlockedUni),
            0x18 => Ok(FrameType::NewConnectionId),
            0x19 => Ok(FrameType::RetireConnectionId),
            0x1A => Ok(FrameType::PathChallenge),
            0x1B => Ok(FrameType::PathResponse),
            0x1C => Ok(FrameType::ConnectionCloseTransport),
            0x1D => Ok(FrameType::ConnectionCloseApp),
            0x1E => Ok(FrameType::HandshakeDone),
            other => Err(CodecError::UnknownFrameType(other)),
        }
    }

    /// Return the canonical wire type byte for this frame type.
    ///
    /// For `Stream(b)`, the inner byte is returned directly.
    pub fn wire_type(self) -> u64 {
        match self {
            FrameType::Padding => 0x00,
            FrameType::Ping => 0x01,
            FrameType::Ack => 0x02,
            FrameType::AckEcn => 0x03,
            FrameType::ResetStream => 0x04,
            FrameType::StopSending => 0x05,
            FrameType::Crypto => 0x06,
            FrameType::NewToken => 0x07,
            FrameType::Stream(b) => b as u64,
            FrameType::MaxData => 0x10,
            FrameType::MaxStreamData => 0x11,
            FrameType::MaxStreamsBidi => 0x12,
            FrameType::MaxStreamsUni => 0x13,
            FrameType::DataBlocked => 0x14,
            FrameType::StreamDataBlocked => 0x15,
            FrameType::StreamsBlockedBidi => 0x16,
            FrameType::StreamsBlockedUni => 0x17,
            FrameType::NewConnectionId => 0x18,
            FrameType::RetireConnectionId => 0x19,
            FrameType::PathChallenge => 0x1A,
            FrameType::PathResponse => 0x1B,
            FrameType::ConnectionCloseTransport => 0x1C,
            FrameType::ConnectionCloseApp => 0x1D,
            FrameType::HandshakeDone => 0x1E,
        }
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_known_frame_types_round_trip() {
        let cases: &[(u64, FrameType)] = &[
            (0x00, FrameType::Padding),
            (0x01, FrameType::Ping),
            (0x02, FrameType::Ack),
            (0x03, FrameType::AckEcn),
            (0x04, FrameType::ResetStream),
            (0x05, FrameType::StopSending),
            (0x06, FrameType::Crypto),
            (0x07, FrameType::NewToken),
            (0x08, FrameType::Stream(0x08)),
            (0x0F, FrameType::Stream(0x0F)),
            (0x10, FrameType::MaxData),
            (0x11, FrameType::MaxStreamData),
            (0x12, FrameType::MaxStreamsBidi),
            (0x13, FrameType::MaxStreamsUni),
            (0x14, FrameType::DataBlocked),
            (0x15, FrameType::StreamDataBlocked),
            (0x16, FrameType::StreamsBlockedBidi),
            (0x17, FrameType::StreamsBlockedUni),
            (0x18, FrameType::NewConnectionId),
            (0x19, FrameType::RetireConnectionId),
            (0x1A, FrameType::PathChallenge),
            (0x1B, FrameType::PathResponse),
            (0x1C, FrameType::ConnectionCloseTransport),
            (0x1D, FrameType::ConnectionCloseApp),
            (0x1E, FrameType::HandshakeDone),
        ];
        for (raw, expected) in cases {
            let vi = VarInt::from_u64(*raw).unwrap();
            let ft = FrameType::from_varint(vi).expect("should be known");
            assert_eq!(ft, *expected, "mismatch for {raw:#04x}");
            assert_eq!(ft.wire_type(), *raw, "wire_type mismatch for {raw:#04x}");
        }
    }

    #[test]
    fn unknown_frame_type_rejected() {
        let vi = VarInt::from_u64(0xFF).unwrap();
        assert!(matches!(
            FrameType::from_varint(vi),
            Err(CodecError::UnknownFrameType(0xFF))
        ));
    }
}
