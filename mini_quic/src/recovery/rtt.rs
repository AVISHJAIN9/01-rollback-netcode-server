// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: RTT Estimation (RFC 9002 §5.3)
// SRTT, RTTVAR, Minimum RTT, and Probe Timeout (PTO) computation

use std::time::Duration;

/// RTT Estimator implementing RFC 9002 §5.3 algorithms.
///
/// Tracks Sample RTT, Smoothed RTT (SRTT), RTT Variation (RTTVAR), Minimum RTT,
/// and computes Probe Timeout (PTO) for loss detection.
#[derive(Clone, Debug)]
pub struct RttEstimator {
    /// Latest sample RTT from a packet acknowledgment.
    sample_rtt: Option<Duration>,

    /// Smoothed RTT (SRTT): exponential moving average of sample RTT.
    srtt: Option<Duration>,

    /// RTT Variation (RTTVAR): exponential moving average of RTT samples.
    rttvar: Duration,

    /// Minimum RTT observed in connection lifetime.
    min_rtt: Option<Duration>,

    /// Maximum ACK delay observed from peer (transport parameter).
    max_ack_delay: Duration,

    /// RTT samples count for initial RTT convergence.
    samples_count: u32,
}

impl RttEstimator {
    /// Create a new RTT estimator with sensible defaults.
    ///
    /// RFC 9002 suggests:
    /// - Initial min_rtt: uninitialized
    /// - Initial SRTT: uninitialized
    /// - Initial RTTVAR: 333 ms
    /// - max_ack_delay: transport parameter (default 25 ms)
    pub fn new(max_ack_delay: Duration) -> Self {
        RttEstimator {
            sample_rtt: None,
            srtt: None,
            rttvar: Duration::from_millis(333),
            min_rtt: None,
            max_ack_delay,
            samples_count: 0,
        }
    }

    /// Update RTT estimator with a new sample.
    ///
    /// RFC 9002 §5.3: On first RTT sample, set SRTT and RTTVAR. On subsequent samples,
    /// use exponential moving averages with smoothing constants.
    pub fn update_sample(&mut self, sample_rtt: Duration) {
        self.samples_count += 1;
        self.sample_rtt = Some(sample_rtt);

        // Update min_rtt
        self.min_rtt = Some(match self.min_rtt {
            None => sample_rtt,
            Some(min) => if sample_rtt < min { sample_rtt } else { min },
        });

        // Adjust sample for max_ack_delay (remove ack delay from sample).
        // This assumes the sample was taken as: send_time to ack_time - max_ack_delay.
        let ack_delay_sample = if sample_rtt > self.max_ack_delay {
            sample_rtt - self.max_ack_delay
        } else {
            sample_rtt
        };

        match self.srtt {
            None => {
                // First RTT sample: initialize SRTT and RTTVAR
                self.srtt = Some(ack_delay_sample);
                self.rttvar = ack_delay_sample / 2;
            }
            Some(srtt) => {
                // RFC 9002 Equation 5: RTTVAR_new = (3/4) * RTTVAR + (1/4) * |SRTT - sample|
                let rtt_var_sample = if ack_delay_sample > srtt {
                    ack_delay_sample - srtt
                } else {
                    srtt - ack_delay_sample
                };

                self.rttvar = self.rttvar * 3 / 4 + rtt_var_sample / 4;

                // RFC 9002 Equation 4: SRTT_new = (7/8) * SRTT + (1/8) * sample
                self.srtt = Some(srtt * 7 / 8 + ack_delay_sample / 8);
            }
        }
    }

    /// Compute Probe Timeout (PTO) for RTO-style loss detection.
    ///
    /// RFC 9002 §6.2 Equation 6:
    /// PTO = SRTT + max(4 * RTTVAR, kGranularity) + max_ack_delay
    ///
    /// kGranularity is typically 1 ms.
    pub fn compute_pto(&self) -> Duration {
        const K_GRANULARITY: Duration = Duration::from_millis(1);

        let srtt = self.srtt.unwrap_or(Duration::from_millis(333));
        let rttvar_term = {
            let term = self.rttvar * 4;
            if term > K_GRANULARITY { term } else { K_GRANULARITY }
        };

        srtt + rttvar_term + self.max_ack_delay
    }

