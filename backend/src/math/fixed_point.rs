use serde::{Deserialize, Serialize};
use std::ops::{Add, Div, Mul, Sub};

/// Fixed-point Q16.16 signed integer arithmetic primitive.
/// Complies with FR-001 for cross-platform deterministic physics simulation.
/// Uses 16 integer bits and 16 fractional bits.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct FixedI32(pub i32);

impl FixedI32 {
    pub const FRACTIONAL_BITS: u32 = 16;
    pub const ONE: Self = Self(1 << 16);
    pub const ZERO: Self = Self(0);
    pub const HALF: Self = Self(1 << 15);
    pub const MIN: Self = Self(i32::MIN);
    pub const MAX: Self = Self(i32::MAX);

    #[inline(always)]
    pub const fn from_int(v: i32) -> Self {
        Self(v << 16)
    }

    #[inline(always)]
    pub const fn to_int(self) -> i32 {
        self.0 >> 16
    }

    #[inline(always)]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub const fn raw(self) -> i32 {
        self.0
    }

    #[inline(always)]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline(always)]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        if self.0 < min.0 {
            min
        } else if self.0 > max.0 {
            max
        } else {
            self
        }
    }

    #[inline(always)]
    pub fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    #[inline(always)]
    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Deterministic integer square root for Q16.16
    pub fn isqrt(self) -> Self {
        if self.0 <= 0 {
            return Self::ZERO;
        }
        // Multiply by 2^16 before integer sqrt to maintain Q16.16 precision: sqrt(x * 2^16) = sqrt(x) * 2^8 -> shift left by 8
        let val = (self.0 as u64) << 16;
        let mut x0 = val / 2;
        if x0 == 0 {
            return Self::from_raw(1);
        }
        let mut x1 = (x0 + val / x0) / 2;
        while x1 < x0 {
            x0 = x1;
            x1 = (x0 + val / x0) / 2;
        }
        Self::from_raw(x0 as i32)
    }

    /// Deterministic Trigonometric Sine using 256-entry Quadrant LUT
    pub fn sin(angle_lut_idx: u8) -> Self {
        let quadrant = (angle_lut_idx >> 6) & 0x03;
        let index = (angle_lut_idx & 0x3F) as usize;

        // 64-entry first quadrant sine lookup table (scaled to Q16.16 ONE = 65536)
        const SIN_TABLE: [i32; 64] = [
            0, 1608, 3216, 4821, 6424, 8022, 9616, 11204, 12785, 14359, 15924, 17479, 19024, 20557,
            22078, 23586, 25080, 26558, 28020, 29465, 30893, 32303, 33692, 35062, 36410, 37736,
            39040, 40320, 41576, 42806, 44011, 45189, 46341, 47464, 48559, 49624, 50660, 51665,
            52639, 53581, 54491, 55368, 56212, 57022, 57798, 58538, 59244, 59914, 60547, 61145,
            61705, 62228, 62714, 63162, 63572, 63944, 64277, 64571, 64827, 65043, 65220, 65358,
            65457, 65516,
        ];

        let val = match quadrant {
            0 => SIN_TABLE[index],
            1 => SIN_TABLE[63 - index],
            2 => -SIN_TABLE[index],
            3 => -SIN_TABLE[63 - index],
            _ => unreachable!(),
        };
        Self::from_raw(val)
    }

    /// Deterministic Trigonometric Cosine: cos(x) = sin(x + 64)
    pub fn cos(angle_lut_idx: u8) -> Self {
        Self::sin(angle_lut_idx.wrapping_add(64))
    }
}

impl Add for FixedI32 {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0))
    }
}

impl Sub for FixedI32 {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0))
    }
}

impl Mul for FixedI32 {
    type Output = Self;
    #[inline(always)]
    fn mul(self, rhs: Self) -> Self {
        let p = (self.0 as i64) * (rhs.0 as i64);
        Self((p >> 16) as i32)
    }
}

impl Div for FixedI32 {
    type Output = Self;
    #[inline(always)]
    fn div(self, rhs: Self) -> Self {
        if rhs.0 == 0 {
            return Self::ZERO;
        }
        let d = (self.0 as i64) << 16;
        Self((d / (rhs.0 as i64)) as i32)
    }
}

/// 2D Vector with FixedI32 coordinates
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Vec2Fixed {
    pub x: FixedI32,
    pub y: FixedI32,
}

impl Vec2Fixed {
    pub const ZERO: Self = Self {
        x: FixedI32::ZERO,
        y: FixedI32::ZERO,
    };

    #[inline(always)]
    pub const fn new(x: FixedI32, y: FixedI32) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub fn length_squared(self) -> FixedI32 {
        (self.x * self.x) + (self.y * self.y)
    }

    #[inline(always)]
    pub fn length(self) -> FixedI32 {
        self.length_squared().isqrt()
    }
}

impl Add for Vec2Fixed {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2Fixed {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

/// Deterministic, serializable 64-bit PCG / Xoshiro PRNG
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeterministicRng {
    pub state: u64,
    pub inc: u64,
}

impl DeterministicRng {
    pub const fn new(seed: u64) -> Self {
        Self {
            state: seed,
            inc: 0xDA3E39CB94B95BDB | 1,
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let oldstate = self.state;
        self.state = oldstate
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.inc);
        let xorshifted = (((oldstate >> 18) ^ oldstate) >> 27) as u32;
        let rot = (oldstate >> 59) as u32;
        (xorshifted >> rot) | (xorshifted << ((32 - rot) & 31))
    }

    pub fn next_range(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        let span = max - min;
        min + (self.next_u32() % span)
    }
}
