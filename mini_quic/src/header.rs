//! QUIC Packet Header encoding and decoding — RFC 9000 §17.
//!
//! # Structure
//!
//! ```text
//! Long Header (§17.2):
//!   Byte 0:  1 | Fixed(1) | Long Type(2) | Type-specific(4)
//!   Bytes 1-4: Version (u32 big-endian)
//!   Byte 5:  DCID Length (0..=20)
//!   N bytes: Destination Connection ID
//!   Byte:    SCID Length (0..=20)
//!   M bytes: Source Connection ID
//!   [Initial only] Token Length (VarInt) + Token bytes
//!   Length (VarInt) — remaining bytes (packet number + payload)
//!   Packet Number (1–4 bytes determined by first_byte & 0x03)
//!
//! Short Header / 1-RTT (§17.3):
//!   Byte 0:  0 | Fixed(1) | Spin(0x20) | Rsvd(0x18) | KeyPhase(0x04) | PN Len(0x03)
//!   K bytes: Destination Connection ID (known length, passed in)
//!   Packet Number (1–4 bytes)
//! ```

use alloc::vec::Vec;

use crate::{
    cid::ConnectionId,
    error::CodecError,
    packet_number::{read_packet_number, write_packet_number},
    varint::VarInt,
};

// ─── Constants ────────────────────────────────────────────────────────────────

/// QUIC version 1 wire value.
pub const QUIC_VERSION_1: u32 = 0x0000_0001;
/// Version Negotiation pseudo-version.
pub const QUIC_VERSION_NEGOTIATION: u32 = 0x0000_0000;

/// Mask selecting the Header Form bit (long=1, short=0).
pub const HEADER_FORM_MASK: u8 = 0x80;
/// Mask for the Fixed Bit — must be 1 (RFC 9000 §17.2, §17.3).
pub const FIXED_BIT_MASK: u8 = 0x40;
/// Mask for the two Long Packet Type bits.
pub const LONG_PACKET_TYPE_MASK: u8 = 0x30;
/// Mask for the lower 4 type-specific bits in a long header.
pub const LONG_TYPE_SPECIFIC_MASK: u8 = 0x0F;
/// Mask for the Packet Number Length bits in both header forms.
pub const PN_LENGTH_MASK: u8 = 0x03;

// Short-header specific masks (§17.3.1)
/// Spin Bit mask (short header only).
pub const SPIN_BIT_MASK: u8 = 0x20;
/// Reserved Bits mask — must be 0 after header protection removal.
pub const SHORT_RESERVED_MASK: u8 = 0x18;
/// Key Phase bit mask (short header only).
pub const KEY_PHASE_MASK: u8 = 0x04;

// ─── Long-Header Packet Type ──────────────────────────────────────────────────

/// The four packet types encoded in the two Long Packet Type bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LongPacketType {
    Initial = 0x00,
    ZeroRtt = 0x01,
    Handshake = 0x02,
    Retry = 0x03,
}

impl LongPacketType {
    /// Decode from the 2-bit field `(first_byte & 0x30) >> 4`.
    pub fn from_bits(bits: u8) -> Result<Self, CodecError> {
        match bits & 0x03 {
            0x00 => Ok(LongPacketType::Initial),
            0x01 => Ok(LongPacketType::ZeroRtt),
            0x02 => Ok(LongPacketType::Handshake),
            0x03 => Ok(LongPacketType::Retry),
            other => Err(CodecError::UnknownLongPacketType(other)),
        }
    }

    /// Return the 2-bit representation.
    pub fn bits(self) -> u8 {
        self as u8
    }
}

// ─── Long Header ──────────────────────────────────────────────────────────────

/// A decoded QUIC Long Header packet.
///
/// All fields after the initial byte and version are extracted verbatim.
/// The `remaining_length` field reflects the VarInt `Length` field from the
/// wire (packet number bytes + payload bytes); it is not checked against
/// the actual payload slice length here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongHeader {
    /// The 4-bit type-specific bits from byte 0 (bits 3:0).
    pub type_specific_bits: u8,
    /// Long packet type from bits 5:4 of byte 0.
    pub packet_type: LongPacketType,
    /// QUIC version (big-endian u32 at bytes 1–4).
    pub version: u32,
    /// Destination Connection ID.
    pub dcid: ConnectionId,
    /// Source Connection ID.
    pub scid: ConnectionId,
    /// Token bytes (non-empty only for Initial packets).
    pub token: Vec<u8>,
    /// The `Length` VarInt field: bytes remaining after this field
    /// (= packet number length + payload length).
    pub remaining_length: VarInt,
    /// Truncated packet number as read from the wire.
    pub packet_number: u32,
    /// Number of bytes used to encode `packet_number` on the wire (1–4).
    pub packet_number_len: usize,
}

