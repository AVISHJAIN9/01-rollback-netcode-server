//! Transport Parameter encoding and decoding — RFC 9000 §18.
//!
//! Transport parameters are encoded as a sequence of:
//! ```text
//! [Parameter ID: VarInt] [Length: VarInt] [Value: `Length` bytes]
//! ```
//!
//! Each parameter has a defined type (integer, byte string, or empty).
//! Unknown parameters must be silently ignored per §18.1.
//!
//! # Encoding conventions for integer parameters
//!
//! Integer-valued parameters are encoded as VarInts.  The length field
//! gives the encoded VarInt length (1, 2, 4, or 8 bytes).
//!
//! # Constants
//!
//! Parameter IDs are defined as `pub const` values.

use alloc::vec::Vec;
use alloc::vec;

use crate::{
    cid::ConnectionId,
    error::CodecError,
    varint::VarInt,
};

// ─── Parameter ID Constants ───────────────────────────────────────────────────

pub const PARAM_ORIGINAL_DESTINATION_CONNECTION_ID: u64 = 0x00;
pub const PARAM_MAX_IDLE_TIMEOUT:                   u64 = 0x01;
pub const PARAM_STATELESS_RESET_TOKEN:              u64 = 0x02;
pub const PARAM_MAX_UDP_PAYLOAD_SIZE:               u64 = 0x03;
pub const PARAM_INITIAL_MAX_DATA:                   u64 = 0x04;
pub const PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u64 = 0x05;
pub const PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE: u64 = 0x06;
pub const PARAM_INITIAL_MAX_STREAM_DATA_UNI:        u64 = 0x07;
pub const PARAM_INITIAL_MAX_STREAMS_BIDI:           u64 = 0x08;
pub const PARAM_INITIAL_MAX_STREAMS_UNI:            u64 = 0x09;
pub const PARAM_ACK_DELAY_EXPONENT:                 u64 = 0x0a;
pub const PARAM_MAX_ACK_DELAY:                      u64 = 0x0b;
pub const PARAM_DISABLE_ACTIVE_MIGRATION:           u64 = 0x0c;
pub const PARAM_ACTIVE_CONNECTION_ID_LIMIT:         u64 = 0x0e;

// ─── TransportParameters ─────────────────────────────────────────────────────

/// The full set of QUIC transport parameters negotiated during the handshake.
///
/// All fields use `Option<T>` so that missing parameters can be distinguished
/// from zero-valued parameters.  The RFC specifies defaults for most parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportParameters {
    /// 0x00: The original DCID chosen by the client.
    pub original_destination_connection_id: Option<ConnectionId>,
    /// 0x01: Maximum idle timeout in milliseconds (0 = disabled).
    pub max_idle_timeout_ms: Option<u64>,
    /// 0x02: 16-byte stateless reset token (server only).
    pub stateless_reset_token: Option<[u8; 16]>,
    /// 0x03: Maximum UDP payload size (default 65527).
    pub max_udp_payload_size: Option<u64>,
    /// 0x04: Initial connection-level receive window in bytes.
    pub initial_max_data: Option<u64>,
    /// 0x05: Initial stream-level receive window for locally-initiated bidi streams.
    pub initial_max_stream_data_bidi_local: Option<u64>,
    /// 0x06: Initial stream-level receive window for remotely-initiated bidi streams.
    pub initial_max_stream_data_bidi_remote: Option<u64>,
    /// 0x07: Initial stream-level receive window for unidirectional streams.
    pub initial_max_stream_data_uni: Option<u64>,
    /// 0x08: Maximum number of bidirectional streams the peer may open.
    pub initial_max_streams_bidi: Option<u64>,
    /// 0x09: Maximum number of unidirectional streams the peer may open.
    pub initial_max_streams_uni: Option<u64>,
    /// 0x0a: Exponent used to decode the ACK Delay field (default 3).
    pub ack_delay_exponent: Option<u64>,
    /// 0x0b: Maximum ACK delay the endpoint will use in ms (default 25).
    pub max_ack_delay_ms: Option<u64>,
    /// 0x0e: Maximum number of connection IDs the peer should provide (default 2).
    pub active_connection_id_limit: Option<u64>,
    /// 0x0c: If present (value length == 0), active migration is disabled.
    pub disable_active_migration: bool,
}

