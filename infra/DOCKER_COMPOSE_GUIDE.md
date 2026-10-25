# Docker Compose Setup Guide

**Status:** ⏳ Requires Docker & Docker Compose to be installed and running  
**Date:** 2026-10-10

---

## Prerequisites

- **Docker:** >= 20.10
- **Docker Compose:** >= 2.0
- **RAM:** 4 GB minimum for all services
- **Disk:** 2 GB for database volume

---

## Quick Start

### 1. Start All Services

```bash
cd infra
docker-compose up -d
```

**Expected output:**
```
[+] Running 4/4
 ✔ rollback-postgres  Healthy
 ✔ rollback-redis     Healthy
 ✔ rollback-backend   Healthy
 ✔ rollback-frontend  Started
```

### 2. Verify Services are Running

```bash
docker-compose ps
```

**Output:**
```
NAME                 IMAGE                          STATUS
rollback-postgres    postgres:16-alpine             Up (healthy)
rollback-redis       redis:7.2-alpine               Up (healthy)
rollback-backend     rollback-netcode-server        Up (healthy)
rollback-frontend    rollback-netcode-frontend     Up
```

### 3. Access Services

| Service | URL | Purpose |
|---------|-----|---------|
| **Frontend** | http://localhost:3000 | React app |
| **Backend Admin** | http://localhost:8080 | REST API (metrics, admin) |
| **Postgres** | localhost:5432 | Database |
| **Redis** | localhost:6379 | Cache/Session store |
| **Backend UDP** | localhost:7777 | Game server (UDP) |

### 4. Stop All Services

```bash
docker-compose down
```

---

## Service Configuration

### Postgres (rollback-postgres)

**Image:** `postgres:16-alpine`  
**Port:** 5432  
**Credentials:**
- User: `postgres`
- Password: `postgrespassword`
- Database: `rollback_db`

**Health Check:**
```bash
# Manual check
docker exec rollback-postgres pg_isready -U postgres

# Or via psql
psql -h localhost -U postgres -d rollback_db -c "SELECT 1"
```

**Data Persistence:**
- Stored in Docker volume `pgdata:/var/lib/postgresql/data`
- Persists across restarts

**Connect Locally:**
```bash
psql -h localhost -U postgres -d rollback_db
```

---

### Redis (rollback-redis)

**Image:** `redis:7.2-alpine`  
**Port:** 6379  
**Features:**
- Append-only file (AOF) enabled for durability
- Fsync every second

**Health Check:**
```bash
# Manual check
redis-cli -h localhost ping

# Expected output: PONG
```

**Data Persistence:**
- Snapshots + AOF (append-only file)
- Survives restarts

**Connect Locally:**
```bash
redis-cli -h localhost
```

---

### Backend (rollback-backend)

**Image:** Built from `infra/Dockerfile.backend`  
**Ports:**
- **7777/UDP:** Game server (rollback protocol)
- **8080/TCP:** HTTP admin API

**Environment Variables:**
```yaml
ROLLBACK_POSTGRES_URL: "postgres://postgres:postgrespassword@postgres:5432/rollback_db"
ROLLBACK_REDIS_URL: "redis://redis:6379/0"
ROLLBACK_TICK_RATE: "60"
ROLLBACK_RECONNECT_TIMEOUT_SECS: "30"
ROLLBACK_STALE_SESSION_THRESHOLD_SECS: "40"
ROLLBACK_UDP_GAME_PORT: "7777"
ROLLBACK_HTTP_ADMIN_PORT: "8080"
ROLLBACK_ENABLE_AI_CHEAT_DETECTION: "0"
ROLLBACK_ENABLE_JITTER_PREDICTION: "0"
ROLLBACK_ENABLE_ROLLBACK_RISK_METRICS: "0"
ROLLBACK_ENABLE_MATCHMAKING_OPTIMIZER: "0"
```

**Health Check:**
```bash
# Manual check
curl http://localhost:8080/health
```

**Dependencies:**
- Waits for Postgres to be healthy
- Waits for Redis to be healthy

**View Logs:**
```bash
docker-compose logs -f backend
```

---

### Frontend (rollback-frontend)

**Image:** Built from `infra/Dockerfile.frontend`  
**Port:** 3000  
**Environment Variables:**
```yaml
REACT_APP_API_URL: "http://localhost:8080"
REACT_APP_WS_URL: "ws://localhost:8080"
```

**Access:**
```
http://localhost:3000
```

---

## Common Tasks

### View Logs

**All services:**
```bash
docker-compose logs -f
```

**Specific service:**
```bash
docker-compose logs -f backend
docker-compose logs -f postgres
```

**Last 50 lines:**
```bash
docker-compose logs --tail 50 backend
```

---

### Connect to Database

**From host machine:**
```bash
psql -h localhost -U postgres -d rollback_db
```

**From inside container:**
```bash
docker exec -it rollback-postgres psql -U postgres -d rollback_db
```