impl LongHeader {
    /// Encode this `LongHeader` into `dst`.
    ///
    /// Returns the number of bytes written (which is the header size without
    /// any payload).
    ///
    /// # Wire layout
    /// ```text
    /// [first_byte] [version:4] [dcid_len:1] [dcid] [scid_len:1] [scid]
    /// [token_len:varint] [token] [length:varint] [packet_number:1-4]
    /// ```
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        let mut pos = 0;

        // ── Byte 0 ────────────────────────────────────────────────────────────
        // bit 7: Header Form = 1 (long)
        // bit 6: Fixed Bit   = 1
        // bit 5-4: Long Packet Type
        // bit 3-0: type-specific bits (lower 4 contain PN length in bits 1:0)
        let first_byte: u8 = HEADER_FORM_MASK
            | FIXED_BIT_MASK
            | ((self.packet_type.bits() & 0x03) << 4)
            | (self.type_specific_bits & 0x0F);

        if dst.len() < 1 {
            return Err(CodecError::BufferTooSmall {
                needed: 1,
                available: 0,
            });
        }
        dst[pos] = first_byte;
        pos += 1;

        // ── Version (4 bytes big-endian) ──────────────────────────────────────
        if dst.len() < pos + 4 {
            return Err(CodecError::BufferTooSmall {
                needed: pos + 4,
                available: dst.len(),
            });
        }
        let ver_bytes = self.version.to_be_bytes();
        dst[pos..pos + 4].copy_from_slice(&ver_bytes);
        pos += 4;

        // ── DCID ──────────────────────────────────────────────────────────────
        let dcid_bytes = self.dcid.as_bytes();
        if dst.len() < pos + 1 + dcid_bytes.len() {
            return Err(CodecError::BufferTooSmall {
                needed: pos + 1 + dcid_bytes.len(),
                available: dst.len(),
            });
        }
        dst[pos] = dcid_bytes.len() as u8;
        pos += 1;
        dst[pos..pos + dcid_bytes.len()].copy_from_slice(dcid_bytes);
        pos += dcid_bytes.len();

        // ── SCID ──────────────────────────────────────────────────────────────
        let scid_bytes = self.scid.as_bytes();
        if dst.len() < pos + 1 + scid_bytes.len() {
            return Err(CodecError::BufferTooSmall {
                needed: pos + 1 + scid_bytes.len(),
                available: dst.len(),
            });
        }
        dst[pos] = scid_bytes.len() as u8;
        pos += 1;
        dst[pos..pos + scid_bytes.len()].copy_from_slice(scid_bytes);
        pos += scid_bytes.len();

        // ── Token (Initial only) ──────────────────────────────────────────────
        // For non-Initial packets the token slice is empty; we still emit a
        // zero-length VarInt to keep the decode logic symmetric.
        {
            let token_len = VarInt::from_u64(self.token.len() as u64)?;
            let tl_len = token_len.encoded_len();
            if dst.len() < pos + tl_len + self.token.len() {
                return Err(CodecError::BufferTooSmall {
                    needed: pos + tl_len + self.token.len(),
                    available: dst.len(),
                });
            }
            token_len.encode(&mut dst[pos..])?;
            pos += tl_len;
            dst[pos..pos + self.token.len()].copy_from_slice(&self.token);
            pos += self.token.len();
        }

        // ── Length VarInt ─────────────────────────────────────────────────────
        {
            let rl_len = self.remaining_length.encoded_len();
            if dst.len() < pos + rl_len {
                return Err(CodecError::BufferTooSmall {
                    needed: pos + rl_len,
                    available: dst.len(),
                });
            }
            self.remaining_length.encode(&mut dst[pos..])?;
            pos += rl_len;
        }

        // ── Packet Number ─────────────────────────────────────────────────────
        if dst.len() < pos + self.packet_number_len {
            return Err(CodecError::BufferTooSmall {
                needed: pos + self.packet_number_len,
                available: dst.len(),
            });
        }
        write_packet_number(&mut dst[pos..], self.packet_number, self.packet_number_len)?;
        pos += self.packet_number_len;

