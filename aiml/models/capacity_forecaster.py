"""
aiml/models/capacity_forecaster.py

Autoregressive time-series model forecasting active room concurrency
and compute load across hourly/daily demand cycles.
"""

import numpy as np
from sklearn.linear_model import Ridge
from sklearn.metrics import mean_absolute_error, mean_squared_error

class CapacityForecaster:
    def __init__(self, lag_hours: int = 24):
        self.lag_hours = lag_hours
        self.model = Ridge(alpha=1.0)
        self.is_trained = False

    def generate_synthetic_load_data(self, n_hours: int = 720, seed: int = 42) -> np.ndarray:
        """Generates 30 days of hourly active room concurrency with diurnal patterns."""
        np.random.seed(seed)
        t = np.arange(n_hours)
        diurnal = 50.0 * np.sin(2 * np.pi * t / 24.0 - np.pi / 2) # Peak at evening
        weekly = 20.0 * np.sin(2 * np.pi * t / 168.0) # Weekend boost
        noise = np.random.normal(0, 5.0, n_hours)
        base = 100.0
        concurrency = np.maximum(10.0, base + diurnal + weekly + noise)
        return concurrency

    def fit(self, load_series: np.ndarray):
        X, y = [], []
        for i in range(len(load_series) - self.lag_hours):
            X.append(load_series[i:i + self.lag_hours])
            y.append(load_series[i + self.lag_hours])
        X = np.array(X)
        y = np.array(y)
        self.model.fit(X, y)
        self.is_trained = True

    def forecast_next(self, recent_history: np.ndarray) -> float:
        assert len(recent_history) >= self.lag_hours
        history_window = recent_history[-self.lag_hours:].reshape(1, -1)
        pred = self.model.predict(history_window)[0]
        return float(max(0.0, pred))

    def evaluate(self, train_series: np.ndarray, test_series: np.ndarray) -> dict:
        self.fit(train_series)
        all_series = np.concatenate([train_series[-self.lag_hours:], test_series])
        X_test, y_test = [], []
        for i in range(len(test_series)):
            X_test.append(all_series[i:i + self.lag_hours])
            y_test.append(test_series[i])
        X_test = np.array(X_test)
        y_test = np.array(y_test)
        
        preds = self.model.predict(X_test)
        mae = mean_absolute_error(y_test, preds)
        rmse = np.sqrt(mean_squared_error(y_test, preds))
        
        return {
            "model": "Autoregressive Ridge Capacity Forecaster",
            "mae_rooms": float(mae),
            "rmse_rooms": float(rmse),
            "is_within_budget": mae < 15.0 # Max error within 15 active rooms
        }
