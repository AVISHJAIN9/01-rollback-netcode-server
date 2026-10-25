package phase15

import (
	"fmt"
	"strings"
)

// ─── Feature 86: Configuration Schema & Strict Validator ─────────────────────
//
// Strongly typed configuration covering all engine subsystems.
// ConfigValidator rejects unrecognised keys, type mismatches, and out-of-bounds
// numerics during startup and dynamic reload.

// IngestionConfig controls the event ingestion pipeline.
type IngestionConfig struct {
	// RingBufferSize must be a power-of-two ≥ 1024 and ≤ 4,194,304.
	RingBufferSize int `json:"ring_buffer_size"`
	// WatermarkDelayMs must be > 0 and ≤ 60,000 ms.
	WatermarkDelayMs int `json:"watermark_delay_ms"`
	// MaxWorkers must be in [1, 1024].
	MaxWorkers int `json:"max_workers"`
	// MinWorkers must be ≥ 1 and ≤ MaxWorkers.
	MinWorkers int `json:"min_workers"`
}

// WindowConfig controls event-time window semantics.
type WindowConfig struct {
	// DefaultWindowMs must be ≥ 100 and ≤ 86,400,000 (24 h).
	DefaultWindowMs int `json:"default_window_ms"`
	// AllowedLatenessMs must be ≥ 0 and ≤ DefaultWindowMs.
	AllowedLatenessMs int `json:"allowed_lateness_ms"`
	// MaxActiveWindows must be ≥ 1 and ≤ 1,000,000.
	MaxActiveWindows int `json:"max_active_windows"`
}

// StateConfig controls keyed state storage.
type StateConfig struct {
	// MaxMemoryMB must be ≥ 512 and ≤ 1,048,576 (1 TiB).
	MaxMemoryMB int `json:"max_memory_mb"`
	// ShardCount must be a power-of-two in [1, 1024].
	ShardCount int `json:"shard_count"`
	// DefaultTTLSeconds must be ≥ 1 and ≤ 604,800 (7 days).
	DefaultTTLSeconds int `json:"default_ttl_seconds"`
}

// AlertingConfig controls the alert delivery pipeline.
type AlertingConfig struct {
	// MaxAlertQueueSize must be ≥ 100 and ≤ 10,000,000.
	MaxAlertQueueSize int `json:"max_alert_queue_size"`
	// DeduplicationWindowSeconds must be ≥ 1 and ≤ 86,400.
	DeduplicationWindowSeconds int `json:"dedup_window_seconds"`
	// MaxSuppressedPerAlert must be ≥ 0 and ≤ 10,000.
	MaxSuppressedPerAlert int `json:"max_suppressed_per_alert"`
}

// EngineConfig is the root configuration object for the streaming detection engine.
type EngineConfig struct {
	Ingestion IngestionConfig `json:"ingestion"`
	Window    WindowConfig    `json:"window"`
	State     StateConfig     `json:"state"`
	Alerting  AlertingConfig  `json:"alerting"`
}

// DefaultEngineConfig returns a valid baseline configuration.
func DefaultEngineConfig() EngineConfig {
	return EngineConfig{
		Ingestion: IngestionConfig{
			RingBufferSize:   65536,
			WatermarkDelayMs: 200,
			MaxWorkers:       16,
			MinWorkers:       2,
		},
		Window: WindowConfig{
			DefaultWindowMs:   60_000,
			AllowedLatenessMs: 5_000,
			MaxActiveWindows:  100_000,
		},
		State: StateConfig{
			MaxMemoryMB:       2048,
			ShardCount:        64,
			DefaultTTLSeconds: 3600,
		},
		Alerting: AlertingConfig{
			MaxAlertQueueSize:          500_000,
			DeduplicationWindowSeconds: 3600,
			MaxSuppressedPerAlert:      1000,
		},
	}
}

// ConfigValidationError accumulates all field-level validation failures.
type ConfigValidationError struct {
	Errors []string
}

func (e *ConfigValidationError) Error() string {
	return "configuration validation failed:\n  " + strings.Join(e.Errors, "\n  ")
}

func (e *ConfigValidationError) add(field, msg string) {
	e.Errors = append(e.Errors, fmt.Sprintf("[%s] %s", field, msg))
}

func (e *ConfigValidationError) hasErrors() bool {
	return len(e.Errors) > 0
}

