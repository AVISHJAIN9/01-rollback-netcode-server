/// Packet drop simulator for chaos testing
/// 
/// Uses seeded random number generation to deterministically drop packets
/// at a configurable loss rate.

/// Drop model for testing packet loss resilience
#[derive(Debug, Clone)]
pub struct DropModel {
    loss_rate: f64,  // 0.0 to 1.0 (e.g., 0.30 = 30% loss)
    seed: u64,
    rng_state: u64,
}

impl DropModel {
    /// Create a new drop model with a loss rate and seed
    /// 
    /// # Arguments
    /// - `loss_rate`: Fraction of packets to drop (0.0 to 1.0)
    /// - `seed`: Seed for deterministic randomness
    /// 
    /// # Example
    /// ```ignore
    /// let drop_model = DropModel::new(0.30, 12345);  // 30% loss, seeded
    /// assert!(drop_model.should_drop());  // Some packets dropped
    /// ```
    pub fn new(loss_rate: f64, seed: u64) -> Self {
        assert!(loss_rate >= 0.0 && loss_rate <= 1.0, "loss_rate must be 0.0-1.0");
        Self {
            loss_rate,
            seed,
            rng_state: seed,
        }
    }

    /// Determine if this packet should be dropped
    /// 
    /// Uses a simple LCG (linear congruential generator) for deterministic randomness
    pub fn should_drop(&mut self) -> bool {
        // LCG: x' = (a * x + c) mod m
        // Using glibc parameters: a=1103515245, c=12345, m=2^31
        const A: u64 = 1103515245;
        const C: u64 = 12345;
        const M: u64 = 1u64 << 31;

        self.rng_state = ((A.wrapping_mul(self.rng_state).wrapping_add(C)) % M) as u64;
        let rand_frac = (self.rng_state as f64) / (M as f64);
        rand_frac < self.loss_rate
    }

    /// Get the current loss rate
    pub fn loss_rate(&self) -> f64 {
        self.loss_rate
    }

    /// Reset RNG state to initial seed
    pub fn reset(&mut self) {
        self.rng_state = self.seed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_model_0pct_loss() {
        let mut model = DropModel::new(0.0, 42);
        for _ in 0..100 {
            assert!(!model.should_drop(), "0% loss should never drop");
        }
    }

    #[test]
    fn test_drop_model_100pct_loss() {
        let mut model = DropModel::new(1.0, 42);
        for _ in 0..100 {
            assert!(model.should_drop(), "100% loss should always drop");
        }
    }

    #[test]
    fn test_drop_model_30pct_loss() {
        let mut model = DropModel::new(0.30, 12345);
        let mut drops = 0;
        let trials = 10000;
        for _ in 0..trials {
            if model.should_drop() {
                drops += 1;
            }
        }
        let actual_rate = (drops as f64) / (trials as f64);
        // Allow ±3% variance
        assert!(
            (actual_rate - 0.30).abs() < 0.03,
            "Expected ~30% loss, got {:.2}%",
            actual_rate * 100.0
        );
    }

    #[test]
    fn test_drop_model_deterministic() {
        let mut model1 = DropModel::new(0.30, 54321);
        let mut model2 = DropModel::new(0.30, 54321);
        
        // Same seed should produce identical sequence
        for _ in 0..100 {
            assert_eq!(model1.should_drop(), model2.should_drop());
        }
    }

    #[test]
    fn test_drop_model_reset() {
        let mut model = DropModel::new(0.30, 99999);
        
        // Get first 10 values
        let mut first_run = Vec::new();
        for _ in 0..10 {
            first_run.push(model.should_drop());
        }
        
        // Reset and check sequence repeats
        model.reset();
        for (i, expected) in first_run.iter().enumerate() {
            let actual = model.should_drop();
            assert_eq!(
                &actual, expected,
                "Sequence mismatch after reset at iteration {}",
                i
            );
        }
    }
}