impl TransportParameters {
    /// Construct a `TransportParameters` with all fields set to `None`
    /// (no parameters asserted).
    pub fn empty() -> Self {
        TransportParameters {
            original_destination_connection_id: None,
            max_idle_timeout_ms: None,
            stateless_reset_token: None,
            max_udp_payload_size: None,
            initial_max_data: None,
            initial_max_stream_data_bidi_local: None,
            initial_max_stream_data_bidi_remote: None,
            initial_max_stream_data_uni: None,
            initial_max_streams_bidi: None,
            initial_max_streams_uni: None,
            ack_delay_exponent: None,
            max_ack_delay_ms: None,
            active_connection_id_limit: None,
            disable_active_migration: false,
        }
    }

    // ── Encoding ─────────────────────────────────────────────────────────────

    /// Encode the transport parameters into `dst`.
    ///
    /// Each set parameter is written as `[id: VarInt][length: VarInt][value]`.
    /// Returns the number of bytes written.
    pub fn encode(&self, dst: &mut [u8]) -> Result<usize, CodecError> {
        let mut pos = 0;

        // Helper macros as closures to write one integer parameter.
        let mut write_int = |dst: &mut [u8], pos: &mut usize, id: u64, val: u64| -> Result<(), CodecError> {
            let id_vi = VarInt::from_u64(id)?;
            let val_vi = VarInt::from_u64(val)?;
            let val_len = val_vi.encoded_len();
            let len_vi = VarInt::from_u64(val_len as u64)?;

            *pos += id_vi.encode(&mut dst[*pos..])?;
            *pos += len_vi.encode(&mut dst[*pos..])?;
            *pos += val_vi.encode(&mut dst[*pos..])?;
            Ok(())
        };

        // Helper to write a raw-bytes parameter.
        let mut write_bytes =
            |dst: &mut [u8], pos: &mut usize, id: u64, bytes: &[u8]| -> Result<(), CodecError> {
                let id_vi = VarInt::from_u64(id)?;
                let len_vi = VarInt::from_u64(bytes.len() as u64)?;
                *pos += id_vi.encode(&mut dst[*pos..])?;
                *pos += len_vi.encode(&mut dst[*pos..])?;
                if dst.len() < *pos + bytes.len() {
                    return Err(CodecError::BufferTooSmall {
                        needed: *pos + bytes.len(),
                        available: dst.len(),
                    });
                }
                dst[*pos..*pos + bytes.len()].copy_from_slice(bytes);
                *pos += bytes.len();
                Ok(())
            };

        // Write each present parameter in canonical order.
        if let Some(cid) = &self.original_destination_connection_id {
            write_bytes(dst, &mut pos, PARAM_ORIGINAL_DESTINATION_CONNECTION_ID, cid.as_bytes())?;
        }
        if let Some(v) = self.max_idle_timeout_ms {
            write_int(dst, &mut pos, PARAM_MAX_IDLE_TIMEOUT, v)?;
        }
        if let Some(token) = &self.stateless_reset_token {
            write_bytes(dst, &mut pos, PARAM_STATELESS_RESET_TOKEN, token)?;
        }
        if let Some(v) = self.max_udp_payload_size {
            write_int(dst, &mut pos, PARAM_MAX_UDP_PAYLOAD_SIZE, v)?;
        }
        if let Some(v) = self.initial_max_data {
            write_int(dst, &mut pos, PARAM_INITIAL_MAX_DATA, v)?;
        }
        if let Some(v) = self.initial_max_stream_data_bidi_local {
            write_int(dst, &mut pos, PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL, v)?;
        }
        if let Some(v) = self.initial_max_stream_data_bidi_remote {
            write_int(dst, &mut pos, PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE, v)?;
        }
        if let Some(v) = self.initial_max_stream_data_uni {
            write_int(dst, &mut pos, PARAM_INITIAL_MAX_STREAM_DATA_UNI, v)?;
        }
        if let Some(v) = self.initial_max_streams_bidi {
            write_int(dst, &mut pos, PARAM_INITIAL_MAX_STREAMS_BIDI, v)?;
        }
        if let Some(v) = self.initial_max_streams_uni {
            write_int(dst, &mut pos, PARAM_INITIAL_MAX_STREAMS_UNI, v)?;
        }
        if let Some(v) = self.ack_delay_exponent {
            write_int(dst, &mut pos, PARAM_ACK_DELAY_EXPONENT, v)?;
        }
        if let Some(v) = self.max_ack_delay_ms {
            write_int(dst, &mut pos, PARAM_MAX_ACK_DELAY, v)?;
        }
        if self.disable_active_migration {
            // Zero-length parameter.
            write_bytes(dst, &mut pos, PARAM_DISABLE_ACTIVE_MIGRATION, &[])?;
        }
        if let Some(v) = self.active_connection_id_limit {
            write_int(dst, &mut pos, PARAM_ACTIVE_CONNECTION_ID_LIMIT, v)?;
        }

        Ok(pos)
    }

