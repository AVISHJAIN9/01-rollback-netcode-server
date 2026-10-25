/// Authentication module: signed tokens with expiry, rotation, and revocation
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Token claims: player identity, session, expiry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenClaims {
    pub player_id: String,
    pub session_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub version: u32,  // Token version for rotation support
}

impl TokenClaims {
    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if token is valid (not expired, not revoked)
    pub async fn is_valid(&self, revocation_list: &RevocationList) -> bool {
        !self.is_expired() && !revocation_list.is_revoked(self).await
    }
}

/// In-memory revocation list for tokens
pub struct RevocationList {
    revoked: Arc<RwLock<HashSet<String>>>,
}

impl RevocationList {
    pub fn new() -> Self {
        RevocationList {
            revoked: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Add token to revocation list
    pub async fn revoke(&self, player_id: &str, session_id: Uuid) {
        let key = format!("{}:{}", player_id, session_id);
        self.revoked.write().await.insert(key);
    }

    /// Check if token is revoked
    pub async fn is_revoked(&self, claims: &TokenClaims) -> bool {
        let key = format!("{}:{}", claims.player_id, claims.session_id);
        self.revoked.read().await.contains(&key)
    }

    /// Clear all revocations (for testing)
    pub async fn clear(&self) {
        self.revoked.write().await.clear();
    }
}

/// Token manager: create and validate tokens
pub struct TokenManager {
    secret: [u8; 32],  // Fixed size for blake3::keyed_hash
    ttl_secs: u64,
    revocation_list: Arc<RevocationList>,
}

impl TokenManager {
    pub fn new(secret: String, ttl_secs: u64) -> Self {
        // Derive 32-byte key from secret string
        let mut key = [0u8; 32];
        let secret_hash = blake3::hash(secret.as_bytes());
        key.copy_from_slice(&secret_hash.as_bytes()[..32]);

        TokenManager {
            secret: key,
            ttl_secs,
            revocation_list: Arc::new(RevocationList::new()),
        }
    }

    /// Create a new signed token
    pub fn create_token(
        &self,
        player_id: String,
        session_id: Uuid,
        version: u32,
    ) -> String {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(self.ttl_secs as i64);

        let claims = TokenClaims {
            player_id,
            session_id,
            issued_at: now,
            expires_at,
            version,
        };

        // Serialize claims
        let serialized = serde_json::to_string(&claims).unwrap();
        // Sign with keyed hash
        let hash = blake3::keyed_hash(&self.secret, serialized.as_bytes());
        let signature_hex = format!("{}", hash.to_hex());

        // Return base64(claims).hash format for easy parsing
        let encoded = base64_encode(&serialized);
        format!("{}.{}", encoded, signature_hex)
    }

    /// Validate and extract claims from token
    pub fn validate_token(&self, token: &str) -> Result<TokenClaims, String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 2 {
            return Err("Invalid token format".to_string());
        }

        let encoded = parts[0];
        let signature_hex = parts[1];

        // Decode claims
        let serialized =
            base64_decode(encoded).map_err(|e| format!("Token decode failed: {}", e))?;

        // Verify signature
        let expected_sig = blake3::keyed_hash(&self.secret, serialized.as_bytes());
        let expected_hex = format!("{}", expected_sig.to_hex());

        if signature_hex != expected_hex {
            return Err("Token signature mismatch (tampering detected)".to_string());
        }

        // Parse claims
        let claims: TokenClaims =
            serde_json::from_str(&serialized).map_err(|e| format!("Invalid claims: {}", e))?;

        // Check expiry
        if claims.is_expired() {
            return Err("Token expired".to_string());
        }

        Ok(claims)
    }

    /// Revoke a token
    pub async fn revoke_token(&self, claims: &TokenClaims) {
        self.revocation_list.revoke(&claims.player_id, claims.session_id).await;
    }

    /// Check if token is valid (not expired, not tampered, not revoked)
    pub async fn is_token_valid(&self, token: &str) -> bool {
        match self.validate_token(token) {
            Ok(claims) => claims.is_valid(&self.revocation_list).await,
            Err(_) => false,
        }
    }

    pub fn get_revocation_list(&self) -> Arc<RevocationList> {
        self.revocation_list.clone()
    }
}

/// Simple base64 encoding helper
fn base64_encode(data: &str) -> String {
    use std::fmt::Write;
    let bytes = data.as_bytes();
    let mut result = String::new();
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    for chunk in bytes.chunks(3) {
        let b1 = chunk[0];
        let b2 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b3 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let _ = write!(result, "{}", TABLE[(b1 >> 2) as usize] as char);
        let _ = write!(
            result,
            "{}",
            TABLE[(((b1 & 0x03) << 4) | (b2 >> 4)) as usize] as char
        );
        if chunk.len() > 1 {
            let _ = write!(
                result,
                "{}",
                TABLE[(((b2 & 0x0f) << 2) | (b3 >> 6)) as usize] as char
            );
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            let _ = write!(result, "{}", TABLE[(b3 & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Simple base64 decoding helper
fn base64_decode(data: &str) -> Result<String, String> {
    let mut bytes = Vec::new();
    let data = data.trim_end_matches('=');
    let table = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    for chunk in data.chars().collect::<Vec<_>>().chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let idx = table.find(c).ok_or("Invalid character")?;
            n |= (idx as u32) << (18 - 6 * i);
        }
        bytes.push((n >> 16) as u8);
        if chunk.len() > 2 {
            bytes.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            bytes.push(n as u8);
        }
    }

    String::from_utf8(bytes).map_err(|e| format!("UTF-8 error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_validate_token() {
        let manager = TokenManager::new("test-secret".to_string(), 3600);
        let player_id = "player-1".to_string();
        let session_id = Uuid::new_v4();

        let token = manager.create_token(player_id.clone(), session_id, 1);
        let claims = manager.validate_token(&token).expect("Valid token");

        assert_eq!(claims.player_id, player_id);
        assert_eq!(claims.session_id, session_id);
        assert!(!claims.is_expired());
    }

    #[tokio::test]
    async fn test_expired_token_rejected() {
        let manager = TokenManager::new("test-secret".to_string(), 0);  // 0 TTL
        let token = manager.create_token("player-1".to_string(), Uuid::new_v4(), 1);

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(manager.validate_token(&token).is_err());
    }

    #[tokio::test]
    async fn test_tampered_token_rejected() {
        let manager = TokenManager::new("test-secret".to_string(), 3600);
        let token = manager.create_token("player-1".to_string(), Uuid::new_v4(), 1);

        // Tamper: modify the token
        let tampered = format!("{}X", token);
        assert!(manager.validate_token(&tampered).is_err());
    }

    #[tokio::test]
    async fn test_revoked_token_rejected() {
        let manager = TokenManager::new("test-secret".to_string(), 3600);
        let player_id = "player-1".to_string();
        let session_id = Uuid::new_v4();
        let token = manager.create_token(player_id.clone(), session_id, 1);

        // Token is valid initially
        assert!(manager.is_token_valid(&token).await);

        // Revoke it
        let claims = manager.validate_token(&token).unwrap();
        manager.revoke_token(&claims).await;

        // Now rejected
        assert!(!manager.is_token_valid(&token).await);
    }

    #[tokio::test]
    async fn test_wrong_secret_rejects_token() {
        let manager1 = TokenManager::new("secret-1".to_string(), 3600);
        let manager2 = TokenManager::new("secret-2".to_string(), 3600);

        let token = manager1.create_token("player-1".to_string(), Uuid::new_v4(), 1);
        assert!(manager2.validate_token(&token).is_err());
    }
}
