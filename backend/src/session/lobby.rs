// Lobby state machine for player gathering before match start
// Supports create, join, leave, ready, auto-start, and concurrent joins
// Arc<RwLock<>> for thread-safe concurrent mutation

use chrono::{DateTime, Utc, Duration};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::session::{PlayerId, SessionError};

/// Unique identifier for a lobby
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LobbyId(pub String);

impl LobbyId {
    pub fn generate() -> Self {
        LobbyId(format!("LOBBY-{}", Uuid::new_v4()))
    }
}

/// Per-player ready state
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadyState {
    NotReady,
    Ready,
}

/// A lobby: transient in-memory structure for players to gather before match
#[derive(Clone, Debug)]
pub struct Lobby {
    pub lobby_id: LobbyId,
    pub players: Vec<PlayerId>,
    pub ready_states: HashMap<PlayerId, ReadyState>,
    pub max_players: usize,
    pub created_at: DateTime<Utc>,
    pub auto_start_after_secs: u64,
}

impl Lobby {
    /// Create a new lobby
    pub fn new(max_players: usize, auto_start_after_secs: u64) -> Result<Self, SessionError> {
        if max_players < 2 || max_players > 4 {
            return Err(SessionError::InvalidCapacity(max_players));
        }
        Ok(Lobby {
            lobby_id: LobbyId::generate(),
            players: Vec::new(),
            ready_states: HashMap::new(),
            max_players,
            created_at: Utc::now(),
            auto_start_after_secs,
        })
    }

    /// Create a lobby with a specific created_at timestamp (for testing)
    pub fn new_with_created_at(
        max_players: usize,
        auto_start_after_secs: u64,
        created_at: DateTime<Utc>,
    ) -> Result<Self, SessionError> {
        if max_players < 2 || max_players > 4 {
            return Err(SessionError::InvalidCapacity(max_players));
        }
        Ok(Lobby {
            lobby_id: LobbyId::generate(),
            players: Vec::new(),
            ready_states: HashMap::new(),
            max_players,
            created_at,
            auto_start_after_secs,
        })
    }

    /// Add a player to the lobby
    pub fn join_player(&mut self, player_id: PlayerId) -> Result<(), SessionError> {
        // Idempotent: if player already here, OK
        if self.players.contains(&player_id) {
            return Ok(());
        }

        if self.players.len() >= self.max_players {
            return Err(SessionError::SessionFull);
        }

        self.players.push(player_id.clone());
        self.ready_states.insert(player_id, ReadyState::NotReady);
        Ok(())
    }

    /// Remove a player from the lobby
    pub fn leave_player(&mut self, player_id: &PlayerId) -> Result<(), SessionError> {
        if !self.players.contains(player_id) {
            return Err(SessionError::PlayerNotInSession(player_id.0.clone()));
        }
        self.players.retain(|p| p != player_id);
        self.ready_states.remove(player_id);
        Ok(())
    }

    /// Mark a player as ready
    pub fn set_ready(&mut self, player_id: &PlayerId) -> Result<(), SessionError> {
        if !self.players.contains(player_id) {
            return Err(SessionError::PlayerNotInSession(player_id.0.clone()));
        }
        // Check if already ready (idempotent error)
        if self.ready_states.get(player_id) == Some(&ReadyState::Ready) {
            return Err(SessionError::PlayerAlreadyJoined(player_id.0.clone()));
        }
        self.ready_states.insert(player_id.clone(), ReadyState::Ready);
        Ok(())
    }

    /// Check if all players are ready
    pub fn all_ready(&self) -> bool {
        if self.players.is_empty() {
            return false;
        }
        self.players.iter().all(|p| {
            self.ready_states.get(p) == Some(&ReadyState::Ready)
        })
    }

    /// Check if auto-start timeout has fired
    pub fn should_auto_start(&self) -> bool {
        let elapsed = Utc::now() - self.created_at;
        let threshold = Duration::seconds(self.auto_start_after_secs as i64);
        elapsed >= threshold
    }

    /// Check if lobby should transition (all ready OR timeout)
    pub fn can_transition_to_match_ready(&self) -> bool {
        !self.players.is_empty() && (self.all_ready() || self.should_auto_start())
    }
}

/// Global lobby registry: maps LobbyId to Lobby
pub struct LobbyRegistry {
    lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>>,
}

impl LobbyRegistry {
    pub fn new() -> Self {
        Self {
            lobbies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create and register a new lobby
    pub async fn create_lobby(
        &self,
        max_players: usize,
        auto_start_after_secs: u64,
    ) -> Result<LobbyId, SessionError> {
        let lobby = Lobby::new(max_players, auto_start_after_secs)?;
        let lobby_id = lobby.lobby_id.clone();
        let mut lobbies = self.lobbies.write().await;
        lobbies.insert(lobby_id.clone(), lobby);
        Ok(lobby_id)
    }

    /// Get a lobby by ID
    pub async fn get_lobby(&self, lobby_id: &LobbyId) -> Result<Option<Lobby>, SessionError> {
        let lobbies = self.lobbies.read().await;
        Ok(lobbies.get(lobby_id).cloned())
    }

    /// Join a player to a lobby
    pub async fn join_lobby(
        &self,
        lobby_id: &LobbyId,
        player_id: PlayerId,
    ) -> Result<(), SessionError> {
        let mut lobbies = self.lobbies.write().await;
        if let Some(lobby) = lobbies.get_mut(lobby_id) {
            lobby.join_player(player_id)?;
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Leave a lobby
    pub async fn leave_lobby(
        &self,
        lobby_id: &LobbyId,
        player_id: &PlayerId,
    ) -> Result<(), SessionError> {
        let mut lobbies = self.lobbies.write().await;
        if let Some(lobby) = lobbies.get_mut(lobby_id) {
            lobby.leave_player(player_id)?;
            // Clean up empty lobbies
            if lobby.players.is_empty() {
                lobbies.remove(lobby_id);
            }
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Set a player as ready in a lobby
    pub async fn set_player_ready(
        &self,
        lobby_id: &LobbyId,
        player_id: &PlayerId,
    ) -> Result<(), SessionError> {
        let mut lobbies = self.lobbies.write().await;
        if let Some(lobby) = lobbies.get_mut(lobby_id) {
            lobby.set_ready(player_id)?;
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Check if lobby can transition to match ready
    pub async fn can_transition_to_match_ready(&self, lobby_id: &LobbyId) -> Result<bool, SessionError> {
        let lobbies = self.lobbies.read().await;
        if let Some(lobby) = lobbies.get(lobby_id) {
            Ok(lobby.can_transition_to_match_ready())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }
}

impl Default for LobbyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for LobbyRegistry {
    fn clone(&self) -> Self {
        Self {
            lobbies: Arc::clone(&self.lobbies),
        }
    }
}
