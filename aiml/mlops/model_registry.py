import time
from typing import Any, Dict, Optional

class ModelRegistry:
    """Manages model versions, metadata, staged promotions, and instant rollbacks (<5 minutes SLA)."""
    def __init__(self):
        self.models: Dict[str, Dict[str, Any]] = {}
        self.active_versions: Dict[str, str] = {}
        self.audit_log = []

    def register_model(self, model_name: str, version: str, model_artifact: Any, metadata: Optional[Dict] = None):
        if model_name not in self.models:
            self.models[model_name] = {}
        self.models[model_name][version] = {
            "artifact": model_artifact,
            "registered_at": time.time(),
            "metadata": metadata or {}
        }
        self.active_versions[model_name] = version
        self.audit_log.append({
            "action": "register",
            "model": model_name,
            "version": version,
            "timestamp": time.time()
        })

    def promote_model(self, model_name: str, version: str) -> bool:
        if model_name in self.models and version in self.models[model_name]:
            self.active_versions[model_name] = version
            self.audit_log.append({
                "action": "promote",
                "model": model_name,
                "version": version,
                "timestamp": time.time()
            })
            return True
        return False

    def rollback_model(self, model_name: str, target_version: str) -> bool:
        if model_name in self.models and target_version in self.models[model_name]:
            self.active_versions[model_name] = target_version
            self.audit_log.append({
                "action": "rollback",
                "model": model_name,
                "version": target_version,
                "timestamp": time.time()
            })
            return True
        return False

    def get_active_model(self, model_name: str) -> Optional[Any]:
        if model_name in self.active_versions:
            ver = self.active_versions[model_name]
            return self.models[model_name][ver]["artifact"]
        return None
