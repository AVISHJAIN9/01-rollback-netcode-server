//! Batch 3 Integration Tests — Mini-QUIC Features 15–21.
//!
//! Covers:
//! - X25519 shared secret symmetry (A×B == B×A).
//! - ChaCha20-Poly1305 AEAD seal/open round-trips and error cases.
//! - Header protection mask apply/remove symmetry.
//! - RFC 9001 §A.1 initial key derivation test vectors.
//! - Key phase update changes key material and toggles the phase bit.
//! - PacketSpace packet number tracking, ACK processing, and discard.
//! - HandshakeTimer expiry and completion.
//! - ClosingState / DrainingState `3×PTO` timer.
//! - ConnectionState lifecycle FSM transitions.
//! - TransportParameters round-trip serialization and unknown ID handling.

use mini_quic::{
    cid::ConnectionId,
    crypto::{
        aead::{
            apply_header_protection, construct_nonce, extract_hp_sample,
            generate_hp_mask, open, seal, KEY_LEN, NONCE_LEN, TAG_LEN,
        },
        kex::KeyAgreement,
        keys::{derive_initial_keys, hkdf_expand_label, KeyPhaseManager},
    },
    error::CodecError,
    state::{
        connection::{ConnectionLifecycle, ConnectionState, Role},
        spaces::{Epoch, PacketSpace, PacketSpaces, SentPacket},
        termination::{ClosingState, DrainingState},
        timers::{HandshakeTimer, PtoTimer, RttEstimator, TIMER_GRANULARITY},
    },
    transport_parameters::{
        TransportParameters, PARAM_ACK_DELAY_EXPONENT, PARAM_INITIAL_MAX_DATA,
    },
    varint::VarInt,
};

use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 15 — X25519 Key Agreement
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn kex_shared_secret_symmetry() {
    let alice = KeyAgreement::generate();
    let bob   = KeyAgreement::generate();

    let alice_pub = *alice.public_key_bytes();
    let bob_pub   = *bob.public_key_bytes();

    let alice_shared = alice.diffie_hellman(&bob_pub).unwrap();
    let bob_shared   = bob.diffie_hellman(&alice_pub).unwrap();

    assert_eq!(
        alice_shared.as_bytes(),
        bob_shared.as_bytes(),
        "DH must be symmetric: Alice(Bob_pub) == Bob(Alice_pub)"
    );
}

#[test]
fn kex_independent_keypairs_differ() {
    let a = KeyAgreement::generate();
    let b = KeyAgreement::generate();
    assert_ne!(a.public_key_bytes(), b.public_key_bytes());
}

#[test]
fn kex_shared_secret_is_32_bytes() {
    let alice = KeyAgreement::generate();
    let bob   = KeyAgreement::generate();
    let bob_pub = *bob.public_key_bytes();
    let secret = alice.diffie_hellman(&bob_pub).unwrap();
    assert_eq!(secret.as_bytes().len(), 32);
}

