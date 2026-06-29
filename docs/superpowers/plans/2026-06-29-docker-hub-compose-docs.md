# Docker Hub Compose Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Docker Hub publishing, Docker Compose usage, and bilingual documentation for `codex-auth-proxy`.

**Architecture:** The existing CI workflow remains the release control point. Docker support is file-based: a multi-stage `Dockerfile`, a checked-in `compose.yaml`, and README instructions in English and Chinese.

**Tech Stack:** Rust, Cargo, Docker Buildx, Docker Hub, GitHub Actions, Markdown.

---

### Task 1: Container Files

**Files:**
- Create: `Dockerfile`
- Create: `.dockerignore`
- Create: `compose.yaml`

- [ ] Add a multi-stage Dockerfile that builds `codex-auth-proxy` with Rust 1.95.0 and copies the release binary into a Debian slim runtime image.
- [ ] Add `.dockerignore` entries for build outputs, Git metadata, local Codex auth data, and editor files.
- [ ] Add `compose.yaml` using `skybro/codex-auth-proxy:latest`, local-only port binding, `CODEX_HOME=/data/codex`, `CODEX_PROXY_API_KEY`, and a persistent Docker volume named `codex-home`.

### Task 2: Docker Hub Workflow

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] Add `packages: write` permission only where needed for Docker metadata/build actions.
- [ ] Add a Docker publish job that runs on `workflow_dispatch` and `v*` tag refs.
- [ ] Login to Docker Hub using `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN`.
- [ ] Push `latest` on manual dispatch and tag builds.
- [ ] Push the tag name only for `v*` tag builds.

### Task 3: Bilingual Docs

**Files:**
- Modify: `README.md`
- Create: `README.zh-CN.md`

- [ ] Update the English README with language links, Docker Hub image name, Compose setup, manual Docker run, and publish workflow notes.
- [ ] Add a Chinese README with equivalent commands and security notes.
- [ ] Ensure both documents say that `README.md` is the default English documentation.

### Task 4: Verification

**Commands:**
- `cargo fmt --check`
- `cargo test --locked`
- `docker build -t codex-auth-proxy:local .`

- [ ] Run Rust formatting check.
- [ ] Run the locked test suite.
- [ ] Build the Docker image locally if Docker is available.
