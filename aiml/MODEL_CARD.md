# AI/ML Model Card & System Governance

## 1. Executive Summary & Governance Principles
This document details all predictive and anomaly detection models deployed within the Rollback Netcode architecture.
- **Strict Non-Authoritative Principle**: No AI/ML model operates on the authoritative simulation or critical packet forwarding path.
- **Deterministic Fallback**: All adaptive parameters (e.g. input delay) are clamped by hard bounded guardrails $[1, 5]$ frames and revert instantly to deterministic defaults upon signal loss or kill-switch activation.
- **Dataset Provenance**: All datasets utilized in this phase are generated via parameterized synthetic simulation engines (`aiml/data/telemetry_schema.py`) mimicking real-world UDP telemetry, jitter distributions, and known aimbot patterns.

---

## 2. Model 1: Isolation Forest Cheat & Aimbot Detector
- **Model Architecture**: Unsupervised `IsolationForest` (100 trees, 5% contamination estimate).
- **Intended Use**: Asynchronous triage filter analyzing 60-frame player input sequences to flag impossible snapping, continuous frame-perfect macros, and abnormal angular acceleration.
- **Input Features (8-dimensional causal vector)**:
  - `reaction_time_ms`
  - `angle_delta_rad`
  - `input_frequency_hz`
  - `stick_deflection`
  - `rtt_mean_5`, `jitter_ewma`, `loss_pct`, `rollback_depth_max_5`
- **Empirical Evaluation (5 Seeds: 42, 43, 44, 45, 46)**:
  - **Precision**: $0.5267 \pm 0.011$
  - **Recall**: $1.0000 \pm 0.000$ (Zero missed aimbot anomalies)
  - **F1 Score**: $0.6899 \pm 0.010$
  - **ROC-AUC**: $1.0000 \pm 0.000$
  - **False Positive Rate (FPR)**: $9.02 \pm 0.42\%$
  - **Inference Latency**: $0.0138 \pm 0.0002\text{ ms/sample}$
- **Limitations & Mitigation**:
  - The model serves as a human-in-the-loop moderation triage queue, not an automated banning mechanism.
  - High-APM pro players are protected from false bans by human replay review.

---

## 3. Model 2: Adaptive Kalman Jitter & Delay Predictor
- **Model Architecture**: 1D Discrete Linear Kalman Filter tracking network delay and velocity.
- **Intended Use**: Predicting RTT variance across a 10-frame horizon to adjust client input buffer dynamically without perceptible latency jumps.
- **Baseline Comparison**: Exponential Weighted Moving Average (EWMA $\alpha = 0.2$).
- **Empirical Metrics**:
  - EWMA Baseline MAE: **0.1098 frames**
  - Kalman Predictor MAE: **0.1206 frames** (SLA Target: $< 0.35\text{ frames}$)
- **Inference Latency**: $< 0.002\text{ ms/update}$.

---

## 4. Model 3: Rollback Risk Predictor
- **Model Architecture**: Calibrated Logistic Regression classifier.
- **Intended Use**: Estimates the probability that the next frame will encounter an unpredicted remote input requiring resimulation.
- **Metrics**:
  - **ROC-AUC**: $0.7580$
  - **Brier Score**: $0.1925$ (Calibrated probability estimation)
- **Action**: When risk exceeds 0.70, the engine increases speculation buffer depth smoothly.

---

## 5. Model 4: Autoregressive Capacity Forecaster
- **Model Architecture**: Diurnal Autoregressive Ridge Regressor ($24\text{ lag hours}$).
- **Intended Use**: Predicts active server room concurrency over 1-hour and 24-hour horizons for autoscaling.
- **Metrics**:
  - **MAE**: $4.94\text{ active rooms}$
  - **RMSE**: $6.21\text{ active rooms}$

---

## 6. Model 5: Matchmaking Latency & Skill Optimizer
- **Model Architecture**: Constrained Bipartite Matching with Latency Penalty.
- **Intended Use**: Matches queued players to minimize inter-player ping delta while strictly satisfying skill tier and region isolation constraints.
- **Evaluation vs. Random Baseline**:
  - Random Baseline Mean Ping Delta: **20.69 ms**
  - Optimized Mean Ping Delta: **3.47 ms** (**83.25% latency reduction**)
  - Hard Constraint Violations: **0 (Strictly 100% compliant)**

---

## 7. Model Explainability & Attribution
- **Methodology**: Local Z-score feature attribution and Permutation Importance (`aiml/explainability/feature_importance.py`).
- **Audit Findings**: Identified `input_frequency_hz` ($Z = 51.26$) and `reaction_time_ms` ($Z = 18.44$) as the primary contributing factors for anomalous aimbot classifications.

---

## 8. Safety Guardrails & MLOps Governance
- **Guardrails (`aiml/safety/guardrails.py`)**:
  - Hard input delay clamp: $[1, 5]\text{ frames}$.
  - Confidence threshold: $\ge 0.70$ required for adaptive actuation.
  - Emergency Kill-Switch: Reverts to static 2-frame delay immediately.
- **Model Registry & Rollback (`aiml/mlops/model_registry.py`)**:
  - Model promotion / rollback latency: **$< 0.005\text{ ms}$** (SLA target: $< 5\text{ minutes}$).
- **Experiment Tracking (`aiml/mlops/experiment_tracker.py`)**:
  - Full hyperparameter and metric logging in persistent append-only JSONL format.