        Ok(pos)
    }

    /// Decode a `LongHeader` from `src`.
    ///
    /// `src` must begin at the first byte of the packet (the Header Form byte).
    /// Returns `(LongHeader, bytes_consumed)`.
    ///
    /// # Errors
    /// Returns errors for any bound violation, invalid fixed bit, or
    /// unsupported packet type.
    pub fn decode(src: &[u8]) -> Result<(Self, usize), CodecError> {
        let mut pos = 0;

        // ── Byte 0 ────────────────────────────────────────────────────────────
        if src.is_empty() {
            return Err(CodecError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        let first_byte = src[pos];
        pos += 1;

        // Must be a long header.
        debug_assert!(first_byte & HEADER_FORM_MASK != 0);

        // Fixed bit check (skip for Version Negotiation packets; we check
        // the version after parsing it).
        let fixed_bit_ok = (first_byte & FIXED_BIT_MASK) != 0;

        let type_bits = (first_byte & LONG_PACKET_TYPE_MASK) >> 4;
        let packet_type = LongPacketType::from_bits(type_bits)?;
        let type_specific_bits = first_byte & LONG_TYPE_SPECIFIC_MASK;
        // The Packet Number Length is encoded in the lowest 2 bits.
        let pn_len = ((type_specific_bits & PN_LENGTH_MASK) as usize) + 1;

        // ── Version ───────────────────────────────────────────────────────────
        if src.len() < pos + 4 {
            return Err(CodecError::UnexpectedEof {
                needed: pos + 4,
                available: src.len(),
            });
        }
        let version = u32::from_be_bytes([src[pos], src[pos + 1], src[pos + 2], src[pos + 3]]);
        pos += 4;

        // Version Negotiation packets have version == 0 and Fixed Bit may be 0.
        // For all other long-header packets the Fixed Bit must be 1.
        if version != QUIC_VERSION_NEGOTIATION && !fixed_bit_ok {
            return Err(CodecError::InvalidFixedBit);
        }

        // ── DCID ──────────────────────────────────────────────────────────────
        if src.len() < pos + 1 {
            return Err(CodecError::UnexpectedEof {
                needed: pos + 1,
                available: src.len(),
            });
        }
        let dcid_len = src[pos] as usize;
        pos += 1;
        if dcid_len > 20 {
            return Err(CodecError::CidTooLong { len: dcid_len });
        }
        if src.len() < pos + dcid_len {
            return Err(CodecError::UnexpectedEof {
                needed: pos + dcid_len,
                available: src.len(),
            });
        }
        let dcid = ConnectionId::new(&src[pos..pos + dcid_len])
            .map_err(|_| CodecError::CidTooLong { len: dcid_len })?;
        pos += dcid_len;

        // ── SCID ──────────────────────────────────────────────────────────────
        if src.len() < pos + 1 {
            return Err(CodecError::UnexpectedEof {
                needed: pos + 1,
                available: src.len(),
            });
        }
        let scid_len = src[pos] as usize;
        pos += 1;
        if scid_len > 20 {
            return Err(CodecError::CidTooLong { len: scid_len });
        }
        if src.len() < pos + scid_len {
            return Err(CodecError::UnexpectedEof {
                needed: pos + scid_len,
                available: src.len(),
            });
        }
        let scid = ConnectionId::new(&src[pos..pos + scid_len])
            .map_err(|_| CodecError::CidTooLong { len: scid_len })?;
        pos += scid_len;

        // ── Token (always present as a VarInt-length field) ───────────────────
        let (token_len_vi, tl_consumed) = VarInt::decode(&src[pos..])?;
        pos += tl_consumed;
        let token_len = token_len_vi.into_inner() as usize;
        if src.len() < pos + token_len {
            return Err(CodecError::UnexpectedEof {
                needed: pos + token_len,
                available: src.len(),
            });
        }
        let token = src[pos..pos + token_len].to_vec();
        pos += token_len;

        // ── Length VarInt ─────────────────────────────────────────────────────
        let (remaining_length, rl_consumed) = VarInt::decode(&src[pos..])?;
        pos += rl_consumed;

        // ── Packet Number ─────────────────────────────────────────────────────
        let packet_number = read_packet_number(&src[pos..], pn_len)?;
        pos += pn_len;

        Ok((
            LongHeader {
                type_specific_bits,
                packet_type,
                version,
                dcid,
                scid,
                token,
                remaining_length,
                packet_number,
                packet_number_len: pn_len,
            },
            pos,
        ))
    }
}

// ─── Short Header (1-RTT) ─────────────────────────────────────────────────────

/// A decoded QUIC 1-RTT (Short Header) packet.
///
/// The DCID length is not encoded in the short header; it must be provided
/// by the caller based on the connection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortHeader {
    /// Whether the Spin Bit (bit 5 of byte 0) is set.
    pub spin_bit: bool,
    /// Whether the Key Phase bit (bit 2 of byte 0) is set.
    pub key_phase: bool,
    /// Destination Connection ID.
    pub dcid: ConnectionId,
    /// Truncated packet number as read from the wire.
    pub packet_number: u32,
    /// Number of bytes used to encode `packet_number` (1–4).
    pub packet_number_len: usize,
}

