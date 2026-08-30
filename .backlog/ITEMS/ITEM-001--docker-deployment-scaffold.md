## ITEM-001: Docker Deployment Scaffold

**As a** developer deploying Inferon
**I want** to bring up the whole Slice 1 stack with a single command
**So that** every component ships as a container image and local/dev/CI environments are identical

**Acceptance Criteria:**
1. `docker compose up` starts the proxy, Postgres, and Redis containers from a clean checkout
2. Proxy builds from a multi-stage Dockerfile (Rust toolchain → slim runtime image) with cached dependencies
3. Postgres and Redis run from official images with persistent volumes for data
4. Health checks configured: proxy `/healthz` returns 200, `pg_isready`, Redis `PING`
5. Configuration via `.env` (with `.env.example` committed): `DATABASE_URL`, `REDIS_URL`, master upstream key, `DEFAULT_MODEL=deepseek-chat`, `PORT=8080`
6. Container logs go to stdout (12-factor); no log files inside the image
7. Builds and runs on both macOS (Apple silicon) and Linux

**Technical Notes:**
- Multi-stage build with `rust` image + cargo-chef; runtime = **gcr.io/distroless/cc-debian12:nonroot** (glibc + ca-certificates, non-root, no shell)
- Distroless has no curl/shell → the proxy exposes a `--healthcheck` self-check mode (GET /healthz via tokio TcpStream, exit 0/1); compose healthcheck uses it
- Compose verification delegated to a Docker host via `scripts/smoke.sh` (no local Docker daemon on the dev machine)
