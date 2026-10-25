import numpy as np

class EWMARttBaseline:
    """Exponential Weighted Moving Average baseline for network RTT smoothing."""
    def __init__(self, alpha: float = 0.2):
        self.alpha = alpha
        self.smoothed_rtt = None

    def update(self, rtt: float) -> float:
        if self.smoothed_rtt is None:
            self.smoothed_rtt = rtt
        else:
            self.smoothed_rtt = self.alpha * rtt + (1 - self.alpha) * self.smoothed_rtt
        return self.smoothed_rtt

class EWMAPredictor:
    """EWMA jitter and frame delay predictor baseline."""
    def __init__(self, alpha: float = 0.2, frame_time_ms: float = 16.66):
        self.alpha = alpha
        self.frame_time_ms = frame_time_ms
        self.smoothed_jitter = None

    def predict(self, current_jitter_ms: float) -> float:
        if self.smoothed_jitter is None:
            self.smoothed_jitter = current_jitter_ms
        else:
            self.smoothed_jitter = self.alpha * current_jitter_ms + (1.0 - self.alpha) * self.smoothed_jitter
        # Return predicted delay in frames
        return max(0.0, self.smoothed_jitter / self.frame_time_ms)
