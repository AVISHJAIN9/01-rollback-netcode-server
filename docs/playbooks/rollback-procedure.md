# Incident Playbook: Production Deployment Rollback

## 1. Criteria for Rollback
- Critical desync rate > 0.05% of active sessions after new build deployment.
- Security vulnerability identified in protocol parsing logic.

## 2. Execution (< 5 Minutes)
1. **Kubernetes Rollback**:
   ```bash
   kubectl rollout undo deployment/rollback-netcode-server -n prod
   ```
2. **AI Model Reversion**:
   ```bash
   python3 aiml/mlops/model_registry.py --rollback cheat_detector --version v1.0
   ```
3. **Verify State Agreement**:
   - Execute `make test` against staging canary.
