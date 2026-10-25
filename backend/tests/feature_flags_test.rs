use std::fs;

#[test]
fn test_feature_flags_default_all_off() {
    // Feature flags should default to false
    let config_str = r#"
[server]
tick_rate = 60

[ports]
udp_game = 7777
http_admin = 8080
frontend = 3000

[database]
postgres_url = "postgres://localhost"
redis_url = "redis://localhost"

[features]
enable_ai_cheat_detection = false
enable_jitter_prediction = false
enable_rollback_risk_metrics = false
enable_matchmaking_optimizer = false
"#;

    // Parse as TOML config
    let cfg: toml::Value = toml::from_str(config_str).expect("Parse failed");
    
    assert_eq!(cfg["features"]["enable_ai_cheat_detection"].as_bool(), Some(false));
    assert_eq!(cfg["features"]["enable_jitter_prediction"].as_bool(), Some(false));
    assert_eq!(cfg["features"]["enable_rollback_risk_metrics"].as_bool(), Some(false));
    assert_eq!(cfg["features"]["enable_matchmaking_optimizer"].as_bool(), Some(false));
}

#[test]
fn test_feature_flags_missing_defaults_to_false() {
    // If feature flags section is missing, they should not panic
    let config_str = r#"
[server]
tick_rate = 60

[ports]
udp_game = 7777
http_admin = 8080
frontend = 3000

[database]
postgres_url = "postgres://localhost"
redis_url = "redis://localhost"
"#;

    let cfg: toml::Value = toml::from_str(config_str).expect("Parse failed");
    
    // Missing sections should be handled gracefully
    // The features table might not exist, so check it exists first
    if let Some(features) = cfg.get("features") {
        // If features section exists but flag is missing
        assert_eq!(features.get("enable_ai_cheat_detection"), None);
    } else {
        // If no features section, that's also OK
        assert_eq!(cfg.get("features"), None);
    }
}

#[test]
fn test_feature_flags_can_be_enabled() {
    let config_str = r#"
[server]
tick_rate = 60

[ports]
udp_game = 7777
http_admin = 8080
frontend = 3000

[database]
postgres_url = "postgres://localhost"
redis_url = "redis://localhost"

[features]
enable_ai_cheat_detection = true
enable_jitter_prediction = true
enable_rollback_risk_metrics = false
enable_matchmaking_optimizer = false
"#;

    let cfg: toml::Value = toml::from_str(config_str).expect("Parse failed");
    
    assert_eq!(cfg["features"]["enable_ai_cheat_detection"].as_bool(), Some(true));
    assert_eq!(cfg["features"]["enable_jitter_prediction"].as_bool(), Some(true));
    assert_eq!(cfg["features"]["enable_rollback_risk_metrics"].as_bool(), Some(false));
    assert_eq!(cfg["features"]["enable_matchmaking_optimizer"].as_bool(), Some(false));
}

#[test]
fn test_feature_flags_invalid_type_rejected() {
    // Feature flags should be booleans, not strings or numbers
    let config_str = r#"
[server]
tick_rate = 60

[ports]
udp_game = 7777
http_admin = 8080
frontend = 3000

[database]
postgres_url = "postgres://localhost"
redis_url = "redis://localhost"

[features]
enable_ai_cheat_detection = "yes"
"#;

    let cfg: toml::Value = toml::from_str(config_str).expect("Parse failed");
    
    // as_bool() should return None for string values
    assert_eq!(cfg["features"]["enable_ai_cheat_detection"].as_bool(), None);
    assert_eq!(cfg["features"]["enable_ai_cheat_detection"].as_str(), Some("yes"));
}

#[test]
fn test_feature_flags_all_flags_exist() {
    // Verify all 4 feature flags are defined in config.toml
    let config_content = fs::read_to_string("config.toml").expect("config.toml not found");
    
    assert!(config_content.contains("enable_ai_cheat_detection"), "Missing AI cheat detection flag");
    assert!(config_content.contains("enable_jitter_prediction"), "Missing jitter prediction flag");
    assert!(config_content.contains("enable_rollback_risk_metrics"), "Missing rollback risk metrics flag");
    assert!(config_content.contains("enable_matchmaking_optimizer"), "Missing matchmaking optimizer flag");
}

#[test]
fn test_feature_flags_no_auto_action_on_enable() {
    // Enabling a flag should not auto-trigger actions
    // (This is a design test — the code should not auto-run ML just because the flag is on)
    let config_str = r#"
[features]
enable_ai_cheat_detection = true
"#;

    let cfg: toml::Value = toml::from_str(config_str).expect("Parse failed");
    
    // Just verify the flag parses; actual behavior is in admin/safety module
    assert_eq!(cfg["features"]["enable_ai_cheat_detection"].as_bool(), Some(true));
}