**Query:**
```sql
SELECT COUNT(*) FROM users;
SELECT * FROM match_sessions ORDER BY created_at DESC LIMIT 5;
```

---

### Connect to Redis

**From host machine:**
```bash
redis-cli -h localhost
```

**From inside container:**
```bash
docker exec -it rollback-redis redis-cli
```

**Commands:**
```
PING                    # Check connectivity
KEYS *                  # List all keys
GET key_name            # Get value
FLUSHDB                 # Clear database (caution!)
```

---

### Update Configuration

To change feature flags or server settings:

**Option 1: Edit docker-compose.yml**
```yaml
environment:
  ROLLBACK_ENABLE_AI_CHEAT_DETECTION: "1"  # Enable
```

Then restart:
```bash
docker-compose restart backend
```

**Option 2: Override at runtime**
```bash
docker-compose up -e ROLLBACK_ENABLE_AI_CHEAT_DETECTION=1 -d
```

---

### Scale Backend Servers (Future)

```yaml
# Not yet supported; single instance for Phase 1
# Multi-instance planned for Phase 2
```

---

## Troubleshooting

### Services Won't Start

**Check logs:**
```bash
docker-compose logs
```

**Common issues:**
- Port already in use: `ERROR: bind: address already in use`
  - Kill process: `lsof -ti:8080 | xargs kill -9`
- Disk full: `no space left on device`
  - Clean Docker: `docker system prune -a`
- Permission denied: Run with sudo or add user to docker group

---

### Postgres Connection Refused

**Verify Postgres is healthy:**
```bash
docker-compose ps postgres
```

**If not healthy:**
```bash
docker-compose logs postgres
docker-compose restart postgres
docker-compose logs postgres  # Check for errors
```

---

### Redis Unavailable

**Verify Redis is running:**
```bash
redis-cli -h localhost ping
```

**If error:**
```bash
docker-compose logs redis
docker-compose restart redis
```

---

### Backend Crashes Immediately

**Check logs:**
```bash
docker-compose logs backend
```

**Common issues:**
- Database not migrated: Run migrations manually
- Config file missing: Ensure config.toml exists
- Port conflict: Change port in docker-compose.yml

---

### High Memory Usage

**Check container stats:**
```bash
docker stats
```

**If backend memory high:**
- May be due to connection leaks
- Restart: `docker-compose restart backend`
- Investigate: `docker-compose logs backend | grep -i memory`

---

## Performance Tuning

### Postgres Optimization

In `docker-compose.yml`, add environment variables:
```yaml
postgres:
  environment:
    POSTGRES_INITDB_ARGS: "-c shared_buffers=256MB -c effective_cache_size=1GB"
```

### Redis Optimization

Current config is good for development. For production:
```yaml
redis:
  command: >
    redis-server
    --appendonly yes
    --appendfsync everysec
    --maxmemory 512mb
    --maxmemory-policy allkeys-lru
```

---

## Data Cleanup

### Reset Database (Caution!)

```bash
# Stop services
docker-compose down

# Remove volume
docker volume rm rollback-netcode-server_pgdata

# Restart
docker-compose up -d
```

### Backup Database

```bash
docker exec rollback-postgres pg_dump -U postgres rollback_db > backup.sql
```

### Restore Database

```bash
docker exec -i rollback-postgres psql -U postgres rollback_db < backup.sql
```

---

## Development Workflow

### 1. Start Services

```bash
cd infra
docker-compose up -d
```

### 2. Make Code Changes

Edit backend Rust code in `backend/src/`

### 3. Rebuild Backend

```bash
docker-compose build backend
docker-compose up -d backend
```

### 4. View Changes

```bash
docker-compose logs -f backend
curl http://localhost:8080/health
```

### 5. Run Tests in Container

```bash
docker exec rollback-backend cargo test
```

---

## Deployment to Cloud

For production deployment, consider:
- **AWS ECS:** Use docker-compose as task definition
- **Kubernetes:** Convert docker-compose to Helm chart
- **GCP Cloud Run:** Serverless option (if stateless)
- **DigitalOcean App Platform:** Simple deployment

Example AWS CloudFormation:
```yaml
# (not included; see AWS docs)
```

---

## Monitoring

### Prometheus Metrics (Future)

Currently not implemented. Planned for Phase 2.

### Manual Health Checks

```bash
# Check all services
docker-compose ps

# Check Postgres
docker exec rollback-postgres pg_isready -U postgres

# Check Redis
redis-cli -h localhost ping

# Check Backend
curl -s http://localhost:8080/health | jq .
```

---

## Sign-Off

- **Document Owner:** Rollback Netcode Team
- **Date:** 2026-10-10
- **Status:** Code provided; user must run with Docker installed

---

## Next Steps

⏳ **User must:**
1. Install Docker and Docker Compose
2. Run `docker-compose up -d` in `infra/` directory
3. Verify services are healthy
4. Access frontend at http://localhost:3000
