//! ChaCha20-Poly1305 AEAD and Header Protection — RFC 9001 §5.3, §5.4.
//!
//! # AEAD (§5.3)
//!
//! Packet payloads are encrypted with ChaCha20-Poly1305 using:
//! - A 32-byte **write key** derived from the packet-space secret.
//! - A 12-byte **nonce** constructed as: `IV XOR (0-padded packet number)`.
//!
//! The authenticated-data (`aad`) is the encoded packet header (up to but
//! not including the payload).
//!
//! # Header Protection (§5.4.4)
//!
//! After AEAD encryption, header bits and the on-wire packet number are
//! masked using a ChaCha20 stream generated from the **header protection key**
//! and a 16-byte **sample** extracted from the ciphertext:
//!
//! ```text
//! counter  = first 4 bytes of sample, interpreted as little-endian u32
//! nonce    = last 8 bytes of sample
//! mask     = ChaCha20_block(hp_key, counter, nonce)[0..5]
//! ```
//!
//! The mask is then XOR-applied to:
//! - Bits `0x1F` of the first header byte (short) or `0x0F` (long).
//! - The packet number bytes (1–4 bytes following the DCID).

use alloc::vec::Vec;

use chacha20poly1305::{
    aead::{Aead, AeadInPlace, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};

use super::CryptoError;

/// Size of a ChaCha20-Poly1305 authentication tag in bytes.
pub const TAG_LEN: usize = 16;

/// Size of a ChaCha20-Poly1305 nonce in bytes.
pub const NONCE_LEN: usize = 12;

/// Size of a ChaCha20-Poly1305 key in bytes.
pub const KEY_LEN: usize = 32;

/// Number of bytes sampled from the ciphertext for header protection.
pub const HP_SAMPLE_LEN: usize = 16;

// ─── AEAD ─────────────────────────────────────────────────────────────────────

/// Encrypt `plaintext` using ChaCha20-Poly1305 and return the ciphertext
/// (which includes the appended 16-byte authentication tag).
///
/// # Parameters
/// - `key`: 32-byte packet-protection key.
/// - `nonce`: 12-byte nonce (`iv XOR padded_packet_number`).
/// - `aad`: encoded packet header bytes (authenticated but not encrypted).
/// - `plaintext`: payload bytes to encrypt.
///
/// # Errors
/// - [`CryptoError::InvalidKeyLength`] when `key.len() != 32`.
/// - [`CryptoError::InvalidNonceLength`] when `nonce.len() != 12`.
/// - [`CryptoError::AeadFailure`] on any underlying cipher error.
pub fn seal(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            expected: KEY_LEN,
            provided: key.len(),
        });
    }
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            expected: NONCE_LEN,
            provided: nonce.len(),
        });
    }

    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let n = Nonce::from_slice(nonce);

    cipher
        .encrypt(n, Payload { msg: plaintext, aad })
        .map_err(|_| CryptoError::AeadFailure)
}

/// Decrypt and authenticate `ciphertext_and_tag` using ChaCha20-Poly1305.
///
/// `ciphertext_and_tag` must contain the ciphertext followed by the 16-byte
/// authentication tag.  Returns the plaintext on success.
///
/// # Errors
/// - [`CryptoError::CiphertextTooShort`] when `ciphertext_and_tag.len() < 16`.
/// - [`CryptoError::InvalidKeyLength`] / [`CryptoError::InvalidNonceLength`].
/// - [`CryptoError::AeadFailure`] when authentication fails.
pub fn open(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext_and_tag: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            expected: KEY_LEN,
            provided: key.len(),
        });
    }
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            expected: NONCE_LEN,
            provided: nonce.len(),
        });
    }
    if ciphertext_and_tag.len() < TAG_LEN {
        return Err(CryptoError::CiphertextTooShort);
    }

    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let n = Nonce::from_slice(nonce);

    cipher
        .decrypt(n, Payload { msg: ciphertext_and_tag, aad })
        .map_err(|_| CryptoError::AeadFailure)
}

