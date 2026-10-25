# ADR-003: No Hot-Reload for Feature Flags

**Status:** Accepted  
**Date:** 2026-10-10  
**Participants:** Rollback Netcode Team

---

## Context

The server supports four feature flags for experimental functionality:
- `enable_ai_cheat_detection` — ML-based anomaly detection
- `enable_jitter_prediction` — Kalman filter latency forecasting
- `enable_rollback_risk_metrics` — Observability for rollback risk
- `enable_matchmaking_optimizer` — AI-driven player pairing

Question: Should these flags be:
- **Option A:** Read once at startup (static)
- **Option B:** Read from a config server and updated at runtime (dynamic)

---

## Decision

Flags are read **once at startup** and stored in a static configuration. No hot-reload.

---

## Rationale

### Simplicity
- Single code path per flag (no race conditions or concurrent updates)
- No need for config polling, events, or cache invalidation
- Easier to reason about and test

### Correctness During Play Sessions
- A player's session should not change flag states mid-play
- Example: if `enable_ai_cheat_detection` changes mid-match, some players' data is processed with old flag state → inconsistent results
- Prevents subtle bugs and fairness issues

### Operational Safety
- Forces deliberate decision: "Do I want to restart the server?"
- Prevents accidental flag flips that break running matches
- Downtime is explicit, not silent

### Testing
- Tests can verify flag behavior with simple setup (flag is const)
- No need for complex dynamic config mocking

---

## Consequences

### Positive
- Clear semantics: flag state is immutable during server lifetime
- Easy to implement and test
- No polling overhead

### Negative
- Flag changes require server restart
- During restart, active sessions are interrupted (brief downtime)
- Cannot use feature flags for gradual rollouts (must restart all servers)

### Neutral
- Makes A/B testing harder (would need separate server instances)
- Requires careful planning before flag activation

---

## Implementation

### Pseudo-code

```rust
// backend/src/config.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub enable_ai_cheat_detection: bool,
    pub enable_jitter_prediction: bool,
    pub enable_rollback_risk_metrics: bool,
    pub enable_matchmaking_optimizer: bool,
}

pub struct ServerConfig {
    // ... other fields ...
    pub features: FeatureFlags,
}

// Loaded once at startup
pub async fn load_config(path: &str) -> Result<ServerConfig, Error> {
    let toml = std::fs::read_to_string(path)?;
    toml::from_str(&toml)
}

// Usage: everywhere, flags are read from config (never reloaded)
pub async fn main() {
    let config = load_config("config.toml").await?;
    
    if config.features.enable_ai_cheat_detection {
        log::info!("AI Cheat Detection enabled");
        // Initialize ML model
    }
    
    // config.features are immutable for the lifetime of the server
}
```

### Config File
```toml
[features]
enable_ai_cheat_detection = false
enable_jitter_prediction = false
enable_rollback_risk_metrics = false
enable_matchmaking_optimizer = false
```

### Overrides via Environment
```bash
export ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1
./rollback-server
# Flag is set for this server instance; cannot be changed without restart
```

---

## Alternatives Considered

### 1. Dynamic Feature Flags (Hot-Reload)
- **Approach:** Read flags from Redis/ConfigServer on each check
- **Rejected:** Race conditions, inconsistent state during matches, polling overhead

### 2. Feature Flag Toggle Service (e.g., LaunchDarkly)
- **Approach:** Delegate flag state to external service
- **Rejected:** Adds dependency; overkill for Phase 1; late-binding complexity

### 3. Per-Session Flag Override
- **Approach:** Each session reads its own flag state
- **Rejected:** Complicates testing; unfair if some players have ML detection enabled

---

## Testing

### Test: Flags Respected at Startup
```rust
#[tokio::test]
async fn test_ai_cheat_detection_flag_respected() {
    let config = ServerConfig {
        features: FeatureFlags {
            enable_ai_cheat_detection: true,
            ..Default::default()
        },
        ..Default::default()
    };
    
    assert!(config.features.enable_ai_cheat_detection);
    // Verify ML model is initialized only if flag is true
}
```

### Test: Flags Cannot Be Changed
- Compile-time check: `features` field has no setter → immutable after init
- Runtime check: once server starts with flags set, they remain unchanged

---

## Monitoring

Ops team should:
1. Log flag state at server startup
2. Document required restart for flag changes
3. Plan maintenance windows for flag changes

---

## Related ADRs

- **ADR-001:** In-Memory vs. Postgres Session Repository (testing simplicity)
- **ADR-004:** Deterministic Tick Rate with 60 Hz Default

---

## Future Considerations

- **Phase 2:** Add config polling with graceful reload (if needed for rapid iterations)
- **Phase 3:** Integrate with feature flag service (LaunchDarkly, etc.) for production fleet management

---

## Sign-Off

- **Approved by:** Rollback Netcode Team
- **Date:** 2026-10-10
