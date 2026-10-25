use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rollback_netcode_server::protocol::{GamePacket, MAX_PACKET_BYTES};

#[test]
fn test_10000_packet_decoder_fuzz() {
    let mut rng = StdRng::seed_from_u64(0xDEAD_BEEF_CAFE_BABE);
    let mut buffer = vec![0u8; MAX_PACKET_BYTES + 64];

    for _ in 0..10_000 {
        let len = rng.gen_range(0..=MAX_PACKET_BYTES + 32);
        rng.fill(&mut buffer[..len]);
        // Decoder must NEVER panic on any corrupted/malformed input
        let _ = GamePacket::decode(&buffer[..len]);
    }
}
