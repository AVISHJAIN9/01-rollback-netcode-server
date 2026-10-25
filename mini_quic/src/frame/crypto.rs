//! CRYPTO Frame codec and out-of-order reassembly buffer — RFC 9000 §19.6.
//!
//! # Wire Layout
//!
//! ```text
//! Type (VarInt)     0x06
//! Offset (VarInt)   byte offset within the crypto stream
//! Length (VarInt)   byte count of the following Data field
//! Data (bytes)      TLS record-layer bytes
//! ```
//!
//! # Reassembly
//!
//! `CryptoStreamBuffer` accepts out-of-order `CryptoFrame` deliveries and
//! maintains a sorted, gap-aware list of received intervals.  The
//! `read_contiguous` method returns (and removes from the buffer) any bytes
//! that form a contiguous prefix starting at offset 0 (the next byte the TLS
//! layer expects).

use alloc::vec::Vec;

use crate::{error::CodecError, varint::VarInt};

/// The maximum byte offset representable in a CRYPTO stream: 2^62 − 1.
const MAX_CRYPTO_OFFSET: u64 = (1u64 << 62) - 1;

// ─── CryptoFrame ─────────────────────────────────────────────────────────────

/// A decoded QUIC CRYPTO frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptoFrame {
    /// Byte offset within the crypto stream where `data` begins.
    pub offset: u64,
    /// The TLS handshake bytes carried by this frame.
    pub data: Vec<u8>,
}

impl CryptoFrame {
    // ── Encoding ─────────────────────────────────────────────────────────────

    /// Encode this `CryptoFrame` into `dst`, including the leading `0x06`
    /// type byte.
    ///
    /// Returns the number of bytes written.
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        // Overflow guard: offset + len must fit in 2^62 − 1.
        let end = self
            .offset
            .checked_add(self.data.len() as u64)
            .ok_or(CodecError::CryptoOffsetOverflow)?;
        if end > MAX_CRYPTO_OFFSET {
            return Err(CodecError::CryptoOffsetOverflow);
        }

        let mut pos = 0;

        // Frame type 0x06
        pos += VarInt::from_u64(0x06)?.encode(&mut dst[pos..])?;

        // Offset
        pos += VarInt::from_u64(self.offset)?.encode(&mut dst[pos..])?;

        // Length
        pos += VarInt::from_u64(self.data.len() as u64)?.encode(&mut dst[pos..])?;

        // Data
        if dst.len() < pos + self.data.len() {
            return Err(CodecError::BufferTooSmall {
                needed: pos + self.data.len(),
                available: dst.len(),
            });
        }
        dst[pos..pos + self.data.len()].copy_from_slice(&self.data);
        pos += self.data.len();

        Ok(pos)
    }

    /// Return the encoded wire length in bytes.
    pub fn encoded_len(&self) -> usize {
        let mut len = 1; // type byte 0x06 (single-byte VarInt)
        len += varint_len(self.offset);
        len += varint_len(self.data.len() as u64);
        len += self.data.len();
        len
    }

    // ── Decoding ─────────────────────────────────────────────────────────────

    /// Decode a `CryptoFrame` from `src`.
    ///
    /// `src` must start **after** the `0x06` type byte.
    ///
    /// Returns `(CryptoFrame, bytes_consumed)`.
    pub fn decode(src: &[u8]) -> Result<(Self, usize), CodecError> {
        let mut pos = 0;

        // Offset
        let (offset_vi, n) = VarInt::decode(&src[pos..])?;
        pos += n;
        let offset = offset_vi.into_inner();

        // Length
        let (length_vi, n) = VarInt::decode(&src[pos..])?;
        pos += n;
        let length = length_vi.into_inner() as usize;

        // Overflow guard
        let end = offset
            .checked_add(length as u64)
            .ok_or(CodecError::CryptoOffsetOverflow)?;
        if end > MAX_CRYPTO_OFFSET {
            return Err(CodecError::CryptoOffsetOverflow);
        }

        // Data
        if src.len() < pos + length {
            return Err(CodecError::UnexpectedEof {
                needed: pos + length,
                available: src.len(),
            });
        }
        let data = src[pos..pos + length].to_vec();
        pos += length;

        Ok((CryptoFrame { offset, data }, pos))
    }
}

// ─── CryptoStreamBuffer ──────────────────────────────────────────────────────

/// An ordered, gap-aware reassembly buffer for the CRYPTO stream.
///
/// Out-of-order `CryptoFrame` deliveries are accepted and stored.  When the
/// buffer contains a contiguous prefix starting at `read_offset`, the caller
/// can drain it via [`CryptoStreamBuffer::read_contiguous`].
#[derive(Debug)]
pub struct CryptoStreamBuffer {
    /// The next byte offset the consumer expects (start of contiguous window).
    read_offset: u64,
    /// Sorted list of buffered non-overlapping chunks, ordered by `offset`.
    chunks: Vec<BufferedChunk>,
}

