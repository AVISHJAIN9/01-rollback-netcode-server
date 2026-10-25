# ADR-009: ML Models as Advisory-Only (No Auto-Action)

**Date:** 2026-10-11  
**Status:** ACCEPTED  
**Context:** F4, D5 — ML pipeline and anti-cheat  
**Decision:** ML predictions inform admins but never trigger automatic bans, kicks, or score resets

## Problem

ML models (anomaly detection, jitter prediction, rollback risk) can flag suspicious patterns. But ML is probabilistic:

1. False positives: Legitimate spike in latency flagged as "degradation"
2. False negatives: Real cheater not detected
3. Model drift: Training data from Month 1 doesn't apply to Month 6

Using ML to auto-ban players is dangerous and player-hostile.

## Decision

Three-tier system:

1. **Passive monitoring:** ML models compute scores (confidence 0–100) but never trigger actions
2. **Admin review:** Scores above threshold (e.g., 75) appear in admin dashboard as "Review Incident"
3. **Explicit action:** Admin clicks "Confirm Ban" → audit log recorded → player banned (not auto)

**Models in scope:**

- Anomaly detection (input/state/network patterns): flags unusual behavior
- Jitter predictor: predicts latency degradation; used to adjust adaptive delay
- Rollback risk: estimates session churn risk; used to prioritize matchmaking
- Capacity forecaster: predicts server load; triggers autoscaling alerts (not auto-scale)

**No auto-actions:**

- No automatic bans
- No automatic score adjustments
- No automatic disconnects
- No automatic incident creation (ML creates "alerts", humans create "incidents")

## Rationale

- **Player trust:** Players accept suspension if admins explain it; they reject unexplained auto-bans
- **Legal/compliance:** Auto-bans without human review expose to legal challenge
- **Model improvement:** Admins provide feedback (false positive/negative) to retrain models
- **Gradual rollout:** Start advisory; if model proven stable over 6 months, consider auto-action (Phase 3 ⏳)

## Consequences

- Requires admin interface (`/admin/incidents`) with dashboard and review workflow
- Model retraining loop: feedback from admins → retrain quarterly
- Latency impact: Low (scores computed async, not on hot path)

## Implemented In Code

**⏳** ML models: `backend/src/aiml/models/*.py` — pending F4

**⏳** Advisory wrapper: `backend/src/admin/mod.rs::create_incident_from_ml()` — pending D5

**⏳** Admin review: `backend/src/admin/mod.rs::review_incident()` — pending F2

**⏳** Audit trail: `schema.sql` — `incidents` table with state = (pending, approved, dismissed)

## References

- F4: ML pipeline (data, models, safety)
- D5: Anti-cheat (audit trail, no auto-ban)
- F2: Admin APIs
- Phase 3: Consider auto-action after 6 months of stable operation
