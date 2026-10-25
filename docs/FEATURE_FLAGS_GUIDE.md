# Feature Flags Configuration Guide

**Version:** 1.0  
**Date:** 2026-10-10  
**Status:** Final

---

## Overview

The rollback netcode server supports four feature flags for enabling experimental and advanced features. All flags default to `false` (disabled).

---

## Available Flags

### 1. `enable_ai_cheat_detection`
**Purpose:** Enable ML-based anomaly detection for suspicious player behavior

**When to enable:**
- Large-scale tournaments (100+ players)
- Ranked matchmaking where cheating is costly
- When you want observability into suspected cheaters

**Impact:** 
- Minimal CPU overhead (~2% on server)
- Flags suspicious players for manual review
- Does NOT automatically ban (requires admin action)

**Configuration:**
```toml
[features]
enable_ai_cheat_detection = true
```

or via environment:
```bash
export ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1
./rollback-server
```

**Related Code:**
- `aiml/models/isolation_forest_cheat.py` — ML model
- `backend/src/game/mod.rs` — Integration point

---

### 2. `enable_jitter_prediction`
**Purpose:** Kalman filter latency forecasting for smoother client rendering

**When to enable:**
- Clients on high-latency networks (>100 ms RTT)
- Competitive environments where prediction matters
- When you want to minimize visible jitter

**Impact:**
- Low CPU overhead (<1% on server)
- Improves client-side frame interpolation
- Reduces perceived lag

**Configuration:**
```toml
[features]
enable_jitter_prediction = true
```

or via environment:
```bash
export ROLLBACK_ENABLE_JITTER_PREDICTION=1
./rollback-server
```

**Related Code:**
- `aiml/models/jitter_kalman_predictor.py` — Kalman filter
- `backend/src/network/clock_sync.rs` — Integration point

---

### 3. `enable_rollback_risk_metrics`
**Purpose:** Observability for rollback frequency and risk assessment

**When to enable:**
- Performance debugging
- Understanding rollback patterns
- Improving netcode tuning

**Impact:**
- Low CPU overhead (<1% on server)
- Generates detailed metrics (JSON)
- Useful for diagnostics, not critical for gameplay

**Configuration:**
```toml
[features]
enable_rollback_risk_metrics = true
```

or via environment:
```bash
export ROLLBACK_ENABLE_ROLLBACK_RISK_METRICS=1
./rollback-server
```

**Output:**
```json
{
  "timestamp": "2026-10-10T12:00:00Z",
  "session_id": "uuid-here",
  "total_rollbacks": 42,
  "avg_rollback_distance_frames": 5.2,
  "max_rollback_distance_frames": 15,
  "network_lag_percentile_99": 85.5
}
```

**Related Code:**
- `aiml/models/rollback_risk_predictor.py` — Metrics computation
- `backend/src/game/mod.rs` — Data collection point

---

### 4. `enable_matchmaking_optimizer`
**Purpose:** AI-driven player pairing for balanced matches

**When to enable:**
- Competitive matchmaking
- Tournament pairings
- When you want skill-based matching

**Impact:**
- Moderate CPU overhead (~5% on server during matchmaking)
- Improves match quality (balanced skill levels)
- Requires training data (initial matches may be random)

**Configuration:**
```toml
[features]
enable_matchmaking_optimizer = true
```

or via environment:
```bash
export ROLLBACK_ENABLE_MATCHMAKING_OPTIMIZER=1
./rollback-server
```

**Related Code:**
- `aiml/models/matchmaking_optimizer.py` — ML model
- `backend/src/matchmaking/mod.rs` — Integration point

---

## Configuration Methods

### Method 1: TOML File (Recommended for Production)

**File:** `backend/config.toml`

```toml
[features]
enable_ai_cheat_detection = false
enable_jitter_prediction = false
enable_rollback_risk_metrics = false
enable_matchmaking_optimizer = false
```

**Deployment:**
1. Edit `config.toml`
2. Commit to repository
3. Deploy with new config
4. Restart server (required; no hot-reload)

