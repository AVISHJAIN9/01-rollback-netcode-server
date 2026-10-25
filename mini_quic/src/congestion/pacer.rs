// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Token Bucket Packet Pacer
// Microsecond-precision rate limiting for congestion control

use std::time::{Duration, Instant};

/// Token-Bucket Packet Pacer for rate-limiting packet transmission.
///
/// Implements token bucket algorithm with microsecond precision for smooth pacing.
/// Tokens represent bytes that can be transmitted; refill rate is determined by
/// the sender's bandwidth estimate and congestion window.
#[derive(Clone, Debug)]
pub struct TokenBucketPacer {
    /// Current tokens available (in bytes).
    tokens: f64,

    /// Maximum tokens the bucket can hold (typically congestion window).
    max_tokens: u64,

    /// Token refill rate (bytes per second).
    refill_rate: f64,

    /// Last time the bucket was refilled.
    last_refill: Instant,

    /// Minimum gap between packet sends (at least 1 microsecond for pacing).
    min_packet_gap: Duration,

    /// Current pacing delay (computed from rate).
    pacing_delay: Duration,

    /// Telemetry: packets delayed by pacer.
    packets_delayed: u64,

    /// Telemetry: total bytes paced.
    bytes_paced: u64,
}

impl TokenBucketPacer {
    /// Create a new token bucket pacer.
    ///
    /// `max_tokens`: maximum tokens in bucket (usually congestion window size).
    /// `refill_rate`: tokens per second (bytes/sec, from bandwidth estimate).
    pub fn new(max_tokens: u64, refill_rate: f64) -> Self {
        let now = Instant::now();

        // Initialize with full bucket
        let tokens = max_tokens as f64;

        // Pacing delay = 1 / rate (in seconds, then convert to Duration)
        let pacing_delay = if refill_rate > 0.0 {
            Duration::from_secs_f64(1.0 / refill_rate)
        } else {
            Duration::from_micros(1) // Minimal delay if no rate set
        };

        TokenBucketPacer {
            tokens,
            max_tokens,
            refill_rate,
            last_refill: now,
            min_packet_gap: Duration::from_micros(100), // Minimum 100μs gap
            pacing_delay,
            packets_delayed: 0,
            bytes_paced: 0,
        }
    }

    /// Refill tokens based on elapsed time since last refill.
    fn refill_tokens(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill);
        let elapsed_secs = elapsed.as_secs_f64();

        // Tokens to add = refill_rate * elapsed
        let tokens_to_add = self.refill_rate * elapsed_secs;
        self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens as f64);

        self.last_refill = now;
    }

    /// Check if we can send a packet of given size without waiting.
    ///
    /// Returns `true` if we have enough tokens, `false` if we need to wait.
    pub fn can_send(&mut self, packet_size: usize, now: Instant) -> bool {
        self.refill_tokens(now);
        self.tokens >= packet_size as f64
    }

    /// Consume tokens for sending a packet.
    ///
    /// Returns the time we must wait before sending. If wait time is 0, send immediately.
    pub fn send(&mut self, packet_size: usize, now: Instant) -> Duration {
        self.refill_tokens(now);

        if self.tokens >= packet_size as f64 {
            // Send immediately
            self.tokens -= packet_size as f64;
            self.bytes_paced += packet_size as u64;
            Duration::from_secs(0)
        } else {
            // Need to wait for tokens
            let tokens_deficit = (packet_size as f64) - self.tokens;
            self.packets_delayed += 1;

            // Time to refill needed tokens
            if self.refill_rate > 0.0 {
                let wait_secs = tokens_deficit / self.refill_rate;
                let wait_duration = Duration::from_secs_f64(wait_secs.max(self.min_packet_gap.as_secs_f64()));
                wait_duration
            } else {
                self.min_packet_gap
            }
        }
    }

    /// Update the refill rate (bandwidth estimate).
    /// Call this when bandwidth estimate changes.
    pub fn update_rate(&mut self, new_rate: f64) {
        self.refill_rate = new_rate;

        // Update pacing delay
        self.pacing_delay = if new_rate > 0.0 {
            Duration::from_secs_f64(1.0 / new_rate)
        } else {
            Duration::from_micros(1)
        };
    }

    /// Update the maximum tokens (bucket size).
    /// Call this when congestion window changes.
    pub fn update_max_tokens(&mut self, new_max: u64) {
        self.max_tokens = new_max;
        // Cap current tokens at new max
        self.tokens = self.tokens.min(new_max as f64);
    }

    /// Get current pacing delay (time between packets at current rate).
    pub fn pacing_delay(&self) -> Duration {
        self.pacing_delay
    }

    /// Get current tokens available.
    pub fn tokens(&self) -> f64 {
        self.tokens
    }

    /// Get refill rate in bytes/sec.
    pub fn refill_rate(&self) -> f64 {
        self.refill_rate
    }

    /// Get packets delayed by pacer.
    pub fn packets_delayed(&self) -> u64 {
        self.packets_delayed
    }

    /// Get bytes paced so far.
    pub fn bytes_paced(&self) -> u64 {
        self.bytes_paced
    }

    /// Reset pacer state.
    pub fn reset(&mut self, now: Instant) {
        self.tokens = self.max_tokens as f64;
        self.last_refill = now;
        self.packets_delayed = 0;
        self.bytes_paced = 0;
    }

    /// Compute time until next packet can be sent.
    pub fn next_send_time(&mut self, packet_size: usize, now: Instant) -> Instant {
        let wait_duration = self.send(packet_size, now);
        now + wait_duration
    }
}

