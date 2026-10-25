# C1 Lobby Implementation Plan

## Overview
Implement a lobby state machine that supports create, join, leave, ready, auto-start, and concurrent joins. The lobby is a transient in-memory registry with per-player ready flags, concurrent-safe mutation via Arc<RwLock<>>, and auto-start via explicit tick-based timeout checks (no tokio tasks to minimize dependencies).

**Target:** backend/src/session/lobby.rs (new file) + backend/tests/lobby_test.rs (new file), 8 new tests
**Existing tests:** must keep passing (56 pass)
**Total after:** 64 pass (56 + 8)
**Single commit:** "C1: Lobby implementation (create, join, leave, ready, auto-start, concurrent joins, 8 tests)"

---

## Key Design Decisions

### 1. Lobby vs Session Relationship
- **Lobby:** Per-player state (ready flags, join timestamp), auto-start timeout, temporary in-memory structure
- **Session:** Long-lived session after match starts, tracks tick_count, heartbeat, state machine transitions
- **Transition:** When all players ready (or timeout fires), Lobby transitions to MatchReady → Session::Ready
- Lobby is scoped to Lobby state; once Ready, Lobby is no longer accessed

### 2. Per-Player Ready Tracking
- Add a new type `ReadyState` enum: `NotReady`, `Ready`
- Store per-player ready flags in a HashMap<PlayerId, ReadyState> inside Lobby
- `set_ready(player_id)` toggles a player's ready state
- `all_ready()` checks if every player is Ready

### 3. Auto-Start Timeout
- **No tokio::spawn** — implement as explicit `tick_based_timeout` with a counter
- Lobby stores `created_at` (DateTime<Utc>), `auto_start_after_secs` (u64, e.g., 60)
- Method `check_auto_start_timeout()` returns bool: if now - created_at >= auto_start_after_secs, trigger auto-start
- Tests mock time via hardcoded timestamps; no dependency on time-warp crates

### 4. Concurrent Joins
- **LobbyRegistry:** Arc<RwLock<HashMap<LobbyId, Lobby>>>
- Multiple players can concurrently call `join(lobby_id, player_id)` → write lock acquired per join
- No data races: RwLock serializes join attempts
- Max 4 players per lobby (hardcoded; config-driven later in C5/CI)
- Duplicate joins are idempotent (second join returns Ok with no change)

### 5. Cleanup
- `leave()` removes player from ready state and players list
- If lobby becomes empty (all players left), mark as "empty" or store tombstone
- No automatic deletion; C3 (orphan cleanup task) handles cleanup

