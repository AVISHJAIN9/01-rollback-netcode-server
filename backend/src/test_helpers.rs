/// Test helpers for integration tests (skip logic, service waits)
/// 
/// This module provides utilities for testing with optional Postgres/Redis integration.
/// Tests can be skipped if services are unavailable via ALLOW_SKIP_INTEGRATION=1 env var.

use std::env;
use std::time::Duration;
use tokio::time::sleep;

/// Check if integration tests should skip when services are unavailable
pub fn should_allow_skips() -> bool {
    env::var("ALLOW_SKIP_INTEGRATION")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Skip a test and print reason
/// 
/// # Example
/// ```ignore
/// if let Err(e) = get_postgres_pool().await {
///     if e == "SKIPPED" {
///         return;  // Test skipped gracefully
///     }
/// }
/// ```
pub fn skip_test(reason: &str) -> String {
    println!("SKIPPED: {}", reason);
    "SKIPPED".to_string()
}

/// Wait for a TCP service to be available
/// 
/// # Arguments
/// - `host`: Hostname or IP (e.g., "localhost")
/// - `port`: Port number (e.g., 5432 for Postgres)
/// - `timeout_secs`: Timeout in seconds
/// 
/// # Returns
/// `true` if service became available, `false` on timeout
pub async fn wait_for_service(host: &str, port: u16, timeout_secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Ok(_) = tokio::net::TcpStream::connect(format!("{}:{}", host, port)).await {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100)).await;
    }
}

/// Global test counters (for summary output)
#[cfg(test)]
pub mod counters {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SKIPPED_COUNT: AtomicUsize = AtomicUsize::new(0);
    static PASSED_COUNT: AtomicUsize = AtomicUsize::new(0);

    pub fn record_pass() {
        PASSED_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_skip() {
        SKIPPED_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    pub fn passed() -> usize {
        PASSED_COUNT.load(Ordering::SeqCst)
    }

    pub fn skipped() -> usize {
        SKIPPED_COUNT.load(Ordering::SeqCst)
    }
}