    // ── Decoding ─────────────────────────────────────────────────────────────

    /// Decode transport parameters from `src`.
    ///
    /// Unknown parameter IDs are silently ignored per RFC 9000 §18.1.
    pub fn decode(src: &[u8]) -> Result<Self, CodecError> {
        let mut tp = TransportParameters::empty();
        let mut pos = 0;

        while pos < src.len() {
            // Parameter ID
            let (id_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            let id = id_vi.into_inner();

            // Length
            let (len_vi, n) = VarInt::decode(&src[pos..])?;
            pos += n;
            let length = len_vi.into_inner() as usize;

            // Bounds check
            if src.len() < pos + length {
                return Err(CodecError::UnexpectedEof {
                    needed: pos + length,
                    available: src.len(),
                });
            }

            let value = &src[pos..pos + length];
            pos += length;

            // Dispatch on known parameter IDs.
            match id {
                PARAM_ORIGINAL_DESTINATION_CONNECTION_ID => {
                    let cid = ConnectionId::new(value)
                        .map_err(|_| CodecError::CidTooLong { len: value.len() })?;
                    tp.original_destination_connection_id = Some(cid);
                }
                PARAM_MAX_IDLE_TIMEOUT => {
                    tp.max_idle_timeout_ms = Some(decode_varint_param(value)?);
                }
                PARAM_STATELESS_RESET_TOKEN => {
                    if value.len() != 16 {
                        return Err(CodecError::UnexpectedEof {
                            needed: 16,
                            available: value.len(),
                        });
                    }
                    let mut token = [0u8; 16];
                    token.copy_from_slice(value);
                    tp.stateless_reset_token = Some(token);
                }
                PARAM_MAX_UDP_PAYLOAD_SIZE => {
                    tp.max_udp_payload_size = Some(decode_varint_param(value)?);
                }
                PARAM_INITIAL_MAX_DATA => {
                    tp.initial_max_data = Some(decode_varint_param(value)?);
                }
                PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL => {
                    tp.initial_max_stream_data_bidi_local =
                        Some(decode_varint_param(value)?);
                }
                PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE => {
                    tp.initial_max_stream_data_bidi_remote =
                        Some(decode_varint_param(value)?);
                }
                PARAM_INITIAL_MAX_STREAM_DATA_UNI => {
                    tp.initial_max_stream_data_uni = Some(decode_varint_param(value)?);
                }
                PARAM_INITIAL_MAX_STREAMS_BIDI => {
                    tp.initial_max_streams_bidi = Some(decode_varint_param(value)?);
                }
                PARAM_INITIAL_MAX_STREAMS_UNI => {
                    tp.initial_max_streams_uni = Some(decode_varint_param(value)?);
                }
                PARAM_ACK_DELAY_EXPONENT => {
                    tp.ack_delay_exponent = Some(decode_varint_param(value)?);
                }
                PARAM_MAX_ACK_DELAY => {
                    tp.max_ack_delay_ms = Some(decode_varint_param(value)?);
                }
                PARAM_DISABLE_ACTIVE_MIGRATION => {
                    // Zero-length parameter.
                    tp.disable_active_migration = true;
                }
                PARAM_ACTIVE_CONNECTION_ID_LIMIT => {
                    tp.active_connection_id_limit = Some(decode_varint_param(value)?);
                }
                // Unknown parameter: silently ignore.
                _ => {}
            }
        }

        Ok(tp)
    }
}

/// Decode a VarInt-encoded integer from a parameter value slice.
fn decode_varint_param(value: &[u8]) -> Result<u64, CodecError> {
    if value.is_empty() {
        // Zero-length integer parameter means value = 0.
        return Ok(0);
    }
    let (vi, _) = VarInt::decode(value)?;
    Ok(vi.into_inner())
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn round_trip(tp: &TransportParameters) -> TransportParameters {
        let mut buf = vec![0u8; 1024];
        let written = tp.encode(&mut buf).unwrap();
        TransportParameters::decode(&buf[..written]).unwrap()
    }

    #[test]
    fn empty_parameters_round_trip() {
        let tp = TransportParameters::empty();
        let decoded = round_trip(&tp);
        assert_eq!(decoded, tp);
    }

    #[test]
    fn integer_parameters_round_trip() {
        let mut tp = TransportParameters::empty();
        tp.max_idle_timeout_ms = Some(30_000);
        tp.initial_max_data = Some(1_048_576);
        tp.initial_max_streams_bidi = Some(100);
        tp.ack_delay_exponent = Some(3);
        tp.max_ack_delay_ms = Some(25);
        tp.active_connection_id_limit = Some(4);

        let decoded = round_trip(&tp);
        assert_eq!(decoded.max_idle_timeout_ms, Some(30_000));
        assert_eq!(decoded.initial_max_data, Some(1_048_576));
        assert_eq!(decoded.initial_max_streams_bidi, Some(100));
        assert_eq!(decoded.ack_delay_exponent, Some(3));
        assert_eq!(decoded.max_ack_delay_ms, Some(25));
        assert_eq!(decoded.active_connection_id_limit, Some(4));
    }

    #[test]
    fn original_dcid_round_trip() {
        let mut tp = TransportParameters::empty();
        let cid_bytes = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        tp.original_destination_connection_id =
            Some(ConnectionId::new(&cid_bytes).unwrap());

        let decoded = round_trip(&tp);
        assert_eq!(
            decoded.original_destination_connection_id,
            tp.original_destination_connection_id
        );
    }

    #[test]
    fn stateless_reset_token_round_trip() {
        let mut tp = TransportParameters::empty();
        tp.stateless_reset_token = Some([0xABu8; 16]);

        let decoded = round_trip(&tp);
        assert_eq!(decoded.stateless_reset_token, Some([0xABu8; 16]));
    }

    #[test]
    fn disable_active_migration_round_trip() {
        let mut tp = TransportParameters::empty();
        tp.disable_active_migration = true;

        let decoded = round_trip(&tp);
        assert!(decoded.disable_active_migration);
    }

    #[test]
    fn unknown_parameter_id_silently_ignored() {
        let mut tp = TransportParameters::empty();
        tp.initial_max_data = Some(1024);

        let mut buf = vec![0u8; 512];
        let written = tp.encode(&mut buf).unwrap();

        // Append an unknown parameter (id=0xFF, length=2, value=[0x01, 0x02]).
        let extra_start = written;
        let mut extra = vec![0u8; 16];
        let mut epos = 0;
        epos += VarInt::from_u64(0xFF).unwrap().encode(&mut extra[epos..]).unwrap();
        epos += VarInt::from_u64(2).unwrap().encode(&mut extra[epos..]).unwrap();
        extra[epos] = 0x01; epos += 1;
        extra[epos] = 0x02; epos += 1;
        buf[extra_start..extra_start + epos].copy_from_slice(&extra[..epos]);
        let total = written + epos;

        let decoded = TransportParameters::decode(&buf[..total]).unwrap();
        // Known parameter must be decoded correctly.
        assert_eq!(decoded.initial_max_data, Some(1024));
        // Unknown parameter must not cause an error.
    }

    #[test]
    fn truncated_value_returns_eof() {
        // Write a parameter with length=10 but only 3 bytes follow.
        let mut buf = [0u8; 32];
        let mut pos = 0;
        pos += VarInt::from_u64(PARAM_MAX_IDLE_TIMEOUT).unwrap().encode(&mut buf[pos..]).unwrap();
        pos += VarInt::from_u64(10).unwrap().encode(&mut buf[pos..]).unwrap(); // claims 10 bytes
        // Only write 3 bytes.
        buf[pos] = 0x01; pos += 1;
        buf[pos] = 0x02; pos += 1;
        buf[pos] = 0x03; pos += 1;

        let result = TransportParameters::decode(&buf[..pos]);
        assert!(matches!(result, Err(CodecError::UnexpectedEof { .. })));
    }

    #[test]
    fn all_parameters_round_trip() {
        let mut tp = TransportParameters::empty();
        tp.original_destination_connection_id =
            Some(ConnectionId::new(&[0xAA; 8]).unwrap());
        tp.max_idle_timeout_ms = Some(60_000);
        tp.stateless_reset_token = Some([0xBBu8; 16]);
        tp.max_udp_payload_size = Some(1500);
        tp.initial_max_data = Some(10_000_000);
        tp.initial_max_stream_data_bidi_local = Some(65536);
        tp.initial_max_stream_data_bidi_remote = Some(65536);
        tp.initial_max_stream_data_uni = Some(65536);
        tp.initial_max_streams_bidi = Some(200);
        tp.initial_max_streams_uni = Some(100);
        tp.ack_delay_exponent = Some(3);
        tp.max_ack_delay_ms = Some(25);
        tp.active_connection_id_limit = Some(4);
        tp.disable_active_migration = false;

        assert_eq!(round_trip(&tp), tp);
    }
}
