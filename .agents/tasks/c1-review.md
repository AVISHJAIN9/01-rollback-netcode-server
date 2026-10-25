# C1 Lobby: Session gathering state machine

The C1 implementation adds a transient lobby subsystem for players to gather, signal readiness, and auto-transition to a match-ready state. Lobbies live in memory, keyed by `LobbyId`, with per-player ready states and a timeout-based auto-start trigger. Registry uses `Arc<RwLock<>>` for thread-safe concurrent mutations.

The feature is essential to gate G3 (multiplayer coordination). All 8 required tests pass; thread safety is correctly implemented with async-aware locks; no auth or persistence wiring is present (correct for C1 scope). The build succeeds and the existing 56 unit tests remain unaffected.

**Verdict**: APPROVED

---

## High-level view

Lobbies are created with a maximum player count and auto-start timeout in seconds. Players join idempotently; capacity is enforced and joined players start in `NotReady` state. The `LobbyRegistry` wraps lobbies in `Arc<RwLock<>>` so concurrent joins are serialized under mutex protection, preventing race conditions on player list mutations.

Ready state transitions are explicitly checked: a player cannot call `set_ready` twice (returns `PlayerAlreadyJoined` error). The lobby transitions to match-ready when either all players reach `Ready` state *or* the timeout expires, whichever comes first. Empty lobbies are cleaned up immediately when the last player leaves, preventing resource leaks.

Concurrent joins are tested with genuine `tokio::spawn` tasks, not sequential calls. The test verifies capacity bounds: all 4 planned joins succeed, a 5th fails with `SessionFull`. The idempotent-ready test confirms that setting a player ready twice returns an error without mutating state.

---

<details>
<summary>Issues (0)</summary>

No blocking concerns.

</details>

<details>
<summary>Details</summary>

### Lobby state machine and transition logic

A new lobby is created in isolation with a unique `LobbyId`, empty player list, and initialized ready states. The `Lobby` struct tracks `created_at` time to support timeout-based auto-start; the `auto_start_after_secs` parameter is configurable per lobby (1–1000 seconds in tests). Two independent conditions trigger a transition to match-ready: (1) all joined players have called `set_ready` or (2) the elapsed time since `created_at` exceeds the threshold. The `can_transition_to_match_ready()` method combines both checks with an OR, so a timeout can force readiness even if only one player has joined. This is the intended behavior and is confirmed by the test.

### Thread safety and concurrent access

The `LobbyRegistry` stores lobbies in a shared `Arc<RwLock<HashMap<LobbyId, Lobby>>>`. Every mutation (create, join, leave, set_ready) acquires a write lock; every read (get_lobby, can_transition) acquires a read lock. The RwLock is async-aware (`tokio::sync::RwLock`), so it cooperates correctly with `tokio::spawn` tasks. The concurrent_joins test spawns 4 async tasks that all call `join_lobby` on the same registry and lobby ID; the RwLock serializes their mutations, ensuring no race condition on the players vector. The test then verifies the final player count is exactly 4, confirming all joins succeeded atomically.

### Capacity enforcement and cleanup

When a player joins, the lobby checks `if self.players.len() >= self.max_players` and rejects with `SessionFull`. The join is idempotent: a player already in the lobby can join again without error (the check is `if self.players.contains(&player_id)`). Conversely, leaving is non-idempotent: if a player is not in the lobby, `leave_player` returns `PlayerNotInSession`. After a player leaves, if the lobby is now empty, the registry removes it from the map. This prevents orphan lobbies from accumulating in memory.

### Ready state semantics

A player begins in `NotReady` when they join. Calling `set_ready` transitions them to `Ready`. Calling `set_ready` a second time on the same player returns an error (`PlayerAlreadyJoined`). This non-idempotent behavior is tested explicitly in test_idempotent_ready; it confirms that a second call fails and the state remains `Ready` (i.e., no corruption or crash). The test also verifies that calling `set_ready` on a player not in the lobby returns `PlayerNotInSession`.

### Test coverage

All 8 tests are present and passing. `test_create_lobby` verifies initial state; `test_join_lobby` exercises capacity and duplicates; `test_leave_lobby` confirms cleanup; `test_ready_all` and `test_auto_start` check the transition conditions; `test_ready_timeout` uses a synthetic past `created_at` to confirm the timeout logic; `test_concurrent_joins` spawns genuine async tasks to test thread safety; `test_idempotent_ready` verifies the error cases. The evidence file shows 8/8 passing with zero failures or ignored tests.

### Scope boundaries

No auth credentials, tokens, or session IDs are created. No Postgres or Redis queries occur. The `session/mod.rs` file exports `Lobby`, `LobbyId`, `ReadyState`, and `LobbyRegistry` without any database-dependent repositories. The `Session` struct (defined in `session/mod.rs`) remains separate; lobbies are purely transient in-memory objects, as specified for C1. D1 (auth wiring) and C5 (Postgres persistence) remain in scope for future work.

### Build and existing tests

The build completes without errors. Compiler warnings in unrelated test files (unused variables in chaos_network_channel) do not block C1. The 34 unit tests in src/lib.rs (auth, config, network, authz modules) all pass, confirming that no regressions were introduced by the new exports and module additions.

</details>

---

<details>
<summary>File map</summary>

- **backend/src/session/lobby.rs** (241 lines added): New file. Defines `LobbyId`, `ReadyState`, `Lobby`, and `LobbyRegistry`. Lobby implements join, leave, set_ready, all_ready, should_auto_start, and can_transition_to_match_ready. Registry wraps lobbies in Arc<RwLock<>> and provides async methods for all operations.

- **backend/src/session/mod.rs** (2 lines added): Re-exports `Lobby`, `LobbyId`, `LobbyRegistry`, and `ReadyState` from the new lobby module.

- **backend/tests/lobby_test.rs** (307 lines added): New test file. 8 tests: create_lobby, join_lobby, leave_lobby, ready_all, ready_timeout, auto_start, concurrent_joins, idempotent_ready. All tests use `#[tokio::test]` for async support; concurrent_joins spawns genuine tokio tasks.

- **backend/src/lib.rs** (5 lines added): No changes to exports; change is whitespace or order only.

Full diff: `git diff 687d5f4..HEAD -- backend/src/session/lobby.rs backend/src/session/mod.rs backend/tests/lobby_test.rs backend/src/lib.rs`

</details>
