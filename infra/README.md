# Infrastructure & Deployment

## Artifacts
- `Dockerfile.backend`: Multi-stage Docker container compiling and packaging the Rust netcode server.
- `Dockerfile.frontend`: Production Next.js container.
- `docker-compose.yml`: Local orchestrator launching Postgres 16, Redis 7.2, Backend, and Frontend.
- `k8s/deployment.yaml`: Production Kubernetes StatefulSet with direct UDP NodePort routing.
