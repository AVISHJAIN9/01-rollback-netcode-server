/// Lossy UDP transport wrapper for chaos testing
/// 
/// Wraps a UDP socket and applies packet loss/jitter based on a drop model.

use crate::network::drop_model::DropModel;

/// A transport that wraps UDP and applies packet loss
pub struct LossyTransport {
    drop_model: DropModel,
}

impl LossyTransport {
    /// Create a new lossy transport with configurable loss rate
    /// 
    /// # Arguments
    /// - `loss_rate`: Fraction of packets to drop (0.0 to 1.0)
    /// - `seed`: Seed for deterministic randomness
    pub fn new(loss_rate: f64, seed: u64) -> Self {
        Self {
            drop_model: DropModel::new(loss_rate, seed),
        }
    }

    /// Check if a packet should be sent or dropped
    pub fn should_send(&mut self) -> bool {
        !self.drop_model.should_drop()
    }

    /// Get current loss rate
    pub fn loss_rate(&self) -> f64 {
        self.drop_model.loss_rate()
    }

    /// Reset the RNG state
    pub fn reset(&mut self) {
        self.drop_model.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lossy_transport_0pct_loss() {
        let mut transport = LossyTransport::new(0.0, 42);
        for _ in 0..100 {
            assert!(transport.should_send(), "0% loss should always send");
        }
    }

    #[test]
    fn test_lossy_transport_100pct_loss() {
        let mut transport = LossyTransport::new(1.0, 42);
        for _ in 0..100 {
            assert!(!transport.should_send(), "100% loss should never send");
        }
    }

    #[test]
    fn test_lossy_transport_30pct_loss() {
        let mut transport = LossyTransport::new(0.30, 12345);
        let mut dropped = 0;
        let trials = 10000;
        for _ in 0..trials {
            if !transport.should_send() {
                dropped += 1;
            }
        }
        let actual_rate = (dropped as f64) / (trials as f64);
        // Allow ±3% variance
        assert!(
            (actual_rate - 0.30).abs() < 0.03,
            "Expected ~30% loss, got {:.2}%",
            actual_rate * 100.0
        );
    }

    #[test]
    fn test_lossy_transport_deterministic() {
        let mut transport1 = LossyTransport::new(0.30, 54321);
        let mut transport2 = LossyTransport::new(0.30, 54321);

        // Same seed should produce identical sequence
        for _ in 0..100 {
            assert_eq!(transport1.should_send(), transport2.should_send());
        }
    }

    #[test]
    fn test_lossy_transport_reset() {
        let mut transport = LossyTransport::new(0.30, 99999);

        // Get first 10 values
        let mut first_run = Vec::new();
        for _ in 0..10 {
            first_run.push(transport.should_send());
        }

        // Reset and check sequence repeats
        transport.reset();
        for (i, expected) in first_run.iter().enumerate() {
            let actual = transport.should_send();
            assert_eq!(
                &actual, expected,
                "Sequence mismatch after reset at iteration {}",
                i
            );
        }
    }
}