// ─── Nonce construction ───────────────────────────────────────────────────────

/// Construct the 12-byte AEAD nonce by XOR-ing the 12-byte IV with the
/// packet number left-padded to 12 bytes.
///
/// Per RFC 9001 §5.3:
/// ```text
/// nonce = iv XOR encoded_packet_number
/// ```
/// where `encoded_packet_number` is the full 62-bit packet number written
/// into the low-order bytes of a 12-byte big-endian buffer.
pub fn construct_nonce(iv: &[u8; 12], packet_number: u64) -> [u8; 12] {
    // Encode packet number as big-endian into the low 8 bytes of a 12-byte buf.
    let mut pn_bytes = [0u8; 12];
    let pn_be = packet_number.to_be_bytes(); // 8 bytes
    pn_bytes[4..].copy_from_slice(&pn_be);   // placed in bytes [4..12]

    // XOR with the IV
    let mut nonce = [0u8; 12];
    for i in 0..12 {
        nonce[i] = iv[i] ^ pn_bytes[i];
    }
    nonce
}

// ─── Header Protection ────────────────────────────────────────────────────────

/// Extract a 16-byte header-protection sample from the ciphertext.
///
/// Per RFC 9001 §5.4.2, the sample starts at offset `pn_offset + 4` in the
/// ciphertext (4 is the maximum packet number length).  The caller provides
/// the full payload ciphertext slice starting from the first byte of the
/// encrypted packet number.
///
/// # Errors
/// Returns [`CryptoError::SampleTooShort`] when there are fewer than 20 bytes
/// available (4-byte PN + 16-byte sample).
pub fn extract_hp_sample(ciphertext_from_pn: &[u8]) -> Result<[u8; HP_SAMPLE_LEN], CryptoError> {
    // sample_offset = 4 (max PN len)
    const SAMPLE_OFFSET: usize = 4;
    if ciphertext_from_pn.len() < SAMPLE_OFFSET + HP_SAMPLE_LEN {
        return Err(CryptoError::SampleTooShort);
    }
    let mut sample = [0u8; HP_SAMPLE_LEN];
    sample.copy_from_slice(&ciphertext_from_pn[SAMPLE_OFFSET..SAMPLE_OFFSET + HP_SAMPLE_LEN]);
    Ok(sample)
}

