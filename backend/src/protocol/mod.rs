pub mod error;
pub mod packets;
pub mod version;
pub mod sequence;

pub use error::ProtocolError;
pub use packets::{GamePacket, PacketType, MAX_PACKET_BYTES, PROTOCOL_MAGIC};
pub use version::{
    VersionNegotiator, CAPABILITY_COMPRESSED_SNAPSHOTS, CAPABILITY_DESYNC_EXPLAIN,
    CAPABILITY_INPUT_BUNDLING, CAPABILITY_SPECTATOR, CURRENT_PROTOCOL_VERSION,
    MAX_SUPPORTED_PROTOCOL_VERSION, MIN_SUPPORTED_PROTOCOL_VERSION,
};
pub use sequence::{SequenceValidator, SequenceError};
