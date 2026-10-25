//! Connection ID type — RFC 9000 §5.1.
//!
//! QUIC Connection IDs are 0–20 bytes long.  We store them in a fixed-size
//! stack-allocated array to avoid heap allocation on the hot path.

use core::{
    borrow::Borrow,
    cmp::Ordering,
    hash::{Hash, Hasher},
};

use crate::error::CidError;

/// Maximum connection ID size in bytes (RFC 9000 §5.1).
pub const MAX_CID_SIZE: usize = 20;

/// A stack-allocated QUIC Connection ID (0–20 bytes).
///
/// Uses an internal `[u8; 20]` buffer with a `len` field so that no heap
/// allocation is required.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionId {
    /// Raw storage; only bytes `[0..len]` are meaningful.
    bytes: [u8; MAX_CID_SIZE],
    /// The number of valid bytes in `bytes`.
    len: u8,
}

impl ConnectionId {
    // ── Construction ─────────────────────────────────────────────────────────

    /// Construct a `ConnectionId` from a byte slice.
    ///
    /// # Errors
    /// Returns [`CidError::TooLong`] when `slice.len() > MAX_CID_SIZE`.
    #[inline]
    pub fn new(slice: &[u8]) -> Result<Self, CidError> {
        if slice.len() > MAX_CID_SIZE {
            return Err(CidError::TooLong {
                provided: slice.len(),
            });
        }
        let mut bytes = [0u8; MAX_CID_SIZE];
        bytes[..slice.len()].copy_from_slice(slice);
        Ok(ConnectionId {
            bytes,
            len: slice.len() as u8,
        })
    }

    /// Construct an empty (zero-length) Connection ID.
    #[inline]
    pub const fn empty() -> Self {
        ConnectionId {
            bytes: [0u8; MAX_CID_SIZE],
            len: 0,
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Return a slice of the valid CID bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Return `true` when this Connection ID contains zero bytes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Return the length of this Connection ID in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }
}

// ── Trait implementations ──────────────────────────────────────────────────────

impl PartialEq for ConnectionId {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for ConnectionId {}

impl Hash for ConnectionId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl PartialOrd for ConnectionId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ConnectionId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl Borrow<[u8]> for ConnectionId {
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsRef<[u8]> for ConnectionId {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl core::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for b in self.as_bytes() {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl TryFrom<&[u8]> for ConnectionId {
    type Error = CidError;
    fn try_from(slice: &[u8]) -> Result<Self, CidError> {
        ConnectionId::new(slice)
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cid() {
        let cid = ConnectionId::empty();
        assert!(cid.is_empty());
        assert_eq!(cid.len(), 0);
        assert_eq!(cid.as_bytes(), &[]);
    }

    #[test]
    fn round_trip() {
        let raw = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let cid = ConnectionId::new(&raw).unwrap();
        assert_eq!(cid.as_bytes(), &raw);
        assert_eq!(cid.len(), 8);
    }

    #[test]
    fn max_length() {
        let raw = [0xAAu8; 20];
        let cid = ConnectionId::new(&raw).unwrap();
        assert_eq!(cid.len(), 20);
    }

    #[test]
    fn too_long_rejected() {
        let raw = [0u8; 21];
        assert_eq!(
            ConnectionId::new(&raw),
            Err(CidError::TooLong { provided: 21 })
        );
    }

    #[test]
    fn equality_based_on_content() {
        let a = ConnectionId::new(&[1, 2, 3]).unwrap();
        let b = ConnectionId::new(&[1, 2, 3]).unwrap();
        let c = ConnectionId::new(&[1, 2, 4]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn ordering() {
        let a = ConnectionId::new(&[0x01]).unwrap();
        let b = ConnectionId::new(&[0x02]).unwrap();
        assert!(a < b);
        assert!(b > a);
    }
}
