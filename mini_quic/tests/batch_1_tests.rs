//! Batch 1 Integration Tests — Mini-QUIC Features 1–7.
//!
//! Covers:
//! - VarInt 1/2/4/8-byte boundaries and error cases.
//! - Long Header (Initial, Handshake, 0-RTT, Retry) round-trip encode/decode.
//! - Short Header spin bit, key phase, and reserved-bit rejection.
//! - Packet number reconstruction across 8-bit and 16-bit wrap-around.
//! - STREAM frame all 8 flag combinations (0x08–0x0F).

use mini_quic::{
    cid::ConnectionId,
    error::{CidError, CodecError, VarIntError},
    frame::{
        stream::{StreamFrame, STREAM_FLAG_FIN, STREAM_FLAG_LEN, STREAM_FLAG_OFF},
        FrameType,
    },
    header::{
        LongHeader, LongPacketType, PacketHeader, ShortHeader,
        FIXED_BIT_MASK, HEADER_FORM_MASK, QUIC_VERSION_1, SHORT_RESERVED_MASK,
    },
    packet_number::{
        decode_packet_number, encode_packet_number, read_packet_number, write_packet_number,
    },
    varint::{VarInt, VARINT_MAX},
};

// ═══════════════════════════════════════════════════════════════════════════════
// VarInt Tests
// ═══════════════════════════════════════════════════════════════════════════════