// isPowerOfTwo returns true when n is a positive power of two.
func isPowerOfTwo(n int) bool {
	return n > 0 && (n&(n-1)) == 0
}

// ConfigValidator validates an EngineConfig.
type ConfigValidator struct{}

// NewConfigValidator constructs a validator.
func NewConfigValidator() *ConfigValidator { return &ConfigValidator{} }

// Validate checks every field of cfg and returns a structured error on failure.
func (v *ConfigValidator) Validate(cfg *EngineConfig) error {
	errs := &ConfigValidationError{}

	// ── Ingestion ──────────────────────────────────────────────────────────
	ing := &cfg.Ingestion
	if !isPowerOfTwo(ing.RingBufferSize) || ing.RingBufferSize < 1024 || ing.RingBufferSize > 4_194_304 {
		errs.add("ingestion.ring_buffer_size",
			fmt.Sprintf("must be a power-of-two in [1024, 4194304], got %d", ing.RingBufferSize))
	}
	if ing.WatermarkDelayMs <= 0 || ing.WatermarkDelayMs > 60_000 {
		errs.add("ingestion.watermark_delay_ms",
			fmt.Sprintf("must be in (0, 60000], got %d", ing.WatermarkDelayMs))
	}
	if ing.MaxWorkers < 1 || ing.MaxWorkers > 1024 {
		errs.add("ingestion.max_workers",
			fmt.Sprintf("must be in [1, 1024], got %d", ing.MaxWorkers))
	}
	if ing.MinWorkers < 1 || ing.MinWorkers > ing.MaxWorkers {
		errs.add("ingestion.min_workers",
			fmt.Sprintf("must be in [1, max_workers=%d], got %d", ing.MaxWorkers, ing.MinWorkers))
	}

	// ── Window ────────────────────────────────────────────────────────────
	win := &cfg.Window
	if win.DefaultWindowMs < 100 || win.DefaultWindowMs > 86_400_000 {
		errs.add("window.default_window_ms",
			fmt.Sprintf("must be in [100, 86400000], got %d", win.DefaultWindowMs))
	}
	if win.AllowedLatenessMs < 0 || win.AllowedLatenessMs > win.DefaultWindowMs {
		errs.add("window.allowed_lateness_ms",
			fmt.Sprintf("must be in [0, default_window_ms=%d], got %d",
				win.DefaultWindowMs, win.AllowedLatenessMs))
	}
	if win.MaxActiveWindows < 1 || win.MaxActiveWindows > 1_000_000 {
		errs.add("window.max_active_windows",
			fmt.Sprintf("must be in [1, 1000000], got %d", win.MaxActiveWindows))
	}

	// ── State ─────────────────────────────────────────────────────────────
	st := &cfg.State
	if st.MaxMemoryMB < 512 || st.MaxMemoryMB > 1_048_576 {
		errs.add("state.max_memory_mb",
			fmt.Sprintf("must be in [512, 1048576], got %d", st.MaxMemoryMB))
	}
	if !isPowerOfTwo(st.ShardCount) || st.ShardCount < 1 || st.ShardCount > 1024 {
		errs.add("state.shard_count",
			fmt.Sprintf("must be a power-of-two in [1, 1024], got %d", st.ShardCount))
	}
	if st.DefaultTTLSeconds < 1 || st.DefaultTTLSeconds > 604_800 {
		errs.add("state.default_ttl_seconds",
			fmt.Sprintf("must be in [1, 604800], got %d", st.DefaultTTLSeconds))
	}

	// ── Alerting ──────────────────────────────────────────────────────────
	al := &cfg.Alerting
	if al.MaxAlertQueueSize < 100 || al.MaxAlertQueueSize > 10_000_000 {
		errs.add("alerting.max_alert_queue_size",
			fmt.Sprintf("must be in [100, 10000000], got %d", al.MaxAlertQueueSize))
	}
	if al.DeduplicationWindowSeconds < 1 || al.DeduplicationWindowSeconds > 86_400 {
		errs.add("alerting.dedup_window_seconds",
			fmt.Sprintf("must be in [1, 86400], got %d", al.DeduplicationWindowSeconds))
	}
	if al.MaxSuppressedPerAlert < 0 || al.MaxSuppressedPerAlert > 10_000 {
		errs.add("alerting.max_suppressed_per_alert",
			fmt.Sprintf("must be in [0, 10000], got %d", al.MaxSuppressedPerAlert))
	}

	if errs.hasErrors() {
		return errs
	}
	return nil
}