impl ShortHeader {
    /// Encode this `ShortHeader` into `dst`.
    ///
    /// Returns the number of bytes written (header bytes only; payload is
    /// appended by the caller).
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        let mut pos = 0;

        // ── Byte 0 ────────────────────────────────────────────────────────────
        // bit 7: Header Form = 0 (short)
        // bit 6: Fixed Bit   = 1
        // bit 5: Spin Bit
        // bit 4-3: Reserved (must be 0)
        // bit 2: Key Phase
        // bit 1-0: Packet Number Length - 1
        if self.packet_number_len == 0 || self.packet_number_len > 4 {
            return Err(CodecError::InvalidPacketNumberLength(self.packet_number_len));
        }
        let first_byte: u8 = FIXED_BIT_MASK // Header Form=0 is implicit (bit 7 = 0)
            | if self.spin_bit { SPIN_BIT_MASK } else { 0 }
            // Reserved bits 0x18 remain 0
            | if self.key_phase { KEY_PHASE_MASK } else { 0 }
            | ((self.packet_number_len - 1) as u8 & PN_LENGTH_MASK);

        if dst.is_empty() {
            return Err(CodecError::BufferTooSmall {
                needed: 1,
                available: 0,
            });
        }
        dst[pos] = first_byte;
        pos += 1;

        // ── DCID ──────────────────────────────────────────────────────────────
        let dcid_bytes = self.dcid.as_bytes();
        if dst.len() < pos + dcid_bytes.len() {
            return Err(CodecError::BufferTooSmall {
                needed: pos + dcid_bytes.len(),
                available: dst.len(),
            });
        }
        dst[pos..pos + dcid_bytes.len()].copy_from_slice(dcid_bytes);
        pos += dcid_bytes.len();

        // ── Packet Number ─────────────────────────────────────────────────────
        if dst.len() < pos + self.packet_number_len {
            return Err(CodecError::BufferTooSmall {
                needed: pos + self.packet_number_len,
                available: dst.len(),
            });
        }
        write_packet_number(&mut dst[pos..], self.packet_number, self.packet_number_len)?;
        pos += self.packet_number_len;

        Ok(pos)
    }

    /// Decode a `ShortHeader` from `src`.
    ///
    /// `dcid_len` must be the DCID length negotiated during the handshake.
    /// Returns `(ShortHeader, bytes_consumed)`.
    ///
    /// # Errors
    /// Returns [`CodecError::InvalidFixedBit`] when bit 6 is zero.
    /// Returns [`CodecError::NonZeroReservedBits`] when bits 4:3 are non-zero.
    pub fn decode(src: &[u8], dcid_len: usize) -> Result<(Self, usize), CodecError> {
        let mut pos = 0;

        // ── Byte 0 ────────────────────────────────────────────────────────────
        if src.is_empty() {
            return Err(CodecError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        let first_byte = src[pos];
        pos += 1;

        // Header Form must be 0 for short header (asserted by caller context).
        debug_assert!((first_byte & HEADER_FORM_MASK) == 0);

        // Fixed Bit must be 1.
        if (first_byte & FIXED_BIT_MASK) == 0 {
            return Err(CodecError::InvalidFixedBit);
        }

        // Reserved bits must be 0 (after header protection is removed).
        if (first_byte & SHORT_RESERVED_MASK) != 0 {
            return Err(CodecError::NonZeroReservedBits);
        }

        let spin_bit = (first_byte & SPIN_BIT_MASK) != 0;
        let key_phase = (first_byte & KEY_PHASE_MASK) != 0;
        let pn_len = ((first_byte & PN_LENGTH_MASK) as usize) + 1;

        // ── DCID ──────────────────────────────────────────────────────────────
        if src.len() < pos + dcid_len {
            return Err(CodecError::UnexpectedEof {
                needed: pos + dcid_len,
                available: src.len(),
            });
        }
        let dcid = ConnectionId::new(&src[pos..pos + dcid_len])
            .map_err(|_| CodecError::CidTooLong { len: dcid_len })?;
        pos += dcid_len;

        // ── Packet Number ─────────────────────────────────────────────────────
        let packet_number = read_packet_number(&src[pos..], pn_len)?;
        pos += pn_len;

        Ok((
            ShortHeader {
                spin_bit,
                key_phase,
                dcid,
                packet_number,
                packet_number_len: pn_len,
            },
            pos,
        ))
    }
}

// ─── Top-Level Packet Header Dispatcher ──────────────────────────────────────

/// The two header forms dispatched from the first byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketHeader {
    Long(LongHeader),
    Short(ShortHeader),
}

