use std::fmt;

/// Session state machine states
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionState {
    /// Players joining, not yet ready
    Lobby,
    /// All players ready, waiting to start
    Ready,
    /// Game active, players playing
    Active,
    /// Player(s) disconnected, reconnect window open
    Reconnecting,
    /// Game finished
    Finished,
    /// Session cleaned up, resources released
    Cleaned,
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SessionState::Lobby => write!(f, "Lobby"),
            SessionState::Ready => write!(f, "Ready"),
            SessionState::Active => write!(f, "Active"),
            SessionState::Reconnecting => write!(f, "Reconnecting"),
            SessionState::Finished => write!(f, "Finished"),
            SessionState::Cleaned => write!(f, "Cleaned"),
        }
    }
}

/// Requested state transition
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionTransition {
    /// Move from Lobby to Ready when all joined
    ReadyUp,
    /// Start active play
    Start,
    /// Player disconnect detected
    PlayerDisconnected,
    /// Player reconnected
    PlayerReconnected,
    /// Match finished
    Finish,
    /// Cleanup session
    Cleanup,
}

/// Session state machine errors
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    /// Transition not valid from current state
    InvalidTransition(SessionState, SessionState),
    /// Cannot join in current state
    CannotJoinInState(SessionState),
    /// Session is at max capacity
    SessionFull,
    /// Player already joined
    PlayerAlreadyJoined(String),
    /// Player not in session
    PlayerNotInSession(String),
    /// Invalid player capacity
    InvalidCapacity(usize),
    /// Reconnect token expired
    TokenExpired,
    /// Reconnect token not found
    TokenNotFound,
    /// Player ID mismatch on token validation
    PlayerMismatch { expected: String, actual: String },
    /// Session not found
    SessionNotFound,
    /// Database error (generic)
    DatabaseError(String),
    /// Repository error
    RepositoryError(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SessionError::InvalidTransition(from, to) => {
                write!(f, "Cannot transition from {} to {}", from, to)
            }
            SessionError::CannotJoinInState(state) => {
                write!(f, "Cannot join session in state: {}", state)
            }
            SessionError::SessionFull => write!(f, "Session is at maximum capacity"),
            SessionError::PlayerAlreadyJoined(id) => write!(f, "Player {} already joined", id),
            SessionError::PlayerNotInSession(id) => write!(f, "Player {} not in session", id),
            SessionError::InvalidCapacity(cap) => write!(f, "Invalid capacity: {}", cap),
            SessionError::TokenExpired => write!(f, "Reconnect token expired"),
            SessionError::TokenNotFound => write!(f, "Reconnect token not found"),
            SessionError::PlayerMismatch { expected, actual } => {
                write!(f, "Player mismatch: expected {}, got {}", expected, actual)
            }
            SessionError::SessionNotFound => write!(f, "Session not found"),
            SessionError::DatabaseError(e) => write!(f, "Database error: {}", e),
            SessionError::RepositoryError(e) => write!(f, "Repository error: {}", e),
        }
    }
}

impl std::error::Error for SessionError {}
