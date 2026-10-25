use crate::math::{DeterministicRng, FixedI32, Vec2Fixed};
use serde::{Deserialize, Serialize};

pub const ARENA_WIDTH_INT: i32 = 800;
pub const ARENA_HEIGHT_INT: i32 = 400;
pub const PLAYER_WIDTH_INT: i32 = 40;
pub const PLAYER_HEIGHT_INT: i32 = 60;
pub const MOVE_SPEED_INT: i32 = 4;
pub const JUMP_VELOCITY_INT: i32 = 12;
pub const GRAVITY_INT: i32 = 1;
pub const MAX_HEALTH: u8 = 100;
pub const ATTACK_DAMAGE: u8 = 15;
pub const ATTACK_COOLDOWN_TICKS: u8 = 12;
pub const HITSTUN_DURATION_TICKS: u8 = 8;

pub const INPUT_LEFT: u16 = 1 << 0;
pub const INPUT_RIGHT: u16 = 1 << 1;
pub const INPUT_JUMP: u16 = 1 << 2;
pub const INPUT_ATTACK: u16 = 1 << 3;
pub const INPUT_BLOCK: u16 = 1 << 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionState {
    #[default]
    Idle,
    Moving,
    Jumping,
    Attacking,
    Blocking,
    Hitstun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerState {
    pub position: Vec2Fixed,
    pub velocity: Vec2Fixed,
    pub health: u8,
    pub facing_right: bool,
    pub is_grounded: bool,
    pub action_state: ActionState,
    pub attack_cooldown: u8,
    pub hitstun_remaining: u8,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            position: Vec2Fixed::ZERO,
            velocity: Vec2Fixed::ZERO,
            health: MAX_HEALTH,
            facing_right: true,
            is_grounded: true,
            action_state: ActionState::Idle,
            attack_cooldown: 0,
            hitstun_remaining: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameState {
    pub tick: u32,
    pub players: [PlayerState; 2],
    pub rng: DeterministicRng,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new(0x1234_5678_9ABC_DEF0)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum IllegalTransition {
    TeleportationDetected { player: usize, dist_squared: i32 },
    SpeedLimitExceeded { player: usize, speed: i32 },
    AttackCooldownViolation { player: usize },
    HealthDesync { player: usize, prev: u8, next: u8 },
}

impl GameState {
    pub fn new(seed: u64) -> Self {
        let p1 = PlayerState {
            position: Vec2Fixed::new(FixedI32::from_int(100), FixedI32::from_int(0)),
            facing_right: true,
            ..Default::default()
        };

        let p2 = PlayerState {
            position: Vec2Fixed::new(FixedI32::from_int(700), FixedI32::from_int(0)),
            facing_right: false,
            ..Default::default()
        };

        Self {
            tick: 0,
            players: [p1, p2],
            rng: DeterministicRng::new(seed),
        }
    }

    /// Pure deterministic physics tick step function compliant with FR-001/FR-003
    pub fn step(&self, inputs: &[u16; 2]) -> Self {
        let mut next = self.clone();
        next.tick += 1;

        // 1. Process individual player inputs and movement
        for (i, p) in next.players.iter_mut().enumerate() {
            let input = inputs[i];

            // Handle hitstun decrement
            if p.hitstun_remaining > 0 {
                p.hitstun_remaining -= 1;
                p.action_state = ActionState::Hitstun;
                p.velocity.x = FixedI32::ZERO;
            } else {
                if p.attack_cooldown > 0 {
                    p.attack_cooldown -= 1;
                }

                // Blocking
                if (input & INPUT_BLOCK) != 0 && p.is_grounded {
                    p.action_state = ActionState::Blocking;
                    p.velocity.x = FixedI32::ZERO;
                } else if (input & INPUT_ATTACK) != 0 && p.attack_cooldown == 0 {
                    // Attack initiation
                    p.action_state = ActionState::Attacking;
                    p.attack_cooldown = ATTACK_COOLDOWN_TICKS;
                    p.velocity.x = FixedI32::ZERO;
                } else {
                    // Horizontal movement
                    let mut move_x = FixedI32::ZERO;
                    if (input & INPUT_LEFT) != 0 {
                        move_x = move_x - FixedI32::from_int(MOVE_SPEED_INT);
                        p.facing_right = false;
                    }
                    if (input & INPUT_RIGHT) != 0 {
                        move_x = move_x + FixedI32::from_int(MOVE_SPEED_INT);
                        p.facing_right = true;
                    }
                    p.velocity.x = move_x;

                    // Jumping
                    if (input & INPUT_JUMP) != 0 && p.is_grounded {
                        p.velocity.y = FixedI32::from_int(JUMP_VELOCITY_INT);
                        p.is_grounded = false;
                        p.action_state = ActionState::Jumping;
                    } else if p.is_grounded {
                        p.action_state = if p.velocity.x.raw() != 0 {
                            ActionState::Moving
                        } else {
                            ActionState::Idle
                        };
                    }
                }
            }

            // Apply gravity if airborne
            if !p.is_grounded {
                p.velocity.y = p.velocity.y - FixedI32::from_int(GRAVITY_INT);
            }

            // Integrate kinematics
            p.position.x = p.position.x + p.velocity.x;
            p.position.y = p.position.y + p.velocity.y;

            // Ground floor collision (y = 0)
            if p.position.y.raw() <= 0 {
                p.position.y = FixedI32::ZERO;
                p.velocity.y = FixedI32::ZERO;
                p.is_grounded = true;
                if p.action_state == ActionState::Jumping {
                    p.action_state = ActionState::Idle;
                }
            }

            // Arena horizontal boundary clamping [0, ARENA_WIDTH_INT - PLAYER_WIDTH_INT]
            let min_x = FixedI32::ZERO;
            let max_x = FixedI32::from_int(ARENA_WIDTH_INT - PLAYER_WIDTH_INT);
            p.position.x = p.position.x.clamp(min_x, max_x);
        }

        // 2. Resolve Attack Hitboxes & Damage between players
        for attacker_idx in 0..2 {
            let defender_idx = 1 - attacker_idx;
            if next.players[attacker_idx].action_state == ActionState::Attacking
                && next.players[attacker_idx].attack_cooldown == (ATTACK_COOLDOWN_TICKS - 1)
            {
                let attacker_pos = next.players[attacker_idx].position;
                let defender_pos = next.players[defender_idx].position;
                let dist_x = (defender_pos.x - attacker_pos.x).raw().abs();

                // Hitbox reach: within 60 units horizontally and 40 units vertically
                if dist_x <= FixedI32::from_int(60).raw()
                    && (defender_pos.y - attacker_pos.y).raw().abs() <= FixedI32::from_int(40).raw()
                {
                    // Check if defender is blocking towards attacker
                    let defender_facing_attacker = if attacker_pos.x.raw() < defender_pos.x.raw() {
                        !next.players[defender_idx].facing_right
                    } else {
                        next.players[defender_idx].facing_right
                    };

                    let is_blocked = next.players[defender_idx].action_state
                        == ActionState::Blocking
                        && defender_facing_attacker;

                    if is_blocked {
                        // Chip damage on block
                        next.players[defender_idx].health =
                            next.players[defender_idx].health.saturating_sub(3);
                    } else {
                        // Full hit damage and hitstun
                        next.players[defender_idx].health = next.players[defender_idx]
                            .health
                            .saturating_sub(ATTACK_DAMAGE);
                        next.players[defender_idx].hitstun_remaining = HITSTUN_DURATION_TICKS;
                        next.players[defender_idx].action_state = ActionState::Hitstun;
                    }
                }
            }
        }

        next
    }

    /// Endianness-independent canonical 32-bit state checksum using Blake3
    pub fn compute_checksum(&self) -> u32 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.tick.to_le_bytes());
        for p in &self.players {
            hasher.update(&p.position.x.raw().to_le_bytes());
            hasher.update(&p.position.y.raw().to_le_bytes());
            hasher.update(&p.velocity.x.raw().to_le_bytes());
            hasher.update(&p.velocity.y.raw().to_le_bytes());
            hasher.update(&[p.health]);
            hasher.update(&[p.facing_right as u8]);
            hasher.update(&[p.is_grounded as u8]);
            hasher.update(&[p.action_state as u8]);
            hasher.update(&[p.attack_cooldown]);
            hasher.update(&[p.hitstun_remaining]);
        }
        hasher.update(&self.rng.state.to_le_bytes());
        hasher.update(&self.rng.inc.to_le_bytes());

        let hash = hasher.finalize();
        let bytes = hash.as_bytes();
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Server authority legal-transition validator
    pub fn validate_transition(
        prev: &Self,
        next: &Self,
        inputs: &[u16; 2],
    ) -> Result<(), IllegalTransition> {
        let expected = prev.step(inputs);
        for i in 0..2 {
            let p_act = next.players[i];
            let p_exp = expected.players[i];

            let dx = (p_act.position.x - p_exp.position.x).raw().abs();
            let dy = (p_act.position.y - p_exp.position.y).raw().abs();

            if dx > FixedI32::from_int(8).raw() || dy > FixedI32::from_int(8).raw() {
                return Err(IllegalTransition::TeleportationDetected {
                    player: i,
                    dist_squared: dx * dx + dy * dy,
                });
            }

            if p_act.velocity.x.raw().abs() > FixedI32::from_int(10).raw() {
                return Err(IllegalTransition::SpeedLimitExceeded {
                    player: i,
                    speed: p_act.velocity.x.raw().abs(),
                });
            }

            if p_act.health != p_exp.health {
                return Err(IllegalTransition::HealthDesync {
                    player: i,
                    prev: p_exp.health,
                    next: p_act.health,
                });
            }
        }
        Ok(())
    }
}
