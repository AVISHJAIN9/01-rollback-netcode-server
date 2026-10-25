/// NTP-style clock offset and jitter estimator.
/// Complies with FR-012 and FR-013.
#[derive(Debug, Clone, Default)]
pub struct ClockEstimator {
    pub rtt_ms: f64,
    pub clock_offset_ms: f64,
    pub jitter_ms: f64,
}

impl ClockEstimator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_probe(&mut self, t1: u64, t2: u64, t3: u64, t4: u64) {
        let rtt = (t4.saturating_sub(t1)) as f64 - (t3.saturating_sub(t2)) as f64;
        let offset = ((t2 as f64 - t1 as f64) + (t3 as f64 - t4 as f64)) / 2.0;
        let delta = (rtt - self.rtt_ms).abs();
        self.rtt_ms = rtt.max(0.0);
        self.clock_offset_ms = offset;
        self.jitter_ms = self.jitter_ms * 0.8 + delta * 0.2;
    }
}
