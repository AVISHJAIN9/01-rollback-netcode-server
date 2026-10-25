use super::{Session, ReconnectToken, SessionError};
use uuid::Uuid;
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

/// Repository trait for session persistence
#[async_trait::async_trait]
pub trait SessionRepository: Send + Sync {
    /// Create a new session
    async fn create(&self, session: Session) -> Result<Session, SessionError>;
    
    /// Get session by ID
    async fn get(&self, session_id: Uuid) -> Result<Option<Session>, SessionError>;
    
    /// Get session by room code
    async fn get_by_room_code(&self, room_code: &str) -> Result<Option<Session>, SessionError>;
    
    /// Update session state
    async fn update(&self, session: Session) -> Result<(), SessionError>;
    
    /// Update heartbeat for a session
    async fn heartbeat(&self, session_id: Uuid) -> Result<(), SessionError>;
    
    /// Find stale sessions (no heartbeat for > N seconds)
    async fn find_stale(&self, seconds: u64) -> Result<Vec<Session>, SessionError>;
    
    /// Store a reconnect token
    async fn store_reconnect_token(&self, token: ReconnectToken) -> Result<(), SessionError>;
    
    /// Get reconnect token
    async fn get_reconnect_token(&self, token_str: &str) -> Result<Option<ReconnectToken>, SessionError>;
    
    /// Validate a reconnect token (checks existence and expiry)
    async fn validate_reconnect_token(&self, token_str: &str) -> Result<ReconnectToken, SessionError>;

    /// Validate a reconnect token with player_id verification (session hijacking defense)
    async fn validate_reconnect_token_for_player(
        &self,
        token_str: &str,
        expected_player_id: &super::PlayerId,
    ) -> Result<ReconnectToken, SessionError> {
        let token = self.validate_reconnect_token(token_str).await?;
        if token.player_id != *expected_player_id {
            return Err(SessionError::PlayerMismatch {
                expected: expected_player_id.0.clone(),
                actual: token.player_id.0.clone(),
            });
        }
        Ok(token)
    }
    
    /// Delete a session
    async fn delete(&self, session_id: Uuid) -> Result<(), SessionError>;
}

/// In-memory session repository for STEP 3
#[derive(Clone)]
pub struct InMemorySessionRepository {
    sessions: Arc<RwLock<HashMap<Uuid, Session>>>,
    room_codes: Arc<RwLock<HashMap<String, Uuid>>>,
    reconnect_tokens: Arc<RwLock<HashMap<String, ReconnectToken>>>,
}

impl InMemorySessionRepository {
    /// Create a new in-memory repository
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            room_codes: Arc::new(RwLock::new(HashMap::new())),
            reconnect_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemorySessionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SessionRepository for InMemorySessionRepository {
    async fn create(&self, session: Session) -> Result<Session, SessionError> {
        let mut sessions = self.sessions.write().await;
        let mut room_codes = self.room_codes.write().await;
        
        // Check for duplicate room code
        if room_codes.contains_key(&session.room_code) {
            return Err(SessionError::RepositoryError(
                format!("Room code {} already exists", session.room_code),
            ));
        }
        
        let session_id = session.session_id;
        let room_code = session.room_code.clone();
        
        sessions.insert(session_id, session.clone());
        room_codes.insert(room_code, session_id);
        
        Ok(session)
    }
    
    async fn get(&self, session_id: Uuid) -> Result<Option<Session>, SessionError> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(&session_id).cloned())
    }
    
    async fn get_by_room_code(&self, room_code: &str) -> Result<Option<Session>, SessionError> {
        let room_codes = self.room_codes.read().await;
        if let Some(session_id) = room_codes.get(room_code) {
            let sessions = self.sessions.read().await;
            Ok(sessions.get(session_id).cloned())
        } else {
            Ok(None)
        }
    }
    
    async fn update(&self, session: Session) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        if !sessions.contains_key(&session.session_id) {
            return Err(SessionError::SessionNotFound);
        }
        sessions.insert(session.session_id, session);
        Ok(())
    }
    
    async fn heartbeat(&self, session_id: Uuid) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            session.heartbeat();
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }
    
    async fn find_stale(&self, seconds: u64) -> Result<Vec<Session>, SessionError> {
        let sessions = self.sessions.read().await;
        let stale_threshold = chrono::Duration::seconds(seconds as i64);
        let now = chrono::Utc::now();
        
        let stale_sessions = sessions
            .values()
            .filter(|s| now - s.last_heartbeat > stale_threshold)
            .cloned()
            .collect();
        
        Ok(stale_sessions)
    }
    
    async fn store_reconnect_token(&self, token: ReconnectToken) -> Result<(), SessionError> {
        let mut tokens = self.reconnect_tokens.write().await;
        tokens.insert(token.token.clone(), token);
        Ok(())
    }
    
    async fn get_reconnect_token(&self, token_str: &str) -> Result<Option<ReconnectToken>, SessionError> {
        let tokens = self.reconnect_tokens.read().await;
        Ok(tokens.get(token_str).cloned())
    }
    
    async fn validate_reconnect_token(&self, token_str: &str) -> Result<ReconnectToken, SessionError> {
        let tokens = self.reconnect_tokens.read().await;
        
        if let Some(token) = tokens.get(token_str) {
            if token.is_valid() {
                Ok(token.clone())
            } else {
                Err(SessionError::TokenExpired)
            }
        } else {
            Err(SessionError::TokenNotFound)
        }
    }
    
    async fn delete(&self, session_id: Uuid) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(&session_id) {
            let mut room_codes = self.room_codes.write().await;
            room_codes.remove(&session.room_code);
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }
}
