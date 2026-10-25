"""
aiml/models/rollback_risk_predictor.py

Predicts the probability of severe rollback divergence (depth >= 3)
given real-time network conditions (RTT, jitter, packet loss).
"""

import numpy as np
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import roc_auc_score, brier_score_loss

class RollbackRiskPredictor:
    def __init__(self):
        self.model = LogisticRegression(class_weight='balanced')
        self.is_trained = False

    def fit(self, X: np.ndarray, rollback_depths: np.ndarray):
        # Target: binary indicator of severe rollback (depth >= 3)
        y_severe = (rollback_depths >= 3).astype(int)
        # Features: RTT, jitter, loss
        features = X[:, [0, 1, 2]]
        self.model.fit(features, y_severe)
        self.is_trained = True

    def predict_risk(self, rtt: float, jitter: float, loss: float) -> float:
        if not self.is_trained:
            # Fallback heuristic
            return min(1.0, (jitter * 0.1) + (loss * 0.05))
        prob = self.model.predict_proba([[rtt, jitter, loss]])[0, 1]
        return float(prob)

    def evaluate(self, X_test: np.ndarray, rollback_depths_test: np.ndarray) -> dict:
        y_true = (rollback_depths_test >= 3).astype(int)
        features = X_test[:, [0, 1, 2]]
        probs = self.model.predict_proba(features)[:, 1]
        auc = roc_auc_score(y_true, probs)
        brier = brier_score_loss(y_true, probs)
        return {
            "model": "Logistic Regression Rollback Risk Predictor",
            "roc_auc": float(auc),
            "brier_score": float(brier),
            "is_calibrated": brier < 0.25
        }