    /// Compute PTO with exponential backoff for multiple consecutive PTOs.
    ///
    /// RFC 9002: PTO backoff doubles on each consecutive loss.
    /// pto_count: number of consecutive PTOs fired (0 for first, 1 for second, etc.)
    pub fn compute_pto_with_backoff(&self, pto_count: u32) -> Duration {
        let base_pto = self.compute_pto();
        let backoff = 1u64 << pto_count.min(31); // Avoid overflow; cap at 2^31
        base_pto.saturating_mul(backoff as u32)
    }

    /// Get the current Smoothed RTT estimate.
    pub fn srtt(&self) -> Option<Duration> {
        self.srtt
    }

    /// Get the current RTT Variation estimate.
    pub fn rttvar(&self) -> Duration {
        self.rttvar
    }

    /// Get the minimum RTT observed in this connection.
    pub fn min_rtt(&self) -> Option<Duration> {
        self.min_rtt
    }

    /// Get the latest sample RTT.
    pub fn sample_rtt(&self) -> Option<Duration> {
        self.sample_rtt
    }

    /// Get the maximum ACK delay peer parameter.
    pub fn max_ack_delay(&self) -> Duration {
        self.max_ack_delay
    }

    /// Get number of RTT samples collected so far.
    pub fn samples_count(&self) -> u32 {
        self.samples_count
    }

    /// Reset RTT estimator state (e.g., on connection migration).
    pub fn reset(&mut self) {
        self.sample_rtt = None;
        self.srtt = None;
        self.rttvar = Duration::from_millis(333);
        self.min_rtt = None;
        self.samples_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtt_estimator_initialization() {
        let est = RttEstimator::new(Duration::from_millis(25));
        assert_eq!(est.srtt(), None);
        assert_eq!(est.min_rtt(), None);
        assert_eq!(est.rttvar(), Duration::from_millis(333));
    }

    #[test]
    fn test_rtt_first_sample() {
        let mut est = RttEstimator::new(Duration::from_millis(25));
        est.update_sample(Duration::from_millis(100));

        assert_eq!(est.srtt(), Some(Duration::from_millis(100)));
        assert_eq!(est.min_rtt(), Some(Duration::from_millis(100)));
        assert_eq!(est.rttvar(), Duration::from_millis(50));
        assert_eq!(est.samples_count(), 1);
    }

    #[test]
    fn test_rtt_subsequent_samples() {
        let mut est = RttEstimator::new(Duration::from_millis(25));
        est.update_sample(Duration::from_millis(100));
        est.update_sample(Duration::from_millis(110));
        est.update_sample(Duration::from_millis(90));

        assert!(est.srtt().unwrap() > Duration::from_millis(90));
        assert!(est.srtt().unwrap() < Duration::from_millis(110));
        assert_eq!(est.min_rtt(), Some(Duration::from_millis(90)));
    }

    #[test]
    fn test_pto_computation() {
        let mut est = RttEstimator::new(Duration::from_millis(25));
        est.update_sample(Duration::from_millis(100));

        let pto = est.compute_pto();
        // PTO = SRTT + max(4 * RTTVAR, 1ms) + max_ack_delay
        // = 100 + max(4 * 50, 1) + 25 = 100 + 200 + 25 = 325
        assert_eq!(pto, Duration::from_millis(325));
    }

    #[test]
    fn test_pto_backoff() {
        let mut est = RttEstimator::new(Duration::from_millis(25));
        est.update_sample(Duration::from_millis(100));

        let pto0 = est.compute_pto_with_backoff(0);
        let pto1 = est.compute_pto_with_backoff(1);
        let pto2 = est.compute_pto_with_backoff(2);

        assert_eq!(pto1, pto0 * 2);
        assert_eq!(pto2, pto0 * 4);
    }

    #[test]
    fn test_rtt_reset() {
        let mut est = RttEstimator::new(Duration::from_millis(25));
        est.update_sample(Duration::from_millis(100));
        est.reset();

        assert_eq!(est.srtt(), None);
        assert_eq!(est.min_rtt(), None);
        assert_eq!(est.samples_count(), 0);
    }
}
