// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Anti-Amplification Defense (RFC 9000 §8.1)
// 3x received byte quota on unvalidated paths to prevent amplification attacks

/// Anti-amplification defense tracker per path.
#[derive(Clone, Debug)]
pub struct AntiAmplificationDefense {
    /// Bytes received from peer on this path.
    bytes_received: u64,

    /// Bytes allowed to send (3x received bytes).
    bytes_allowed: u64,

    /// Bytes already sent.
    bytes_sent: u64,

    /// Whether the path has been validated (after address verification).
    is_validated: bool,
}

impl AntiAmplificationDefense {
    pub fn new() -> Self {
        AntiAmplificationDefense {
            bytes_received: 0,
            bytes_allowed: 0,
            bytes_sent: 0,
            is_validated: false,
        }
    }

    /// Record bytes received from peer.
    ///
    /// Updates the allowed send quota (3x rule).
    pub fn on_bytes_received(&mut self, bytes: u64) {
        self.bytes_received += bytes;
        self.bytes_allowed = self.bytes_received.saturating_mul(3);
    }

    /// Check if we can send the given number of bytes.
    pub fn can_send(&self, bytes: u64) -> bool {
        if self.is_validated {
            return true; // No limit on validated paths
        }

        self.bytes_sent + bytes <= self.bytes_allowed
    }

    /// Record bytes sent.
    pub fn on_bytes_sent(&mut self, bytes: u64) {
        self.bytes_sent += bytes;
    }

    /// Mark path as validated (removes anti-amplification limit).
    pub fn validate_path(&mut self) {
        self.is_validated = true;
    }

    /// Get available send quota (remaining bytes under 3x limit).
    pub fn available_quota(&self) -> u64 {
        if self.is_validated {
            u64::MAX
        } else {
            self.bytes_allowed.saturating_sub(self.bytes_sent)
        }
    }

    /// Get bytes received so far.
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }

    /// Get bytes sent so far.
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Check if path is validated.
    pub fn is_validated(&self) -> bool {
        self.is_validated
    }

    /// Reset state (e.g., on path close).
    pub fn reset(&mut self) {
        self.bytes_received = 0;
        self.bytes_allowed = 0;
        self.bytes_sent = 0;
        self.is_validated = false;
    }
}

/// Multi-path anti-amplification manager.
#[derive(Clone, Debug)]
pub struct AntiAmplificationManager {
    /// Per-path defense trackers (indexed by path identifier).
    defenses: std::collections::HashMap<String, AntiAmplificationDefense>,
}

impl AntiAmplificationManager {
    pub fn new() -> Self {
        AntiAmplificationManager {
            defenses: std::collections::HashMap::new(),
        }
    }

    /// Get or create defense for a path.
    pub fn get_or_create(&mut self, path_id: &str) -> &mut AntiAmplificationDefense {
        self.defenses
            .entry(path_id.to_string())
            .or_insert_with(AntiAmplificationDefense::new)
    }

    /// Record bytes received on a path.
    pub fn on_bytes_received(&mut self, path_id: &str, bytes: u64) {
        self.get_or_create(path_id).on_bytes_received(bytes);
    }

    /// Check if we can send on a path.
    pub fn can_send(&self, path_id: &str, bytes: u64) -> bool {
        self.defenses
            .get(path_id)
            .map_or(false, |d| d.can_send(bytes))
    }

    /// Record bytes sent on a path.
    pub fn on_bytes_sent(&mut self, path_id: &str, bytes: u64) {
        if let Some(defense) = self.defenses.get_mut(path_id) {
            defense.on_bytes_sent(bytes);
        }
    }

    /// Validate a path (removes limit).
    pub fn validate_path(&mut self, path_id: &str) {
        if let Some(defense) = self.defenses.get_mut(path_id) {
            defense.validate_path();
        }
    }

    /// Remove a path.
    pub fn remove_path(&mut self, path_id: &str) {
        self.defenses.remove(path_id);
    }

    /// Get available quota for a path.
    pub fn available_quota(&self, path_id: &str) -> u64 {
        self.defenses
            .get(path_id)
            .map_or(0, |d| d.available_quota())
    }
}

impl Default for AntiAmplificationDefense {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for AntiAmplificationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anti_amplification_init() {
        let defense = AntiAmplificationDefense::new();
        assert_eq!(defense.bytes_received(), 0);
        assert_eq!(defense.bytes_sent(), 0);
        assert!(!defense.is_validated());
    }

    #[test]
    fn test_anti_amplification_3x_rule() {
        let mut defense = AntiAmplificationDefense::new();

        // Receive 100 bytes
        defense.on_bytes_received(100);

        // Can send up to 300 bytes
        assert!(defense.can_send(300));
        assert!(!defense.can_send(301));

        // Send 200 bytes
        defense.on_bytes_sent(200);
        assert_eq!(defense.available_quota(), 100);
    }

    #[test]
    fn test_anti_amplification_validation() {
        let mut defense = AntiAmplificationDefense::new();

        defense.on_bytes_received(100);
        defense.validate_path();

        // After validation, can send unlimited
        assert!(defense.can_send(1000000));
        assert!(defense.is_validated());
    }

    #[test]
    fn test_anti_amplification_manager() {
        let mut mgr = AntiAmplificationManager::new();

        mgr.on_bytes_received("path1", 100);
        assert!(mgr.can_send("path1", 300));
        assert!(!mgr.can_send("path1", 301));

        mgr.on_bytes_sent("path1", 200);
        assert_eq!(mgr.available_quota("path1"), 100);

        mgr.validate_path("path1");
        assert!(mgr.can_send("path1", 1000000));
    }

    #[test]
    fn test_anti_amplification_reset() {
        let mut defense = AntiAmplificationDefense::new();

        defense.on_bytes_received(100);
        defense.on_bytes_sent(50);
        defense.reset();

        assert_eq!(defense.bytes_received(), 0);
        assert_eq!(defense.bytes_sent(), 0);
    }
}
