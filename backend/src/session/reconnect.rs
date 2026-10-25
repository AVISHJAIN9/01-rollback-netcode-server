// Reconnect module for session resumption with fake-clock support
// Uses tokio::time::Instant for compatibility with tokio::time::pause()

use tokio::time::{Instant, Duration};
use uuid::Uuid;
use std::collections::HashMap;

/// A reconnect token binding player_id, session_id, and expiry
#[derive(Clone, Debug)]
pub struct ReconnectToken {
    pub player_id: String,
    pub session_id: Uuid,
    pub token_str: String,
    pub issued_at: Instant,
    pub expires_at: Instant,
}

/// Errors that can occur during token validation
#[derive(Clone, Debug, PartialEq)]
pub enum ReconnectError {
    /// Token has expired
    Expired,
    /// Token has already been consumed
    Reused,
    /// Token's player_id does not match the provided player_id
    WrongPlayer,
    /// Token's session_id does not match the provided session_id
    WrongSession,
    /// Token not found in the manager's registry
    NotFound,
}

/// Manages reconnect tokens and last-seen ticks per player
pub struct ReconnectManager {
    /// Active tokens: token_str -> ReconnectToken
    active_tokens: HashMap<String, ReconnectToken>,
    /// Last acknowledged tick per player: player_id -> tick
    last_ticks: HashMap<String, u64>,
}

impl ReconnectManager {
    /// Create a new reconnect manager
    pub fn new() -> Self {
        Self {
            active_tokens: HashMap::new(),
            last_ticks: HashMap::new(),
        }
    }

    /// Issue a new reconnect token for a player in a session
    pub fn issue_token(&mut self, player_id: &str, session_id: Uuid) -> ReconnectToken {
        let now = Instant::now();
        let token_str = format!("{}-{}-{}", player_id, session_id, uuid::Uuid::new_v4());
        let token = ReconnectToken {
            player_id: player_id.to_string(),
            session_id,
            token_str: token_str.clone(),
            issued_at: now,
            expires_at: now + Duration::from_secs(30),
        };
        self.active_tokens.insert(token_str, token.clone());
        token
    }

    /// Record the last acknowledged tick for a player
    pub fn record_tick(&mut self, player_id: &str, tick: u64) {
        self.last_ticks.insert(player_id.to_string(), tick);
    }

    /// Validate a token and return the resume tick (last_tick + 1) on success
    /// On success, the token is consumed (removed from active registry)
    pub fn validate_token(
        &mut self,
        token_str: &str,
        player_id: &str,
        session_id: Uuid,
    ) -> Result<u64, ReconnectError> {
        // Check if token exists
        let token = self.active_tokens.get(token_str).ok_or(ReconnectError::NotFound)?;

        // Check expiry
        if Instant::now() >= token.expires_at {
            // Remove expired token
            self.active_tokens.remove(token_str);
            return Err(ReconnectError::Expired);
        }

        // Check player_id matches
        if token.player_id != player_id {
            return Err(ReconnectError::WrongPlayer);
        }

        // Check session_id matches
        if token.session_id != session_id {
            return Err(ReconnectError::WrongSession);
        }

        // Token is valid; consume it
        self.active_tokens.remove(token_str);

        // Return resume tick: last_tick + 1
        let resume_tick = self.last_ticks.get(player_id).copied().unwrap_or(0) + 1;
        Ok(resume_tick)
    }

    /// Get the last acknowledged tick for a player (for testing)
    pub fn get_last_tick(&self, player_id: &str) -> Option<u64> {
        self.last_ticks.get(player_id).copied()
    }

    /// Check if a token is still active (for testing)
    pub fn has_active_token(&self, token_str: &str) -> bool {
        self.active_tokens.contains_key(token_str)
    }
}

impl Default for ReconnectManager {
    fn default() -> Self {
        Self::new()
    }
}
