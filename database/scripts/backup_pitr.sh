#!/usr/bin/env bash
set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/tmp/backups/rollback_db}"
mkdir -p "$BACKUP_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "[*] Creating Point-in-Time Base Backup at ${TIMESTAMP}..."
echo "Simulating: pg_basebackup -h localhost -p 5432 -U postgres -D ${BACKUP_DIR}/base_${TIMESTAMP} -Ft -z -P"
touch "${BACKUP_DIR}/base_${TIMESTAMP}.tar.gz"
echo "[+] Base backup created: ${BACKUP_DIR}/base_${TIMESTAMP}.tar.gz"
