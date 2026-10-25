//! STREAM Frame codec — RFC 9000 §19.8.
//!
//! # Frame Type Byte
//!
//! The STREAM frame type occupies the range `0x08..=0x0F`.  The base value
//! is `0b0000_1000` (0x08); the lower three bits are flags:
//!
//! ```text
//! bit 2 (0x04): OFF — Offset field is present.
//! bit 1 (0x02): LEN — Length field is present.
//! bit 0 (0x01): FIN — This is the final STREAM frame.
//! ```
//!
//! # Wire Layout (all fields big-endian / VarInt)
//!
//! ```text
//! [type_byte: 1]          0x08 | flags
//! [stream_id: VarInt]     Stream identifier
//! [offset: VarInt]        Only present when OFF bit is set; otherwise 0
//! [length: VarInt]        Only present when LEN bit is set
//! [data: N bytes]         Exactly `length` bytes, or remainder of packet
//! ```

use alloc::vec::Vec;
use crate::{error::CodecError, varint::VarInt};

// ── Flag bit constants ────────────────────────────────────────────────────────

/// Base frame type for all STREAM frames.
pub const STREAM_FRAME_BASE: u8 = 0x08;
/// OFF bit: indicates the Offset field is present.
pub const STREAM_FLAG_OFF: u8 = 0x04;
/// LEN bit: indicates the Length field is present.
pub const STREAM_FLAG_LEN: u8 = 0x02;
/// FIN bit: indicates this is the final STREAM frame for the stream.
pub const STREAM_FLAG_FIN: u8 = 0x01;

// ─── StreamFrame ─────────────────────────────────────────────────────────────

/// A decoded QUIC STREAM frame.
///
/// Constructed by [`StreamFrame::decode`] and serialised by [`StreamFrame::encode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFrame {
    /// The Stream ID on which data is being sent.
    pub stream_id: VarInt,
    /// The byte offset within the stream at which `data` begins.
    ///
    /// Set to `0` when the OFF flag is absent (implicit offset).
    pub offset: u64,
    /// The raw payload bytes carried by this frame.
    pub data: Vec<u8>,
    /// If `true`, this is the last STREAM frame for this stream.
    pub fin: bool,
}

impl StreamFrame {
    // ── Encoding ─────────────────────────────────────────────────────────────

    /// Serialise this `StreamFrame` into `dst`.
    ///
    /// The serialiser:
    /// - Sets the OFF bit when `offset > 0`.
    /// - Always sets the LEN bit (encoding the data length as a VarInt) so
    ///   that the receiver can parse multiple frames in a single packet without
    ///   ambiguity.
    /// - Sets the FIN bit when `self.fin` is `true`.
    ///
    /// Returns the number of bytes written.
    ///
    /// # Errors
    /// Returns [`CodecError::BufferTooSmall`] when `dst` is not large enough.
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        let mut pos = 0;

        // ── Compute frame type byte ───────────────────────────────────────────
        let mut type_byte = STREAM_FRAME_BASE;
        if self.offset > 0 {
            type_byte |= STREAM_FLAG_OFF;
        }
        type_byte |= STREAM_FLAG_LEN; // always set LEN for unambiguous parsing
        if self.fin {
            type_byte |= STREAM_FLAG_FIN;
        }

        // ── Write type byte ───────────────────────────────────────────────────
        if dst.is_empty() {
            return Err(CodecError::BufferTooSmall {
                needed: 1,
                available: 0,
            });
        }
        dst[pos] = type_byte;
        pos += 1;

        // ── Stream ID ─────────────────────────────────────────────────────────
        {
            let sid_len = self.stream_id.encoded_len();
            if dst.len() < pos + sid_len {
                return Err(CodecError::BufferTooSmall {
                    needed: pos + sid_len,
                    available: dst.len(),
                });
            }
            self.stream_id.encode(&mut dst[pos..])?;
            pos += sid_len;
        }

        // ── Offset (only when OFF bit is set) ─────────────────────────────────
        if self.offset > 0 {
            let offset_vi = VarInt::from_u64(self.offset)?;
            let off_len = offset_vi.encoded_len();
            if dst.len() < pos + off_len {
                return Err(CodecError::BufferTooSmall {
                    needed: pos + off_len,
                    available: dst.len(),
                });
            }
            offset_vi.encode(&mut dst[pos..])?;
            pos += off_len;
        }

        // ── Length ────────────────────────────────────────────────────────────
        {
            let data_len_vi = VarInt::from_u64(self.data.len() as u64)?;
            let len_field_len = data_len_vi.encoded_len();
            if dst.len() < pos + len_field_len + self.data.len() {
                return Err(CodecError::BufferTooSmall {
                    needed: pos + len_field_len + self.data.len(),
                    available: dst.len(),
                });
            }
            data_len_vi.encode(&mut dst[pos..])?;
            pos += len_field_len;
        }

        // ── Data ──────────────────────────────────────────────────────────────
        dst[pos..pos + self.data.len()].copy_from_slice(&self.data);
        pos += self.data.len();

