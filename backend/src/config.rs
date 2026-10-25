use serde::{Deserialize, Serialize};
use std::path::Path;

/// Server configuration, loaded from config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSettings,
    pub ports: PortSettings,
    pub database: DatabaseSettings,
    pub features: FeatureFlags,
}

/// Server timing and behavior settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    /// Tick rate in Hz (must be > 0)
    pub tick_rate: u32,
    /// Reconnect timeout in seconds (must be > 0)
    pub reconnect_timeout_secs: u64,
    /// Stale session detection threshold in seconds (must be > 0)
    pub stale_session_threshold_secs: u64,
}

impl ServerSettings {
    /// Validate server settings
    fn validate(&self) -> Result<(), String> {
        if self.tick_rate == 0 {
            return Err("tick_rate must be > 0".to_string());
        }
        if self.reconnect_timeout_secs == 0 {
            return Err("reconnect_timeout_secs must be > 0".to_string());
        }
        if self.stale_session_threshold_secs == 0 {
            return Err("stale_session_threshold_secs must be > 0".to_string());
        }
        Ok(())
    }
}

/// Network port configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSettings {
    /// UDP game server port (must be > 1023)
    pub udp_game: u16,
    /// HTTP admin/metrics port (must be > 1023)
    pub http_admin: u16,
    /// Frontend WebSocket port (must be > 1023)
    pub frontend: u16,
}

impl PortSettings {
    /// Validate port settings
    fn validate(&self) -> Result<(), String> {
        if self.udp_game == 0 {
            return Err("udp_game port must be > 0".to_string());
        }
        if self.http_admin == 0 {
            return Err("http_admin port must be > 0".to_string());
        }
        if self.frontend == 0 {
            return Err("frontend port must be > 0".to_string());
        }
        Ok(())
    }
}

/// Database connection settings (REQUIRED)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSettings {
    /// Postgres connection URL (REQUIRED)
    pub postgres_url: String,
    /// Redis connection URL (REQUIRED)
    pub redis_url: String,
}

impl DatabaseSettings {
    /// Validate database settings
    fn validate(&self) -> Result<(), String> {
        if self.postgres_url.is_empty() {
            return Err("postgres_url is required and cannot be empty".to_string());
        }
        if !self.postgres_url.starts_with("postgres://") && !self.postgres_url.starts_with("postgresql://") {
            return Err("postgres_url must start with postgres:// or postgresql://".to_string());
        }
        if self.redis_url.is_empty() {
            return Err("redis_url is required and cannot be empty".to_string());
        }
        if !self.redis_url.starts_with("redis://") {
            return Err("redis_url must start with redis://".to_string());
        }
        Ok(())
    }
}

/// Feature flags (all default to false)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// ML-based cheat detection (default false)
    pub enable_ai_cheat_detection: bool,
    /// Kalman filter jitter prediction (default false)
    pub enable_jitter_prediction: bool,
    /// Observability for rollback risk (default false)
    pub enable_rollback_risk_metrics: bool,
    /// AI-driven matchmaking optimizer (default false)
    pub enable_matchmaking_optimizer: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            server: ServerSettings {
                tick_rate: 60,
                reconnect_timeout_secs: 30,
                stale_session_threshold_secs: 40,
            },
            ports: PortSettings {
                udp_game: 7777,
                http_admin: 8080,
                frontend: 3000,
            },
            database: DatabaseSettings {
                postgres_url: "postgres://postgres:postgrespassword@localhost:5432/rollback_db".to_string(),
                redis_url: "redis://localhost:6379/0".to_string(),
            },
            features: FeatureFlags {
                enable_ai_cheat_detection: false,
                enable_jitter_prediction: false,
                enable_rollback_risk_metrics: false,
                enable_matchmaking_optimizer: false,
            },
        }
    }
}

impl ServerConfig {
    /// Validate the entire configuration
    pub fn validate(&self) -> Result<(), String> {
        self.server.validate()?;
        self.ports.validate()?;
        self.database.validate()?;
        Ok(())
    }
}

