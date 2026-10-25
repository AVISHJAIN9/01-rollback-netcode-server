use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesyncError {
    /// Checksums don't match - indicates forged/corrupted state
    ChecksumMismatch { expected: u32, actual: u32 },
    /// Frame not found in server history
    FrameNotFound { frame: u64 },
    /// Invalid state detected
    InvalidState { reason: String },
}

impl std::fmt::Display for DesyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesyncError::ChecksumMismatch { expected, actual } => {
                write!(f, "Checksum mismatch: expected {:#x}, got {:#x}", expected, actual)
            }
            DesyncError::FrameNotFound { frame } => {
                write!(f, "Frame {} not found in server history", frame)
            }
            DesyncError::InvalidState { reason } => {
                write!(f, "Invalid state: {}", reason)
            }
        }
    }
}

impl std::error::Error for DesyncError {}

/// Authoritative checksum comparator and divergence detector.
/// Complies with FR-010 and FR-011.
#[derive(Debug, Default)]
pub struct DesyncDetector {
    pub authoritative_hashes: HashMap<u64, u32>,
}

impl DesyncDetector {
    pub fn new() -> Self {
        Self {
            authoritative_hashes: HashMap::new(),
        }
    }

    pub fn record_authoritative_hash(&mut self, frame: u64, hash: u32) {
        self.authoritative_hashes.insert(frame, hash);
    }

    pub fn verify_client_hash(&self, frame: u64, client_hash: u32) -> Result<bool, &'static str> {
        if let Some(&auth_hash) = self.authoritative_hashes.get(&frame) {
            Ok(auth_hash == client_hash)
        } else {
            Err("Frame checksum not available in server history")
        }
    }

    /// Validate that a client's checksum matches the server's authoritative checksum.
    /// Returns Ok(()) if they match, or DesyncError::ChecksumMismatch if they differ.
    pub fn validate_checksum(&self, frame: u64, client_checksum: u32) -> Result<(), DesyncError> {
        if let Some(&auth_checksum) = self.authoritative_hashes.get(&frame) {
            if auth_checksum == client_checksum {
                Ok(())
            } else {
                Err(DesyncError::ChecksumMismatch {
                    expected: auth_checksum,
                    actual: client_checksum,
                })
            }
        } else {
            Err(DesyncError::FrameNotFound { frame })
        }
    }

    /// Compute a simple hash from raw state bytes
    pub fn compute_hash_simple(data: &[u8]) -> u32 {
        let mut hash: u32 = 0;
        for byte in data {
            hash = hash.wrapping_mul(31).wrapping_add(*byte as u32);
        }
        hash
    }
}

