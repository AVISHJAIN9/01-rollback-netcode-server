// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Stream ID Space Management (RFC 9000 §2.1)
// Client/Server and Bi/Uni-directional stream ID allocation

/// Stream endpoint role.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum StreamRole {
    Client,
    Server,
}

/// Stream directionality.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum StreamDir {
    Bidirectional,
    Unidirectional,
}

/// Stream ID allocator for RFC 9000 §2.1 stream ID space.
///
/// Stream ID format (RFC 9000 §2.1):
/// - Bit 0: 0 for client-initiated, 1 for server-initiated
/// - Bit 1: 0 for bidirectional, 1 for unidirectional
/// - Bits 2+: Stream sequence number
#[derive(Clone, Debug)]
pub struct StreamIdAllocator {
    /// Next sequence number to allocate for each (role, direction) pair.
    next_sequence: [u64; 4], // [client_bi, client_uni, server_bi, server_uni]

    /// Local endpoint role.
    local_role: StreamRole,

    /// Maximum concurrent bidirectional streams (local limit).
    max_streams_bidi: u64,

    /// Maximum concurrent unidirectional streams (local limit).
    max_streams_uni: u64,

    /// Active stream count (bidirectional).
    active_bidi: u64,

    /// Active stream count (unidirectional).
    active_uni: u64,
}

impl StreamIdAllocator {
    /// Create a new stream ID allocator.
    pub fn new(local_role: StreamRole, max_bidi: u64, max_uni: u64) -> Self {
        StreamIdAllocator {
            next_sequence: [0, 0, 0, 0],
            local_role,
            max_streams_bidi: max_bidi,
            max_streams_uni: max_uni,
            active_bidi: 0,
            active_uni: 0,
        }
    }

    /// Encode a stream ID from role, direction, and sequence number.
    fn encode_stream_id(role: StreamRole, dir: StreamDir, sequence: u64) -> u64 {
        let role_bit = match role {
            StreamRole::Client => 0,
            StreamRole::Server => 1,
        };

        let dir_bit = match dir {
            StreamDir::Bidirectional => 0,
            StreamDir::Unidirectional => 1,
        };

        (sequence << 2) | (dir_bit << 1) | role_bit
    }

    /// Decode a stream ID into role, direction, and sequence number.
    pub fn decode_stream_id(stream_id: u64) -> (StreamRole, StreamDir, u64) {
        let role = if (stream_id & 1) == 0 {
            StreamRole::Client
        } else {
            StreamRole::Server
        };

        let dir = if ((stream_id >> 1) & 1) == 0 {
            StreamDir::Bidirectional
        } else {
            StreamDir::Unidirectional
        };

        let sequence = stream_id >> 2;

        (role, dir, sequence)
    }

    /// Allocate a new local stream ID (bidirectional or unidirectional).
    ///
    /// Returns `None` if the maximum concurrent stream limit is reached.
    pub fn allocate(&mut self, dir: StreamDir) -> Option<u64> {
        match dir {
            StreamDir::Bidirectional => {
                if self.active_bidi >= self.max_streams_bidi {
                    return None;
                }
                let sequence = self.next_sequence[self.local_role_bidi_index()];
                self.next_sequence[self.local_role_bidi_index()] += 1;
                self.active_bidi += 1;

                Some(Self::encode_stream_id(self.local_role, StreamDir::Bidirectional, sequence))
            }
            StreamDir::Unidirectional => {
                if self.active_uni >= self.max_streams_uni {
                    return None;
                }
                let sequence = self.next_sequence[self.local_role_uni_index()];
                self.next_sequence[self.local_role_uni_index()] += 1;
                self.active_uni += 1;

                Some(Self::encode_stream_id(self.local_role, StreamDir::Unidirectional, sequence))
            }
        }
    }

