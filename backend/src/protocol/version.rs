use crate::protocol::error::ProtocolError;

pub const CURRENT_PROTOCOL_VERSION: u8 = 1;
pub const MIN_SUPPORTED_PROTOCOL_VERSION: u8 = 1;
pub const MAX_SUPPORTED_PROTOCOL_VERSION: u8 = 1;

pub const CAPABILITY_INPUT_BUNDLING: u16 = 1 << 0;
pub const CAPABILITY_COMPRESSED_SNAPSHOTS: u16 = 1 << 1;
pub const CAPABILITY_SPECTATOR: u16 = 1 << 2;
pub const CAPABILITY_DESYNC_EXPLAIN: u16 = 1 << 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionNegotiator {
    pub min_supported: u8,
    pub max_supported: u8,
}

impl Default for VersionNegotiator {
    fn default() -> Self {
        Self {
            min_supported: MIN_SUPPORTED_PROTOCOL_VERSION,
            max_supported: MAX_SUPPORTED_PROTOCOL_VERSION,
        }
    }
}

impl VersionNegotiator {
    pub fn negotiate(&self, client_version: u8) -> Result<u8, ProtocolError> {
        if client_version >= self.min_supported && client_version <= self.max_supported {
            Ok(client_version)
        } else {
            Err(ProtocolError::VersionMismatch {
                client_version,
                min_version: self.min_supported,
                max_version: self.max_supported,
            })
        }
    }

    pub fn validate_capabilities(&self, caps: u16) -> Result<(), ProtocolError> {
        let allowed_mask = CAPABILITY_INPUT_BUNDLING
            | CAPABILITY_COMPRESSED_SNAPSHOTS
            | CAPABILITY_SPECTATOR
            | CAPABILITY_DESYNC_EXPLAIN;

        if (caps & !allowed_mask) != 0 {
            Err(ProtocolError::InvalidCapabilities(caps))
        } else {
            Ok(())
        }
    }
}
