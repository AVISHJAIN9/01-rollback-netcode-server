/// D2: Desync Attack Detection
/// Tests deterministic checksum validation to detect forged or corrupted game state.

use rollback_netcode_server::desync::{DesyncDetector, DesyncError};

/// Test 1: Matching checksums pass validation
#[test]
fn test_matching_checksums_pass() {
    let mut detector = DesyncDetector::new();

    // Simulate 10 frames of deterministic state
    let test_data = vec![
        (1, 0x12345678u32),
        (2, 0x23456789u32),
        (3, 0x3456789Au32),
        (4, 0x456789ABu32),
        (5, 0x56789ABCu32),
    ];

    for (frame, checksum) in &test_data {
        detector.record_authoritative_hash(*frame, *checksum);
    }

    println!("✓ Recorded {} authoritative checksums", test_data.len());

    // Client sends matching checksums
    for (frame, checksum) in &test_data {
        let result = detector.validate_checksum(*frame, *checksum);
        assert!(result.is_ok(), "Frame {} checksum validation failed", frame);
    }

    println!("✓ All matching checksums validated successfully");
}

/// Test 2: Mismatched checksums are detected
#[test]
fn test_mismatched_checksums_detected() {
    let mut detector = DesyncDetector::new();

    // Record server's checksum at frame 5
    detector.record_authoritative_hash(5, 0x12345678u32);

    // Client sends different checksum for frame 5
    let result = detector.validate_checksum(5, 0xDEADBEEFu32);

    assert!(
        matches!(result, Err(DesyncError::ChecksumMismatch { expected: 0x12345678, actual: 0xDEADBEEF })),
        "Expected ChecksumMismatch error, got {:?}",
        result
    );

    println!("✓ Checksum mismatch correctly detected");
}

/// Test 3: Forged state triggers resync error
#[test]
fn test_forged_state_triggers_desync() {
    let mut detector = DesyncDetector::new();

    // Record legitimate checksums for frames 1-10
    for frame in 1..=10 {
        let checksum = ((frame as u32) * 0x12345678).wrapping_add(0xABCDEF);
        detector.record_authoritative_hash(frame, checksum);
    }

    println!("✓ Recorded checksums for frames 1-10");

    // Client attempts to forge state at frame 7
    let forged_checksum = 0xFFFFFFFFu32;
    let result = detector.validate_checksum(7, forged_checksum);

    assert!(
        matches!(result, Err(DesyncError::ChecksumMismatch { .. })),
        "Expected forged state to be detected as ChecksumMismatch"
    );

    println!("✓ Forged state at frame 7 detected as DesyncError::ChecksumMismatch");
}

/// Test 4: Unknown frame returns FrameNotFound error
#[test]
fn test_unknown_frame_error() {
    let detector = DesyncDetector::new();

    // Try to validate a frame that was never recorded
    let result = detector.validate_checksum(999, 0x12345678u32);

    assert!(
        matches!(result, Err(DesyncError::FrameNotFound { frame: 999 })),
        "Expected FrameNotFound error, got {:?}",
        result
    );

    println!("✓ Unknown frame correctly returned FrameNotFound error");
}

/// Test 5: Simple hash computation consistency
#[test]
fn test_compute_hash_simple_consistency() {
    let data1 = vec![1, 2, 3, 4, 5];
    let data2 = vec![1, 2, 3, 4, 5];
    let data3 = vec![1, 2, 3, 4, 6]; // Different last byte

    let hash1 = DesyncDetector::compute_hash_simple(&data1);
    let hash2 = DesyncDetector::compute_hash_simple(&data2);
    let hash3 = DesyncDetector::compute_hash_simple(&data3);

    assert_eq!(
        hash1, hash2,
        "Identical data should produce identical hashes"
    );
    assert_ne!(hash1, hash3, "Different data should produce different hashes");

    println!("✓ Hash computation is deterministic and sensitive to data changes");
    println!("  Hash(data1) = {:#x}", hash1);
    println!("  Hash(data3) = {:#x}", hash3);
}
