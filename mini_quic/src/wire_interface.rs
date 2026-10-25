// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Wire Format Interface & Type Adapters
// RFC 9000 Core Types, Version Negotiation, and Error Taxonomy

use std::fmt;
use std::net::SocketAddr;

/// Variable-length integer encoding (RFC 9000 §16).
/// Represents integers from 0 to 2^62-1 using 1, 2, 4, or 8 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VarInt(u64);

impl VarInt {
    pub const MIN: VarInt = VarInt(0);
    pub const MAX: VarInt = VarInt((1u64 << 62) - 1);

    pub fn new(value: u64) -> Result<Self, VarIntError> {
        if value > Self::MAX.0 {
            Err(VarIntError::OutOfRange)
        } else {
            Ok(VarInt(value))
        }
    }

    pub fn get(&self) -> u64 {
        self.0
    }

    pub fn encoded_len(&self) -> usize {
        match self.0 {
            0..=0x3f => 1,
            0x40..=0x3fff => 2,
            0x4000..=0x3fffffff => 4,
            _ => 8,
        }
    }

    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, VarIntError> {
        if buf.is_empty() {
            return Err(VarIntError::BufferTooShort);
        }
        let len = self.encoded_len();
        if buf.len() < len {
            return Err(VarIntError::BufferTooShort);
        }
        match len {
            1 => {
                buf[0] = (self.0 as u8) & 0x3f;
            }
            2 => {
                let v = self.0 as u16;
                buf[0] = ((v >> 8) as u8) | 0x40;
                buf[1] = (v & 0xff) as u8;
            }
            4 => {
                let v = self.0 as u32;
                buf[0] = ((v >> 24) as u8) | 0x80;
                buf[1] = ((v >> 16) & 0xff) as u8;
                buf[2] = ((v >> 8) & 0xff) as u8;
                buf[3] = (v & 0xff) as u8;
            }
            8 => {
                buf[0] = ((self.0 >> 56) as u8) | 0xc0;
                buf[1] = ((self.0 >> 48) & 0xff) as u8;
                buf[2] = ((self.0 >> 40) & 0xff) as u8;
                buf[3] = ((self.0 >> 32) & 0xff) as u8;
                buf[4] = ((self.0 >> 24) & 0xff) as u8;
                buf[5] = ((self.0 >> 16) & 0xff) as u8;
                buf[6] = ((self.0 >> 8) & 0xff) as u8;
                buf[7] = (self.0 & 0xff) as u8;
            }
            _ => unreachable!(),
        }
        Ok(len)
    }

