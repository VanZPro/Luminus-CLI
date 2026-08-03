---
name: docker-skill
description: Docker and Compose: Dockerfiles, multi-stage builds, compose stacks, debugging containers.
---

# Docker Skill
- Prefer multi-stage builds; pin base image digests when shipping.
- One process per container; use healthchecks.
- Compose: explicit networks, named volumes, no `latest` in prod.
- Debug: `docker compose logs -f`, `docker exec`, inspect health.
- Never commit secrets in Dockerfile `ENV`.