/// An individual buffered data chunk waiting to be consumed.
#[derive(Debug, Clone)]
struct BufferedChunk {
    offset: u64,
    data: Vec<u8>,
}

impl CryptoStreamBuffer {
    /// Construct an empty buffer starting at offset 0.
    pub fn new() -> Self {
        CryptoStreamBuffer {
            read_offset: 0,
            chunks: Vec::new(),
        }
    }

    /// Return the next byte offset the consumer is waiting for.
    pub fn read_offset(&self) -> u64 {
        self.read_offset
    }

    /// Insert data from `frame` into the buffer.
    ///
    /// Overlapping and duplicate regions are handled:
    /// - Data that falls entirely before `read_offset` (already consumed) is
    ///   silently dropped.
    /// - Data that partially overlaps consumed data is trimmed on the left.
    /// - Overlapping chunks in the pending list are merged / trimmed so the
    ///   stored chunks are always non-overlapping and sorted.
    pub fn ingest(&mut self, frame: &CryptoFrame) {
        let frame_end = match frame.offset.checked_add(frame.data.len() as u64) {
            Some(e) => e,
            None => return, // overflow — silently drop
        };

        // If the entire frame is before the read cursor, drop it.
        if frame_end <= self.read_offset {
            return;
        }

        // Trim the leading portion that is already consumed.
        let (start_offset, data_slice) = if frame.offset < self.read_offset {
            let skip = (self.read_offset - frame.offset) as usize;
            (self.read_offset, &frame.data[skip..])
        } else {
            (frame.offset, frame.data.as_slice())
        };

        if data_slice.is_empty() {
            return;
        }

        let end_offset = start_offset + data_slice.len() as u64;

        // Find insertion point in sorted chunk list.
        let insert_pos = self
            .chunks
            .partition_point(|c| c.offset < start_offset);

        // Collect the indices of existing chunks that overlap [start_offset, end_offset).
        let mut remove_start = insert_pos;
        let mut remove_end = insert_pos;
        while remove_end < self.chunks.len()
            && self.chunks[remove_end].offset < end_offset
        {
            remove_end += 1;
        }

        // Build the merged data.
        let mut merged_offset = start_offset;
        let mut merged_data: Vec<u8> = Vec::with_capacity(data_slice.len());

        // Check if the chunk immediately before overlaps on the left.
        if insert_pos > 0 {
            let prev = &self.chunks[insert_pos - 1];
            let prev_end = prev.offset + prev.data.len() as u64;
            if prev_end >= start_offset {
                // Extend backward to include prev.
                merged_offset = prev.offset;
                merged_data.extend_from_slice(&prev.data);
                remove_start = insert_pos - 1;
            }
        }

        // Fill in new data at [start_offset, end_offset).
        let new_rel_start = (start_offset - merged_offset) as usize;
        if merged_data.len() < new_rel_start + data_slice.len() {
            // Extend merged_data to fit new data.
            if merged_data.len() < new_rel_start {
                merged_data.resize(new_rel_start, 0);
            }
            let extend_len = new_rel_start + data_slice.len() - merged_data.len();
            merged_data.extend_from_slice(&data_slice[data_slice.len() - extend_len..]);
        }
        // Overlay new data (in case we extended or overwrite partially).
        let overlay_start = (start_offset - merged_offset) as usize;
        if overlay_start + data_slice.len() <= merged_data.len() {
            merged_data[overlay_start..overlay_start + data_slice.len()]
                .copy_from_slice(data_slice);
        } else {
            merged_data.resize(overlay_start, 0);
            merged_data.extend_from_slice(data_slice);
        }

        // Absorb any overlapping chunks on the right.
        for i in remove_start..remove_end {
            let c = &self.chunks[i];
            let c_end = c.offset + c.data.len() as u64;
            let merged_end = merged_offset + merged_data.len() as u64;
            if c_end > merged_end {
                // This chunk extends beyond our current merged end.
                let tail_start = (merged_end - c.offset) as usize;
                merged_data.extend_from_slice(&c.data[tail_start..]);
            }
        }

        // Replace the range [remove_start, remove_end) with the merged chunk.
        let merged_chunk = BufferedChunk {
            offset: merged_offset,
            data: merged_data,
        };
        self.chunks.splice(remove_start..remove_end, [merged_chunk]);
    }

