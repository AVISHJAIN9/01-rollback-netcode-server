# ADR 004: AI/ML Guardrails, Graceful Fallback & Kill Switch

## Status
Accepted

## Context
Machine learning models are integrated for cheat anomaly detection (Tier A) and adaptive jitter prediction (Tier A). However, ML models are probabilistic and must never compromise game correctness, introduce unbounded input lag, or false-punish legitimate players.

## Decision
1. **Graceful Degradation**: If ML inference times out or fails, the engine instantly falls back to hardcoded deterministic rule engines (EWMA for jitter, static angular velocity thresholds for cheat detection).
2. **Bounded Output Range**: Adaptive input delay adjustments are strictly bounded between 1 frame (16.6ms) and 5 frames (83.3ms), with maximum adjustment rate of 1 frame per 60 ticks.
3. **Flag, Never Auto-Ban**: Anomaly detection models output suspicion scores; players are flagged for moderation review, never automatically banned by AI.
4. **Kill Switch**: Operators can disable all ML subsystems via dynamic configuration flags with zero server downtime.

## Consequences
- Simulation correctness and user experience are guaranteed even during complete ML failure.
