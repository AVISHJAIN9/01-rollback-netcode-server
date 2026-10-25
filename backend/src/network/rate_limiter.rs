/// Rate limiter for UDP handshake DoS mitigation
/// Implements per-IP handshake rate limiting with a sliding window

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitError {
    /// Client IP has exceeded handshake rate limit
    HandshakeLimited { ip: String, limit: usize, window_secs: u64 },
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitError::HandshakeLimited { ip, limit, window_secs } => {
                write!(f, "Handshake rate limited for {}: {} per {} seconds", ip, limit, window_secs)
            }
        }
    }
}

impl std::error::Error for RateLimitError {}

/// Rate limiter that tracks handshake attempts per IP address
/// with a sliding window of 1 second (configurable)
#[derive(Debug)]
pub struct RateLimiter {
    /// Per-IP tracking: Vec<Instant> of handshake times
    ip_handshakes: HashMap<String, Vec<Instant>>,
    /// Max handshakes per window
    max_handshakes: usize,
    /// Window duration in seconds
    window_secs: u64,
}

impl RateLimiter {
    /// Create a new rate limiter with default settings:
    /// - 10 handshakes per second per IP
    /// - 1 second sliding window
    pub fn new() -> Self {
        Self::with_limits(10, 1)
    }

    /// Create a rate limiter with custom limits
    pub fn with_limits(max_handshakes: usize, window_secs: u64) -> Self {
        Self {
            ip_handshakes: HashMap::new(),
            max_handshakes,
            window_secs,
        }
    }

    /// Check if a handshake from this peer should be rate limited
    /// Returns Ok(()) if allowed, Err if rate limited
    pub fn check_handshake(&mut self, peer: SocketAddr) -> Result<(), RateLimitError> {
        let ip_str = peer.ip().to_string();
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(self.window_secs);

        // Get or create entry for this IP
        let handshakes = self.ip_handshakes.entry(ip_str.clone()).or_insert_with(Vec::new);

        // Remove old handshake times outside the window
        handshakes.retain(|&t| t >= cutoff);

        // Check if we're over the limit
        if handshakes.len() >= self.max_handshakes {
            return Err(RateLimitError::HandshakeLimited {
                ip: ip_str,
                limit: self.max_handshakes,
                window_secs: self.window_secs,
            });
        }

        // Record this handshake
        handshakes.push(now);

        // Clean up IPs with no recent activity to prevent memory leak
        if self.ip_handshakes.len() > 1000 {
            self.ip_handshakes.retain(|_, v| !v.is_empty());
        }

        Ok(())
    }

    /// Get the current count of recent handshakes for a given IP
    pub fn get_handshake_count(&self, peer: SocketAddr) -> usize {
        let ip_str = peer.ip().to_string();
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(self.window_secs);

        if let Some(handshakes) = self.ip_handshakes.get(&ip_str) {
            handshakes.iter().filter(|&&t| t >= cutoff).count()
        } else {
            0
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_below_limit_allowed() {
        let mut limiter = RateLimiter::with_limits(5, 1);
        let peer = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();

        // Send 5 requests (at or below limit) - all should pass
        for i in 0..5 {
            let result = limiter.check_handshake(peer);
            assert!(result.is_ok(), "Request {} should be allowed", i + 1);
        }

        println!("✓ 5 requests within limit all allowed");
    }

    #[test]
    fn test_above_limit_blocked() {
        let mut limiter = RateLimiter::with_limits(5, 1);
        let peer = "127.0.0.1:8001".parse::<SocketAddr>().unwrap();

        // Send 5 requests - should pass
        for _ in 0..5 {
            assert!(limiter.check_handshake(peer).is_ok());
        }

        // 6th request should be blocked
        let result = limiter.check_handshake(peer);
        assert!(matches!(result, Err(RateLimitError::HandshakeLimited { .. })));

        println!("✓ 6th request blocked when limit is 5");
    }

    #[test]
    fn test_independent_ips() {
        let mut limiter = RateLimiter::with_limits(3, 1);
        let peer1 = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();
        let peer2 = "127.0.0.2:8000".parse::<SocketAddr>().unwrap();

        // IP1 sends 3 requests (at limit)
        for _ in 0..3 {
            assert!(limiter.check_handshake(peer1).is_ok());
        }

        // IP2 should still be able to send (independent limit)
        for i in 0..3 {
            let result = limiter.check_handshake(peer2);
            assert!(result.is_ok(), "IP2 request {} should be allowed", i + 1);
        }

        println!("✓ Different IPs have independent limits");
    }
}