---

### Method 2: Environment Variables (Good for CI/Docker)

**Prefix:** `ROLLBACK_`

```bash
export ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1
export ROLLBACK_ENABLE_JITTER_PREDICTION=0
export ROLLBACK_ENABLE_ROLLBACK_RISK_METRICS=1
export ROLLBACK_ENABLE_MATCHMAKING_OPTIMIZER=0
```

**Docker Compose:**
```yaml
services:
  backend:
    environment:
      ROLLBACK_ENABLE_AI_CHEAT_DETECTION: "1"
      ROLLBACK_ENABLE_JITTER_PREDICTION: "0"
```

**Kubernetes:**
```yaml
env:
  - name: ROLLBACK_ENABLE_AI_CHEAT_DETECTION
    value: "1"
```

---

### Method 3: Priority (Environment Overrides TOML)

If both TOML and environment variables are set, **environment variables take precedence**:

```bash
# config.toml has: enable_ai_cheat_detection = false
# Environment has: ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1

# Result: Feature is ENABLED (env wins)
./rollback-server

# Output:
# INFO: Feature enable_ai_cheat_detection = true (via environment)
```

---

## Usage in Code

### Check Feature Flag at Runtime

```rust
// backend/src/main.rs

use rollback_netcode_server::config::load_config;

#[tokio::main]
async fn main() {
    let config = load_config(Some("config.toml")).await.unwrap();

    if config.features.enable_ai_cheat_detection {
        log::info!("AI Cheat Detection enabled");
        // Initialize ML model
        initialize_cheat_detector().await;
    }

    if config.features.enable_jitter_prediction {
        log::info!("Jitter Prediction enabled");
        // Initialize predictor
        initialize_jitter_predictor().await;
    }

    // ... rest of server setup
}
```

### Inside Game Loop

```rust
// backend/src/game/mod.rs

pub async fn run_match(mut session: Session, config: &ServerConfig) {
    loop {
        // Process inputs
        session.apply_player_inputs();

        // Check for cheating (if enabled)
        if config.features.enable_ai_cheat_detection {
            let flags = detect_anomalies(&session).await;
            if !flags.is_empty() {
                log::warn!("Suspicious behavior detected: {:?}", flags);
                // Log for admin review, don't ban automatically
            }
        }

        // Predict latency (if enabled)
        if config.features.enable_jitter_prediction {
            let lag_prediction = predict_jitter(&session).await;
            session.set_predicted_lag(lag_prediction);
        }

        // Tick simulation
        session.tick();
    }
}
```

---

## Deployment Strategy

### Phase 1: Feature Disabled (Default)
All flags are off. Server runs baseline rollback netcode.

```toml
[features]
enable_ai_cheat_detection = false
enable_jitter_prediction = false
enable_rollback_risk_metrics = false
enable_matchmaking_optimizer = false
```

### Phase 2: Feature Testing (Canary)
Deploy to 5% of servers with feature enabled:

```bash
# Deploy canary version with flag enabled
kubectl set env deployment/rollback-backend ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1 -n canary

# Monitor metrics for 1 hour
# Check CPU overhead
# Check for crashes or errors
```

### Phase 3: Feature Gradual Rollout
If tests pass, roll out to more servers:

```bash
# Week 1: 20% of servers
kubectl set env deployment/rollback-backend ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1 -n prod --proportion 0.2

# Week 2: 50% of servers
kubectl set env deployment/rollback-backend ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1 -n prod --proportion 0.5

# Week 3: 100% of servers
kubectl set env deployment/rollback-backend ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1 -n prod
```

### Phase 4: Full Deployment
All servers running with feature enabled.

---

## Monitoring & Alerts

### Logs to Watch

```
INFO: Feature enable_ai_cheat_detection = true (via TOML)
INFO: Feature enable_ai_cheat_detection = true (via environment override)
WARN: AI Cheat Detection took 150ms (exceeds 100ms threshold)
WARN: Suspicious behavior detected: [desync_spike, input_ahead_of_server]
```

### Metrics to Track

