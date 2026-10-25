# ADR-004: Deterministic Tick Rate with 60 Hz Default

**Status:** Accepted  
**Date:** 2026-10-10  
**Participants:** Rollback Netcode Team

---

## Context

The rollback netcode engine requires a fixed timestep for:
- **Deterministic replay:** All players run the same simulation on the same inputs
- **Rollback resimulation:** Must execute the same number of frames to reach target tick
- **Desync detection:** Checksums computed at known tick intervals

Question: Should tick rate be:
- **Option A:** Fixed at compile-time (e.g., 60 Hz always)
- **Option B:** Configurable at runtime (e.g., 30, 60, 120 Hz)
- **Option C:** Adaptive based on network conditions (variable frame rate)

---

## Decision

**Fixed tick rate of 60 Hz (16.67 ms per frame)**, configurable at startup but not adaptive.

---

## Rationale

### Deterministic Replay
- All players must run identical simulation for identical inputs
- If tick rate varies, replay at runtime gets wrong frame count
- Fixed rate ensures 1 input = 1 frame deterministically

### Simple Rollback Semantics
- "Rollback to frame N" means resimulate frames [N, current]
- With variable tick rate, frame count is ambiguous
- Fixed rate makes resimulation unambiguous and predictable

### Testing & Verification
- Tests can predict frame count: `for i in 0..100 { tick() }` = exactly 100 frames
- Determinism tests compare frame-by-frame checksums
- Easier to verify correctness with fixed assumptions

### Configuration Trade-off
- 60 Hz is a sweet spot:
  - Low enough for older hardware (16.67 ms is gentle)
  - High enough for smooth 60 FPS client rendering
  - Network overhead manageable (60 inputs/sec)

---

## Consequences

### Positive
- Clear semantics for all players (everyone on tick 0, 1, 2, ...)
- Rollback logic is straightforward and predictable
- Testing is deterministic and reproducible

### Negative
- Clients with high jitter may lag behind (if 60 Hz is too fast for their network)
- Cannot adapt to poor network conditions in real-time
- 30 Hz players on same server as 60 Hz players = mismatch issues

### Neutral
- Requires client to implement frame interpolation (visual smoothness)
- If client can't keep up, they get jitter (not visible rollback issues, just lag)

---

## Implementation

### Tick Loop

```rust
// backend/src/game/mod.rs

pub struct GameServer {
    tick_rate: u32,  // Hz (e.g., 60)
    current_tick: u64,
}

impl GameServer {
    pub async fn run_game_loop(&mut self) {
        let frame_time_ms = 1000.0 / self.tick_rate as f64;  // 16.67 ms for 60 Hz
        let mut last_tick_time = std::time::Instant::now();

        loop {
            let elapsed = last_tick_time.elapsed().as_secs_f64() * 1000.0;
            
            if elapsed >= frame_time_ms {
                self.tick();
                self.current_tick += 1;
                last_tick_time = std::time::Instant::now();
            }
            
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    fn tick(&mut self) {
        // Deterministic: given same inputs, always produces same state
        for player in &mut self.players {
            let input = player.current_input;
            player.apply_input(input);
        }
        self.update_physics();
        self.compute_checksum();
    }
}
```

### Config

```toml
[server]
tick_rate = 60  # Hz (60 = 16.67 ms per frame)
```

### Usage

```rust
// Deterministic: tick 100 times
for _ in 0..100 {
    game.tick();
}
// Exactly 100 frames executed, no variance
```

---

## Alternatives Considered

### 1. Variable Tick Rate (Adaptive)
- **Approach:** Speed up or slow down based on client round-trip time
- **Rejected:** Breaks determinism; frame count is ambiguous across clients

### 2. Client-Side Prediction with Server Correction
- **Approach:** Client ticks at 60 Hz locally, server ticks at 30 Hz
- **Rejected:** Adds complexity; still need fixed server tick for replay

### 3. Multiple Servers at Different Tick Rates
- **Approach:** 30 Hz server, 60 Hz server; clients choose
- **Rejected:** Fragmentation; cross-server play impossible

---

## Network Implications

### Input Sampling
- Client reads input from OS every frame (60 Hz = 16.67 ms)
- If client frame rate is 120 FPS, interpolates between inputs
- Server always receives at 60 Hz (or less if packet loss)

### Reconnect Handling
- Reconnecting client is told: "You're at tick 1000"
- Client fast-forwards local simulation to tick 1000 + apply new inputs
- Fast-forward is deterministic because tick rate is fixed

### Jitter Prediction (Future)
- **ADR deferred to Phase 2:** ML model predicts client lag
- With fixed 60 Hz, prediction input is consistent (lag relative to frame time)

---

## Testing

### Test: Fixed Tick Count
```rust
#[tokio::test]
async fn test_100_ticks_exactly_100_frames() {
    let mut game = GameServer::new(60);  // 60 Hz
    
    for _ in 0..100 {
        game.tick();
    }
    
    assert_eq!(game.current_tick, 100);
    // Determinism: if we run again with same inputs, same checksum
}
```

### Test: Replay Determinism
```rust
#[tokio::test]
async fn test_replay_tick_50_to_100_reproduces_state() {
    let mut game1 = GameServer::new(60);
    let mut game2 = GameServer::new(60);
    
    // game1: tick 0-100
    for _ in 0..100 {
        game1.tick();
    }
    let state_at_100 = game1.get_state();
    
    // game2: tick 0-50, then reset to 50, then tick 50-100
    for _ in 0..50 {
        game2.tick();
    }
    game2.rollback_to_tick(50);
    for _ in 0..50 {
        game2.tick();
    }
    let state_after_replay = game2.get_state();
    
    assert_eq!(state_at_100, state_after_replay);
    // Determinism verified
}
```

---

## Monitoring

Operations team should:
1. Log tick rate at server startup
2. Monitor actual frame time (should be ~16.67 ms for 60 Hz)
3. Alert if frame time exceeds threshold (e.g., > 20 ms = server overloaded)

---

## Related ADRs

- **ADR-001:** In-Memory vs. Postgres Session Repository (testing simplicity)
- **ADR-005:** Separate Connections for Multi-Instance Reads

---

## Future Considerations

- **Phase 2:** Jitter prediction to help high-latency clients
- **Phase 2:** Optional variable tick rate for specific match types (if needed)
- **Phase 3:** Dynamic tick adjustment based on server CPU load (conservative)

---

## Sign-Off

- **Approved by:** Rollback Netcode Team
- **Date:** 2026-10-10