    /// Register a remotely-initiated stream (from STREAM frame).
    ///
    /// Returns `true` if the stream ID is valid and was registered,
    /// `false` if it exceeds the flow control limit.
    pub fn register_remote_stream(&mut self, stream_id: u64) -> bool {
        let (role, dir, sequence) = Self::decode_stream_id(stream_id);

        // Verify role (remote stream must be from opposite role)
        if role == self.local_role {
            return false; // Invalid: remote stream from local role
        }

        match dir {
            StreamDir::Bidirectional => {
                let idx = if role == StreamRole::Client { 0 } else { 2 };
                let next = &mut self.next_sequence[idx];
                if sequence >= *next {
                    *next = sequence + 1;
                    true
                } else {
                    true // Stream already registered
                }
            }
            StreamDir::Unidirectional => {
                let idx = if role == StreamRole::Client { 1 } else { 3 };
                let next = &mut self.next_sequence[idx];
                if sequence >= *next {
                    *next = sequence + 1;
                    true
                } else {
                    true // Stream already registered
                }
            }
        }
    }

    /// Mark a stream as closed (decrement active count).
    pub fn close_stream(&mut self, stream_id: u64) {
        let (_, dir, _) = Self::decode_stream_id(stream_id);

        match dir {
            StreamDir::Bidirectional => {
                self.active_bidi = self.active_bidi.saturating_sub(1);
            }
            StreamDir::Unidirectional => {
                self.active_uni = self.active_uni.saturating_sub(1);
            }
        }
    }

    /// Check if a stream ID belongs to the local endpoint.
    pub fn is_local_stream(&self, stream_id: u64) -> bool {
        let (role, _, _) = Self::decode_stream_id(stream_id);
        role == self.local_role
    }

    /// Check if a stream ID is bidirectional.
    pub fn is_bidi_stream(stream_id: u64) -> bool {
        let (_, dir, _) = Self::decode_stream_id(stream_id);
        dir == StreamDir::Bidirectional
    }

    /// Check if a stream ID is unidirectional.
    pub fn is_uni_stream(stream_id: u64) -> bool {
        let (_, dir, _) = Self::decode_stream_id(stream_id);
        dir == StreamDir::Unidirectional
    }

    /// Get active bidirectional stream count.
    pub fn active_bidi_count(&self) -> u64 {
        self.active_bidi
    }

    /// Get active unidirectional stream count.
    pub fn active_uni_count(&self) -> u64 {
        self.active_uni
    }

    /// Get maximum concurrent bidirectional streams.
    pub fn max_bidi_streams(&self) -> u64 {
        self.max_streams_bidi
    }

    /// Get maximum concurrent unidirectional streams.
    pub fn max_uni_streams(&self) -> u64 {
        self.max_streams_uni
    }

    /// Update maximum stream limits (from peer transport parameters).
    pub fn update_limits(&mut self, max_bidi: u64, max_uni: u64) {
        self.max_streams_bidi = max_bidi;
        self.max_streams_uni = max_uni;
    }

    fn local_role_bidi_index(&self) -> usize {
        if self.local_role == StreamRole::Client { 0 } else { 2 }
    }

