use std::collections::BTreeMap;
use crate::simulation::ring_buffer::SnapshotRing;
use crate::simulation::state::GameState;
use thiserror::Error;

pub const MAX_ROLLBACK_FRAMES: u32 = 15;

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum RollbackError {
    #[error("Rollback depth {depth} exceeds MAX_ROLLBACK ({MAX_ROLLBACK_FRAMES}); full authoritative snapshot resync required")]
    NeedsFullResync { depth: u32 },

    #[error("Target tick {tick} not found in retained snapshot ring buffer (tail: {tail})")]
    SnapshotEvicted { tick: u32, tail: u32 },

    #[error("Player index {0} out of bounds")]
    InvalidPlayerIndex(usize),

    #[error("Input for tick {tick} already confirmed at watermark {confirmed}")]
    DuplicateConfirmedInput { tick: u32, confirmed: u32 },
}

pub trait PredictionPolicy: Send + Sync {
    fn predict_input(&self, player_idx: usize, tick: u32, history: &InputHistory) -> u16;
}

#[derive(Default, Clone, Debug)]
pub struct RepeatLastInputPolicy;

impl PredictionPolicy for RepeatLastInputPolicy {
    fn predict_input(&self, _player_idx: usize, tick: u32, history: &InputHistory) -> u16 {
        history.get_latest_input_before(tick).unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct InputHistory {
    pub inputs: BTreeMap<u32, u16>,
    pub confirmed_tick: u32,
}

impl InputHistory {
    pub fn new() -> Self {
        Self {
            inputs: BTreeMap::new(),
            confirmed_tick: 0,
        }
    }

    pub fn insert_input(&mut self, tick: u32, input: u16) -> Result<bool, RollbackError> {
        if self.inputs.contains_key(&tick) {
            // Deduplicate duplicate inputs without error
            return Ok(false);
        }
        self.inputs.insert(tick, input);
        if tick == self.confirmed_tick + 1 {
            // Advance confirmed watermark
            let mut current = tick;
            while self.inputs.contains_key(&(current + 1)) {
                current += 1;
            }
            self.confirmed_tick = current;
        }
        Ok(true)
    }

    pub fn get_input(&self, tick: u32) -> Option<u16> {
        self.inputs.get(&tick).copied()
    }

    pub fn get_latest_input_before(&self, tick: u32) -> Option<u16> {
        self.inputs
            .range(..=tick)
            .next_back()
            .map(|(_, &v)| v)
    }

    pub fn prune(&mut self, before_tick: u32) {
        self.inputs.retain(|&t, _| t >= before_tick);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackStats {
    pub rollback_tick: u32,
    pub target_tick: u32,
    pub depth: u32,
    pub frames_resimulated: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceClassification {
    InputMismatch { tick: u32 },
    SimulationMismatch { tick: u32, local_hash: u32, remote_hash: u32 },
    Corruption { tick: u32 },
}

pub struct RollbackEngine<P: PredictionPolicy = RepeatLastInputPolicy> {
    pub ring: SnapshotRing<128>,
    pub input_histories: [InputHistory; 2],
    pub current_tick: u32,
    pub prediction_policy: P,
}

impl Default for RollbackEngine<RepeatLastInputPolicy> {
    fn default() -> Self {
        Self::new(RepeatLastInputPolicy)
    }
}

impl<P: PredictionPolicy> RollbackEngine<P> {
    pub fn new(prediction_policy: P) -> Self {
        Self::new_with_initial_state(GameState::default(), prediction_policy)
    }

    pub fn new_with_initial_state(initial_state: GameState, prediction_policy: P) -> Self {
        let mut ring = SnapshotRing::new();
        ring.save(initial_state);

        Self {
            ring,
            input_histories: [InputHistory::new(), InputHistory::new()],
            current_tick: 0,
            prediction_policy,
        }
    }

    pub fn current_state(&self) -> GameState {
        self.ring
            .restore(self.current_tick)
            .unwrap_or_default()
    }

    /// Advances simulation by 1 tick using confirmed or predicted inputs
    pub fn advance_local_tick(&mut self, local_player_idx: usize, local_input: u16) -> GameState {
        let next_tick = self.current_tick + 1;
        self.input_histories[local_player_idx]
            .insert_input(next_tick, local_input)
            .ok();

        let p0_input = self.input_histories[0]
            .get_input(next_tick)
            .unwrap_or_else(|| self.prediction_policy.predict_input(0, next_tick, &self.input_histories[0]));

        let p1_input = self.input_histories[1]
            .get_input(next_tick)
            .unwrap_or_else(|| self.prediction_policy.predict_input(1, next_tick, &self.input_histories[1]));

        let current_state = self.current_state();
        let next_state = current_state.step(&[p0_input, p1_input]);

        self.ring.save(next_state.clone());
        self.current_tick = next_tick;
        next_state
    }

    /// Handles late incoming remote input; triggers rollback resimulation if newly received
    pub fn handle_remote_input(
        &mut self,
        remote_player_idx: usize,
        tick: u32,
        input: u16,
    ) -> Result<Option<RollbackStats>, RollbackError> {
        if remote_player_idx >= 2 {
            return Err(RollbackError::InvalidPlayerIndex(remote_player_idx));
        }

        if tick > self.current_tick {
            // Future input: store directly
            self.input_histories[remote_player_idx].insert_input(tick, input)?;
            return Ok(None);
        }

        let already_present = self.input_histories[remote_player_idx].get_input(tick).is_some();
        if already_present {
            // Input already confirmed and simulated for this exact tick
            return Ok(None);
        }

        let depth = self.current_tick - tick;
        if depth > MAX_ROLLBACK_FRAMES {
            return Err(RollbackError::NeedsFullResync { depth });
        }

        self.input_histories[remote_player_idx].insert_input(tick, input)?;

        // Resimulate from tick - 1 to current_tick
        let base_tick = tick.saturating_sub(1);
        let mut state = self.ring.restore(base_tick).ok_or(RollbackError::SnapshotEvicted {
            tick: base_tick,
            tail: self.ring.tail_tick(),
        })?;

        let mut frames_resimulated = 0;
        for t in (base_tick + 1)..=self.current_tick {
            let i0 = self.input_histories[0].get_input(t).unwrap_or_else(|| {
                self.prediction_policy.predict_input(0, t, &self.input_histories[0])
            });
            let i1 = self.input_histories[1].get_input(t).unwrap_or_else(|| {
                self.prediction_policy.predict_input(1, t, &self.input_histories[1])
            });

            state = state.step(&[i0, i1]);
            self.ring.save(state.clone());
            frames_resimulated += 1;
        }

        Ok(Some(RollbackStats {
            rollback_tick: tick,
            target_tick: self.current_tick,
            depth,
            frames_resimulated,
        }))
    }

    /// Authoritative full snapshot resync injection
    pub fn force_resync_state(&mut self, snapshot: GameState) {
        self.current_tick = snapshot.tick;
        self.ring.save(snapshot);
    }
}

pub struct DivergenceDetector;

impl DivergenceDetector {
    pub fn classify(
        tick: u32,
        local_hash: u32,
        remote_hash: u32,
        local_inputs: Option<[u16; 2]>,
        remote_inputs: Option<[u16; 2]>,
    ) -> DivergenceClassification {
        if local_hash == remote_hash {
            return DivergenceClassification::SimulationMismatch {
                tick,
                local_hash,
                remote_hash,
            };
        }

        if local_inputs != remote_inputs {
            DivergenceClassification::InputMismatch { tick }
        } else if local_hash == 0 || remote_hash == 0 {
            DivergenceClassification::Corruption { tick }
        } else {
            DivergenceClassification::SimulationMismatch {
                tick,
                local_hash,
                remote_hash,
            }
        }
    }
}
