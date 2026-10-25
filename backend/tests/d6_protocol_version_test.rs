/// D6: Protocol Version Negotiation
/// Tests that v1.0 is accepted and v0.9 and v2.0 are rejected with clear errors

use rollback_netcode_server::protocol::{VersionNegotiator, ProtocolError};

/// Test 1: Version 1.0 is accepted
#[test]
fn test_v1_0_accepted() {
    let negotiator = VersionNegotiator::default();

    let result = negotiator.negotiate(1);
    assert_eq!(result, Ok(1), "Version 1 should be negotiated successfully");

    println!("✓ Version 1.0 accepted");
}

/// Test 2: Version 0.9 is rejected
#[test]
fn test_v0_9_rejected() {
    let negotiator = VersionNegotiator::default();

    let result = negotiator.negotiate(0);
    assert!(
        result.is_err(),
        "Version 0 should be rejected"
    );

    // Verify the error contains version mismatch information
    if let Err(ProtocolError::VersionMismatch {
        client_version,
        min_version,
        max_version,
    }) = result
    {
        assert_eq!(client_version, 0);
        assert_eq!(min_version, 1);
        assert_eq!(max_version, 1);
        println!("✓ Version 0 rejected with VersionMismatch error");
        println!("  Error details: client={}, expected min={}, max={}", client_version, min_version, max_version);
    } else {
        panic!("Expected VersionMismatch error, got {:?}", result);
    }
}

/// Test 3: Version 2.0 is rejected
#[test]
fn test_v2_0_rejected() {
    let negotiator = VersionNegotiator::default();

    let result = negotiator.negotiate(2);
    assert!(
        result.is_err(),
        "Version 2 should be rejected"
    );

    // Verify the error contains version mismatch information
    if let Err(ProtocolError::VersionMismatch {
        client_version,
        min_version,
        max_version,
    }) = result
    {
        assert_eq!(client_version, 2);
        assert_eq!(min_version, 1);
        assert_eq!(max_version, 1);
        println!("✓ Version 2 rejected with VersionMismatch error");
        println!("  Error details: client={}, expected min={}, max={}", client_version, min_version, max_version);
    } else {
        panic!("Expected VersionMismatch error, got {:?}", result);
    }
}

/// Test 4: Future version (255) is rejected
#[test]
fn test_future_version_rejected() {
    let negotiator = VersionNegotiator::default();

    let result = negotiator.negotiate(255);
    assert!(
        result.is_err(),
        "Version 255 should be rejected"
    );

    if let Err(ProtocolError::VersionMismatch { client_version, .. }) = result {
        assert_eq!(client_version, 255);
        println!("✓ Far-future version (255) rejected");
    }
}

/// Test 5: Negotiation is idempotent
#[test]
fn test_negotiation_idempotent() {
    let negotiator = VersionNegotiator::default();

    // Multiple calls with same version should give same result
    for i in 0..3 {
        let accepted = negotiator.negotiate(1);
        let rejected = negotiator.negotiate(0);

        assert_eq!(accepted, Ok(1), "Iteration {}: v1 should be accepted", i);
        assert!(rejected.is_err(), "Iteration {}: v0 should be rejected", i);
    }

    println!("✓ Negotiation is idempotent across multiple calls");
}

/// Test 6: Out-of-range versions are rejected
#[test]
fn test_out_of_range_versions() {
    let negotiator = VersionNegotiator::default();

    // Test several out-of-range versions
    let out_of_range = vec![3, 5, 10, 50, 100, 200];

    for version in out_of_range {
        let result = negotiator.negotiate(version);
        assert!(
            result.is_err(),
            "Version {} should be rejected",
            version
        );
    }

    println!("✓ All out-of-range versions correctly rejected");
}

/// Test 7: Version negotiator error message contains details
#[test]
fn test_version_error_message() {
    let negotiator = VersionNegotiator::default();

    let result = negotiator.negotiate(3);
    assert!(result.is_err());

    if let Err(error) = result {
        let error_msg = format!("{}", error);
        println!("✓ Error message: {}", error_msg);
        
        // The error message should mention version mismatch
        assert!(
            error_msg.contains("version") || error_msg.contains("Version"),
            "Error message should mention version"
        );
    }
}