    fn local_role_uni_index(&self) -> usize {
        if self.local_role == StreamRole::Client { 1 } else { 3 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_id_encoding() {
        let stream_id = StreamIdAllocator::encode_stream_id(
            StreamRole::Client,
            StreamDir::Bidirectional,
            0,
        );
        assert_eq!(stream_id, 0);

        let stream_id = StreamIdAllocator::encode_stream_id(
            StreamRole::Server,
            StreamDir::Bidirectional,
            0,
        );
        assert_eq!(stream_id, 1);

        let stream_id = StreamIdAllocator::encode_stream_id(
            StreamRole::Client,
            StreamDir::Unidirectional,
            0,
        );
        assert_eq!(stream_id, 2);

        let stream_id = StreamIdAllocator::encode_stream_id(
            StreamRole::Server,
            StreamDir::Unidirectional,
            0,
        );
        assert_eq!(stream_id, 3);

        // Sequence number 1
        let stream_id = StreamIdAllocator::encode_stream_id(
            StreamRole::Client,
            StreamDir::Bidirectional,
            1,
        );
        assert_eq!(stream_id, 4);
    }

    #[test]
    fn test_stream_id_decoding() {
        let (role, dir, seq) = StreamIdAllocator::decode_stream_id(0);
        assert_eq!(role, StreamRole::Client);
        assert_eq!(dir, StreamDir::Bidirectional);
        assert_eq!(seq, 0);

        let (role, dir, seq) = StreamIdAllocator::decode_stream_id(1);
        assert_eq!(role, StreamRole::Server);
        assert_eq!(dir, StreamDir::Bidirectional);
        assert_eq!(seq, 0);

        let (role, dir, seq) = StreamIdAllocator::decode_stream_id(2);
        assert_eq!(role, StreamRole::Client);
        assert_eq!(dir, StreamDir::Unidirectional);
        assert_eq!(seq, 0);

        let (role, dir, seq) = StreamIdAllocator::decode_stream_id(4);
        assert_eq!(role, StreamRole::Client);
        assert_eq!(dir, StreamDir::Bidirectional);
        assert_eq!(seq, 1);
    }

    #[test]
    fn test_stream_allocator_init() {
        let allocator = StreamIdAllocator::new(StreamRole::Client, 10, 5);
        assert_eq!(allocator.active_bidi_count(), 0);
        assert_eq!(allocator.active_uni_count(), 0);
        assert_eq!(allocator.max_bidi_streams(), 10);
        assert_eq!(allocator.max_uni_streams(), 5);
    }

    #[test]
    fn test_allocate_bidirectional() {
        let mut allocator = StreamIdAllocator::new(StreamRole::Client, 10, 5);

        let id0 = allocator.allocate(StreamDir::Bidirectional).unwrap();
        let id1 = allocator.allocate(StreamDir::Bidirectional).unwrap();

        let (role0, dir0, seq0) = StreamIdAllocator::decode_stream_id(id0);
        assert_eq!(role0, StreamRole::Client);
        assert_eq!(dir0, StreamDir::Bidirectional);
        assert_eq!(seq0, 0);

        let (role1, dir1, seq1) = StreamIdAllocator::decode_stream_id(id1);
        assert_eq!(role1, StreamRole::Client);
        assert_eq!(dir1, StreamDir::Bidirectional);
        assert_eq!(seq1, 1);

        assert_eq!(allocator.active_bidi_count(), 2);
    }

    #[test]
    fn test_allocate_limit() {
        let mut allocator = StreamIdAllocator::new(StreamRole::Client, 2, 1);

        allocator.allocate(StreamDir::Bidirectional).unwrap();
        allocator.allocate(StreamDir::Bidirectional).unwrap();

        // Third allocation should fail
        assert_eq!(allocator.allocate(StreamDir::Bidirectional), None);

        allocator.allocate(StreamDir::Unidirectional).unwrap();

        // Second unidirectional should fail
        assert_eq!(allocator.allocate(StreamDir::Unidirectional), None);
    }

    #[test]
    fn test_close_stream() {
        let mut allocator = StreamIdAllocator::new(StreamRole::Client, 10, 5);

        let id = allocator.allocate(StreamDir::Bidirectional).unwrap();
        assert_eq!(allocator.active_bidi_count(), 1);

        allocator.close_stream(id);
        assert_eq!(allocator.active_bidi_count(), 0);

        // Should be able to allocate again
        allocator.allocate(StreamDir::Bidirectional).unwrap();
        assert_eq!(allocator.active_bidi_count(), 1);
    }

    #[test]
    fn test_is_local_stream() {
        let allocator = StreamIdAllocator::new(StreamRole::Client, 10, 5);

        // Client stream: 0b00
        assert!(allocator.is_local_stream(0));
        // Server stream: 0b01
        assert!(!allocator.is_local_stream(1));
    }

    #[test]
    fn test_stream_direction_checkers() {
        assert!(StreamIdAllocator::is_bidi_stream(0)); // 0b00
        assert!(!StreamIdAllocator::is_bidi_stream(2)); // 0b10
        assert!(StreamIdAllocator::is_uni_stream(2)); // 0b10
        assert!(!StreamIdAllocator::is_uni_stream(0)); // 0b00
    }
}
