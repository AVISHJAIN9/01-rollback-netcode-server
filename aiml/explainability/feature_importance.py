"""
aiml/explainability/feature_importance.py

Computes permutation feature importance and local anomaly attribution
to provide human moderators with clear explanations for cheat flag triggers.
"""

from typing import List, Dict, Any
import numpy as np

class AnomalyExplainer:
    def __init__(self, model, feature_names: List[str]):
        self.model = model
        self.feature_names = feature_names

    def explain_instance(self, sample_features: np.ndarray, reference_normal: np.ndarray) -> Dict[str, Any]:
        """
        Computes z-score distance against normal distribution to identify
        which specific feature caused the anomaly trigger.
        """
        mean = np.mean(reference_normal, axis=0)
        std = np.std(reference_normal, axis=0) + 1e-6
        z_scores = (sample_features - mean) / std
        
        attributions = {}
        for idx, name in enumerate(self.feature_names):
            attributions[name] = float(z_scores[idx])
            
        top_contributor = max(attributions.keys(), key=lambda k: abs(attributions[k]))
        return {
            "top_contributing_feature": top_contributor,
            "top_z_score": float(attributions[top_contributor]),
            "feature_attributions": attributions,
            "explanation": f"Flagged primarily due to abnormal {top_contributor} (z-score: {attributions[top_contributor]:.2f})"
        }

    def compute_global_permutation_importance(self, X_val: np.ndarray, y_val: np.ndarray, n_repeats: int = 5) -> Dict[str, float]:
        """Calculates global permutation feature importance."""
        base_score = float(np.mean(self.model.score_samples(X_val)))
        importances = {}
        
        for idx, name in enumerate(self.feature_names):
            drop_scores = []
            for _ in range(n_repeats):
                X_perm = np.array(X_val)
                np.random.shuffle(X_perm[:, idx])
                perm_score = float(np.mean(self.model.score_samples(X_perm)))
                drop_scores.append(abs(base_score - perm_score))
            importances[name] = float(np.mean(drop_scores))
            
        return importances