/// Generate a 5-byte header-protection mask from the HP key and sample.
///
/// RFC 9001 §5.4.4 — ChaCha20 header protection:
/// ```text
/// counter = sample[0..4]  (little-endian u32)
/// nonce   = sample[4..12] (8 bytes, little-endian as specified in §5.4.4)
/// mask    = ChaCha20(hp_key, counter, nonce)[0..5]
/// ```
///
/// We use the `chacha20poly1305` crate's underlying ChaCha20 keystream.
///
/// # Errors
/// - [`CryptoError::InvalidKeyLength`] when `hp_key.len() != 32`.
pub fn generate_hp_mask(
    hp_key: &[u8],
    sample: &[u8; HP_SAMPLE_LEN],
) -> Result<[u8; 5], CryptoError> {
    if hp_key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            expected: KEY_LEN,
            provided: hp_key.len(),
        });
    }

    // Build a ChaCha20 stream using the sample as the counter+nonce.
    // RFC 9001 §5.4.4:
    //   counter = sample[0..4] as little-endian u32
    //   nonce   = sample[4..16] (12 bytes; the crate treats the first 4 bytes
    //             as the block counter and the remaining 8 as the nonce stream)
    //
    // The chacha20poly1305 crate exposes ChaCha20 via the `chacha20` sub-crate
    // which is re-exported.  We use a keystream-only approach by encrypting
    // a zero block.
    // (no import needed — we use our pure-Rust chacha20_block function below)

    // We call ChaCha20 directly through a zero-plaintext encrypt with a
    // specially constructed nonce.  The sample maps as:
    //   bytes [0..4]  → block counter (little-endian u32, embedded in a 96-bit nonce)
    //   bytes [4..16] → the 96-bit nonce position
    //
    // The Nonce for ChaCha20Poly1305 is 12 bytes.  RFC 9001 constructs
    // the ChaCha20 call differently from the AEAD (uses raw block counter),
    // so we need direct ChaCha20 access.
    //
    // Implementation: use chacha20::ChaCha20 directly via the re-exported path.
    // Since chacha20poly1305 re-exports chacha20, we use it via the raw cipher.

    // Use the `chacha20` crate directly through chacha20poly1305's re-export:
    // chacha20poly1305 depends on chacha20 which is re-exported as a sub-module.
    // We can also just call seal on 5 zero bytes with the right key/nonce.
    //
    // RFC 9001 §5.4.4:
    //   mask = ChaCha20(hp_key, counter=sample[0..4], nonce=sample[4..16])[0..5]
    //
    // We reconstruct this as: encrypt 5 zero bytes with a fresh cipher
    // instance where the counter is embedded in the nonce according to
    // how ChaCha20Poly1305 constructs its keystream counter.
    //
    // The standard IETF ChaCha20 uses a 96-bit nonce + 32-bit counter.
    // The 12-byte nonce in chacha20poly1305::Nonce is the 96-bit part;
    // the internal counter starts at 0 for AEAD but the HP spec sets it
    // to `sample[0..4]` as LE u32.
    //
    // To handle this precisely without unsafe/internal features, we encrypt
    // a zero block using the sample[4..16] as the nonce and then account for
    // the counter offset by XOR-ing the keystream position.
    //
    // Practical approach: use the `chacha20` crate directly (it is a
    // transitive dependency of `chacha20poly1305`).  Since we can't import
    // it directly from Cargo.toml without adding it explicitly, we implement
    // the mask via a 64-byte keystream generated by sealing zeros and then
    // discarding the Poly1305 tag — the ChaCha20 keystream is the same.
    //
    // Correct RFC 9001 §5.4.4 approach using ChaCha20Poly1305 as a proxy:
    // Seal a 5-zero-byte message with counter=0, nonce=sample[4..16] prefixed
    // with counter LE bytes. Since AEAD always starts counter at 1 for the
    // message (0 for the Poly1305 key), and HP uses counter=sample[0..4]
    // for the mask itself, we adjust:
    //
    // Simplest correct implementation: call the underlying `chacha20` cipher.
    // We add it as a dev-dependency or use the re-exported type.
    //
    // Since chacha20poly1305 v0.10 re-exports chacha20::ChaCha20Rng etc.,
    // we use the actual algorithm directly by treating the seal output at
    // counter=0 as: keystream[0..64] = ChaCha20_block(key, 0, nonce).
    // The AEAD's first keystream block (counter=0) is the Poly1305 key bytes,
    // and counter=1 is the first message keystream block.
    //
    // For header protection, counter = LE32(sample[0..4]).
    // If sample[0..4] == 0 we can use the Poly1305-key block.
    // If sample[0..4] == 1 we can seal a zero-length message and use block 1.
    // For arbitrary counters, we need direct ChaCha20 access.
    //
    // Resolution: add `chacha20` as an explicit dependency.  Until then, we
    // implement a minimal ChaCha20 block function in pure Rust here, which is
    // the standard algorithm and fully RFC 8439 compliant.

    let counter = u32::from_le_bytes(sample[0..4].try_into().unwrap());
    let nonce_bytes: [u8; 12] = sample[4..16].try_into().unwrap();

    let key_arr: [u32; 8] = {
        let mut k = [0u32; 8];
        for i in 0..8 {
            k[i] = u32::from_le_bytes(hp_key[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        k
    };

    let nonce_words: [u32; 3] = [
        u32::from_le_bytes(nonce_bytes[0..4].try_into().unwrap()),
        u32::from_le_bytes(nonce_bytes[4..8].try_into().unwrap()),
        u32::from_le_bytes(nonce_bytes[8..12].try_into().unwrap()),
    ];

    let block = chacha20_block(&key_arr, counter, &nonce_words);

    let mut mask = [0u8; 5];
    for (i, &w) in block[0..2].iter().enumerate() {
        let bytes = w.to_le_bytes();
        let base = i * 4;
        for j in 0..4 {
            if base + j < 5 {
                mask[base + j] = bytes[j];
            }
        }
    }
    // Fill byte 4 from word[1] byte 0 (already done above if i==1,j==0)
    // Verify: words[0] gives bytes [0..4], words[1] byte 0 gives byte 4.
    mask[4] = (block[1] & 0xFF) as u8;

    Ok(mask)
}

/// Pure-Rust ChaCha20 block function (RFC 8439 §2.1.1).
///
/// Produces a 16-word (64-byte) keystream block.
fn chacha20_block(key: &[u32; 8], counter: u32, nonce: &[u32; 3]) -> [u32; 16] {
    // RFC 8439 §2.3: initial state
    #[rustfmt::skip]
    let mut state = [
        0x61707865u32, 0x3320646e, 0x79622d32, 0x6b206574, // constants
        key[0], key[1], key[2], key[3],                     // key[0..3]
        key[4], key[5], key[6], key[7],                     // key[4..7]
        counter,                                             // counter
        nonce[0], nonce[1], nonce[2],                        // nonce
    ];

    let initial = state;

    // 20 rounds = 10 double-rounds
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        // Diagonal rounds
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    // Add initial state
    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial[i]);
    }

    state
}

