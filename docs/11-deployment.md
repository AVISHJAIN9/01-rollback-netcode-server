# 11 - Deployment & Infrastructure Guide: Rollback Netcode Server

## 1. Containerization & Orchestration [Specified]
- **Docker Multi-Stage Builds**:
  - `infra/Dockerfile.backend`: Minimal scratch/Alpine container running the compiled Rust binary.
  - `infra/Dockerfile.frontend`: Optimized Next.js standalone container.
- **Docker Compose**: Orchestrates backend server, frontend UI, PostgreSQL 16, and Redis 7.2.
- **Kubernetes**: StatefulSet / Agones GameServer CRD deployment with direct host-port UDP routing.

## 2. Environment Variables Configuration [Specified]
```env
# Server Network Config
UDP_BIND_ADDR=0.0.0.0:9000
HTTP_BIND_ADDR=0.0.0.0:8080
TICK_RATE=60
MAX_ROLLBACK_FRAMES=128

# Database & Cache
DATABASE_URL=postgres://postgres:postgres@localhost:5432/rollback_db
REDIS_URL=redis://localhost:6379/0

# Authentication & AI
JWT_SECRET=super_secret_dev_key_change_in_prod
OPENAI_API_KEY=sk-placeholder-for-llm-explainer
```
