// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Path Validation & Connection Migration (RFC 9000 §9)
// Multi-path tracking, PATH_CHALLENGE/PATH_RESPONSE, and migration triggers

use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Path state for connection migration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathState {
    /// Path is active and receiving data.
    Active,

    /// Path is being validated.
    Validating,

    /// Path has been validated.
    Validated,

    /// Path is abandoned or failed validation.
    Abandoned,
}

/// Individual path for connection migration tracking.
#[derive(Clone, Debug)]
pub struct Path {
    /// Remote socket address for this path.
    pub remote_addr: SocketAddr,

    /// Local socket address (if known).
    pub local_addr: Option<SocketAddr>,

    /// Path state.
    pub state: PathState,

    /// Last time we received data on this path.
    pub last_active: Instant,

    /// RTT estimate for this path.
    pub rtt: Option<Duration>,

    /// Minimum RTT observed on this path.
    pub min_rtt: Option<Duration>,

    /// MTU discovered on this path (via PMTUD if applicable).
    pub mtu: usize,

    /// Number of packets sent on this path.
    pub packets_sent: u64,

    /// Number of packets received on this path.
    pub packets_received: u64,

    /// PATH_CHALLENGE payload (8 bytes) for validation.
    pub path_challenge_payload: Option<[u8; 8]>,

    /// Time PATH_CHALLENGE was sent.
    pub challenge_sent_time: Option<Instant>,

    /// Whether this path has been acknowledged by the peer (valid).
    pub is_peer_validated: bool,
}

impl Path {
    /// Create a new path.
    pub fn new(remote_addr: SocketAddr, mtu: usize) -> Self {
        Path {
            remote_addr,
            local_addr: None,
            state: PathState::Validating,
            last_active: Instant::now(),
            rtt: None,
            min_rtt: None,
            mtu,
            packets_sent: 0,
            packets_received: 0,
            path_challenge_payload: None,
            challenge_sent_time: None,
            is_peer_validated: false,
        }
    }

    /// Mark this path as active.
    pub fn on_packet_received(&mut self) {
        self.last_active = Instant::now();
        self.packets_received += 1;

        if self.state == PathState::Validating {
            self.state = PathState::Validated;
        }
    }

    /// Record RTT sample on this path.
    pub fn update_rtt(&mut self, rtt: Duration) {
        self.rtt = Some(rtt);
        self.min_rtt = Some(match self.min_rtt {
            None => rtt,
            Some(min) => if rtt < min { rtt } else { min },
        });
    }

    /// Check if path is idle (no activity for timeout duration).
    pub fn is_idle(&self, timeout: Duration) -> bool {
        Instant::now().duration_since(self.last_active) > timeout
    }

    /// Get path state name for debugging.
    pub fn state_name(&self) -> &'static str {
        match self.state {
            PathState::Active => "active",
            PathState::Validating => "validating",
            PathState::Validated => "validated",
            PathState::Abandoned => "abandoned",
        }
    }
}

/// Path manager for multi-path scenarios and connection migration.
#[derive(Clone, Debug)]
pub struct PathManager {
    /// Active path (current sending path).
    active_path: Option<Path>,

    /// Candidate paths (being validated or in reserve).
    candidate_paths: Vec<Path>,

    /// Maximum number of candidate paths to track.
    max_paths: usize,

    /// Idle timeout for paths.
    idle_timeout: Duration,

    /// MTU baseline (typically 1200 bytes for QUIC Initial).
    mtu: usize,

    /// Telemetry: number of migrations.
    migrations_count: u32,

    /// Telemetry: validation failures.
    validation_failures: u32,
}

impl PathManager {
    /// Create a new path manager with initial active path.
    pub fn new(initial_addr: SocketAddr, mtu: usize) -> Self {
        PathManager {
            active_path: Some(Path::new(initial_addr, mtu)),
            candidate_paths: Vec::new(),
            max_paths: 4,
            idle_timeout: Duration::from_secs(600), // 10 minutes
            mtu,
            migrations_count: 0,
            validation_failures: 0,
        }
    }

    /// Get the active path.
    pub fn active_path(&self) -> Option<&Path> {
        self.active_path.as_ref()
    }

    /// Get mutable reference to active path.
    pub fn active_path_mut(&mut self) -> Option<&mut Path> {
        self.active_path.as_mut()
    }

    /// Register a new candidate path (e.g., from NAT rebinding).
    pub fn add_candidate_path(&mut self, remote_addr: SocketAddr) -> Option<()> {
        // Check if path already exists
        if self.active_path.as_ref().map_or(false, |p| p.remote_addr == remote_addr) {
            return Some(());
        }

        if self.candidate_paths.iter().any(|p| p.remote_addr == remote_addr) {
            return Some(());
        }

        if self.candidate_paths.len() >= self.max_paths {
            return None; // Too many candidate paths
        }

        let mut path = Path::new(remote_addr, self.mtu);
        path.state = PathState::Validating;
        self.candidate_paths.push(path);

        Some(())
    }