/// Load configuration from file (config.toml) or environment variables
pub async fn load_config(config_path: Option<&str>) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let config_path = config_path.unwrap_or("config.toml");

    // If config file doesn't exist, use defaults
    if !Path::new(config_path).exists() {
        log::info!("Config file not found at {}; using defaults", config_path);
        let config = ServerConfig::default();
        config.validate()?;
        return Ok(config);
    }

    // Load from TOML
    let config_str = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    let mut config: ServerConfig = toml::from_str(&config_str)
        .map_err(|e| format!("Failed to parse config TOML: {}", e))?;

    // Override with environment variables (prefix: ROLLBACK_)
    if let Ok(tick_rate) = std::env::var("ROLLBACK_TICK_RATE") {
        config.server.tick_rate = tick_rate.parse()?;
    }
    if let Ok(reconnect_secs) = std::env::var("ROLLBACK_RECONNECT_TIMEOUT_SECS") {
        config.server.reconnect_timeout_secs = reconnect_secs.parse()?;
    }
    if let Ok(stale_secs) = std::env::var("ROLLBACK_STALE_SESSION_THRESHOLD_SECS") {
        config.server.stale_session_threshold_secs = stale_secs.parse()?;
    }
    if let Ok(udp_port) = std::env::var("ROLLBACK_UDP_GAME_PORT") {
        config.ports.udp_game = udp_port.parse()?;
    }
    if let Ok(http_port) = std::env::var("ROLLBACK_HTTP_ADMIN_PORT") {
        config.ports.http_admin = http_port.parse()?;
    }
    if let Ok(frontend_port) = std::env::var("ROLLBACK_FRONTEND_PORT") {
        config.ports.frontend = frontend_port.parse()?;
    }
    if let Ok(postgres_url) = std::env::var("ROLLBACK_POSTGRES_URL") {
        if !postgres_url.is_empty() {
            config.database.postgres_url = postgres_url;
        }
    }
    if let Ok(redis_url) = std::env::var("ROLLBACK_REDIS_URL") {
        if !redis_url.is_empty() {
            config.database.redis_url = redis_url;
        }
    }
    if let Ok(flag) = std::env::var("ROLLBACK_ENABLE_AI_CHEAT_DETECTION") {
        config.features.enable_ai_cheat_detection = flag == "1" || flag.to_lowercase() == "true";
    }
    if let Ok(flag) = std::env::var("ROLLBACK_ENABLE_JITTER_PREDICTION") {
        config.features.enable_jitter_prediction = flag == "1" || flag.to_lowercase() == "true";
    }
    if let Ok(flag) = std::env::var("ROLLBACK_ENABLE_ROLLBACK_RISK_METRICS") {
        config.features.enable_rollback_risk_metrics = flag == "1" || flag.to_lowercase() == "true";
    }
    if let Ok(flag) = std::env::var("ROLLBACK_ENABLE_MATCHMAKING_OPTIMIZER") {
        config.features.enable_matchmaking_optimizer = flag == "1" || flag.to_lowercase() == "true";
    }

    // Validate after all overrides applied
    config.validate()?;
    
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.server.tick_rate, 60);
        assert_eq!(config.server.reconnect_timeout_secs, 30);
        assert_eq!(config.server.stale_session_threshold_secs, 40);
        assert_eq!(config.ports.udp_game, 7777);
        assert_eq!(config.ports.http_admin, 8080);
        assert_eq!(config.ports.frontend, 3000);
        assert!(!config.features.enable_ai_cheat_detection);
    }

    #[test]
    fn test_default_config_is_valid() {
        let config = ServerConfig::default();
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn test_load_missing_config_returns_defaults() {
        let result = load_config(Some("nonexistent.toml")).await;
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.server.tick_rate, 60);
    }

    #[test]
    fn test_invalid_tick_rate_rejected() {
        let mut config = ServerConfig::default();
        config.server.tick_rate = 0;
        assert!(config.validate().is_err());
        assert_eq!(config.validate().unwrap_err(), "tick_rate must be > 0");
    }

    #[test]
    fn test_invalid_reconnect_timeout_rejected() {
        let mut config = ServerConfig::default();
        config.server.reconnect_timeout_secs = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_port_rejected() {
        let mut config = ServerConfig::default();
        config.ports.udp_game = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_missing_postgres_url_rejected() {
        let mut config = ServerConfig::default();
        config.database.postgres_url = String::new();
        assert!(config.validate().is_err());
        assert_eq!(config.validate().unwrap_err(), "postgres_url is required and cannot be empty");
    }

    #[test]
    fn test_invalid_postgres_url_format_rejected() {
        let mut config = ServerConfig::default();
        config.database.postgres_url = "mysql://localhost".to_string();
        assert!(config.validate().is_err());
        assert!(config.validate().unwrap_err().contains("postgres://"));
    }

    #[test]
    fn test_missing_redis_url_rejected() {
        let mut config = ServerConfig::default();
        config.database.redis_url = String::new();
        assert!(config.validate().is_err());
        assert_eq!(config.validate().unwrap_err(), "redis_url is required and cannot be empty");
    }

    #[test]
    fn test_invalid_redis_url_format_rejected() {
        let mut config = ServerConfig::default();
        config.database.redis_url = "memcached://localhost".to_string();
        assert!(config.validate().is_err());
        assert!(config.validate().unwrap_err().contains("redis://"));
    }
}
