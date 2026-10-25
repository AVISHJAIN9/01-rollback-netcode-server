"""
aiml/models/isolation_forest_cheat.py

Isolation Forest model for player input anomaly detection (aimbot, inhuman reaction time).
Trains on normal human player telemetry and detects statistical outliers.
"""

import time
import numpy as np
from sklearn.ensemble import IsolationForest
from sklearn.metrics import precision_score, recall_score, f1_score, roc_auc_score, confusion_matrix

class IsolationForestCheatDetector:
    def __init__(self, contamination: float = 0.10, random_state: int = 42):
        self.contamination = contamination
        self.random_state = random_state
        self.model = IsolationForest(
            n_estimators=100,
            contamination=contamination,
            random_state=random_state
        )
        self.is_trained = False

    def train_and_evaluate(self, records: list) -> dict:
        """Trains on normal records and evaluates on mixed test records."""
        X_all = np.array([
            [r.p0_reaction_time_ms, r.p0_angle_delta_rad, r.p0_input_frequency_hz, r.p0_stick_deflection]
            for r in records
        ])
        y_all = np.array([r.is_anomaly for r in records])
        
        # 60/20/20 train/val/test split
        n = len(records)
        n_train = int(n * 0.6)
        n_val = int(n * 0.2)
        
        # Train strictly on normal records in the training partition
        train_mask = (y_all[:n_train] == 0)
        X_train = X_all[:n_train][train_mask]
        
        t0 = time.perf_counter()
        self.model.fit(X_train)
        self.is_trained = True
        t_fit = time.perf_counter() - t0
        
        # Test split
        X_test = X_all[n_train + n_val:]
        y_test = y_all[n_train + n_val:]
        
        t0 = time.perf_counter()
        scores = -self.model.score_samples(X_test)
        y_pred = (self.model.predict(X_test) == -1).astype(int)
        t_infer = (time.perf_counter() - t0) / len(X_test) * 1000.0 # ms per sample
        
        p = float(precision_score(y_test, y_pred, zero_division=0))
        r = float(recall_score(y_test, y_pred, zero_division=0))
        f1 = float(f1_score(y_test, y_pred, zero_division=0))
        auc = float(roc_auc_score(y_test, scores)) if len(np.unique(y_test)) > 1 else 1.0
        cm = confusion_matrix(y_test, y_pred)
        fpr = float(cm[0][1] / (cm[0][0] + cm[0][1])) if (cm[0][0] + cm[0][1]) > 0 else 0.0
        
        return {
            "model_name": "IsolationForestCheatDetector",
            "training_time_s": float(t_fit),
            "inference_latency_ms": float(t_infer),
            "precision": p,
            "recall": r,
            "f1": f1,
            "roc_auc": auc,
            "fpr": fpr,
            "confusion_matrix": cm.tolist()
        }

    def predict_anomaly_score(self, sample: np.ndarray) -> float:
        if not self.is_trained:
            raise RuntimeError("Model must be trained before predicting.")
        return float(-self.model.score_samples(sample.reshape(1, -1))[0])
