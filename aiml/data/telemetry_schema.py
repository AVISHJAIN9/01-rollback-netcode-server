"""
aiml/data/telemetry_schema.py

Defines the versioned schema (v1.0) for network, session, and gameplay telemetry.
Provides dataset validation, completeness checking, and synthetic generation for training/eval.
"""

import json
from dataclasses import dataclass, asdict
from typing import List, Dict, Any
import numpy as np

SCHEMA_VERSION = "1.0.0"

@dataclass
class TelemetryRecord:
    session_id: str
    tick: int
    timestamp_ms: float
    rtt_ms: float
    jitter_ms: float
    packet_loss_pct: float
    rollback_depth: int
    resim_duration_us: float
    p0_stick_deflection: float
    p0_angle_delta_rad: float
    p0_input_frequency_hz: float
    p0_reaction_time_ms: float
    is_anomaly: int = 0

class TelemetryDatasetValidator:
    REQUIRED_FIELDS = [
        "session_id", "tick", "timestamp_ms", "rtt_ms", "jitter_ms",
        "packet_loss_pct", "rollback_depth", "resim_duration_us"
    ]

    @staticmethod
    def validate_records(records: List[Dict[str, Any]]) -> Dict[str, Any]:
        if not records:
            return {"valid": False, "completeness_pct": 0.0, "error": "Empty dataset"}
        
        valid_count = 0
        for r in records:
            if all(f in r and r[f] is not None for f in TelemetryDatasetValidator.REQUIRED_FIELDS):
                valid_count += 1
        
        completeness_pct = (valid_count / len(records)) * 100.0
        return {
            "schema_version": SCHEMA_VERSION,
            "total_records": len(records),
            "valid_records": valid_count,
            "completeness_pct": completeness_pct,
            "is_valid": completeness_pct >= 95.0
        }

def generate_synthetic_telemetry(n_samples: int = 2000, seed: int = 42) -> List[TelemetryRecord]:
    """Generates synthetic telemetry records labeled explicitly as synthetic data."""
    np.random.seed(seed)
    records = []
    
    for i in range(n_samples):
        # 10% injected anomaly rate
        is_cheat = 1 if (np.random.rand() < 0.10) else 0
        
        if is_cheat:
            reaction = float(np.random.normal(0.5, 0.2))
            angle_delta = float(np.random.normal(0.001, 0.0005))
            freq = float(np.random.normal(60.0, 1.0))
            stick = float(np.random.uniform(0.95, 1.0))
        else:
            reaction = float(np.random.normal(8.0, 2.5))
            angle_delta = float(np.random.normal(0.05, 0.02))
            freq = float(np.random.exponential(15.0))
            stick = float(np.random.uniform(0.0, 1.0))
            
        rtt = float(np.random.gamma(shape=4.0, scale=8.0)) # ~32ms mean RTT
        jitter = float(np.random.exponential(3.0))
        loss = float(np.random.choice([0.0, 0.5, 2.0, 5.0], p=[0.7, 0.15, 0.1, 0.05]))
        rollback = int(np.random.poisson(lam=jitter / 4.0))
        resim_time = float(rollback * 1.25)
        
        records.append(TelemetryRecord(
            session_id=f"sess_{i // 100:04d}",
            tick=i % 100,
            timestamp_ms=float(i * 16.66),
            rtt_ms=max(5.0, rtt),
            jitter_ms=max(0.1, jitter),
            packet_loss_pct=loss,
            rollback_depth=min(15, rollback),
            resim_duration_us=resim_time,
            p0_stick_deflection=max(0.0, min(1.0, stick)),
            p0_angle_delta_rad=max(0.0, angle_delta),
            p0_input_frequency_hz=max(1.0, freq),
            p0_reaction_time_ms=max(0.1, reaction),
            is_anomaly=is_cheat
        ))
        
    return records