fn varint_round_trip(val: u64) {
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
fn varint_1byte_lower_bound() {
    varint_round_trip(0);
    let vi = VarInt::from_u64(0).unwrap();
    let mut buf = [0u8; 1];
    vi.encode(&mut buf).unwrap();
    assert_eq!(buf[0], 0x00);
}

#[test]
fn varint_1byte_upper_bound() {
    varint_round_trip(63);
    let vi = VarInt::from_u64(63).unwrap();
    let mut buf = [0u8; 1];
    vi.encode(&mut buf).unwrap();
    assert_eq!(buf[0], 0x3F);
    assert_eq!(vi.encoded_len(), 1);
}

#[test]
fn varint_2byte_lower_bound() {
    varint_round_trip(64);
    let vi = VarInt::from_u64(64).unwrap();
    assert_eq!(vi.encoded_len(), 2);
}

#[test]
fn varint_2byte_upper_bound() {
    varint_round_trip(0x3FFF);
    assert_eq!(VarInt::from_u64(0x3FFF).unwrap().encoded_len(), 2);
}

#[test]
fn varint_4byte_lower_bound() {
    varint_round_trip(0x4000);
    assert_eq!(VarInt::from_u64(0x4000).unwrap().encoded_len(), 4);
}

#[test]
fn varint_4byte_upper_bound() {
    varint_round_trip(0x3FFF_FFFF);
    assert_eq!(VarInt::from_u64(0x3FFF_FFFF).unwrap().encoded_len(), 4);
}

#[test]
fn varint_8byte_lower_bound() {
    varint_round_trip(0x4000_0000);
    assert_eq!(VarInt::from_u64(0x4000_0000).unwrap().encoded_len(), 8);
}

#[test]
fn varint_8byte_max_bound() {
    varint_round_trip(VARINT_MAX);
    assert_eq!(VarInt::from_u64(VARINT_MAX).unwrap().encoded_len(), 8);
}

#[test]
fn varint_too_large_rejected() {
    assert_eq!(
        VarInt::from_u64(VARINT_MAX + 1),
        Err(VarIntError::ValueTooLarge(VARINT_MAX + 1))
    );
    assert_eq!(
        VarInt::from_u64(u64::MAX),
        Err(VarIntError::ValueTooLarge(u64::MAX))
    );
}

#[test]
fn varint_decode_empty_returns_eof() {
    assert_eq!(
        VarInt::decode(&[]),
        Err(CodecError::UnexpectedEof { needed: 1, available: 0 })
    );
}

#[test]
fn varint_encode_buffer_too_small() {
    let vi = VarInt::from_u64(0x40).unwrap(); // needs 2 bytes
    let mut buf = [0u8; 1];
    assert_eq!(
        vi.encode(&mut buf),
        Err(CodecError::BufferTooSmall { needed: 2, available: 1 })
    );
}

#[test]
fn varint_rfc_examples() {
    // RFC 9000 §A.1
    let vi = VarInt::from_u64(37).unwrap();
    let mut b = [0u8; 1];
    vi.encode(&mut b).unwrap();
    assert_eq!(b[0], 0x25);

    let vi = VarInt::from_u64(15293).unwrap();
    let mut b = [0u8; 2];
    vi.encode(&mut b).unwrap();
    assert_eq!(b, [0x7b, 0xbd]);

    let vi = VarInt::from_u64(494_878_333).unwrap();
    let mut b = [0u8; 4];
    vi.encode(&mut b).unwrap();
    assert_eq!(b, [0x9d, 0x7f, 0x3e, 0x7d]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Long Header Tests
// ═══════════════════════════════════════════════════════════════════════════════

fn encode_decode_long_header(hdr: LongHeader) -> (LongHeader, usize) {
    let mut buf = vec![0u8; 256];
    let written = hdr.encode(&mut buf).unwrap();
    let (decoded, consumed) = LongHeader::decode(&buf[..written]).unwrap();
    assert_eq!(consumed, written);
    (decoded, written)
}

#[test]
fn long_header_initial_round_trip() {
    let hdr = LongHeader {
        type_specific_bits: 0x00,
        packet_type: LongPacketType::Initial,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::new(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]).unwrap(),
        scid: ConnectionId::new(&[0xAA, 0xBB, 0xCC, 0xDD]).unwrap(),
        token: vec![0x10, 0x20, 0x30],
        remaining_length: VarInt::from_u64(256).unwrap(),
        packet_number: 0x01,
        packet_number_len: 1,
    };
    let (decoded, _) = encode_decode_long_header(hdr.clone());
    assert_eq!(decoded, hdr);
}

#[test]
fn long_header_initial_max_cids_and_token() {
    let hdr = LongHeader {
        type_specific_bits: 0x03,
        packet_type: LongPacketType::Initial,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::new(&[0xFF; 20]).unwrap(),
        scid: ConnectionId::new(&[0xEE; 20]).unwrap(),
        token: vec![0xAB; 100],
        remaining_length: VarInt::from_u64(1024).unwrap(),
        packet_number: 0xDEAD_BEEF,
        packet_number_len: 4,
    };
    let (decoded, _) = encode_decode_long_header(hdr.clone());
    assert_eq!(decoded, hdr);
}

#[test]
fn long_header_initial_empty_token_and_cids() {
    let hdr = LongHeader {
        type_specific_bits: 0x01,
        packet_type: LongPacketType::Initial,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::empty(),
        scid: ConnectionId::empty(),
        token: vec![],
        remaining_length: VarInt::from_u64(42).unwrap(),
        packet_number: 0x0100,
        packet_number_len: 2,
    };
    let (decoded, _) = encode_decode_long_header(hdr.clone());
    assert_eq!(decoded, hdr);
}

#[test]
fn long_header_handshake_round_trip() {
    let hdr = LongHeader {
        type_specific_bits: 0x00,
        packet_type: LongPacketType::Handshake,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::new(&[0x11, 0x22, 0x33]).unwrap(),
        scid: ConnectionId::new(&[0x44, 0x55, 0x66]).unwrap(),
        token: vec![],
        remaining_length: VarInt::from_u64(500).unwrap(),
        packet_number: 0x07,
        packet_number_len: 1,
    };
    let (decoded, _) = encode_decode_long_header(hdr.clone());
    assert_eq!(decoded, hdr);
}

#[test]
fn long_header_zero_rtt_round_trip() {
    let hdr = LongHeader {
        type_specific_bits: 0x02,
        packet_type: LongPacketType::ZeroRtt,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::new(&[0xDE, 0xAD]).unwrap(),
        scid: ConnectionId::new(&[0xBE, 0xEF]).unwrap(),
        token: vec![],
        remaining_length: VarInt::from_u64(100).unwrap(),
        packet_number: 0x0001_0203,
        packet_number_len: 3,
    };
    let (decoded, _) = encode_decode_long_header(hdr.clone());
    assert_eq!(decoded, hdr);
}

#[test]
fn long_header_retry_round_trip() {
    let hdr = LongHeader {
        type_specific_bits: 0x00,
        packet_type: LongPacketType::Retry,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::new(&[0xCA, 0xFE]).unwrap(),
        scid: ConnectionId::new(&[0xBA, 0xBE]).unwrap(),
        token: vec![0x01, 0x02, 0x03, 0x04, 0x05],
        remaining_length: VarInt::from_u64(20).unwrap(),
        packet_number: 0x00,
        packet_number_len: 1,
    };
    let (decoded, _) = encode_decode_long_header(hdr.clone());
    assert_eq!(decoded, hdr);
}

#[test]
fn long_header_fixed_bit_zero_rejected() {
    let first_byte: u8 = HEADER_FORM_MASK; // bit 6 = 0 (Fixed Bit missing)
    let mut src = vec![first_byte];
    src.extend_from_slice(&QUIC_VERSION_1.to_be_bytes());
    src.extend_from_slice(&[0u8; 16]);
    assert_eq!(LongHeader::decode(&src), Err(CodecError::InvalidFixedBit));
}

#[test]
fn long_header_truncated_returns_eof() {
    let src = [0xC0u8, 0x00, 0x00]; // only 3 bytes — too short for version
    assert!(matches!(LongHeader::decode(&src), Err(CodecError::UnexpectedEof { .. })));
}

#[test]
fn long_header_dcid_too_long_rejected() {
    let first_byte: u8 = HEADER_FORM_MASK | FIXED_BIT_MASK;
    let mut src = vec![first_byte];
    src.extend_from_slice(&QUIC_VERSION_1.to_be_bytes());
    src.push(21); // DCID length = 21 — invalid
    src.extend_from_slice(&[0u8; 21]);
    src.extend_from_slice(&[0u8; 8]);
    assert_eq!(LongHeader::decode(&src), Err(CodecError::CidTooLong { len: 21 }));
}

#[test]
fn packet_header_dispatcher_selects_long() {
    let hdr = LongHeader {
        type_specific_bits: 0x00,
        packet_type: LongPacketType::Initial,
        version: QUIC_VERSION_1,
        dcid: ConnectionId::new(&[0x01]).unwrap(),
        scid: ConnectionId::new(&[0x02]).unwrap(),
        token: vec![],
        remaining_length: VarInt::from_u64(10).unwrap(),
        packet_number: 0x05,
        packet_number_len: 1,
    };
    let mut buf = vec![0u8; 64];
    let written = hdr.encode(&mut buf).unwrap();
    let (ph, consumed) = PacketHeader::decode(&buf[..written], 0).unwrap();
    assert_eq!(consumed, written);
    assert!(matches!(ph, PacketHeader::Long(_)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Short Header Tests
// ═══════════════════════════════════════════════════════════════════════════════

fn encode_decode_short_header(sh: ShortHeader, dcid_len: usize) -> ShortHeader {
    let mut buf = vec![0u8; 64];
    let written = sh.encode(&mut buf).unwrap();
    let (decoded, consumed) = ShortHeader::decode(&buf[..written], dcid_len).unwrap();
    assert_eq!(consumed, written);
    decoded
}

#[test]
fn short_header_basic_round_trip() {
    let sh = ShortHeader {
        spin_bit: false,
        key_phase: false,
        dcid: ConnectionId::new(&[0xAA, 0xBB, 0xCC, 0xDD]).unwrap(),
        packet_number: 0x42,
        packet_number_len: 1,
    };
    let decoded = encode_decode_short_header(sh.clone(), 4);
    assert_eq!(decoded, sh);
}

#[test]
fn short_header_spin_bit_set() {
    let sh = ShortHeader {
        spin_bit: true,
        key_phase: false,
        dcid: ConnectionId::new(&[0x01, 0x02]).unwrap(),
        packet_number: 0x0001,
        packet_number_len: 2,
    };
    let decoded = encode_decode_short_header(sh.clone(), 2);
    assert!(decoded.spin_bit);
    assert!(!decoded.key_phase);
}

#[test]
fn short_header_key_phase_set() {
    let sh = ShortHeader {
        spin_bit: false,
        key_phase: true,
        dcid: ConnectionId::new(&[0x10, 0x20, 0x30]).unwrap(),
        packet_number: 0x0000_0001,
        packet_number_len: 4,
    };
    let decoded = encode_decode_short_header(sh.clone(), 3);
    assert!(!decoded.spin_bit);
    assert!(decoded.key_phase);
}

#[test]
fn short_header_both_spin_and_key_phase() {
    let sh = ShortHeader {
        spin_bit: true,
        key_phase: true,
        dcid: ConnectionId::new(&[0xFF; 8]).unwrap(),
        packet_number: 0x0001_0203,
        packet_number_len: 3,
    };
    let decoded = encode_decode_short_header(sh.clone(), 8);
    assert_eq!(decoded, sh);
}

#[test]
fn short_header_reserved_bits_rejected() {
    for reserved_mask in [0x08u8, 0x10u8] {
        let bad_byte = FIXED_BIT_MASK | reserved_mask;
        let mut src = vec![bad_byte];
        src.extend_from_slice(&[0xABu8; 4]); // fake DCID
        src.extend_from_slice(&[0x01u8]);    // fake PN
        assert_eq!(
            ShortHeader::decode(&src, 4),
            Err(CodecError::NonZeroReservedBits),
            "reserved bit 0x{reserved_mask:02x} must be rejected"
        );
    }
}

#[test]
fn short_header_fixed_bit_zero_rejected() {
    let bad_byte: u8 = 0x00; // no FIXED_BIT_MASK
    let src = [bad_byte, 0xAB, 0xCD, 0x01];
    assert_eq!(ShortHeader::decode(&src, 2), Err(CodecError::InvalidFixedBit));
}

#[test]
fn packet_header_dispatcher_selects_short() {
    let sh = ShortHeader {
        spin_bit: false,
        key_phase: false,
        dcid: ConnectionId::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap(),
        packet_number: 0x99,
        packet_number_len: 1,
    };
    let mut buf = vec![0u8; 32];
    let written = sh.encode(&mut buf).unwrap();
    let (ph, consumed) = PacketHeader::decode(&buf[..written], 4).unwrap();
    assert_eq!(consumed, written);
    assert!(matches!(ph, PacketHeader::Short(_)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Connection ID Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cid_empty() {
    let cid = ConnectionId::empty();
    assert!(cid.is_empty());
    assert_eq!(cid.len(), 0);
}

#[test]
fn cid_max_length() {
    let raw = [0xAAu8; 20];
    let cid = ConnectionId::new(&raw).unwrap();
    assert_eq!(cid.len(), 20);
    assert_eq!(cid.as_bytes(), &raw);
}

#[test]
fn cid_too_long_rejected() {
    assert_eq!(
        ConnectionId::new(&[0u8; 21]),
        Err(CidError::TooLong { provided: 21 })
    );
}

#[test]
fn cid_ordering() {
    let a = ConnectionId::new(&[0x01]).unwrap();
    let b = ConnectionId::new(&[0x02]).unwrap();
    assert!(a < b);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Packet Number Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn pn_encode_1_byte() {
    let (trunc, nbytes) = encode_packet_number(10, 0);
    assert_eq!(nbytes, 1);
    assert_eq!(trunc, 10);
}

#[test]
fn pn_encode_2_bytes() {
    let (_, nbytes) = encode_packet_number(200, 0);
    assert_eq!(nbytes, 2);
}

#[test]
fn pn_encode_3_bytes() {
    let (_, nbytes) = encode_packet_number(50_000, 0);
    assert_eq!(nbytes, 3);
}

#[test]
fn pn_encode_4_bytes() {
    let (_, nbytes) = encode_packet_number(10_000_000, 0);
    assert_eq!(nbytes, 4);
}

#[test]
fn pn_write_read_round_trip_all_widths() {
    for nbytes in 1..=4usize {
        let max_val: u32 = if nbytes < 4 { (1u32 << (nbytes * 8)) - 1 } else { u32::MAX };
        let test_val = max_val / 2;
        let mut buf = [0u8; 4];
        write_packet_number(&mut buf, test_val, nbytes).unwrap();
        let read = read_packet_number(&buf, nbytes).unwrap();
        assert_eq!(read, test_val, "roundtrip failed for {nbytes} bytes");
    }
}

#[test]
fn pn_decode_simple_8bit() {
    assert_eq!(decode_packet_number(0, 1, 8), 1);
}

#[test]
fn pn_decode_8bit_forward_wrap() {
    // largest=254, truncated=0 → candidate=0 ≤ 255-128=127 → shift forward → 256
    assert_eq!(decode_packet_number(254, 0, 8), 256);
}

#[test]
fn pn_decode_8bit_backward_wrap() {
    // largest=511, truncated=255, 8-bit → candidate=767 > 512+128=640 → shift back → 511
    assert_eq!(decode_packet_number(0x1FF, 0xFF, 8), 511);
}

#[test]
fn pn_decode_16bit_high_pn() {
    // largest=0x10000, truncated=5 → candidate=0x10005, within window
    assert_eq!(decode_packet_number(0x1_0000, 5, 16), 0x1_0005);
}

#[test]
fn pn_decode_32bit_wrap() {
    // Just past a 32-bit boundary: largest=0xFFFF_FFFE, truncated=1
    // expected=0xFFFF_FFFF, candidate=1 ≤ expected-hwin → shift forward
    let result = decode_packet_number(0xFFFF_FFFE, 1, 32);
    assert_eq!(result, 0x1_0000_0001);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Frame Type Registry Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn frame_type_all_known_round_trip() {
    let cases: &[(u64, FrameType)] = &[
        (0x00, FrameType::Padding),
        (0x01, FrameType::Ping),
        (0x02, FrameType::Ack),
        (0x03, FrameType::AckEcn),
        (0x08, FrameType::Stream(0x08)),
        (0x0F, FrameType::Stream(0x0F)),
        (0x10, FrameType::MaxData),
        (0x18, FrameType::NewConnectionId),
        (0x1C, FrameType::ConnectionCloseTransport),
        (0x1E, FrameType::HandshakeDone),
    ];
    for (raw, expected) in cases {
        let vi = VarInt::from_u64(*raw).unwrap();
        let ft = FrameType::from_varint(vi).expect("should be known");
        assert_eq!(&ft, expected, "mismatch for {raw:#04x}");
        assert_eq!(ft.wire_type(), *raw);
    }
}

#[test]
fn frame_type_unknown_rejected() {
    let vi = VarInt::from_u64(0xFF).unwrap();
    assert!(matches!(
        FrameType::from_varint(vi),
        Err(CodecError::UnknownFrameType(0xFF))
    ));
}

// ═══════════════════════════════════════════════════════════════════════════════
// STREAM Frame Tests — all 8 flag combinations 0x08..=0x0F
// ═══════════════════════════════════════════════════════════════════════════════

fn stream_round_trip(frame: StreamFrame) {
    let cap = frame.encoded_len();
    let mut buf = vec![0u8; cap + 8];
    let written = frame.encode(&mut buf).unwrap();
    assert_eq!(written, cap);
    let (decoded, consumed) = StreamFrame::decode(&buf[..written]).unwrap();
    assert_eq!(consumed, written);
    assert_eq!(decoded.stream_id, frame.stream_id);
    assert_eq!(decoded.offset, frame.offset);
    assert_eq!(decoded.data, frame.data);
    assert_eq!(decoded.fin, frame.fin);
}

#[test]
fn stream_0x08_no_flags() {
    // 0x08: no OFF, no LEN, no FIN — use manual decode (no-LEN path)
    let src = [0x08u8, 0x01, 0xDE, 0xAD];
    let (sf, consumed) = StreamFrame::decode(&src).unwrap();
    assert_eq!(consumed, 4);
    assert_eq!(sf.stream_id.into_inner(), 1);
    assert_eq!(sf.offset, 0);
    assert_eq!(sf.data, [0xDE, 0xAD]);
    assert!(!sf.fin);
}

#[test]
fn stream_0x09_fin_only() {
    // 0x09: no OFF, no LEN, FIN
    let src = [0x09u8, 0x02]; // stream_id=2, no data
    let (sf, _) = StreamFrame::decode(&src).unwrap();
    assert_eq!(sf.stream_id.into_inner(), 2);
    assert!(sf.fin);
    assert!(sf.data.is_empty());
}

#[test]
fn stream_0x0a_len_only() {
    // 0x0A: no OFF, LEN present, no FIN
    stream_round_trip(StreamFrame {
        stream_id: VarInt::from_u64(3).unwrap(),
        offset: 0,
        data: b"hello".to_vec(),
        fin: false,
    });
}

#[test]
fn stream_0x0b_len_fin() {
    stream_round_trip(StreamFrame {
        stream_id: VarInt::from_u64(5).unwrap(),
        offset: 0,
        data: b"world".to_vec(),
        fin: true,
    });
}

#[test]
fn stream_0x0c_off_only() {
    // 0x0C: OFF present, no LEN, no FIN — manual decode
    // type=0x0C, stream_id=0x01, offset=0x10 (1 byte varint), data=remainder
    let mut src = vec![0x0Cu8];
    VarInt::from_u64(1).unwrap().encode(&mut { let mut b = [0u8;1]; b }).unwrap();
    // Build manually
    let mut buf = [0u8; 32];
    let mut pos = 0;
    buf[pos] = 0x0C; pos += 1;
    pos += VarInt::from_u64(1).unwrap().encode(&mut buf[pos..]).unwrap(); // stream_id
    pos += VarInt::from_u64(0x10).unwrap().encode(&mut buf[pos..]).unwrap(); // offset
    buf[pos] = 0xAB; buf[pos+1] = 0xCD; pos += 2; // data (remainder)

    let (sf, consumed) = StreamFrame::decode(&buf[..pos]).unwrap();
    assert_eq!(consumed, pos);
    assert_eq!(sf.stream_id.into_inner(), 1);
    assert_eq!(sf.offset, 0x10);
    assert_eq!(sf.data, [0xAB, 0xCD]);
    assert!(!sf.fin);
}

#[test]
fn stream_0x0d_off_fin() {
    stream_round_trip(StreamFrame {
        stream_id: VarInt::from_u64(7).unwrap(),
        offset: 1024,
        data: b"".to_vec(),
        fin: true,
    });
}

#[test]
fn stream_0x0e_off_len() {
    stream_round_trip(StreamFrame {
        stream_id: VarInt::from_u64(0x3FFF).unwrap(),
        offset: 0x4000,
        data: vec![0xFF; 100],
        fin: false,
    });
}

#[test]
fn stream_0x0f_all_flags() {
    stream_round_trip(StreamFrame {
        stream_id: VarInt::from_u64(99).unwrap(),
        offset: 8192,
        data: b"final payload".to_vec(),
        fin: true,
    });
}

#[test]
fn stream_invalid_type_rejected() {
    let src = [0x07u8, 0x01, 0x00]; // 0x07 = NEW_TOKEN, not STREAM
    assert!(matches!(
        StreamFrame::decode(&src),
        Err(CodecError::UnknownFrameType(0x07))
    ));
}
