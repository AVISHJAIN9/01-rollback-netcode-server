// Orphan session cleanup task using fake clock
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrphanCleanupTask {
    stale_threshold: Duration,
    last_run: Instant,
}

impl OrphanCleanupTask {
    pub fn new(stale_threshold_secs: u64) -> Self {
        Self {
            stale_threshold: Duration::from_secs(stale_threshold_secs),
            last_run: Instant::now(),
        }
    }

    /// Mark session as stale if heartbeat older than threshold
    pub fn mark_stale_sessions(
        &self,
        sessions: &[(Uuid, Instant)],
        now: Instant,
    ) -> Vec<Uuid> {
        sessions
            .iter()
            .filter_map(|(id, heartbeat)| {
                if now.duration_since(*heartbeat) > self.stale_threshold {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if cleanup should run
    pub fn should_run(&self, now: Instant) -> bool {
        now.duration_since(self.last_run) >= Duration::from_secs(10)
    }

    /// Record cleanup execution
    pub fn record_run(&mut self, now: Instant) {
        self.last_run = now;
    }

    /// Refresh heartbeat (prevents marking as stale)
    pub fn refresh_heartbeat(heartbeat: &mut Instant, now: Instant) {
        *heartbeat = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_stale_after_40s() {
        let cleanup = OrphanCleanupTask::new(40);
        let base = Instant::now();
        let session_id = Uuid::new_v4();

        // Session with heartbeat 50s ago
        let old_heartbeat = base - Duration::from_secs(50);
        let stale = cleanup.mark_stale_sessions(&[(session_id, old_heartbeat)], base);

        assert_eq!(stale, vec![session_id], "Session older than 40s should be marked stale");
    }

    #[test]
    fn test_active_session_not_stale() {
        let cleanup = OrphanCleanupTask::new(40);
        let base = Instant::now();
        let session_id = Uuid::new_v4();

        // Session with heartbeat 30s ago (within threshold)
        let fresh_heartbeat = base - Duration::from_secs(30);
        let stale = cleanup.mark_stale_sessions(&[(session_id, fresh_heartbeat)], base);

        assert!(stale.is_empty(), "Session younger than 40s should not be marked stale");
    }

    #[test]
    fn test_heartbeat_refresh() {
        let base = Instant::now();
        let mut heartbeat = base - Duration::from_secs(50);

        OrphanCleanupTask::refresh_heartbeat(&mut heartbeat, base);

        assert_eq!(heartbeat, base, "Heartbeat should be refreshed to now");

        let cleanup = OrphanCleanupTask::new(40);
        let session_id = Uuid::new_v4();
        let stale = cleanup.mark_stale_sessions(&[(session_id, heartbeat)], base);

        assert!(stale.is_empty(), "Refreshed heartbeat should prevent stale marking");
    }
}
