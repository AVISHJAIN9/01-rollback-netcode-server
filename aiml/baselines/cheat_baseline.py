import numpy as np

class RuleBasedCheatBaseline:
    """Baseline rule engine detecting superhuman angular velocity and instant snapping."""
    def __init__(self, max_angular_velocity: float = 720.0):
        self.max_angular_velocity = max_angular_velocity

    def predict(self, input_features: np.ndarray) -> np.ndarray:
        """Returns boolean anomaly mask based on fixed threshold."""
        # Feature index 2 is angular velocity in deg/sec
        return (input_features[:, 2] > self.max_angular_velocity).astype(int)
