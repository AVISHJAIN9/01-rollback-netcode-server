// Session management module for STEP 3
// Includes state machine, repository trait, and in-memory implementation

pub mod repository;
pub mod state;
pub mod lobby;
pub mod reconnect;
pub mod cleanup;
pub mod matchmaking;
pub mod postgres_repo;

pub use repository::{SessionRepository, InMemorySessionRepository};
pub use state::{SessionState, SessionTransition, SessionError};
pub use lobby::{Lobby, LobbyId, LobbyRegistry, ReadyState};
pub use reconnect::{ReconnectManager, ReconnectToken as ReconnectTokenV2, ReconnectError};
pub use matchmaking::{MatchmakingQueue, InMemoryQueue, RedisQueue};
pub use postgres_repo::PostgresSessionRepository;

use uuid::Uuid;
use chrono::{DateTime, Utc};

/// A game session that tracks player state and game progress
#[derive(Clone, Debug)]
pub struct Session {
    pub session_id: Uuid,
    pub match_id: Uuid,
    pub room_code: String,
    pub state: SessionState,
    pub players: Vec<PlayerId>,
    pub max_players: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub tick_count: u64,
    pub ticks_until_finish: Option<u64>,
}

/// A player identifier in a session
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PlayerId(pub String);

/// Reconnect token for a player rejoining a session
#[derive(Clone, Debug)]
pub struct ReconnectToken {
    pub token: String,
    pub player_id: PlayerId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl ReconnectToken {
    /// Create a new reconnect token valid for 5 minutes
    pub fn new(player_id: PlayerId) -> Self {
        let issued_at = Utc::now();
        let expires_at = issued_at + chrono::Duration::minutes(5);
        Self {
            token: format!("{}-{}", player_id.0, Uuid::new_v4()),
            player_id,
            issued_at,
            expires_at,
        }
    }

    /// Check if token is still valid
    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
}

impl Session {
    /// Create a new session in Lobby state
    pub fn new(
        match_id: Uuid,
        room_code: String,
        max_players: usize,
    ) -> Result<Self, SessionError> {
        if max_players < 2 || max_players > 8 {
            return Err(SessionError::InvalidCapacity(max_players));
        }

        Ok(Session {
            session_id: Uuid::new_v4(),
            match_id,
            room_code,
            state: SessionState::Lobby,
            players: Vec::new(),
            max_players,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_heartbeat: Utc::now(),
            tick_count: 0,
            ticks_until_finish: None,
        })
    }

    /// Transition session to a new state
    pub fn transition(&mut self, target: SessionState) -> Result<(), SessionError> {
        let valid = match (&self.state, &target) {
            // From Lobby
            (SessionState::Lobby, SessionState::Ready) => !self.players.is_empty() && self.players.len() == self.max_players,
            (SessionState::Lobby, SessionState::Lobby) => false, // Already in Lobby
            
            // From Ready
            (SessionState::Ready, SessionState::Active) => true,
            (SessionState::Ready, SessionState::Lobby) => true, // Allow going back
            
            // From Active
            (SessionState::Active, SessionState::Reconnecting) => true,
            (SessionState::Active, SessionState::Finished) => true,
            
            // From Reconnecting
            (SessionState::Reconnecting, SessionState::Active) => true,
            
            // From Finished
            (SessionState::Finished, SessionState::Cleaned) => true,
            
            // All other transitions are invalid
            _ => false,
        };

        if !valid {
            return Err(SessionError::InvalidTransition(self.state.clone(), target));
        }

        self.state = target;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Try to add a player to the session
    pub fn join_player(&mut self, player_id: PlayerId) -> Result<(), SessionError> {
        // Can only join in Lobby state
        if self.state != SessionState::Lobby {
            return Err(SessionError::CannotJoinInState(self.state.clone()));
        }

        // Check capacity
        if self.players.len() >= self.max_players {
            return Err(SessionError::SessionFull);
        }

        // Check for duplicates
        if self.players.contains(&player_id) {
            return Err(SessionError::PlayerAlreadyJoined(player_id.0.clone()));
        }

        self.players.push(player_id);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Remove a player from the session
    pub fn leave_player(&mut self, player_id: &PlayerId) -> Result<(), SessionError> {
        if !self.players.contains(player_id) {
            return Err(SessionError::PlayerNotInSession(player_id.0.clone()));
        }

        self.players.retain(|p| p != player_id);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Mark a player as ready
    pub fn set_player_ready(&mut self, player_id: &PlayerId) -> Result<(), SessionError> {
        if !self.players.contains(player_id) {
            return Err(SessionError::PlayerNotInSession(player_id.0.clone()));
        }

        // This is simplified; in a real impl would track per-player ready state
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Check if all players are ready (for this simple impl, all joined = ready)
    pub fn all_players_ready(&self) -> bool {
        self.players.len() == self.max_players && !self.players.is_empty()
    }

    /// Update heartbeat timestamp
    pub fn heartbeat(&mut self) {
        self.last_heartbeat = Utc::now();
    }

    /// Increment tick counter
    pub fn tick(&mut self) {
        self.tick_count += 1;
        self.last_heartbeat = Utc::now();
    }

    /// Check if session is stale (no heartbeat for > 30 seconds)
    pub fn is_stale(&self) -> bool {
        let elapsed = Utc::now() - self.last_heartbeat;
        elapsed > chrono::Duration::seconds(30)
    }
}