    /// Validate a path (mark as peer-validated after PATH_RESPONSE).
    pub fn validate_path(&mut self, remote_addr: SocketAddr) -> Result<(), String> {
        if let Some(active) = &mut self.active_path {
            if active.remote_addr == remote_addr {
                active.is_peer_validated = true;
                active.state = PathState::Validated;
                return Ok(());
            }
        }

        if let Some(candidate) = self.candidate_paths.iter_mut().find(|p| p.remote_addr == remote_addr) {
            candidate.is_peer_validated = true;
            candidate.state = PathState::Validated;
            return Ok(());
        }

        Err("Path not found".to_string())
    }

    /// Attempt migration to a validated candidate path.
    pub fn migrate_to_path(&mut self, remote_addr: SocketAddr) -> Result<(), String> {
        let candidate = self
            .candidate_paths
            .iter()
            .position(|p| p.remote_addr == remote_addr && p.is_peer_validated)
            .ok_or_else(|| "Path not validated or not found".to_string())?;

        let mut new_active = self.candidate_paths.remove(candidate);
        new_active.state = PathState::Active;

        if let Some(old_active) = self.active_path.take() {
            self.candidate_paths.push(old_active);
        }

        self.active_path = Some(new_active);
        self.migrations_count += 1;

        Ok(())
    }

    /// Clean up idle paths.
    pub fn cleanup_idle_paths(&mut self) {
        self.candidate_paths.retain(|p| !p.is_idle(self.idle_timeout));
    }

    /// Get migrations count.
    pub fn migrations_count(&self) -> u32 {
        self.migrations_count
    }

    /// Get validation failures count.
    pub fn validation_failures(&self) -> u32 {
        self.validation_failures
    }

    /// Record validation failure.
    pub fn record_validation_failure(&mut self) {
        self.validation_failures += 1;
    }

    /// Get all validated paths (including active).
    pub fn validated_paths(&self) -> Vec<SocketAddr> {
        let mut addrs = Vec::new();

        if let Some(active) = &self.active_path {
            if active.is_peer_validated {
                addrs.push(active.remote_addr);
            }
        }

        for candidate in &self.candidate_paths {
            if candidate.is_peer_validated {
                addrs.push(candidate.remote_addr);
            }
        }

        addrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_path_creation() {
        let addr = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let path = Path::new(addr, 1200);

        assert_eq!(path.remote_addr, addr);
        assert_eq!(path.state, PathState::Validating);
        assert_eq!(path.mtu, 1200);
    }

    #[test]
    fn test_path_on_packet_received() {
        let addr = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let mut path = Path::new(addr, 1200);

        path.on_packet_received();
        assert_eq!(path.packets_received, 1);
        assert_eq!(path.state, PathState::Validated);
    }

    #[test]
    fn test_path_update_rtt() {
        let addr = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let mut path = Path::new(addr, 1200);

        path.update_rtt(Duration::from_millis(50));
        assert_eq!(path.rtt, Some(Duration::from_millis(50)));
        assert_eq!(path.min_rtt, Some(Duration::from_millis(50)));

        path.update_rtt(Duration::from_millis(30));
        assert_eq!(path.min_rtt, Some(Duration::from_millis(30)));
    }

    #[test]
    fn test_path_manager_init() {
        let addr = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let mgr = PathManager::new(addr, 1200);

        assert!(mgr.active_path().is_some());
        assert_eq!(mgr.active_path().unwrap().remote_addr, addr);
    }

    #[test]
    fn test_path_manager_add_candidate() {
        let addr1 = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let addr2 = SocketAddr::from_str("127.0.0.1:5678").unwrap();

        let mut mgr = PathManager::new(addr1, 1200);
        let result = mgr.add_candidate_path(addr2);

        assert!(result.is_some());
        assert_eq!(mgr.candidate_paths.len(), 1);
    }

    #[test]
    fn test_path_manager_migration() {
        let addr1 = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let addr2 = SocketAddr::from_str("127.0.0.1:5678").unwrap();

        let mut mgr = PathManager::new(addr1, 1200);
        mgr.add_candidate_path(addr2).unwrap();
        mgr.validate_path(addr2).unwrap();
        mgr.migrate_to_path(addr2).unwrap();

        assert_eq!(mgr.active_path().unwrap().remote_addr, addr2);
        assert_eq!(mgr.migrations_count(), 1);
    }

    #[test]
    fn test_path_manager_validated_paths() {
        let addr1 = SocketAddr::from_str("127.0.0.1:1234").unwrap();
        let addr2 = SocketAddr::from_str("127.0.0.1:5678").unwrap();

        let mut mgr = PathManager::new(addr1, 1200);
        mgr.active_path_mut().unwrap().is_peer_validated = true;
        mgr.add_candidate_path(addr2).unwrap();
        mgr.validate_path(addr2).unwrap();

        let validated = mgr.validated_paths();
        assert_eq!(validated.len(), 2);
    }
}
