# Incident Playbook: Game Server Outage Recovery

## 1. Trigger Conditions
- UDP Packet loss > 25% across active rooms for > 60 seconds.
- Tick loop execution time > 16.6ms (dropped server frame rate).
- Database persistence connection failure.

## 2. Mitigation Steps
1. **Graceful Degradation Activation**:
   - The engine automatically decouples database telemetry writes to an in-memory ring buffer.
   - Simulation loop continues forward uninterrupted.
2. **Traffic Re-routing**:
   - Gateway redirects new match creation to healthy standby region.
3. **Session Recovery**:
   - Disconnected clients utilize 30s reconnect tokens upon socket reconnection.
