#!/bin/bash
set -o pipefail

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
EVIDENCE_DIR="$REPO_ROOT/docs/evidence"
LOG_FILE="$EVIDENCE_DIR/verify_latest.txt"

mkdir -p "$EVIDENCE_DIR"

{
  echo "=== Rollback Netcode Server Verification ==="
  echo "Date: $(date)"
  echo "Repo: $REPO_ROOT"
  echo
  
  # Check environment
  echo "1. ENVIRONMENT CHECK"
  echo "  - DATABASE_URL: ${DATABASE_URL:-NOT SET}"
  if command -v pg_isready >/dev/null 2>&1; then
    if pg_isready -h localhost 2>/dev/null; then
      echo "  - Postgres: ✓ reachable"
    else
      echo "  - Postgres: ✗ NOT reachable (tests will skip DB tests)"
    fi
  else
    echo "  - Postgres: pg_isready not found"
  fi
  echo
  
  # Run tests
  echo "2. CARGO TEST --ALL"
  cd "$REPO_ROOT/backend"
  
  if [ "$DATABASE_URL" = "" ]; then
    echo "  (Skipping DB tests: DATABASE_URL not set)"
    ALLOW_SKIP_INTEGRATION=1 cargo test --all --no-fail-fast 2>&1
  else
    cargo test --all --no-fail-fast 2>&1
  fi
  
  TEST_EXIT=$?
  echo
  
  # Git stats
  echo "3. GIT STATISTICS"
  cd "$REPO_ROOT"
  echo "  - Latest commits:"
  git log --oneline -5
  echo
  echo "  - Diff stats:"
  git diff --numstat HEAD | awk '{a+=$1; b+=$2} END {printf "    Added: %d, Deleted: %d\n", a, b}'
  echo
  
  # Load STATUS.md and extract gate info
  echo "4. RELEASE GATES"
  if [ -f "docs/STATUS.md" ]; then
    # Parse gates from Release Gates tab (if it exists)
    echo "  - See docs/STATUS.md for task-by-task status"
  fi
  echo
  
  # Final verdict
  echo "5. VERDICT"
  if [ $TEST_EXIT -eq 0 ]; then
    echo "  ✓ All tests passed"
  else
    echo "  ✗ Some tests failed (exit code: $TEST_EXIT)"
  fi
  
  exit $TEST_EXIT
  
} | tee "$LOG_FILE"

EXIT_CODE=$?
echo
echo "Full output saved to: $LOG_FILE"
exit $EXIT_CODE