#[inline(always)]
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]); s[d] ^= s[a]; s[d] = s[d].rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]); s[b] ^= s[c]; s[b] = s[b].rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]); s[d] ^= s[a]; s[d] = s[d].rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]); s[b] ^= s[c]; s[b] = s[b].rotate_left(7);
}

/// Apply (or remove) header protection by XOR-ing the mask against the
/// first-byte flag bits and the packet-number bytes.
///
/// # Parameters
/// - `header`: the full packet header buffer (mutable).
/// - `pn_offset`: byte offset within `header` where the packet number starts.
/// - `pn_len`: packet number length in bytes (1–4).
/// - `mask`: the 5-byte mask from `generate_hp_mask`.
/// - `is_long_header`: `true` for long headers (mask bits `0x0F`),
///                     `false` for short headers (mask bits `0x1F`).
///
/// This function is its own inverse: calling it twice restores the original.
pub fn apply_header_protection(
    header: &mut [u8],
    pn_offset: usize,
    pn_len: usize,
    mask: &[u8; 5],
    is_long_header: bool,
) {
    // Mask the flag bits in byte 0.
    let flag_mask = if is_long_header { 0x0Fu8 } else { 0x1Fu8 };
    header[0] ^= mask[0] & flag_mask;

    // Mask the packet number bytes.
    for i in 0..pn_len {
        header[pn_offset + i] ^= mask[1 + i];
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_key() -> [u8; 32] { [0u8; 32] }
    fn zero_iv()  -> [u8; 12] { [0u8; 12] }

    #[test]
    fn seal_open_round_trip() {
        let key = [0x42u8; 32];
        let iv = [0x11u8; 12];
        let pn = 7u64;
        let nonce = construct_nonce(&iv, pn);
        let aad = b"header bytes";
        let plaintext = b"payload data to encrypt and authenticate";

        let ciphertext = seal(&key, &nonce, aad, plaintext).unwrap();
        assert_eq!(ciphertext.len(), plaintext.len() + TAG_LEN);

        let recovered = open(&key, &nonce, aad, &ciphertext).unwrap();
        assert_eq!(recovered.as_slice(), plaintext);
    }

    #[test]
    fn open_fails_on_tag_mismatch() {
        let key = [0xBBu8; 32];
        let iv = [0x22u8; 12];
        let nonce = construct_nonce(&iv, 0);
        let aad = b"aad";
        let plaintext = b"secret";

        let mut ciphertext = seal(&key, &nonce, aad, plaintext).unwrap();
        // Flip a byte in the tag.
        let tag_start = ciphertext.len() - TAG_LEN;
        ciphertext[tag_start] ^= 0xFF;

        assert!(matches!(open(&key, &nonce, aad, &ciphertext), Err(CryptoError::AeadFailure)));
    }

    #[test]
    fn open_fails_with_wrong_aad() {
        let key = [0xCCu8; 32];
        let iv  = [0x33u8; 12];
        let nonce = construct_nonce(&iv, 1);
        let aad = b"correct aad";
        let plaintext = b"data";

        let ciphertext = seal(&key, &nonce, aad, plaintext).unwrap();
        let result = open(&key, &nonce, b"wrong aad", &ciphertext);
        assert!(matches!(result, Err(CryptoError::AeadFailure)));
    }

    #[test]
    fn nonce_construction_xors_correctly() {
        let iv = [0xFFu8; 12];
        let pn = 1u64;
        let nonce = construct_nonce(&iv, pn);
        // pn=1 in BE 8 bytes = [0,0,0,0,0,0,0,1]; placed in bytes [4..12] of a 12-byte buf.
        // IV is all 0xFF.
        // Expected: bytes 0..4 = 0xFF (no PN overlap), byte 11 = 0xFF ^ 0x01 = 0xFE.
        assert_eq!(nonce[0], 0xFF);
        assert_eq!(nonce[4], 0xFF);
        assert_eq!(nonce[11], 0xFE);
    }

    #[test]
    fn hp_mask_is_5_bytes() {
        let hp_key = [0xAAu8; 32];
        let sample = [0x01u8; 16];
        let mask = generate_hp_mask(&hp_key, &sample).unwrap();
        assert_eq!(mask.len(), 5);
    }

    #[test]
    fn hp_mask_apply_is_its_own_inverse() {
        // Apply mask twice must restore original header.
        let hp_key = [0xDDu8; 32];
        let sample = [0x55u8; 16];
        let mask = generate_hp_mask(&hp_key, &sample).unwrap();

        let mut header = [0x40u8, 0x01, 0x02, 0x03, 0x04, 0x00, 0x00]; // short hdr
        let original = header;
        apply_header_protection(&mut header, 1, 4, &mask, false);
        apply_header_protection(&mut header, 1, 4, &mask, false);
        assert_eq!(header, original);
    }

    #[test]
    fn invalid_key_length_rejected() {
        let short_key = [0u8; 16];
        let nonce = [0u8; 12];
        let res = seal(&short_key, &nonce, b"", b"data");
        assert!(matches!(res, Err(CryptoError::InvalidKeyLength { expected: 32, provided: 16 })));
    }

    #[test]
    fn invalid_nonce_length_rejected() {
        let key = [0u8; 32];
        let short_nonce = [0u8; 8];
        let res = seal(&key, &short_nonce, b"", b"data");
        assert!(matches!(res, Err(CryptoError::InvalidNonceLength { expected: 12, provided: 8 })));
    }

    #[test]
    fn ciphertext_too_short_rejected() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        // Only 4 bytes — shorter than the 16-byte tag minimum.
        let res = open(&key, &nonce, b"", &[0u8; 4]);
        assert!(matches!(res, Err(CryptoError::CiphertextTooShort)));
    }

    #[test]
    fn chacha20_rfc8439_test_vector() {
        // RFC 8439 §2.1.1 test vector.
        let key = [0u32; 8]; // all zeros
        let counter = 0u32;
        let nonce = [0u32; 3]; // all zeros
        let block = chacha20_block(&key, counter, &nonce);

        // First word of expected output (from RFC 8439):
        assert_eq!(block[0], 0xade0b876u32);
        assert_eq!(block[1], 0x903df1a0u32);
    }
}