### 6. No Hot-Reload / Auth Wiring
- Lobby module is pure state logic
- No config.toml reading (that's for session creation, in C5)
- No auth handshake (D1 wires auth into connection protocol later)

---

## Exact Struct & Enum Definitions

### In `backend/src/session/lobby.rs`:

```rust
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::session::{PlayerId, SessionError};

/// Unique identifier for a lobby
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LobbyId(pub String);

impl LobbyId {
    pub fn generate() -> Self {
        LobbyId(format!("LOBBY-{}", Uuid::new_v4()))
    }
}

/// Per-player ready state
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadyState {
    NotReady,
    Ready,
}

/// A lobby: transient in-memory structure for players to gather before match
#[derive(Clone, Debug)]
pub struct Lobby {
    pub lobby_id: LobbyId,
    pub players: Vec<PlayerId>,
    pub ready_states: HashMap<PlayerId, ReadyState>,
    pub max_players: usize,
    pub created_at: DateTime<Utc>,
    pub auto_start_after_secs: u64,
}

impl Lobby {
    /// Create a new lobby
    pub fn new(max_players: usize, auto_start_after_secs: u64) -> Result<Self, SessionError> {
        if max_players < 2 || max_players > 4 {
            return Err(SessionError::InvalidCapacity(max_players));
        }
        Ok(Lobby {
            lobby_id: LobbyId::generate(),
            players: Vec::new(),
            ready_states: HashMap::new(),
            max_players,
            created_at: Utc::now(),
            auto_start_after_secs,
        })
    }

    /// Add a player to the lobby
    pub fn join_player(&mut self, player_id: PlayerId) -> Result<(), SessionError> {
        // Idempotent: if player already here, OK
        if self.players.contains(&player_id) {
            return Ok(());
        }
        
        if self.players.len() >= self.max_players {
            return Err(SessionError::SessionFull);
        }
        
        self.players.push(player_id.clone());
        self.ready_states.insert(player_id, ReadyState::NotReady);
        Ok(())
    }

    /// Remove a player from the lobby
    pub fn leave_player(&mut self, player_id: &PlayerId) -> Result<(), SessionError> {
        if !self.players.contains(player_id) {
            return Err(SessionError::PlayerNotInSession(player_id.0.clone()));
        }
        self.players.retain(|p| p != player_id);
        self.ready_states.remove(player_id);
        Ok(())
    }

    /// Mark a player as ready
    pub fn set_ready(&mut self, player_id: &PlayerId) -> Result<(), SessionError> {
        if !self.players.contains(player_id) {
            return Err(SessionError::PlayerNotInSession(player_id.0.clone()));
        }
        self.ready_states.insert(player_id.clone(), ReadyState::Ready);
        Ok(())
    }

    /// Check if all players are ready
    pub fn all_ready(&self) -> bool {
        if self.players.is_empty() {
            return false;
        }
        self.players.iter().all(|p| {
            self.ready_states.get(p) == Some(&ReadyState::Ready)
        })
    }

    /// Check if auto-start timeout has fired
    pub fn should_auto_start(&self) -> bool {
        let elapsed = Utc::now() - self.created_at;
        let threshold = chrono::Duration::seconds(self.auto_start_after_secs as i64);
        elapsed >= threshold
    }

    /// Check if lobby should transition (all ready OR timeout)
    pub fn can_transition_to_match_ready(&self) -> bool {
        !self.players.is_empty() && (self.all_ready() || self.should_auto_start())
    }
}

/// Global lobby registry: maps LobbyId to Lobby
pub struct LobbyRegistry {
    lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>>,
}

impl LobbyRegistry {
    pub fn new() -> Self {
        Self {
            lobbies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create and register a new lobby
    pub async fn create_lobby(
        &self,
        max_players: usize,
        auto_start_after_secs: u64,
    ) -> Result<LobbyId, SessionError> {
        let lobby = Lobby::new(max_players, auto_start_after_secs)?;
        let lobby_id = lobby.lobby_id.clone();
        let mut lobbies = self.lobbies.write().await;
        lobbies.insert(lobby_id.clone(), lobby);
        Ok(lobby_id)
    }

    /// Get a lobby by ID
    pub async fn get_lobby(&self, lobby_id: &LobbyId) -> Result<Option<Lobby>, SessionError> {
        let lobbies = self.lobbies.read().await;
        Ok(lobbies.get(lobby_id).cloned())
    }

    /// Join a player to a lobby
    pub async fn join_lobby(
        &self,
        lobby_id: &LobbyId,
        player_id: PlayerId,
    ) -> Result<(), SessionError> {
        let mut lobbies = self.lobbies.write().await;
        if let Some(lobby) = lobbies.get_mut(lobby_id) {
            lobby.join_player(player_id)?;
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Leave a lobby
    pub async fn leave_lobby(
        &self,
        lobby_id: &LobbyId,
        player_id: &PlayerId,
    ) -> Result<(), SessionError> {
        let mut lobbies = self.lobbies.write().await;
        if let Some(lobby) = lobbies.get_mut(lobby_id) {
            lobby.leave_player(player_id)?;
            // Optionally clean up empty lobbies
            if lobby.players.is_empty() {
                lobbies.remove(lobby_id);
            }
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Set a player as ready in a lobby
    pub async fn set_player_ready(
        &self,
        lobby_id: &LobbyId,
        player_id: &PlayerId,
    ) -> Result<(), SessionError> {
        let mut lobbies = self.lobbies.write().await;
        if let Some(lobby) = lobbies.get_mut(lobby_id) {
            lobby.set_ready(player_id)?;
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Check if lobby can transition to match ready
    pub async fn can_transition_to_match_ready(&self, lobby_id: &LobbyId) -> Result<bool, SessionError> {
        let lobbies = self.lobbies.read().await;
        if let Some(lobby) = lobbies.get(lobby_id) {
            Ok(lobby.can_transition_to_match_ready())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }
}

impl Default for LobbyRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Updates to Existing Files

### 1. `backend/src/session/mod.rs`
Add to the end (after other mod declarations):
```rust
pub mod lobby;
pub use lobby::{Lobby, LobbyId, LobbyRegistry, ReadyState};
```

### 2. `backend/src/lib.rs`
No changes needed (session module already exported).

---

## Test File: `backend/tests/lobby_test.rs`

### Test 1: `create_lobby`
- Create a lobby via `LobbyRegistry::create_lobby(2, 60)`
- Assert LobbyId is returned
- Verify lobby.players is empty
- Verify lobby.max_players == 2
- Verify lobby.auto_start_after_secs == 60

### Test 2: `join_lobby`
- Create lobby, join 2 players sequentially
- Assert both players in lobby.players
- Assert ready_states has both players marked NotReady
- Try to join a 3rd player (exceeds capacity)
- Assert SessionFull error returned

### Test 3: `leave_lobby`
- Create lobby, join 2 players
- Leave one player
- Assert player no longer in lobby.players
- Assert ready_states no longer has that player
- Leave the last player
- Assert lobby is cleaned up (removed from registry)

### Test 4: `ready_all`
- Create lobby with 2 players
- Both not ready initially
- Mark player 1 ready
- Assert all_ready() returns false
- Mark player 2 ready
- Assert all_ready() returns true

### Test 5: `ready_timeout`
- Create lobby with auto_start_after_secs = 1
- Don't wait (should_auto_start() returns false immediately)
- Assert can_transition_to_match_ready() returns false initially
- Manually check timeout logic (test checks with hardcoded time deltas; see implementation notes)

### Test 6: `auto_start`
- Create lobby with 1 player and auto_start_after_secs very large (e.g., 1000)
- should_auto_start() returns false (timeout not yet fired)
- Manually create an older lobby (set created_at to past time)
- should_auto_start() now returns true
- can_transition_to_match_ready() returns true

### Test 7: `concurrent_joins`
- Create lobby
- Use tokio::join_all to spawn N concurrent tasks
- Each task calls `join_lobby` with a different player
- All should succeed without data races
- Assert all N players are in the lobby
- Assert no duplicates

### Test 8: `idempotent_ready`
- Create lobby, join 2 players
- Call set_player_ready twice with same player
- Should succeed both times (idempotent)
- Assert player state is Ready after both calls
- Try to set_ready on a player not in lobby
- Assert PlayerNotInSession error

---

## Verification Steps

### After Implementation:

1. **Unit tests pass:**
   ```bash
   cd backend && cargo test --test lobby_test -- --nocapture
   ```
   Expected: All 8 new tests pass.

2. **Existing tests still pass:**
   ```bash
   cd backend && cargo test --all -- --nocapture
   ```
   Expected: 56 original tests + 2 ignored + 8 new = 66 total, all 64 pass.

3. **Compile check:**
   ```bash
   cd backend && cargo build
   ```
   Expected: No errors.

4. **Module exports check:**
   Verify lobby types are accessible from root:
   ```bash
   cd backend && cargo test --lib session::lobby -- --nocapture
   ```
   Expected: Compile without errors; lobby module is accessible.

---

## Implementation Notes

### Time-Based Timeout Testing
- In production, `should_auto_start()` reads `Utc::now()` dynamically
- In tests, create a lobby with `created_at` set manually in the past to simulate elapsed time
- Example: `Lobby { created_at: Utc::now() - Duration::seconds(61), ... }`
- Alternative: add a test-only `created_at` parameter to constructor (cleaner)

### Idempotency of join_player
- Calling `join_player` twice with the same PlayerId should be idempotent
- Check if player exists before adding; if yes, return Ok
- This simplifies network retry logic in the future (C2+)

### Empty Lobby Cleanup
- When last player leaves, `leave_lobby` removes the lobby from registry
- No orphan lobbies linger in memory
- C3 (orphan cleanup task) will add timeout-based cleanup later

### No Tokio Tasks
- No `tokio::spawn` in this module
- Auto-start is checked explicitly via `should_auto_start()`
- Game loop (or future game coordinator) calls `can_transition_to_match_ready()` each tick
- If true, transition Lobby → Session::Ready

### Max Players
- Hardcoded to 2–4 (validate in `Lobby::new`)
- Config-driven max in C5 (config module updated)
- Tests use max_players=2 for simplicity

---

## Files to Create / Modify

| File | Action | Details |
|------|--------|---------|
| `backend/src/session/lobby.rs` | Create | New file with Lobby, LobbyId, ReadyState, LobbyRegistry structs/enums |
| `backend/src/session/mod.rs` | Modify | Add `pub mod lobby;` and re-export types |
| `backend/tests/lobby_test.rs` | Create | New file with 8 tests |

---

## Expected Output

After implementation and commit:

```
cargo test --all

Compiling rollback-netcode-server v0.1.0
...

running 66 tests

test auth_tests::test_signed_token_creation ... ok
test auth_tests::test_token_expiry ... ok
test auth_tests::test_token_revocation ... ok
test auth_tests::test_tampered_token_rejected ... ok
test auth_tests::test_wrong_secret_rejected ... ok
test authz_tests::test_rbac_admin_check ... ok
test authz_tests::test_rbac_resource_check ... ok
test authz_tests::test_rbac_role_hierarchy ... ok
test authz_tests::test_resource_denied ... ok
test authz_tests::test_role_denied ... ok
test authz_tests::test_role_validation ... ok
test authz_tests::test_wildcard_resource_check ... ok
test authz_tests::test_wildcard_role_check ... ok
test authz_tests::test_world_readable_resource ... ok
test config_tests::test_config_defaults ... ok
test config_tests::test_invalid_db_url ... ok
test config_tests::test_invalid_redis_url ... ok
test config_tests::test_missing_postgres_url ... ok
test config_tests::test_missing_redis_url ... ok
test config_tests::test_postgres_url_valid ... ok
test config_tests::test_redis_url_valid ... ok
test config_tests::test_toml_parsing ... ok
test config_tests::test_toml_parsing_invalid_type ... ok
test config_tests::test_validation_succeeds_with_all_required_fields ... ok
test desync_tests::test_forced_desync_detection_and_authoritative_recovery ... ok
test feature_flag_tests::test_feature_flag_off ... ok
test feature_flag_tests::test_feature_flag_on ... ok
test feature_flag_tests::test_feature_flags_missing ... ok
test feature_flag_tests::test_invalid_type_toml ... ok
test feature_flags_tests::test_all_flags_off_by_default ... ok
test feature_flags_tests::test_flag_combination_matrix ... ok
test lobby_test::test_auto_start ... ok
test lobby_test::test_concurrent_joins ... ok
test lobby_test::test_create_lobby ... ok
test lobby_test::test_idempotent_ready ... ok
test lobby_test::test_join_lobby ... ok
test lobby_test::test_leave_lobby ... ok
test lobby_test::test_ready_all ... ok
test lobby_test::test_ready_timeout ... ok
test matchmaking_tests::test_matchmaking_module_exists ... ok
test module_isolation_test::test_module_network_isolation ... ok
test network_tests::test_input_frame_with_5frame_redundancy ... ok
test network_tests::test_input_ping_and_pong_roundtrip ... ok
test network_tests::test_input_ready_and_checkpoint_roundtrip ... ok
test postgres_redis_integration::test_concurrent_enqueue_redis ... ok
test postgres_redis_integration::test_graceful_degradation_redis_offline ... ok
test postgres_redis_integration::test_multiple_connections_read ... ok
test postgres_redis_integration::test_queue_pop_redis ... ok
test postgres_redis_integration::test_session_crud ... ok
test postgres_redis_integration::test_session_heartbeat ... ok
test postgres_redis_integration::test_session_reconnect_token_flow ... ok
test postgres_redis_integration::test_session_stale_detection ... ok
test postgres_redis_integration::test_ttl_expiry_redis ... ok
test protocol_tests::test_checksum_roundtrip ... ok
test protocol_tests::test_error_and_kick_roundtrip ... ok
test protocol_tests::test_handshake_roundtrip ... ok
test protocol_tests::test_version_negotiation_matrix ... ok
test session_tests::test_cannot_join_in_active_state ... ok
test session_tests::test_invalid_capacity ... ok
test session_tests::test_player_join_and_leave ... ok
test session_tests::test_reconnect_token ... ok
test session_tests::test_repository_create_and_get ... ok
test session_tests::test_session_creation ... ok
test session_tests::test_session_orphan_cleanup ... ok
test session_tests::test_session_stale_check ... ok
test session_tests::test_state_transitions ... ok
test session_tests::test_ready_up_logic ... ok
test simulation_tests::test_1000_run_randomized_rollback_determinism ... ok
test simulation_tests::test_desync_incident_created ... ok
test simulation_tests::test_drop_model_with_loss_deterministic_reset ... ok
test simulation_tests::test_drop_model_with_zero_percent_loss ... ok
test simulation_tests::test_drop_model_with_thirty_percent_loss ... ok
test simulation_tests::test_two_peer_convergence_under_network_delay ... ok
test transport_protocol_test::test_codec_roundtrip ... ok
test transport_protocol_test::test_frame_ack_on_state_update ... ok
test transport_protocol_test::test_packet_ordering_with_delays ... ok
test transport_protocol_test::test_ping_pong_handshake ... ok
test transport_protocol_test::test_state_snapshot_capture ... ok
test transport_protocol_test::test_state_snapshot_immutable ... ok
test transport_protocol_test::test_udp_packet_corruption_tolerance ... ok
test transport_protocol_test::test_udp_packet_loss_recovery_with_checksum ... ok
test transport_protocol_test::test_udp_packet_loss_recovery_without_timeout ... ok
test transport_protocol_test::test_udp_retransmit_on_timeout ... ok

test result: ok. 64 passed; 0 failed; 2 ignored

Commit: "C1: Lobby implementation (create, join, leave, ready, auto-start, concurrent joins, 8 tests)"
```

---

## Additional Notes for Implementer

1. **Import statements:** Use `use crate::session::{PlayerId, SessionError};` to avoid circular imports.
2. **Arc<RwLock<>> pattern:** Same as existing repository.rs; well-established in the codebase.
3. **No feature flags yet:** Hardcoded max_players = 2–4. C5 adds config-driven values.
4. **Idempotent join:** Calling join_lobby twice with the same player is OK (no error).
5. **Time mocking in tests:** Use hardcoded Utc timestamps in test lobby initialization.
6. **Commit message:** Single, atomic commit with all three files (lobby.rs, mod.rs update, lobby_test.rs).
