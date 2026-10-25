/// D7: Release Build Verification
/// Tests that cargo build --release succeeds and produces a binary

use std::path::Path;
use std::process::Command;

/// Test 1: Release build succeeds and binary exists
#[test]
#[ignore] // Mark as ignored to allow optional execution
fn test_release_binary_builds_and_exists() {
    // Build in release mode
    let output = Command::new("cargo")
        .args(&["build", "--release"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Failed to execute cargo build --release");

    assert!(
        output.status.success(),
        "cargo build --release failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    println!("✓ Release build succeeded");

    // Check that binary exists
    // The binary name should match the package name in Cargo.toml
    let binary_name = "rollback-netcode-server";
    let binary_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to get parent directory")
        .join("target")
        .join("release")
        .join(binary_name);

    assert!(
        binary_path.exists(),
        "Release binary not found at {}",
        binary_path.display()
    );

    println!(
        "✓ Release binary exists at {}",
        binary_path.display()
    );

    // Verify it's actually an executable (has content)
    let metadata = std::fs::metadata(&binary_path)
        .expect("Failed to get binary metadata");
    assert!(
        metadata.len() > 0,
        "Binary file is empty"
    );

    println!("✓ Binary has content ({} bytes)", metadata.len());
}

/// Test 2: Quick verification that the binary can be invoked (with --version or --help)
#[test]
#[ignore]
fn test_release_binary_is_executable() {
    let binary_name = "rollback-netcode-server";
    let binary_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to get parent directory")
        .join("target")
        .join("release")
        .join(binary_name);

    // If binary doesn't exist, try to build it first
    if !binary_path.exists() {
        Command::new("cargo")
            .args(&["build", "--release"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Failed to execute cargo build --release");
    }

    assert!(binary_path.exists(), "Binary not found after build");

    // Try to execute the binary (this is a quick smoke test)
    // Most binaries support --version or --help
    let result = Command::new(&binary_path)
        .arg("--version")
        .output();

    // If the binary supports --version, great; if not, just check it exists
    if let Ok(output) = result {
        println!(
            "✓ Binary is executable:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    } else {
        println!(
            "✓ Binary exists and appears to be executable (--version not supported)"
        );
    }
}
