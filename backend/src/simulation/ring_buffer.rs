use crate::simulation::state::GameState;

pub const DEFAULT_RING_CAPACITY: usize = 128;

/// Bounded ring buffer for state snapshots keyed by tick.
/// Complies with FR-005, FR-006, and FR-007.
#[derive(Clone, Debug)]
pub struct SnapshotRing<const CAP: usize = DEFAULT_RING_CAPACITY> {
    slots: [Option<GameState>; CAP],
    checksums: [u32; CAP],
    head_tick: u32,
    tail_tick: u32,
}

impl<const CAP: usize> Default for SnapshotRing<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> SnapshotRing<CAP> {
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            checksums: [0u32; CAP],
            head_tick: 0,
            tail_tick: 0,
        }
    }

    #[inline(always)]
    fn slot_index(tick: u32) -> usize {
        (tick as usize) % CAP
    }

    pub fn save(&mut self, state: GameState) {
        let tick = state.tick;
        let checksum = state.compute_checksum();
        let idx = Self::slot_index(tick);

        self.slots[idx] = Some(state);
        self.checksums[idx] = checksum;

        if tick > self.head_tick {
            self.head_tick = tick;
            if self.head_tick >= (CAP as u32) {
                self.tail_tick = self.head_tick - (CAP as u32) + 1;
            }
        }
    }

    pub fn restore(&self, tick: u32) -> Option<GameState> {
        if tick < self.tail_tick || tick > self.head_tick {
            return None;
        }
        let idx = Self::slot_index(tick);
        match &self.slots[idx] {
            Some(state) if state.tick == tick => Some(state.clone()),
            _ => None,
        }
    }

    pub fn get_checksum(&self, tick: u32) -> Option<u32> {
        if tick < self.tail_tick || tick > self.head_tick {
            return None;
        }
        let idx = Self::slot_index(tick);
        Some(self.checksums[idx])
    }

    pub fn head_tick(&self) -> u32 {
        self.head_tick
    }

    pub fn tail_tick(&self) -> u32 {
        self.tail_tick
    }

    pub fn is_retained(&self, tick: u32) -> bool {
        tick >= self.tail_tick && tick <= self.head_tick
    }
}
