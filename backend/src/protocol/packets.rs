use crate::protocol::error::ProtocolError;
use crate::protocol::version::{VersionNegotiator, CURRENT_PROTOCOL_VERSION};
use byteorder::{BigEndian, ByteOrder};

pub const PROTOCOL_MAGIC: u16 = 0x5242; // 'RB' in ASCII
pub const MAX_PACKET_BYTES: usize = 1200; // Safe MTU size

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Handshake = 0x01,
    HandshakeAck = 0x02,
    InputFrame = 0x03,
    StateAck = 0x04,
    Checksum = 0x05,
    SnapshotRequest = 0x06,
    SnapshotResponse = 0x07,
    Ping = 0x08,
    Pong = 0x09,
    LobbyEvent = 0x0A,
    Error = 0x0E,
    Kick = 0x0F,
    Reconnect = 0x10,
}

impl TryFrom<u8> for PacketType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, ProtocolError> {
        match value {
            0x01 => Ok(Self::Handshake),
            0x02 => Ok(Self::HandshakeAck),
            0x03 => Ok(Self::InputFrame),
            0x04 => Ok(Self::StateAck),
            0x05 => Ok(Self::Checksum),
            0x06 => Ok(Self::SnapshotRequest),
            0x07 => Ok(Self::SnapshotResponse),
            0x08 => Ok(Self::Ping),
            0x09 => Ok(Self::Pong),
            0x0A => Ok(Self::LobbyEvent),
            0x0E => Ok(Self::Error),
            0x0F => Ok(Self::Kick),
            0x10 => Ok(Self::Reconnect),
            other => Err(ProtocolError::UnknownMessageType(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamePacket {
    Handshake {
        client_version: u8,
        capabilities: u16,
        session_token: [u8; 16],
    },
    HandshakeAck {
        negotiated_version: u8,
        player_id: u8,
        tick_rate: u8,
        assigned_token: [u8; 16],
    },
    InputFrame {
        player_id: u8,
        sequence_num: u32,
        current_tick: u32,
        input_bitmask: u16,
        redundant_inputs: Vec<u16>, // Up to 7 past historical inputs
    },
    StateAck {
        player_id: u8,
        ack_tick: u32,
        rtt_sample_ms: u16,
    },
    Checksum {
        tick: u32,
        state_checksum: u32,
    },
    SnapshotRequest {
        requested_tick: u32,
    },
    SnapshotResponse {
        snapshot_tick: u32,
        data_payload: Vec<u8>,
    },
    Ping {
        client_timestamp_ms: u64,
    },
    Pong {
        client_timestamp_ms: u64,
        server_timestamp_ms: u64,
    },
    LobbyEvent {
        event_type: u8,
        room_id: [u8; 8],
    },
    Error {
        error_code: u16,
        reason: String,
    },
    Kick {
        reason_code: u8,
    },
    Reconnect {
        reconnect_token: [u8; 16],
        last_received_tick: u32,
    },
}

impl GamePacket {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);
        // Header: Magic (2 bytes), Version (1 byte), MsgType (1 byte)
        buf.extend_from_slice(&PROTOCOL_MAGIC.to_be_bytes());
        buf.push(CURRENT_PROTOCOL_VERSION);

        match self {
            GamePacket::Handshake {
                client_version,
                capabilities,
                session_token,
            } => {
                buf.push(PacketType::Handshake as u8);
                buf.push(*client_version);
                buf.extend_from_slice(&capabilities.to_be_bytes());
                buf.extend_from_slice(session_token);
            }
            GamePacket::HandshakeAck {
                negotiated_version,
                player_id,
                tick_rate,
                assigned_token,
            } => {
                buf.push(PacketType::HandshakeAck as u8);
                buf.push(*negotiated_version);
                buf.push(*player_id);
                buf.push(*tick_rate);
                buf.extend_from_slice(assigned_token);
            }
            GamePacket::InputFrame {
                player_id,
                sequence_num,
                current_tick,
                input_bitmask,
                redundant_inputs,
            } => {
                buf.push(PacketType::InputFrame as u8);
                buf.push(*player_id);
                buf.extend_from_slice(&sequence_num.to_be_bytes());
                buf.extend_from_slice(&current_tick.to_be_bytes());
                buf.extend_from_slice(&input_bitmask.to_be_bytes());
                let red_count = (redundant_inputs.len().min(7)) as u8;
                buf.push(red_count);
                for &input in redundant_inputs.iter().take(7) {
                    buf.extend_from_slice(&input.to_be_bytes());
                }
            }
            GamePacket::StateAck {
                player_id,
                ack_tick,
                rtt_sample_ms,
            } => {
                buf.push(PacketType::StateAck as u8);
                buf.push(*player_id);
                buf.extend_from_slice(&ack_tick.to_be_bytes());
                buf.extend_from_slice(&rtt_sample_ms.to_be_bytes());
            }
            GamePacket::Checksum {
                tick,
                state_checksum,
            } => {
                buf.push(PacketType::Checksum as u8);
                buf.extend_from_slice(&tick.to_be_bytes());
                buf.extend_from_slice(&state_checksum.to_be_bytes());
            }
            GamePacket::SnapshotRequest { requested_tick } => {
                buf.push(PacketType::SnapshotRequest as u8);
                buf.extend_from_slice(&requested_tick.to_be_bytes());
            }
            GamePacket::SnapshotResponse {
                snapshot_tick,
                data_payload,
            } => {
                buf.push(PacketType::SnapshotResponse as u8);
                buf.extend_from_slice(&snapshot_tick.to_be_bytes());
                let len = data_payload.len().min(1000) as u16;
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&data_payload[..len as usize]);
            }
            GamePacket::Ping {
                client_timestamp_ms,
            } => {
                buf.push(PacketType::Ping as u8);
                buf.extend_from_slice(&client_timestamp_ms.to_be_bytes());
            }
            GamePacket::Pong {
                client_timestamp_ms,
                server_timestamp_ms,
            } => {
                buf.push(PacketType::Pong as u8);
                buf.extend_from_slice(&client_timestamp_ms.to_be_bytes());
                buf.extend_from_slice(&server_timestamp_ms.to_be_bytes());
            }
            GamePacket::LobbyEvent {
                event_type,
                room_id,
            } => {
                buf.push(PacketType::LobbyEvent as u8);
                buf.push(*event_type);
                buf.extend_from_slice(room_id);
            }
            GamePacket::Error { error_code, reason } => {
                buf.push(PacketType::Error as u8);
                buf.extend_from_slice(&error_code.to_be_bytes());
                let bytes = reason.as_bytes();
                let len = bytes.len().min(255) as u8;
                buf.push(len);
                buf.extend_from_slice(&bytes[..len as usize]);
            }
            GamePacket::Kick { reason_code } => {
                buf.push(PacketType::Kick as u8);
                buf.push(*reason_code);
            }
            GamePacket::Reconnect {
                reconnect_token,
                last_received_tick,
            } => {
                buf.push(PacketType::Reconnect as u8);
                buf.extend_from_slice(reconnect_token);
                buf.extend_from_slice(&last_received_tick.to_be_bytes());
            }
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_PACKET_BYTES {
            return Err(ProtocolError::PayloadLengthExceeded {
                max_allowed: MAX_PACKET_BYTES,
                actual: bytes.len(),
            });
        }
        if bytes.len() < 4 {
            return Err(ProtocolError::BufferTooShort {
                expected: 4,
                actual: bytes.len(),
            });
        }

        let magic = BigEndian::read_u16(&bytes[0..2]);
        if magic != PROTOCOL_MAGIC {
            return Err(ProtocolError::InvalidMagic(magic));
        }

        let header_version = bytes[2];
        let negotiator = VersionNegotiator::default();
        negotiator.negotiate(header_version)?;

        let msg_type = PacketType::try_from(bytes[3])?;
        let payload = &bytes[4..];

        match msg_type {
            PacketType::Handshake => {
                if payload.len() < 19 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 19,
                        actual: payload.len(),
                    });
                }
                let client_version = payload[0];
                let capabilities = BigEndian::read_u16(&payload[1..3]);
                let mut session_token = [0u8; 16];
                session_token.copy_from_slice(&payload[3..19]);
                Ok(GamePacket::Handshake {
                    client_version,
                    capabilities,
                    session_token,
                })
            }
            PacketType::HandshakeAck => {
                if payload.len() < 19 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 19,
                        actual: payload.len(),
                    });
                }
                let negotiated_version = payload[0];
                let player_id = payload[1];
                let tick_rate = payload[2];
                let mut assigned_token = [0u8; 16];
                assigned_token.copy_from_slice(&payload[3..19]);
                Ok(GamePacket::HandshakeAck {
                    negotiated_version,
                    player_id,
                    tick_rate,
                    assigned_token,
                })
            }
            PacketType::InputFrame => {
                if payload.len() < 12 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 12,
                        actual: payload.len(),
                    });
                }
                let player_id = payload[0];
                let sequence_num = BigEndian::read_u32(&payload[1..5]);
                let current_tick = BigEndian::read_u32(&payload[5..9]);
                let input_bitmask = BigEndian::read_u16(&payload[9..11]);
                let red_count = payload[11] as usize;
                let expected_total = 12 + red_count * 2;
                if payload.len() < expected_total {
                    return Err(ProtocolError::BufferTooShort {
                        expected: expected_total,
                        actual: payload.len(),
                    });
                }
                let mut redundant_inputs = Vec::with_capacity(red_count);
                for i in 0..red_count {
                    let offset = 12 + i * 2;
                    redundant_inputs.push(BigEndian::read_u16(&payload[offset..offset + 2]));
                }
                Ok(GamePacket::InputFrame {
                    player_id,
                    sequence_num,
                    current_tick,
                    input_bitmask,
                    redundant_inputs,
                })
            }
            PacketType::StateAck => {
                if payload.len() < 7 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 7,
                        actual: payload.len(),
                    });
                }
                let player_id = payload[0];
                let ack_tick = BigEndian::read_u32(&payload[1..5]);
                let rtt_sample_ms = BigEndian::read_u16(&payload[5..7]);
                Ok(GamePacket::StateAck {
                    player_id,
                    ack_tick,
                    rtt_sample_ms,
                })
            }
            PacketType::Checksum => {
                if payload.len() < 8 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 8,
                        actual: payload.len(),
                    });
                }
                let tick = BigEndian::read_u32(&payload[0..4]);
                let state_checksum = BigEndian::read_u32(&payload[4..8]);
                Ok(GamePacket::Checksum {
                    tick,
                    state_checksum,
                })
            }
            PacketType::SnapshotRequest => {
                if payload.len() < 4 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 4,
                        actual: payload.len(),
                    });
                }
                let requested_tick = BigEndian::read_u32(&payload[0..4]);
                Ok(GamePacket::SnapshotRequest { requested_tick })
            }
            PacketType::SnapshotResponse => {
                if payload.len() < 6 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 6,
                        actual: payload.len(),
                    });
                }
                let snapshot_tick = BigEndian::read_u32(&payload[0..4]);
                let data_len = BigEndian::read_u16(&payload[4..6]) as usize;
                if payload.len() < 6 + data_len {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 6 + data_len,
                        actual: payload.len(),
                    });
                }
                let data_payload = payload[6..6 + data_len].to_vec();
                Ok(GamePacket::SnapshotResponse {
                    snapshot_tick,
                    data_payload,
                })
            }
            PacketType::Ping => {
                if payload.len() < 8 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 8,
                        actual: payload.len(),
                    });
                }
                let client_timestamp_ms = BigEndian::read_u64(&payload[0..8]);
                Ok(GamePacket::Ping {
                    client_timestamp_ms,
                })
            }
            PacketType::Pong => {
                if payload.len() < 16 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 16,
                        actual: payload.len(),
                    });
                }
                let client_timestamp_ms = BigEndian::read_u64(&payload[0..8]);
                let server_timestamp_ms = BigEndian::read_u64(&payload[8..16]);
                Ok(GamePacket::Pong {
                    client_timestamp_ms,
                    server_timestamp_ms,
                })
            }
            PacketType::LobbyEvent => {
                if payload.len() < 9 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 9,
                        actual: payload.len(),
                    });
                }
                let event_type = payload[0];
                let mut room_id = [0u8; 8];
                room_id.copy_from_slice(&payload[1..9]);
                Ok(GamePacket::LobbyEvent {
                    event_type,
                    room_id,
                })
            }
            PacketType::Error => {
                if payload.len() < 3 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 3,
                        actual: payload.len(),
                    });
                }
                let error_code = BigEndian::read_u16(&payload[0..2]);
                let str_len = payload[2] as usize;
                if payload.len() < 3 + str_len {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 3 + str_len,
                        actual: payload.len(),
                    });
                }
                let reason = String::from_utf8_lossy(&payload[3..3 + str_len]).to_string();
                Ok(GamePacket::Error { error_code, reason })
            }
            PacketType::Kick => {
                if payload.is_empty() {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 1,
                        actual: 0,
                    });
                }
                let reason_code = payload[0];
                Ok(GamePacket::Kick { reason_code })
            }
            PacketType::Reconnect => {
                if payload.len() < 20 {
                    return Err(ProtocolError::BufferTooShort {
                        expected: 20,
                        actual: payload.len(),
                    });
                }
                let mut reconnect_token = [0u8; 16];
                reconnect_token.copy_from_slice(&payload[0..16]);
                let last_received_tick = BigEndian::read_u32(&payload[16..20]);
                Ok(GamePacket::Reconnect {
                    reconnect_token,
                    last_received_tick,
                })
            }
        }
    }
}
