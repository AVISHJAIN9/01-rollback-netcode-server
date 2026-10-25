/// D4: DoS Mitigation — Rate Limiting
/// Tests per-IP handshake rate limiting to prevent connection flood attacks

use rollback_netcode_server::network::RateLimiter;
use std::net::SocketAddr;

/// Test 1: Below limit requests are allowed
#[test]
fn test_below_limit_allowed() {
    let mut limiter = RateLimiter::with_limits(10, 1);
    let peer = "127.0.0.1:7777".parse::<SocketAddr>().expect("Parse failed");

    // Send 5 handshakes (below the 10 per second limit)
    for i in 0..5 {
        let result = limiter.check_handshake(peer);
        assert!(
            result.is_ok(),
            "Request {} should be allowed (below limit)",
            i + 1
        );
    }

    println!("✓ 5 requests below limit of 10 all allowed");
}

/// Test 2: Above limit requests are blocked
#[test]
fn test_above_limit_blocked() {
    let mut limiter = RateLimiter::with_limits(10, 1);
    let peer = "127.0.0.1:7778".parse::<SocketAddr>().expect("Parse failed");

    // Send 10 handshakes (at limit)
    for i in 0..10 {
        let result = limiter.check_handshake(peer);
        assert!(result.is_ok(), "Request {} should be allowed (at limit)", i + 1);
    }

    // 11th handshake should be blocked
    let result = limiter.check_handshake(peer);
    assert!(
        result.is_err(),
        "Request 11 should be blocked (exceeds limit)"
    );

    println!("✓ 11th request correctly blocked when limit is 10");
}

/// Test 3: Different IPs have independent limits
#[test]
fn test_different_ips_independent() {
    let mut limiter = RateLimiter::with_limits(5, 1);
    let peer1 = "192.168.1.100:7777".parse::<SocketAddr>().expect("Parse failed");
    let peer2 = "192.168.1.101:7777".parse::<SocketAddr>().expect("Parse failed");

    // Peer1 sends 5 requests (at limit)
    for _ in 0..5 {
        assert!(
            limiter.check_handshake(peer1).is_ok(),
            "Peer1 should reach limit"
        );
    }

    // Peer2 should still be able to send (independent limits)
    for i in 0..5 {
        let result = limiter.check_handshake(peer2);
        assert!(
            result.is_ok(),
            "Peer2 request {} should be allowed (independent limit)",
            i + 1
        );
    }

    // Verify peer1 is blocked but peer2 can still send more
    assert!(
        limiter.check_handshake(peer1).is_err(),
        "Peer1 6th request should be blocked"
    );
    assert!(
        limiter.check_handshake(peer2).is_err(),
        "Peer2 6th request should be blocked"
    );

    println!("✓ Different IPs correctly have independent limits");
}

/// Test 4: Rate limit counter works correctly
#[test]
fn test_rate_limit_counter() {
    let mut limiter = RateLimiter::with_limits(3, 1);
    let peer = "10.0.0.1:7777".parse::<SocketAddr>().expect("Parse failed");

    // Initial count should be 0
    assert_eq!(limiter.get_handshake_count(peer), 0);

    // After 1 request
    limiter.check_handshake(peer).ok();
    assert_eq!(limiter.get_handshake_count(peer), 1);

    // After 3 requests
    limiter.check_handshake(peer).ok();
    limiter.check_handshake(peer).ok();
    assert_eq!(limiter.get_handshake_count(peer), 3);

    println!("✓ Rate limit counter accurately tracks handshake count");
}

/// Test 5: Burst rejection pattern
#[test]
fn test_burst_rejection() {
    let mut limiter = RateLimiter::with_limits(3, 1);
    let attacker = "10.0.0.2:9999".parse::<SocketAddr>().expect("Parse failed");

    let mut allowed = 0;
    let mut blocked = 0;

    // Send 11 rapid requests
    for _ in 0..11 {
        if limiter.check_handshake(attacker).is_ok() {
            allowed += 1;
        } else {
            blocked += 1;
        }
    }

    assert_eq!(allowed, 3, "First 3 should be allowed");
    assert_eq!(blocked, 8, "Remaining 8 should be blocked");

    println!("✓ Burst attack: {} allowed, {} blocked", allowed, blocked);
}
