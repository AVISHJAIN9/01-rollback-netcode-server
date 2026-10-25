"""
aiml/models/jitter_kalman_predictor.py

Adaptive 1D Kalman Filter estimating true network transit latency, jitter,
and optimal input delay frame buffer allocation.
"""

import numpy as np

class AdaptiveKalmanJitterPredictor:
    def __init__(self, process_noise: float = 1e-3, measurement_noise: float = 1e-1):
        self.x = 0.0 # Estimated jitter / latency ms
        self.p = 1.0 # Error covariance
        self.q = process_noise
        self.r = measurement_noise

    def step(self, measurement: float) -> float:
        # Prediction step
        p_pred = self.p + self.q
        
        # Measurement update
        k = p_pred / (p_pred + self.r)
        self.x = self.x + k * (measurement - self.x)
        self.p = (1 - k) * p_pred
        return self.x

    def predict_next_delay(self, jitter_ms: float) -> float:
        filtered_jitter = self.step(jitter_ms)
        # Convert jitter estimate to frame delay (at 60Hz = 16.66ms/frame)
        frames = filtered_jitter / 16.66
        return float(max(0.0, frames))