    /// Drain and return all contiguous bytes starting at `read_offset`.
    ///
    /// If the first buffered chunk starts exactly at `read_offset`, those bytes
    /// are removed from the buffer and returned.  Otherwise returns an empty
    /// `Vec`.
    pub fn read_contiguous(&mut self) -> Vec<u8> {
        if self.chunks.is_empty() {
            return Vec::new();
        }
        if self.chunks[0].offset != self.read_offset {
            return Vec::new();
        }
        let chunk = self.chunks.remove(0);
        self.read_offset += chunk.data.len() as u64;
        chunk.data
    }

    /// Return `true` when the buffer holds no pending chunks.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Return the number of pending (buffered, not yet consumed) chunks.
    pub fn pending_chunks(&self) -> usize {
        self.chunks.len()
    }
}

impl Default for CryptoStreamBuffer {
    fn default() -> Self {
        Self::new()
    }
}

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

    fn round_trip(frame: &CryptoFrame) {
        let cap = frame.encoded_len();
        let mut buf = alloc::vec![0u8; cap];
        let written = frame.encode(&mut buf).unwrap();
        assert_eq!(written, cap);
        // Skip the type byte (1 byte for 0x06).
        let (decoded, consumed) = CryptoFrame::decode(&buf[1..written]).unwrap();
        assert_eq!(consumed, written - 1);
        assert_eq!(&decoded, frame);
    }

    #[test]
    fn crypto_frame_basic_round_trip() {
        round_trip(&CryptoFrame {
            offset: 0,
            data: b"ClientHello".to_vec(),
        });
    }

    #[test]
    fn crypto_frame_nonzero_offset() {
        round_trip(&CryptoFrame {
            offset: 1024,
            data: b"ServerHello".to_vec(),
        });
    }

    #[test]
    fn crypto_frame_empty_data() {
        round_trip(&CryptoFrame {
            offset: 0,
            data: alloc::vec![],
        });
    }

    #[test]
    fn crypto_frame_overflow_rejected() {
        let frame = CryptoFrame {
            offset: MAX_CRYPTO_OFFSET,
            data: alloc::vec![0u8; 1],
        };
        let mut buf = alloc::vec![0u8; 32];
        assert!(matches!(
            frame.encode(&mut buf),
            Err(CodecError::CryptoOffsetOverflow)
        ));
    }

    #[test]
    fn crypto_decode_truncated_returns_eof() {
        // Only 1 byte (offset VarInt), no length VarInt.
        let src = [0x05u8]; // offset = 5
        assert!(matches!(
            CryptoFrame::decode(&src),
            Err(CodecError::UnexpectedEof { .. })
        ));
    }

    // ── CryptoStreamBuffer tests ───────────────────────────────────────────

    #[test]
    fn buffer_in_order_delivery() {
        let mut buf = CryptoStreamBuffer::new();
        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });
        let out = buf.read_contiguous();
        assert_eq!(out, b"hello");
        assert_eq!(buf.read_offset(), 5);
    }

    #[test]
    fn buffer_out_of_order_reassembly() {
        let mut buf = CryptoStreamBuffer::new();
        // Deliver chunk 2 first, then chunk 1.
        buf.ingest(&CryptoFrame { offset: 5, data: b" world".to_vec() });
        // Nothing available yet (gap at offset 0..5).
        assert!(buf.read_contiguous().is_empty());

        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });
        let out = buf.read_contiguous();
        assert_eq!(out, b"hello");
        assert_eq!(buf.read_offset(), 5);

        let out2 = buf.read_contiguous();
        assert_eq!(out2, b" world");
        assert_eq!(buf.read_offset(), 11);
    }

    #[test]
    fn buffer_duplicate_delivery_ignored() {
        let mut buf = CryptoStreamBuffer::new();
        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });
        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });
        let out = buf.read_contiguous();
        assert_eq!(out, b"hello");
        // Second read should be empty.
        assert!(buf.read_contiguous().is_empty());
    }

    #[test]
    fn buffer_already_consumed_data_dropped() {
        let mut buf = CryptoStreamBuffer::new();
        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });
        buf.read_contiguous();

        // Re-deliver already-consumed data.
        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });
        assert!(buf.read_contiguous().is_empty());
        assert_eq!(buf.read_offset(), 5);
    }

    #[test]
    fn buffer_multiple_gaps_filled_in_sequence() {
        let mut buf = CryptoStreamBuffer::new();
        buf.ingest(&CryptoFrame { offset: 10, data: b"world".to_vec() });
        buf.ingest(&CryptoFrame { offset: 5, data: b"xxxxx".to_vec() });
        buf.ingest(&CryptoFrame { offset: 0, data: b"hello".to_vec() });

        let out = buf.read_contiguous();
        assert_eq!(out, b"hello");
        let out2 = buf.read_contiguous();
        assert_eq!(out2, b"xxxxx");
        let out3 = buf.read_contiguous();
        assert_eq!(out3, b"world");
        assert_eq!(buf.read_offset(), 15);
    }
}
