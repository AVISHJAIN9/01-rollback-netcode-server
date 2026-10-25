use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum ProtocolError {
    #[error("Buffer too short: expected at least {expected} bytes, got {actual}")]
    BufferTooShort { expected: usize, actual: usize },

    #[error("Invalid protocol magic: expected 0x5242 ('RB'), found 0x{0:04X}")]
    InvalidMagic(u16),

    #[error("Unsupported protocol version: client requested {client_version}, server supports {min_version}..={max_version}")]
    VersionMismatch {
        client_version: u8,
        min_version: u8,
        max_version: u8,
    },

    #[error("Unknown or invalid message type code: {0}")]
    UnknownMessageType(u8),

    #[error("Payload length exceeded: max {max_allowed} bytes, received {actual}")]
    PayloadLengthExceeded { max_allowed: usize, actual: usize },

    #[error("Malformed packet payload for message type {message_type:?}: {reason}")]
    MalformedPayload {
        message_type: u8,
        reason: &'static str,
    },

    #[error("Authentication failed: invalid or expired token")]
    Unauthorized,

    #[error("Invalid capability flags: 0x{0:04X}")]
    InvalidCapabilities(u16),
}
