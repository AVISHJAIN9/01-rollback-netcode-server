# AIML Subsystem: Rollback Netcode Server

## Modules
1. **Tier A: Isolation Forest Cheat Detector (`models/isolation_forest_cheat.py`)**
   - Detects input macros, unnatural angular acceleration, and aimbot snapping.
   - Evaluated against a hardcoded fixed-threshold rule engine baseline.
2. **Tier A: Adaptive Kalman Jitter Predictor (`models/jitter_kalman_predictor.py`)**
   - Forecasts upcoming network jitter spikes to adjust dynamic client input delay.
   - Evaluated against an Exponential Weighted Moving Average (EWMA) baseline.
3. **Tier B: LLM Desync Explainer (`MODEL_CARD.md`)**
   - Parses binary state differential dumps and provides natural language root-cause analysis.
   - LLM is strictly isolated from the authoritative game loop.

## Setup & Running Evaluations
```bash
pip install -r requirements.txt
python eval/evaluate_cheat_detection.py
python eval/evaluate_jitter_predictor.py
```