/// Advanced pacer with multiple priority buckets for different frame types.
#[derive(Clone, Debug)]
pub struct PriorityPacer {
    /// High-priority bucket (ACKs, control frames): 20% of bandwidth.
    high_priority: TokenBucketPacer,

    /// Normal-priority bucket (stream data): 70% of bandwidth.
    normal_priority: TokenBucketPacer,

    /// Low-priority bucket (probes, idle): 10% of bandwidth.
    low_priority: TokenBucketPacer,

    /// Total bandwidth estimate.
    total_bandwidth: f64,
}

impl PriorityPacer {
    /// Create a priority-based pacer.
    pub fn new(total_bandwidth: f64, max_window: u64, _now: Instant) -> Self {
        PriorityPacer {
            high_priority: TokenBucketPacer::new(
                (max_window as f64 * 0.2) as u64,
                total_bandwidth * 0.2,
            ),
            normal_priority: TokenBucketPacer::new(
                (max_window as f64 * 0.7) as u64,
                total_bandwidth * 0.7,
            ),
            low_priority: TokenBucketPacer::new(
                (max_window as f64 * 0.1) as u64,
                total_bandwidth * 0.1,
            ),
            total_bandwidth,
        }
    }

    /// Check if we can send a high-priority frame.
    pub fn can_send_high_priority(&mut self, size: usize, now: Instant) -> bool {
        self.high_priority.can_send(size, now)
    }

    /// Check if we can send normal-priority data.
    pub fn can_send_normal(&mut self, size: usize, now: Instant) -> bool {
        self.normal_priority.can_send(size, now)
    }

    /// Check if we can send low-priority data.
    pub fn can_send_low_priority(&mut self, size: usize, now: Instant) -> bool {
        self.low_priority.can_send(size, now)
    }

    /// Send high-priority frame and return wait time.
    pub fn send_high_priority(&mut self, size: usize, now: Instant) -> Duration {
        self.high_priority.send(size, now)
    }

    /// Send normal-priority data and return wait time.
    pub fn send_normal(&mut self, size: usize, now: Instant) -> Duration {
        self.normal_priority.send(size, now)
    }

    /// Send low-priority data and return wait time.
    pub fn send_low_priority(&mut self, size: usize, now: Instant) -> Duration {
        self.low_priority.send(size, now)
    }

    /// Update total bandwidth and rebalance buckets.
    pub fn update_bandwidth(&mut self, new_total: f64) {
        self.total_bandwidth = new_total;

        self.high_priority.update_rate(new_total * 0.2);
        self.normal_priority.update_rate(new_total * 0.7);
        self.low_priority.update_rate(new_total * 0.1);
    }

    /// Reset pacer state.
    pub fn reset(&mut self, now: Instant) {
        self.high_priority.reset(now);
        self.normal_priority.reset(now);
        self.low_priority.reset(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_init() {
        let pacer = TokenBucketPacer::new(10000, 100_000.0); // 100KB/s
        assert_eq!(pacer.tokens() as u64, 10000);
        assert_eq!(pacer.refill_rate(), 100_000.0);
    }

    #[test]
    fn test_token_bucket_send_immediate() {
        let mut pacer = TokenBucketPacer::new(10000, 100_000.0);
        let now = Instant::now();

        let wait = pacer.send(500, now);
        assert_eq!(wait, Duration::from_secs(0));
        assert_eq!(pacer.bytes_paced(), 500);
    }

    #[test]
    fn test_token_bucket_send_delayed() {
        let mut pacer = TokenBucketPacer::new(500, 100_000.0); // Only 500 bytes capacity
        let now = Instant::now();

        // Try to send 1000 bytes, should be delayed
        let wait = pacer.send(1000, now);
        assert!(wait > Duration::from_secs(0));
        assert_eq!(pacer.packets_delayed(), 1);
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut pacer = TokenBucketPacer::new(10000, 100_000.0); // 100KB/s
        let now = Instant::now();

        // Send 5000 bytes
        pacer.send(5000, now);
        assert!((pacer.tokens() - 5000.0).abs() < 1.0);

        // Wait 0.05 seconds, should refill 5000 bytes
        let later = now + Duration::from_millis(50);
        assert!(pacer.can_send(5000, later));
    }

    #[test]
    fn test_token_bucket_rate_update() {
        let mut pacer = TokenBucketPacer::new(10000, 100_000.0);
        pacer.update_rate(200_000.0);

        assert_eq!(pacer.refill_rate(), 200_000.0);
        assert!(pacer.pacing_delay() < Duration::from_micros(10));
    }

    #[test]
    fn test_priority_pacer_init() {
        let now = Instant::now();
        let priority = PriorityPacer::new(100_000.0, 10000, now);

        assert_eq!(priority.high_priority.refill_rate(), 20_000.0);
        assert_eq!(priority.normal_priority.refill_rate(), 70_000.0);
        assert_eq!(priority.low_priority.refill_rate(), 10_000.0);
    }

    #[test]
    fn test_priority_pacer_send() {
        let mut priority = PriorityPacer::new(100_000.0, 10000, Instant::now());
        let now = Instant::now();

        let wait_high = priority.send_high_priority(100, now);
        let wait_normal = priority.send_normal(100, now);
        let wait_low = priority.send_low_priority(100, now);

        assert_eq!(wait_high, Duration::from_secs(0));
        assert_eq!(wait_normal, Duration::from_secs(0));
        assert_eq!(wait_low, Duration::from_secs(0));
    }
}