1. **Feature Overhead:**
   - CPU per enabled feature
   - Memory per model (ML models are large)
   - Network per feature

2. **Feature Effectiveness:**
   - Flagged players (cheat detection)
   - Matched pairs quality (matchmaking)
   - Predicted lag vs. actual (jitter prediction)

3. **Errors:**
   - ML model inference failures
   - Feature initialization errors
   - Timeout or resource limits

### Example Prometheus Queries

```yaml
# CPU overhead per feature
rollback_feature_cpu_usage_percent{feature="ai_cheat_detection"}

# Players flagged by cheat detection
rate(rollback_cheat_flags_total[5m])

# Jitter prediction accuracy
rollback_jitter_prediction_mse

# Rollback risk metric output rate
rate(rollback_risk_metrics_exported[5m])
```

---

## Troubleshooting

### Issue: Feature flag not taking effect

**Diagnosis:**
1. Check config file exists: `ls backend/config.toml`
2. Check environment variables: `env | grep ROLLBACK_`
3. Check server logs: `docker logs rollback-backend | grep enable_`

**Solution:**
- If using TOML, ensure `config.toml` is in working directory
- If using env, prefix must be exact: `ROLLBACK_ENABLE_AI_CHEAT_DETECTION` (case matters)
- Restart server after changing flags (no hot-reload)

### Issue: ML Model initialization fails

**Diagnosis:**
- Check logs: `Model initialization failed: path not found`
- Check model file exists: `ls aiml/models/*.pkl`

**Solution:**
- Ensure model files are deployed with server binary
- Update `backend/config.rs` to point to correct model path
- Rebuild and redeploy

### Issue: Feature overhead too high

**Diagnosis:**
- CPU usage spikes when feature enabled
- Latency increases by >10 ms

**Solution:**
- Disable feature and investigate
- Profile the code: `cargo flamegraph`
- Check if ML model is too large or slow
- Consider disabling for lower-end servers

---

## Testing

### Test: Feature Flag Can Be Enabled/Disabled

```rust
#[test]
fn test_feature_flags_default_to_false() {
    let config = ServerConfig::default();
    assert!(!config.features.enable_ai_cheat_detection);
    assert!(!config.features.enable_jitter_prediction);
    assert!(!config.features.enable_rollback_risk_metrics);
    assert!(!config.features.enable_matchmaking_optimizer);
}

#[test]
fn test_feature_flags_can_be_enabled() {
    let config = ServerConfig {
        features: FeatureFlags {
            enable_ai_cheat_detection: true,
            enable_jitter_prediction: true,
            enable_rollback_risk_metrics: true,
            enable_matchmaking_optimizer: false,
        },
        ..Default::default()
    };
    assert!(config.features.enable_ai_cheat_detection);
    assert!(config.features.enable_jitter_prediction);
    assert!(config.features.enable_rollback_risk_metrics);
    assert!(!config.features.enable_matchmaking_optimizer);
}

#[tokio::test]
async fn test_feature_flags_immutable_after_startup() {
    let config = load_config(Some("config.toml")).await.unwrap();
    // Flags are read once; cannot be changed without restart
    let config_again = load_config(Some("config.toml")).await.unwrap();
    assert_eq!(config.features.enable_ai_cheat_detection, config_again.features.enable_ai_cheat_detection);
}
```

---

## Summary

| Flag | Overhead | Use Case | Recommended |
|------|----------|----------|-------------|
| `enable_ai_cheat_detection` | ~2% CPU | Competitive/Tournament | Yes (high-stakes) |
| `enable_jitter_prediction` | ~1% CPU | High-latency networks | Yes (>100ms RTT) |
| `enable_rollback_risk_metrics` | ~1% CPU | Diagnostics/Tuning | Yes (always on) |
| `enable_matchmaking_optimizer` | ~5% CPU | Ranked matchmaking | Yes (ranked mode) |

---

## Sign-Off

- **Document Owner:** Rollback Netcode Team
- **Date:** 2026-10-10
- **Status:** Final