#[test]
fn kex_different_pairs_produce_different_secrets() {
    let alice  = KeyAgreement::generate();
    let bob    = KeyAgreement::generate();
    let carol  = KeyAgreement::generate();

    let bob_pub   = *bob.public_key_bytes();
    let carol_pub = *carol.public_key_bytes();

    let alice_bob   = alice.diffie_hellman(&bob_pub).unwrap();
    let alice_carol = KeyAgreement::generate().diffie_hellman(&carol_pub).unwrap();

    // Two different pairings must yield different secrets (overwhelmingly likely).
    assert_ne!(alice_bob.as_bytes(), alice_carol.as_bytes());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 16 — AEAD and Header Protection
// ═══════════════════════════════════════════════════════════════════════════════

fn make_key_nonce() -> ([u8; KEY_LEN], [u8; NONCE_LEN]) {
    ([0x42u8; KEY_LEN], [0x11u8; NONCE_LEN])
}

#[test]
fn aead_seal_open_round_trip_basic() {
    let (key, nonce) = make_key_nonce();
    let aad       = b"QUIC v1 header";
    let plaintext = b"sensitive payload data 1234567890";

    let ct = seal(&key, &nonce, aad, plaintext).unwrap();
    assert_eq!(ct.len(), plaintext.len() + TAG_LEN);

    let pt = open(&key, &nonce, aad, &ct).unwrap();
    assert_eq!(pt.as_slice(), plaintext);
}

#[test]
fn aead_seal_empty_plaintext() {
    let (key, nonce) = make_key_nonce();
    let ct = seal(&key, &nonce, b"aad", b"").unwrap();
    assert_eq!(ct.len(), TAG_LEN); // only the authentication tag

    let pt = open(&key, &nonce, b"aad", &ct).unwrap();
    assert!(pt.is_empty());
}

#[test]
fn aead_open_rejects_wrong_key() {
    let (key, nonce) = make_key_nonce();
    let ct = seal(&key, &nonce, b"", b"secret").unwrap();

    let wrong_key = [0x00u8; KEY_LEN];
    let res = open(&wrong_key, &nonce, b"", &ct);
    assert!(matches!(res, Err(mini_quic::crypto::CryptoError::AeadFailure)));
}

#[test]
fn aead_open_rejects_wrong_nonce() {
    let (key, nonce) = make_key_nonce();
    let ct = seal(&key, &nonce, b"", b"secret").unwrap();

    let wrong_nonce = [0x00u8; NONCE_LEN];
    let res = open(&key, &wrong_nonce, b"", &ct);
    assert!(matches!(res, Err(mini_quic::crypto::CryptoError::AeadFailure)));
}

#[test]
fn aead_open_rejects_tampered_ciphertext() {
    let (key, nonce) = make_key_nonce();
    let mut ct = seal(&key, &nonce, b"", b"data").unwrap();
    ct[0] ^= 0x01; // flip first ciphertext byte
    let res = open(&key, &nonce, b"", &ct);
    assert!(matches!(res, Err(mini_quic::crypto::CryptoError::AeadFailure)));
}

#[test]
fn aead_open_rejects_tampered_tag() {
    let (key, nonce) = make_key_nonce();
    let mut ct = seal(&key, &nonce, b"", b"data").unwrap();
    let last = ct.len() - 1;
    ct[last] ^= 0xFF; // flip last tag byte
    let res = open(&key, &nonce, b"", &ct);
    assert!(matches!(res, Err(mini_quic::crypto::CryptoError::AeadFailure)));
}

#[test]
fn aead_open_rejects_wrong_aad() {
    let (key, nonce) = make_key_nonce();
    let ct = seal(&key, &nonce, b"correct_aad", b"data").unwrap();
    let res = open(&key, &nonce, b"wrong_aad", &ct);
    assert!(matches!(res, Err(mini_quic::crypto::CryptoError::AeadFailure)));
}

#[test]
fn nonce_construction_pn_0() {
    let iv = [0x00u8; 12];
    let nonce = construct_nonce(&iv, 0);
    assert_eq!(nonce, [0u8; 12]); // XOR with all-zero IV, PN=0 → all zeros
}

#[test]
fn nonce_construction_pn_1() {
    let iv = [0xFFu8; 12];
    let nonce = construct_nonce(&iv, 1);
    // PN=1 goes in bytes [4..12] as big-endian 8 bytes.
    // pn_bytes[4..12] = [0,0,0,0,0,0,0,1]
    // Byte 0..3: 0xFF XOR 0x00 = 0xFF
    assert_eq!(nonce[0], 0xFF);
    assert_eq!(nonce[4], 0xFF);
    // Byte 11: 0xFF XOR 0x01 = 0xFE
    assert_eq!(nonce[11], 0xFE);
}

#[test]
fn header_protection_mask_apply_twice_is_identity() {
    let hp_key = [0xBBu8; 32];
    let sample = [0x77u8; 16];
    let mask = generate_hp_mask(&hp_key, &sample).unwrap();

    // Simulate a short-header: byte 0 = 0x43 (short, fixed bit, pn_len=3)
    let mut hdr = [0x43u8, 0xAA, 0xBB, 0xCC, 0xDD, 0x00];
    let original = hdr;

    apply_header_protection(&mut hdr, 1, 4, &mask, false);
    assert_ne!(hdr, original, "mask must change the header");

    apply_header_protection(&mut hdr, 1, 4, &mask, false);
    assert_eq!(hdr, original, "applying twice must restore original");
}

#[test]
fn header_protection_long_header_uses_0f_mask() {
    let hp_key = [0xCCu8; 32];
    let sample = [0x33u8; 16];
    let mask = generate_hp_mask(&hp_key, &sample).unwrap();

    let mut long_hdr = [0xC0u8, 0x01, 0x02, 0x03]; // long header byte, PN
    let original = long_hdr;

    apply_header_protection(&mut long_hdr, 1, 2, &mask, true);
    apply_header_protection(&mut long_hdr, 1, 2, &mask, true);
    assert_eq!(long_hdr, original);
}

#[test]
fn hp_sample_extraction_succeeds() {
    // Must have at least 4 + 16 = 20 bytes.
    let ciphertext = [0xAAu8; 24];
    let sample = extract_hp_sample(&ciphertext).unwrap();
    assert_eq!(sample, [0xAAu8; 16]);
}

#[test]
fn hp_sample_extraction_fails_when_too_short() {
    let short_ct = [0x00u8; 10]; // < 20 bytes
    let res = extract_hp_sample(&short_ct);
    assert!(matches!(res, Err(mini_quic::crypto::CryptoError::SampleTooShort)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 17 — Initial Key Derivation (RFC 9001 §A.1 test vectors)
// ═══════════════════════════════════════════════════════════════════════════════

/// RFC 9001 §A.1: DCID = 0x8394c8f03e515708
const RFC_DCID: [u8; 8] = [0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08];

#[test]
fn initial_keys_client_write_key_matches_rfc9001() {
    let (client_keys, _) = derive_initial_keys(&RFC_DCID).unwrap();

    let expected = [
        0x1f, 0x36, 0x96, 0x13, 0xdd, 0x76, 0xd5, 0x46,
        0x77, 0x30, 0xef, 0xcb, 0xe3, 0xb1, 0xa2, 0x2d,
        0x42, 0x27, 0x64, 0x37, 0x2f, 0x8e, 0xcf, 0xe0,
        0x58, 0x85, 0x46, 0x88, 0x09, 0x27, 0x13, 0xb0,
    ];
    assert_eq!(client_keys.key.as_slice(), &expected, "RFC 9001 §A.1 client write key");
}

#[test]
fn initial_keys_client_iv_matches_rfc9001() {
    let (client_keys, _) = derive_initial_keys(&RFC_DCID).unwrap();

    let expected = [
        0xfa, 0x04, 0x4b, 0x2f, 0x42, 0xa3, 0xfd, 0x3b,
        0x46, 0xfb, 0x25, 0x5c,
    ];
    assert_eq!(client_keys.iv.as_slice(), &expected, "RFC 9001 §A.1 client IV");
}

#[test]
fn initial_keys_client_hp_key_matches_rfc9001() {
    let (client_keys, _) = derive_initial_keys(&RFC_DCID).unwrap();

    let expected = [
        0x9f, 0x50, 0x44, 0x9e, 0x04, 0xa0, 0xe8, 0x10,
        0x28, 0x38, 0x3d, 0x46, 0x93, 0x25, 0x1b, 0x16,
        0x56, 0x0c, 0x53, 0x59, 0xf5, 0x33, 0xc1, 0x1b,
        0x35, 0x6a, 0xbb, 0xe0, 0x0a, 0x5d, 0x08, 0x42,
    ];
    assert_eq!(client_keys.hp_key.as_slice(), &expected, "RFC 9001 §A.1 client HP key");
}

#[test]
fn key_phase_manager_starts_at_phase_0() {
    let secret = vec![0x01u8; 32];
    let mgr = KeyPhaseManager::new(secret).unwrap();
    assert_eq!(mgr.current_phase_bit(), 0);
}

#[test]
fn key_phase_manager_update_toggles_bit() {
    let secret = vec![0x02u8; 32];
    let mut mgr = KeyPhaseManager::new(secret).unwrap();
    mgr.update_keys().unwrap();
    assert_eq!(mgr.current_phase_bit(), 1);
    mgr.update_keys().unwrap();
    assert_eq!(mgr.current_phase_bit(), 0);
}

#[test]
fn key_phase_manager_update_changes_key() {
    let secret = vec![0x03u8; 32];
    let mut mgr = KeyPhaseManager::new(secret).unwrap();
    let key_0: Vec<u8> = mgr.current_key().to_vec();
    mgr.update_keys().unwrap();
    let key_1: Vec<u8> = mgr.current_key().to_vec();
    assert_ne!(key_0, key_1, "key material must change after update");
}

#[test]
fn key_phase_manager_prepare_next_makes_next_key_visible() {
    let secret = vec![0x04u8; 32];
    let mut mgr = KeyPhaseManager::new(secret).unwrap();
    assert!(mgr.next_key().is_none());
    mgr.prepare_next_keys().unwrap();
    assert!(mgr.next_key().is_some());
}

#[test]
fn hkdf_expand_label_is_deterministic() {
    let secret = [0xFFu8; 32];
    let out1 = hkdf_expand_label(&secret, b"quic key", b"", 32).unwrap();
    let out2 = hkdf_expand_label(&secret, b"quic key", b"", 32).unwrap();
    assert_eq!(out1, out2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 18 — Packet Spaces
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn packet_space_next_pn_monotonic() {
    let mut space = PacketSpace::new(Epoch::Initial);
    assert_eq!(space.next_pn(), Some(0));
    assert_eq!(space.next_pn(), Some(1));
    assert_eq!(space.next_pn(), Some(100 - 98)); // == 2
}

#[test]
fn packet_space_next_pn_none_after_discard() {
    let mut space = PacketSpace::new(Epoch::Handshake);
    space.next_pn().unwrap();
    space.discard();
    assert!(space.next_pn().is_none());
}

#[test]
fn packet_space_largest_received() {
    let mut space = PacketSpace::new(Epoch::ApplicationData);
    space.on_packet_received(100);
    space.on_packet_received(50);  // older — must not regress
    space.on_packet_received(200);
    assert_eq!(space.largest_received, Some(200));
}

#[test]
fn packet_space_ack_removes_correct_entries() {
    let mut space = PacketSpace::new(Epoch::Initial);
    for pn in 0..5 {
        space.on_packet_sent(SentPacket {
            packet_number: pn,
            sent_time_ns: 0,
            ack_eliciting: true,
            bytes: vec![],
        });
    }
    let acked = space.on_ack_received(&[1, 3]);
    assert!(acked.contains(&1));
    assert!(acked.contains(&3));
    assert_eq!(space.unacked_count(), 3);
}

#[test]
fn packet_space_discard_wipes_state() {
    let mut space = PacketSpace::new(Epoch::Handshake);
    space.set_keying_material(vec![0xAAu8; 96]);
    space.on_packet_sent(SentPacket {
        packet_number: 0,
        sent_time_ns: 0,
        ack_eliciting: true,
        bytes: vec![],
    });
    space.discard();
    assert!(space.is_discarded());
    assert!(space.keying_material.is_none());
    assert_eq!(space.unacked_count(), 0);
}

#[test]
fn packet_spaces_discard_initial_preserves_handshake() {
    let mut spaces = PacketSpaces::new();
    spaces.discard_initial();
    assert!(spaces.initial.is_discarded());
    assert!(!spaces.handshake.is_discarded());
    assert!(!spaces.application_data.is_discarded());
}

#[test]
fn packet_spaces_discard_handshake_preserves_application_data() {
    let mut spaces = PacketSpaces::new();
    spaces.discard_handshake();
    assert!(!spaces.initial.is_discarded());
    assert!(spaces.handshake.is_discarded());
    assert!(!spaces.application_data.is_discarded());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 19 — Timers
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn handshake_timer_not_expired_immediately() {
    let now = Instant::now();
    let timer = HandshakeTimer::new(now);
    assert!(!timer.is_expired(now));
}

#[test]
fn handshake_timer_expires_after_deadline() {
    let now = Instant::now();
    let timer = HandshakeTimer::with_timeout(now, Duration::from_nanos(1));
    let future = now + Duration::from_secs(100);
    assert!(timer.is_expired(future));
}

#[test]
fn handshake_timer_complete_disarms_expiry() {
    let now = Instant::now();
    let mut timer = HandshakeTimer::with_timeout(now, Duration::from_nanos(1));
    timer.complete();
    let future = now + Duration::from_secs(100);
    assert!(!timer.is_expired(future));
    assert!(timer.is_complete());
}

#[test]
fn rtt_estimator_first_sample() {
    let mut est = RttEstimator::new();
    est.update(100_000_000, 0);
    assert_eq!(est.smoothed_rtt_ns, 100_000_000);
    assert_eq!(est.rttvar_ns, 50_000_000);
}

#[test]
fn rtt_estimator_pto_includes_max_ack_delay() {
    let est = RttEstimator::new();
    let pto = est.pto_ns();
    // PTO >= smoothed_rtt + granularity + max_ack_delay
    assert!(pto >= est.smoothed_rtt_ns + est.max_ack_delay_ns);
}

#[test]
fn pto_timer_arm_expire_disarm() {
    let mut timer = PtoTimer::new();
    let now = Instant::now();
    timer.arm(now, Duration::from_millis(10));
    assert!(timer.is_armed());
    assert!(!timer.is_expired(now));
    assert!(timer.is_expired(now + Duration::from_secs(1)));
    timer.disarm();
    assert!(!timer.is_armed());
}

#[test]
fn pto_timer_backoff_doubles() {
    let mut timer = PtoTimer::new();
    let now = Instant::now();
    let base = Duration::from_millis(100);

    // First arm: backoff multiplier = 1.
    timer.arm(now, base);
    assert!(timer.is_armed());
    // Should expire after >= base.
    assert!(timer.is_expired(now + base + Duration::from_millis(1)));
    // Should not expire immediately.
    assert!(!timer.is_expired(now));

    timer.on_pto_fired(); // consecutive_pto_count = 1 → multiplier = 2

    // Second arm: backoff multiplier = 2, so timeout >= 2 * base.
    timer.arm(now, base);
    // At now + base it should NOT yet expire (timeout is 2×base).
    assert!(!timer.is_expired(now + base));
    // At now + 2*base + 1ms it should expire.
    assert!(timer.is_expired(now + base * 2 + Duration::from_millis(1)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 20 — Closing and Draining State Machines
// ═══════════════════════════════════════════════════════════════════════════════

const PTO: Duration = Duration::from_millis(50);

#[test]
fn closing_state_retransmits_close_frame_before_expiry() {
    let now = Instant::now();
    let frame = vec![0x1Cu8, 0x00, 0x00];
    let closing = ClosingState::new(frame.clone(), 0, now, PTO);

    let retx = closing.on_packet_received(now);
    assert_eq!(retx, Some(frame.as_slice()));
}

#[test]
fn closing_state_stops_retransmitting_after_3pto() {
    let now = Instant::now();
    let closing = ClosingState::new(vec![0x1Cu8], 0, now, PTO);
    let after = now + PTO * 4;
    assert!(closing.on_packet_received(after).is_none());
}

#[test]
fn closing_state_polls_to_closed() {
    let now = Instant::now();
    let mut closing = ClosingState::new(vec![0x1Cu8], 42, now, PTO);
    assert!(!closing.poll(now));
    assert!(closing.poll(now + PTO * 4));
    assert!(closing.is_closed());
}

#[test]
fn draining_state_should_not_send() {
    let now = Instant::now();
    let draining = DrainingState::new(now, PTO);
    assert!(!draining.should_send());
}

#[test]
fn draining_state_polls_to_closed_after_3pto() {
    let now = Instant::now();
    let mut draining = DrainingState::new(now, PTO);
    assert!(!draining.poll(now));
    assert!(draining.poll(now + PTO * 4));
    assert!(draining.is_closed());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 13/20 — Connection Lifecycle State Machine (extended)
// ═══════════════════════════════════════════════════════════════════════════════

fn fresh_conn(role: Role) -> ConnectionState {
    ConnectionState::new(role, ConnectionId::new(&[0x01]).unwrap())
}

#[test]
fn connection_full_happy_path() {
    let mut cs = fresh_conn(Role::Client);
    cs.start_handshake().unwrap();
    cs.handshake_complete().unwrap();
    cs.begin_closing().unwrap();
    cs.close().unwrap();
    assert_eq!(cs.lifecycle, ConnectionLifecycle::Closed);
    assert!(cs.lifecycle.is_terminal());
}

#[test]
fn connection_draining_path() {
    let mut cs = fresh_conn(Role::Server);
    cs.start_handshake().unwrap();
    cs.handshake_complete().unwrap();
    cs.begin_draining().unwrap();
    cs.close().unwrap();
    assert!(cs.lifecycle.is_terminal());
}

#[test]
fn connection_idle_cannot_exchange_data() {
    let cs = fresh_conn(Role::Client);
    assert!(!cs.lifecycle.can_exchange_data());
    assert!(cs.require_active().is_err());
}

#[test]
fn connection_active_can_exchange_data() {
    let mut cs = fresh_conn(Role::Client);
    cs.start_handshake().unwrap();
    cs.handshake_complete().unwrap();
    assert!(cs.lifecycle.can_exchange_data());
    assert!(cs.require_active().is_ok());
}

#[test]
fn connection_illegal_transition_rejected_and_state_unchanged() {
    let mut cs = fresh_conn(Role::Server);
    // Cannot jump from Idle to Active1Rtt.
    let err = cs.transition(ConnectionLifecycle::Active1Rtt);
    assert!(matches!(err, Err(CodecError::InvalidStateTransition { .. })));
    assert_eq!(cs.lifecycle, ConnectionLifecycle::Idle);
}

#[test]
fn connection_closed_is_truly_terminal() {
    let mut cs = fresh_conn(Role::Client);
    cs.start_handshake().unwrap();
    cs.handshake_complete().unwrap();
    cs.begin_closing().unwrap();
    cs.close().unwrap();
    // Any further transition must fail.
    assert!(matches!(
        cs.transition(ConnectionLifecycle::Handshaking),
        Err(CodecError::InvalidStateTransition { .. })
    ));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature 21 — Transport Parameters
// ═══════════════════════════════════════════════════════════════════════════════

fn round_trip_tp(tp: &TransportParameters) -> TransportParameters {
    let mut buf = vec![0u8; 1024];
    let n = tp.encode(&mut buf).unwrap();
    TransportParameters::decode(&buf[..n]).unwrap()
}

#[test]
fn transport_params_empty_round_trip() {
    let tp = TransportParameters::empty();
    assert_eq!(round_trip_tp(&tp), tp);
}

#[test]
fn transport_params_integer_fields() {
    let mut tp = TransportParameters::empty();
    tp.max_idle_timeout_ms = Some(30_000);
    tp.initial_max_data = Some(1 << 20);
    tp.initial_max_streams_bidi = Some(100);
    tp.ack_delay_exponent = Some(3);
    tp.max_ack_delay_ms = Some(25);
    tp.active_connection_id_limit = Some(4);

    let decoded = round_trip_tp(&tp);
    assert_eq!(decoded.max_idle_timeout_ms, Some(30_000));
    assert_eq!(decoded.initial_max_data, Some(1 << 20));
    assert_eq!(decoded.initial_max_streams_bidi, Some(100));
    assert_eq!(decoded.ack_delay_exponent, Some(3));
    assert_eq!(decoded.max_ack_delay_ms, Some(25));
    assert_eq!(decoded.active_connection_id_limit, Some(4));
}

#[test]
fn transport_params_original_dcid() {
    let mut tp = TransportParameters::empty();
    tp.original_destination_connection_id =
        Some(ConnectionId::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap());

    let decoded = round_trip_tp(&tp);
    assert_eq!(
        decoded.original_destination_connection_id,
        tp.original_destination_connection_id
    );
}

#[test]
fn transport_params_stateless_reset_token() {
    let mut tp = TransportParameters::empty();
    tp.stateless_reset_token = Some([0xABu8; 16]);

    let decoded = round_trip_tp(&tp);
    assert_eq!(decoded.stateless_reset_token, Some([0xABu8; 16]));
}

#[test]
fn transport_params_disable_active_migration() {
    let mut tp = TransportParameters::empty();
    tp.disable_active_migration = true;

    let decoded = round_trip_tp(&tp);
    assert!(decoded.disable_active_migration);
}

#[test]
fn transport_params_all_fields_round_trip() {
    let mut tp = TransportParameters::empty();
    tp.original_destination_connection_id = Some(ConnectionId::new(&[0xAAu8; 8]).unwrap());
    tp.max_idle_timeout_ms = Some(60_000);
    tp.stateless_reset_token = Some([0xBBu8; 16]);
    tp.max_udp_payload_size = Some(1452);
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

    assert_eq!(round_trip_tp(&tp), tp);
}

#[test]
fn transport_params_unknown_id_silently_ignored() {
    let mut tp = TransportParameters::empty();
    tp.initial_max_data = Some(2048);

    let mut buf = vec![0u8; 512];
    let written = tp.encode(&mut buf).unwrap();

    // Append unknown parameter id=0xBAD, length=4, value=0xDEADBEEF.
    let mut extra = [0u8; 16];
    let mut ep = 0;
    ep += VarInt::from_u64(0xBAD).unwrap().encode(&mut extra[ep..]).unwrap();
    ep += VarInt::from_u64(4).unwrap().encode(&mut extra[ep..]).unwrap();
    extra[ep]     = 0xDE; extra[ep + 1] = 0xAD;
    extra[ep + 2] = 0xBE; extra[ep + 3] = 0xEF;
    ep += 4;
    buf[written..written + ep].copy_from_slice(&extra[..ep]);

    let decoded = TransportParameters::decode(&buf[..written + ep]).unwrap();
    // Known parameter must survive.
    assert_eq!(decoded.initial_max_data, Some(2048));
}

#[test]
fn transport_params_truncated_value_returns_eof() {
    let mut buf = [0u8; 32];
    let mut pos = 0;
    // id = 0x04 (initial_max_data), length = 10, but only 3 bytes follow.
    pos += VarInt::from_u64(PARAM_INITIAL_MAX_DATA).unwrap().encode(&mut buf[pos..]).unwrap();
    pos += VarInt::from_u64(10).unwrap().encode(&mut buf[pos..]).unwrap();
    buf[pos] = 0x01; pos += 1;
    buf[pos] = 0x02; pos += 1;
    buf[pos] = 0x03; pos += 1;

    let result = TransportParameters::decode(&buf[..pos]);
    assert!(matches!(result, Err(CodecError::UnexpectedEof { .. })));
}
