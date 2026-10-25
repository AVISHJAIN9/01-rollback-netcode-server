// Package phase16 implements Features 91–96: Hot Reload Recovery & Backpressure Runtime.
package phase16

import (
	"errors"
	"fmt"
	"runtime/debug"
	"time"
)

// ─── Feature 91: Canary Sandbox & Hot-Reload Failure Recovery ────────────────
//
// ReloadSandbox runs candidate rule validation in a panic-isolated goroutine.
// If the candidate panics, times out, or produces validation errors, the sandbox
// rejects the reload and the caller retains the previously active ruleset.

// CanonicalEvent is a minimal normalised event used for sandbox testing.
type CanonicalEvent struct {
	EventID     string
	TimestampNs int64
	EntityID    string
	EventType   string
	Fields      map[string]string
}

// RuleEvalFn is a function that evaluates a rule against an event.
// Returns true when the rule fires.
type RuleEvalFn func(event *CanonicalEvent) bool

// CandidateRuleSet represents a batch of candidate rules to be reloaded.
type CandidateRuleSet struct {
	Version   int
	RuleID    string
	EvalFns   []RuleEvalFn
	// CompilePanic is set to true in tests to simulate a rule that panics during eval.
	CompilePanic bool
}

// SandboxError describes a reload rejection with diagnostic context.
type SandboxError struct {
	RuleSetVersion int
	Cause          string
	PanicDetails   string
}

func (e *SandboxError) Error() string {
	if e.PanicDetails != "" {
		return fmt.Sprintf("sandbox rejected ruleset v%d: %s (panic: %s)",
			e.RuleSetVersion, e.Cause, e.PanicDetails)
	}
	return fmt.Sprintf("sandbox rejected ruleset v%d: %s", e.RuleSetVersion, e.Cause)
}

// SandboxConfig controls sandbox execution parameters.
type SandboxConfig struct {
	// TimeoutMs is the maximum milliseconds to run sandbox evaluation.
	TimeoutMs int64
}

// DefaultSandboxConfig returns safe defaults.
func DefaultSandboxConfig() SandboxConfig {
	return SandboxConfig{TimeoutMs: 500}
}

// ReloadSandbox isolates candidate rule evaluation to protect the live ruleset.
type ReloadSandbox struct {
	cfg             SandboxConfig
	activeRuleSetID int // ID of the currently active (safe) ruleset
}

// NewReloadSandbox constructs a sandbox with the given configuration.
func NewReloadSandbox(cfg SandboxConfig) *ReloadSandbox {
	return &ReloadSandbox{cfg: cfg, activeRuleSetID: 0}
}

// ActiveRuleSetVersion returns the ID of the currently validated active ruleset.
func (s *ReloadSandbox) ActiveRuleSetVersion() int { return s.activeRuleSetID }

// ValidateCandidate runs the candidate ruleset against the synthetic corpus in an
// isolated goroutine with a panic-recovery wrapper and timeout.
// On success the candidate is promoted; on failure the active ID is unchanged.
func (s *ReloadSandbox) ValidateCandidate(
	candidate *CandidateRuleSet,
	syntheticCorpus []*CanonicalEvent,
) error {
	type result struct {
		err         error
		panicDetail string
	}

	resultCh := make(chan result, 1)

	go func() {
		var res result
		defer func() {
			if r := recover(); r != nil {
				res.panicDetail = fmt.Sprintf("%v\n%s", r, debug.Stack())
				res.err = errors.New("rule evaluation panicked")
			}
			resultCh <- res
		}()

		// Simulate compile-time panic injection
		if candidate.CompilePanic {
			panic("simulated compile-time panic: rule AST is malformed")
		}

		// Run each eval function against each corpus event
		for _, evalFn := range candidate.EvalFns {
			for _, event := range syntheticCorpus {
				_ = evalFn(event) // result is not used in sandbox; we care about panics/errors
			}
		}
	}()

	timeout := time.Duration(s.cfg.TimeoutMs) * time.Millisecond
	select {
	case res := <-resultCh:
		if res.err != nil {
			return &SandboxError{
				RuleSetVersion: candidate.Version,
				Cause:          res.err.Error(),
				PanicDetails:   res.panicDetail,
			}
		}
		// Promote candidate to active
		s.activeRuleSetID = candidate.Version
		return nil

	case <-time.After(timeout):
		return &SandboxError{
			RuleSetVersion: candidate.Version,
			Cause:          fmt.Sprintf("sandbox timed out after %dms", s.cfg.TimeoutMs),
		}
	}
}
