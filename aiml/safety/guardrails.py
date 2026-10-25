class SafetyGuardrails:
    """Enforces hard bounds, deterministic fallback, confidence gating, and kill-switch logic for AI models."""
    def __init__(self, min_delay: int = 1, max_delay: int = 5, default_delay: int = 2, min_confidence: float = 0.70):
        self.kill_switch_active = False
        self.min_delay_frames = min_delay
        self.max_delay_frames = max_delay
        self.default_delay = default_delay
        self.min_confidence = min_confidence
        self.max_rate_change_per_min = 1

    def trigger_kill_switch(self):
        self.kill_switch_active = True

    def reset_kill_switch(self):
        self.kill_switch_active = False

    def apply_guardrails(self, proposed_delay: float, confidence: float = 1.0) -> int:
        if self.kill_switch_active or proposed_delay is None:
            return self.default_delay
        
        # If model confidence is low, fall back to default
        if confidence < self.min_confidence:
            return self.default_delay

        # Clamp within hard safety envelope [min_delay, max_delay] frames
        bounded = max(self.min_delay_frames, min(self.max_delay_frames, int(round(proposed_delay))))
        return bounded

    def sanitize_input_delay(self, model_proposed_delay: float, current_delay: int, confidence: float = 1.0) -> int:
        if self.kill_switch_active or model_proposed_delay is None or confidence < self.min_confidence:
            return self.default_delay
        
        bounded = max(self.min_delay_frames, min(self.max_delay_frames, int(round(model_proposed_delay))))
        
        if abs(bounded - current_delay) > self.max_rate_change_per_min:
            return current_delay + (1 if bounded > current_delay else -1)
        
        return bounded

# Alias for backwards compatibility
MLSafetyGuardrails = SafetyGuardrails
