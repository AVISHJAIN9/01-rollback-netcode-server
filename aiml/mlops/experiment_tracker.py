"""
aiml/mlops/experiment_tracker.py

Lightweight JSONL experiment logging and tracking for ML models,
recording hyperparameters, dataset versions, metrics, and git commit tags.
"""

import json
import time
from pathlib import Path
from typing import Dict, Any

class ExperimentTracker:
    def __init__(self, log_path: str = "aiml/mlops/experiments.jsonl"):
        self.log_path = Path(log_path)
        self.log_path.parent.mkdir(parents=True, exist_ok=True)

    def log_run(self, experiment_name: str, params: Dict[str, Any], metrics: Dict[str, Any]) -> Dict[str, Any]:
        entry = {
            "experiment_id": f"exp_{int(time.time() * 1000)}",
            "experiment_name": experiment_name,
            "timestamp": time.time(),
            "params": params,
            "metrics": metrics
        }
        with open(self.log_path, "a") as f:
            f.write(json.dumps(entry) + "\n")
        return entry

    def get_latest_run(self, experiment_name: str) -> Dict[str, Any]:
        if not self.log_path.exists():
            return {}
        runs = []
        with open(self.log_path, "r") as f:
            for line in f:
                if line.strip():
                    runs.append(json.loads(line))
        matching = [r for r in runs if r.get("experiment_name") == experiment_name]
        return matching[-1] if matching else {}
