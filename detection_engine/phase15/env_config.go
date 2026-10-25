package phase15

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

// ─── Feature 87: Multi-Tier Environment Configuration Cascader ────────────────
//
// Precedence (lowest → highest):
//   1. Compiled Defaults (DefaultEngineConfig)
//   2. Base YAML file values (represented here as ConfigOverride map)
//   3. Environment variables (${VAR_NAME:-default} interpolation)
//   4. CLI flag overrides
//
// All tiers produce a flat map[string]string of dotted key paths which are
// then applied over the EngineConfig struct in order.

// ConfigOverride is a flat map of dotted-path keys to string values.
// Example: {"ingestion.max_workers": "16", "state.max_memory_mb": "4096"}
type ConfigOverride map[string]string

// EnvironmentConfigResolver resolves the layered configuration.
type EnvironmentConfigResolver struct {
	baseFile ConfigOverride // simulates YAML file tier
	envPrefix string        // e.g. "STREAM_ENGINE" → env var "STREAM_ENGINE_INGESTION_MAX_WORKERS"
}

// NewEnvironmentConfigResolver constructs a resolver with the given base-file overrides and env prefix.
func NewEnvironmentConfigResolver(baseFile ConfigOverride, envPrefix string) *EnvironmentConfigResolver {
	return &EnvironmentConfigResolver{
		baseFile:  baseFile,
		envPrefix: envPrefix,
	}
}

// interpolateEnvVars replaces ${VAR_NAME} and ${VAR_NAME:-default} tokens in s.
func interpolateEnvVars(s string) string {
	for {
		start := strings.Index(s, "${")
		if start < 0 {
			break
		}
		end := strings.Index(s[start:], "}")
		if end < 0 {
			break
		}
		end += start

		expr := s[start+2 : end] // e.g., "VAR_NAME:-fallback"
		varName := expr
		defaultVal := ""

		if idx := strings.Index(expr, ":-"); idx >= 0 {
			varName = expr[:idx]
			defaultVal = expr[idx+2:]
		}

		val := os.Getenv(varName)
		if val == "" {
			val = defaultVal
		}

		s = s[:start] + val + s[end+1:]
	}
	return s
}

// Resolve applies all configuration tiers in order and returns a validated EngineConfig.
// cliOverrides is the highest-precedence tier (CLI flags).
func (r *EnvironmentConfigResolver) Resolve(cliOverrides ConfigOverride) (EngineConfig, error) {
	// Start with defaults
	cfg := DefaultEngineConfig()

	// Merge base file tier
	if err := applyOverrides(&cfg, r.baseFile); err != nil {
		return cfg, fmt.Errorf("base file config: %w", err)
	}

	// Build env-variable tier: scan all known config keys for matching env vars
	envOverrides := r.collectEnvOverrides()
	if err := applyOverrides(&cfg, envOverrides); err != nil {
		return cfg, fmt.Errorf("env variable config: %w", err)
	}

	// Merge CLI overrides (highest precedence)
	if err := applyOverrides(&cfg, cliOverrides); err != nil {
		return cfg, fmt.Errorf("CLI override config: %w", err)
	}

	// Validate final resolved config
	v := NewConfigValidator()
	if err := v.Validate(&cfg); err != nil {
		return cfg, fmt.Errorf("resolved config invalid: %w", err)
	}

	return cfg, nil
}

// collectEnvOverrides translates environment variables to dotted-path overrides.
// Convention: STREAM_ENGINE_INGESTION_MAX_WORKERS → ingestion.max_workers
func (r *EnvironmentConfigResolver) collectEnvOverrides() ConfigOverride {
	// Full mapping of environment variable suffixes → config paths
	mapping := map[string]string{
		"INGESTION_RING_BUFFER_SIZE":   "ingestion.ring_buffer_size",
		"INGESTION_WATERMARK_DELAY_MS": "ingestion.watermark_delay_ms",
		"INGESTION_MAX_WORKERS":        "ingestion.max_workers",
		"INGESTION_MIN_WORKERS":        "ingestion.min_workers",
		"WINDOW_DEFAULT_WINDOW_MS":     "window.default_window_ms",
		"WINDOW_ALLOWED_LATENESS_MS":   "window.allowed_lateness_ms",
		"WINDOW_MAX_ACTIVE_WINDOWS":    "window.max_active_windows",
		"STATE_MAX_MEMORY_MB":          "state.max_memory_mb",
		"STATE_SHARD_COUNT":            "state.shard_count",
		"STATE_DEFAULT_TTL_SECONDS":    "state.default_ttl_seconds",
		"ALERTING_MAX_QUEUE_SIZE":      "alerting.max_alert_queue_size",
		"ALERTING_DEDUP_WINDOW_SEC":    "alerting.dedup_window_seconds",
		"ALERTING_MAX_SUPPRESSED":      "alerting.max_suppressed_per_alert",
	}

	overrides := make(ConfigOverride)
	prefix := r.envPrefix + "_"

	for envSuffix, cfgPath := range mapping {
		envKey := prefix + envSuffix
		raw := os.Getenv(envKey)
		// Also support ${ENV_KEY:-default} syntax inside values
		if raw == "" {
			continue
		}
		overrides[cfgPath] = interpolateEnvVars(raw)
	}

	return overrides
}

// applyOverrides applies a flat map of dotted-path overrides onto cfg.
func applyOverrides(cfg *EngineConfig, overrides ConfigOverride) error {
	for path, rawVal := range overrides {
		val := interpolateEnvVars(rawVal)
		if err := setConfigPath(cfg, path, val); err != nil {
			return fmt.Errorf("set %s=%s: %w", path, val, err)
		}
	}
	return nil
}

// setConfigPath parses a dotted key path and sets the corresponding field.
func setConfigPath(cfg *EngineConfig, path, val string) error {
	n, err := strconv.Atoi(val)
	if err != nil {
		return fmt.Errorf("non-integer value %q for path %s", val, path)
	}

	switch path {
	// Ingestion
	case "ingestion.ring_buffer_size":
		cfg.Ingestion.RingBufferSize = n
	case "ingestion.watermark_delay_ms":
		cfg.Ingestion.WatermarkDelayMs = n
	case "ingestion.max_workers":
		cfg.Ingestion.MaxWorkers = n
	case "ingestion.min_workers":
		cfg.Ingestion.MinWorkers = n
	// Window
	case "window.default_window_ms":
		cfg.Window.DefaultWindowMs = n
	case "window.allowed_lateness_ms":
		cfg.Window.AllowedLatenessMs = n
	case "window.max_active_windows":
		cfg.Window.MaxActiveWindows = n
	// State
	case "state.max_memory_mb":
		cfg.State.MaxMemoryMB = n
	case "state.shard_count":
		cfg.State.ShardCount = n
	case "state.default_ttl_seconds":
		cfg.State.DefaultTTLSeconds = n
	// Alerting
	case "alerting.max_alert_queue_size":
		cfg.Alerting.MaxAlertQueueSize = n
	case "alerting.dedup_window_seconds":
		cfg.Alerting.DeduplicationWindowSeconds = n
	case "alerting.max_suppressed_per_alert":
		cfg.Alerting.MaxSuppressedPerAlert = n
	default:
		return fmt.Errorf("unknown configuration path: %s", path)
	}

	return nil
}