impl PacketHeader {
    /// Decode a packet header from `src`.
    ///
    /// For short headers, `dcid_len` specifies the known DCID length.
    /// For long headers it is ignored.
    pub fn decode(src: &[u8], dcid_len: usize) -> Result<(Self, usize), CodecError> {
        if src.is_empty() {
            return Err(CodecError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        if src[0] & HEADER_FORM_MASK != 0 {
            let (lh, consumed) = LongHeader::decode(src)?;
            Ok((PacketHeader::Long(lh), consumed))
        } else {
            let (sh, consumed) = ShortHeader::decode(src, dcid_len)?;
            Ok((PacketHeader::Short(sh), consumed))
        }
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn make_initial_header() -> LongHeader {
        LongHeader {
            type_specific_bits: 0x00, // PN length = 1 byte (0+1)
            packet_type: LongPacketType::Initial,
            version: QUIC_VERSION_1,
            dcid: ConnectionId::new(&[0x01, 0x02, 0x03, 0x04]).unwrap(),
            scid: ConnectionId::new(&[0x05, 0x06]).unwrap(),
            token: vec![0xAA, 0xBB],
            remaining_length: VarInt::from_u64(100).unwrap(),
            packet_number: 0x01,
            packet_number_len: 1,
        }
    }

    #[test]
    fn long_header_round_trip() {
        let hdr = make_initial_header();
        let mut buf = [0u8; 64];
        let written = hdr.encode(&mut buf).unwrap();
        assert!(written > 0);

        let (decoded, consumed) = LongHeader::decode(&buf[..written]).unwrap();
        assert_eq!(consumed, written);
        assert_eq!(decoded, hdr);
    }

    #[test]
    fn short_header_round_trip() {
        let sh = ShortHeader {
            spin_bit: true,
            key_phase: false,
            dcid: ConnectionId::new(&[0x11, 0x22, 0x33, 0x44]).unwrap(),
            packet_number: 0x0042,
            packet_number_len: 2,
        };
        let mut buf = [0u8; 16];
        let written = sh.encode(&mut buf).unwrap();
        let (decoded, consumed) = ShortHeader::decode(&buf[..written], 4).unwrap();
        assert_eq!(consumed, written);
        assert_eq!(decoded, sh);
    }

    #[test]
    fn short_header_reserved_bits_rejected() {
        // Manually craft a short-header byte with reserved bits set.
        let bad_byte = FIXED_BIT_MASK | SHORT_RESERVED_MASK; // bits 4:3 = 0b11
        let dcid = [0u8; 4];
        let pn = [0x00u8, 0x01];
        let mut src = Vec::new();
        src.push(bad_byte);
        src.extend_from_slice(&dcid);
        src.extend_from_slice(&pn);

        assert_eq!(
            ShortHeader::decode(&src, 4),
            Err(CodecError::NonZeroReservedBits)
        );
    }

    #[test]
    fn long_header_fixed_bit_rejected() {
        // First byte: Header Form = 1, Fixed Bit = 0, Initial type.
        let bad_byte: u8 = HEADER_FORM_MASK | (LongPacketType::Initial.bits() << 4);
        let version_bytes = QUIC_VERSION_1.to_be_bytes();
        let mut src = vec![bad_byte];
        src.extend_from_slice(&version_bytes);
        // Pad with zeros for DCID/SCID lengths
        src.extend_from_slice(&[0u8; 10]);

        assert_eq!(LongHeader::decode(&src), Err(CodecError::InvalidFixedBit));
    }
}