    pub fn decode(buf: &[u8]) -> Result<(VarInt, usize), VarIntError> {
        if buf.is_empty() {
            return Err(VarIntError::BufferTooShort);
        }
        let first = buf[0];
        let prefix = first >> 6;
        match prefix {
            0 => {
                Ok((VarInt(first as u64 & 0x3f), 1))
            }
            1 => {
                if buf.len() < 2 {
                    return Err(VarIntError::BufferTooShort);
                }
                let v = (((first as u16 & 0x3f) << 8) | (buf[1] as u16)) as u64;
                Ok((VarInt(v), 2))
            }
            2 => {
                if buf.len() < 4 {
                    return Err(VarIntError::BufferTooShort);
                }
                let v = (((first as u32 & 0x3f) << 24)
                    | ((buf[1] as u32) << 16)
                    | ((buf[2] as u32) << 8)
                    | (buf[3] as u32)) as u64;
                Ok((VarInt(v), 4))
            }
            3 => {
                if buf.len() < 8 {
                    return Err(VarIntError::BufferTooShort);
                }
                let v = (((first as u64 & 0x3f) << 56)
                    | ((buf[1] as u64) << 48)
                    | ((buf[2] as u64) << 40)
                    | ((buf[3] as u64) << 32)
                    | ((buf[4] as u64) << 24)
                    | ((buf[5] as u64) << 16)
                    | ((buf[6] as u64) << 8)
                    | (buf[7] as u64));
                Ok((VarInt(v), 8))
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum VarIntError {
    OutOfRange,
    BufferTooShort,
}

impl fmt::Display for VarIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange => write!(f, "VarInt value out of range"),
            Self::BufferTooShort => write!(f, "buffer too short for VarInt"),
        }
    }
}

impl std::error::Error for VarIntError {}

/// Connection ID: Variable-length opaque identifier (RFC 9000 §5.1).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConnectionId(Vec<u8>);

impl ConnectionId {
    pub fn new(id: Vec<u8>) -> Self {
        ConnectionId(id)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u8>> for ConnectionId {
    fn from(v: Vec<u8>) -> Self {
        ConnectionId(v)
    }
}

impl From<&[u8]> for ConnectionId {
    fn from(v: &[u8]) -> Self {
        ConnectionId(v.to_vec())
    }
}

/// Packet Type: Initial, 0-RTT, Handshake, 1-RTT (RFC 9000 §17).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PacketType {
    Initial,
    ZeroRtt,
    Handshake,
    OneRtt,
}

impl PacketType {
    pub fn as_byte(&self) -> u8 {
        match self {
            PacketType::Initial => 0x00,
            PacketType::ZeroRtt => 0x10,
            PacketType::Handshake => 0x20,
            PacketType::OneRtt => 0x30,
        }
    }
}

/// QUIC Frame Types (RFC 9000 §12).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum FrameType {
    Padding = 0x00,
    Ping = 0x01,
    Ack = 0x02,
    ResetStream = 0x03,
    StopSending = 0x04,
    Crypto = 0x06,
    NewToken = 0x07,
    Stream = 0x08,
    MaxData = 0x10,
    MaxStreamData = 0x11,
    MaxStreams = 0x12,
    DataBlocked = 0x14,
    StreamDataBlocked = 0x15,
    StreamsBlocked = 0x16,
    NewConnectionId = 0x18,
    RetireConnectionId = 0x19,
    PathChallenge = 0x1a,
    PathResponse = 0x1b,
    ConnectionClose = 0x1c,
    HandshakeDone = 0x1e,
}

impl FrameType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(FrameType::Padding),
            0x01 => Some(FrameType::Ping),
            0x02 => Some(FrameType::Ack),
            0x03 => Some(FrameType::ResetStream),
            0x04 => Some(FrameType::StopSending),
            0x06 => Some(FrameType::Crypto),
            0x07 => Some(FrameType::NewToken),
            0x08 => Some(FrameType::Stream),
            0x10 => Some(FrameType::MaxData),
            0x11 => Some(FrameType::MaxStreamData),
            0x12 => Some(FrameType::MaxStreams),
            0x14 => Some(FrameType::DataBlocked),
            0x15 => Some(FrameType::StreamDataBlocked),
            0x16 => Some(FrameType::StreamsBlocked),
            0x18 => Some(FrameType::NewConnectionId),
            0x19 => Some(FrameType::RetireConnectionId),
            0x1a => Some(FrameType::PathChallenge),
            0x1b => Some(FrameType::PathResponse),
            0x1c => Some(FrameType::ConnectionClose),
            0x1e => Some(FrameType::HandshakeDone),
            _ => None,
        }
    }
}

/// Transport Error Codes (RFC 9000 §20).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TransportErrorCode {
    NoError = 0x0,
    InternalError = 0x1,
    ConnectionRefused = 0x2,
    FlowControlError = 0x3,
    StreamLimitError = 0x4,
    StreamStateError = 0x5,
    FinalSizeError = 0x6,
    FrameEncodingError = 0x7,
    TransportParameterError = 0x8,
    ProtocolViolation = 0xa,
    InvalidMigration = 0xb,
    CryptoBufferExceeded = 0xd,
    KeyUpdateError = 0xe,
    AeadLimitReached = 0xf,
    NoViablePath = 0x10,
}

impl TransportErrorCode {
    pub fn as_u64(&self) -> u64 {
        *self as u64
    }

    pub fn from_u64(code: u64) -> Option<Self> {
        match code {
            0x0 => Some(TransportErrorCode::NoError),
            0x1 => Some(TransportErrorCode::InternalError),
            0x2 => Some(TransportErrorCode::ConnectionRefused),
            0x3 => Some(TransportErrorCode::FlowControlError),
            0x4 => Some(TransportErrorCode::StreamLimitError),
            0x5 => Some(TransportErrorCode::StreamStateError),
            0x6 => Some(TransportErrorCode::FinalSizeError),
            0x7 => Some(TransportErrorCode::FrameEncodingError),
            0x8 => Some(TransportErrorCode::TransportParameterError),
            0xa => Some(TransportErrorCode::ProtocolViolation),
            0xb => Some(TransportErrorCode::InvalidMigration),
            0xd => Some(TransportErrorCode::CryptoBufferExceeded),
            0xe => Some(TransportErrorCode::KeyUpdateError),
            0xf => Some(TransportErrorCode::AeadLimitReached),
            0x10 => Some(TransportErrorCode::NoViablePath),
            _ => None,
        }
    }
}

/// Crypto Error Codes (RFC 9000 §20.2).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CryptoErrorCode {
    UnknownError = 0x00,
    UnsupportedVersion = 0x01,
    InternalError = 0x02,
    CryptoError = 0x03,
    InvalidEncoding = 0x05,
    InvalidAlert = 0x0a,
}