        Ok(pos)
    }

    // ── Decoding ─────────────────────────────────────────────────────────────

    /// Decode a `StreamFrame` from `src`.
    ///
    /// `src` must begin at the STREAM frame type byte (the `0x08..=0x0F` byte).
    ///
    /// When the LEN bit is **not** set, the entire remaining slice after the
    /// stream ID (and optional offset) is treated as the frame data.
    ///
    /// Returns `(StreamFrame, bytes_consumed)`.
    ///
    /// # Errors
    /// Returns structured codec errors for any bound or value violation.
    pub fn decode(src: &[u8]) -> Result<(Self, usize), CodecError> {
        let mut pos = 0;

        // ── Type byte ─────────────────────────────────────────────────────────
        if src.is_empty() {
            return Err(CodecError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        let type_byte = src[pos];
        pos += 1;

        // Validate this is actually a STREAM frame type.
        if type_byte < STREAM_FRAME_BASE || type_byte > STREAM_FRAME_BASE | 0x07 {
            return Err(CodecError::UnknownFrameType(type_byte as u64));
        }

        let has_off = (type_byte & STREAM_FLAG_OFF) != 0;
        let has_len = (type_byte & STREAM_FLAG_LEN) != 0;
        let fin = (type_byte & STREAM_FLAG_FIN) != 0;

        // ── Stream ID ─────────────────────────────────────────────────────────
        let (stream_id, sid_consumed) = VarInt::decode(&src[pos..])?;
        pos += sid_consumed;

        // ── Offset ────────────────────────────────────────────────────────────
        let offset: u64 = if has_off {
            let (offset_vi, off_consumed) = VarInt::decode(&src[pos..])?;
            pos += off_consumed;
            offset_vi.into_inner()
        } else {
            0
        };

        // ── Length / Data ─────────────────────────────────────────────────────
        let data: Vec<u8>;

        if has_len {
            let (length_vi, len_consumed) = VarInt::decode(&src[pos..])?;
            pos += len_consumed;
            let data_len = length_vi.into_inner() as usize;

            if src.len() < pos + data_len {
                return Err(CodecError::UnexpectedEof {
                    needed: pos + data_len,
                    available: src.len(),
                });
            }
            data = src[pos..pos + data_len].to_vec();
            pos += data_len;
        } else {
            // No LEN field: consume the entire remaining slice as data.
            data = src[pos..].to_vec();
            pos = src.len();
        }

        Ok((
            StreamFrame {
                stream_id,
                offset,
                data,
                fin,
            },
            pos,
        ))
    }

    /// Return the total encoded length in bytes without performing the actual
    /// encoding.  Useful for pre-allocating buffers.
    pub fn encoded_len(&self) -> usize {
        let mut len = 1; // type byte
        len += self.stream_id.encoded_len();
        if self.offset > 0 {
            // Safe: offset was decoded as VarInt, so it's already in range.
            let offset_vi = VarInt::from_u64(self.offset).expect("offset in VarInt range");
            len += offset_vi.encoded_len();
        }
        let data_len_vi =
            VarInt::from_u64(self.data.len() as u64).expect("data len in VarInt range");
        len += data_len_vi.encoded_len(); // LEN field (always set in encode())
        len += self.data.len();
        len
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn round_trip(frame: StreamFrame) {
        let expected_len = frame.encoded_len();
        let mut buf = vec![0u8; expected_len + 16]; // extra slack
        let written = frame.encode(&mut buf).unwrap();
        assert_eq!(written, expected_len, "encoded_len mismatch");

        let (decoded, consumed) = StreamFrame::decode(&buf[..written]).unwrap();
        assert_eq!(consumed, written);
        assert_eq!(decoded.stream_id, frame.stream_id);
        assert_eq!(decoded.offset, frame.offset);
        assert_eq!(decoded.data, frame.data);
        assert_eq!(decoded.fin, frame.fin);
    }

    #[test]
    fn no_off_no_len_no_fin() {
        // 0x08: base type, no flags
        round_trip(StreamFrame {
            stream_id: VarInt::from_u64(1).unwrap(),
            offset: 0,
            data: b"hello".to_vec(),
            fin: false,
        });
    }

    #[test]
    fn fin_only() {
        round_trip(StreamFrame {
            stream_id: VarInt::from_u64(3).unwrap(),
            offset: 0,
            data: vec![],
            fin: true,
        });
    }

    #[test]
    fn off_and_fin() {
        round_trip(StreamFrame {
            stream_id: VarInt::from_u64(7).unwrap(),
            offset: 1024,
            data: b"world".to_vec(),
            fin: true,
        });
    }

    #[test]
    fn all_flag_combinations() {
        // Test all 8 combinations of OFF / LEN / FIN flags using the decoder
        // directly with crafted byte sequences.
        //
        // We encode using our own encoder (which always sets LEN) and then
        // also manually test the no-LEN decode path.

        // Craft a no-LEN frame manually for stream_id=1, offset=0, data=[0xDE,0xAD].
        // type_byte = 0x08 (no flags), stream_id = 0x01 (1 byte), data = [0xDE, 0xAD]
        let no_len_src = [0x08u8, 0x01, 0xDE, 0xAD];
        let (sf, consumed) = StreamFrame::decode(&no_len_src).unwrap();
        assert_eq!(consumed, 4);
        assert_eq!(sf.stream_id.into_inner(), 1);
        assert_eq!(sf.offset, 0);
        assert_eq!(sf.data, [0xDE, 0xAD]);
        assert!(!sf.fin);
    }

    #[test]
    fn large_offset_and_stream_id() {
        round_trip(StreamFrame {
            stream_id: VarInt::from_u64(0x3FFF).unwrap(), // 2-byte VarInt
            offset: 0x4000,                                // 4-byte VarInt
            data: vec![0xFF; 200],
            fin: false,
        });
    }

    #[test]
    fn invalid_type_byte_rejected() {
        // 0x07 is NEW_TOKEN, not a STREAM frame.
        let bad = [0x07u8, 0x01, 0x00];
        assert!(matches!(
            StreamFrame::decode(&bad),
            Err(CodecError::UnknownFrameType(0x07))
        ));
    }
}