impl CryptoErrorCode {
    pub fn as_u64(&self) -> u64 {
        *self as u64
    }

    pub fn from_u64(code: u64) -> Option<Self> {
        match code {
            0x00 => Some(CryptoErrorCode::UnknownError),
            0x01 => Some(CryptoErrorCode::UnsupportedVersion),
            0x02 => Some(CryptoErrorCode::InternalError),
            0x03 => Some(CryptoErrorCode::CryptoError),
            0x05 => Some(CryptoErrorCode::InvalidEncoding),
            0x0a => Some(CryptoErrorCode::InvalidAlert),
            _ => None,
        }
    }
}

/// Packet Header structure.
#[derive(Clone, Debug)]
pub struct PacketHeader {
    pub packet_type: PacketType,
    pub version: u32,
    pub dcid: ConnectionId,
    pub scid: ConnectionId,
    pub packet_number: u64,
}

impl PacketHeader {
    pub fn new(
        packet_type: PacketType,
        version: u32,
        dcid: ConnectionId,
        scid: ConnectionId,
        packet_number: u64,
    ) -> Self {
        PacketHeader {
            packet_type,
            version,
            dcid,
            scid,
            packet_number,
        }
    }
}

/// Version Negotiation packet (RFC 9000 §17.2.1).
#[derive(Clone, Debug)]
pub struct VersionNegotiationPacket {
    pub dcid: ConnectionId,
    pub scid: ConnectionId,
    pub supported_versions: Vec<u32>,
}

impl VersionNegotiationPacket {
    pub fn new(dcid: ConnectionId, scid: ConnectionId, supported_versions: Vec<u32>) -> Self {
        VersionNegotiationPacket {
            dcid,
            scid,
            supported_versions,
        }
    }

    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, EncodeError> {
        if buf.len() < 1 + 4 + 1 + self.dcid.len() + 1 + self.scid.len() + 4 * self.supported_versions.len() {
            return Err(EncodeError::BufferTooShort);
        }

        let mut pos = 0;

        // First byte: 1 in most significant bit, and random for others
        buf[pos] = 0xc0 | (rand::random::<u8>() & 0x3f);
        pos += 1;

        // Version 0x00000000 (Version Negotiation marker)
        buf[pos..pos + 4].copy_from_slice(&[0, 0, 0, 0]);
        pos += 4;

        // DCID Length (1 byte)
        buf[pos] = self.dcid.len() as u8;
        pos += 1;

        // DCID
        buf[pos..pos + self.dcid.len()].copy_from_slice(self.dcid.as_bytes());
        pos += self.dcid.len();

        // SCID Length (1 byte)
        buf[pos] = self.scid.len() as u8;
        pos += 1;

        // SCID
        buf[pos..pos + self.scid.len()].copy_from_slice(self.scid.as_bytes());
        pos += self.scid.len();

        // Supported versions
        for version in &self.supported_versions {
            buf[pos..pos + 4].copy_from_slice(&version.to_be_bytes());
            pos += 4;
        }

        Ok(pos)
    }
}

#[derive(Debug, Clone)]
pub enum EncodeError {
    BufferTooShort,
    InvalidValue,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "buffer too short for encoding"),
            Self::InvalidValue => write!(f, "invalid value for encoding"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Connection state counters and telemetry.
#[derive(Clone, Debug, Default)]
pub struct ConnectionTelemetry {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_lost: u64,
    pub streams_opened: u64,
    pub pto_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_encode_decode() {
        let test_values = [0u64, 63, 64, 0x3fff, 0x4000, 0x3fffffff, 0x40000000];

        for &value in &test_values {
            let varint = VarInt::new(value).unwrap();
            let mut buf = [0u8; 8];
            let len = varint.encode(&mut buf).unwrap();

            let (decoded, decoded_len) = VarInt::decode(&buf[..len]).unwrap();
            assert_eq!(varint, decoded);
            assert_eq!(len, decoded_len);
        }
    }

    #[test]
    fn test_connection_id() {
        let cid = ConnectionId::from(vec![1, 2, 3, 4, 5]);
        assert_eq!(cid.len(), 5);
        assert_eq!(cid.as_bytes(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_frame_type_conversion() {
        assert_eq!(FrameType::from_byte(0x02), Some(FrameType::Ack));
        assert_eq!(FrameType::from_byte(0x08), Some(FrameType::Stream));
        assert_eq!(FrameType::from_byte(0xff), None);
    }

    #[test]
    fn test_transport_error_codes() {
        assert_eq!(
            TransportErrorCode::from_u64(0x3),
            Some(TransportErrorCode::FlowControlError)
        );
        assert_eq!(TransportErrorCode::from_u64(0xff), None);
    }
}
