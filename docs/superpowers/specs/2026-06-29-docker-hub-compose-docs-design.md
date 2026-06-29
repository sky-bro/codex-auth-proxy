# Docker Hub, Compose, and Bilingual Docs Design

## Goal

Publish `codex-auth-proxy` as `skybro/codex-auth-proxy` on Docker Hub and document a Docker Compose based local deployment flow in English and Chinese.

## Publishing Behavior

- Use the existing CI workflow as the single release pipeline.
- On `workflow_dispatch`, build and push `skybro/codex-auth-proxy:latest`.
- On `v*` tags, build and push `skybro/codex-auth-proxy:<tag>` and `skybro/codex-auth-proxy:latest`.
- On pull requests and regular branch pushes, continue building and testing without pushing Docker images.
- Require `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` repository secrets for publish jobs.

## Container Runtime

- Build with a multi-stage Dockerfile.
- Compile the Rust release binary in the builder stage with the same Rust toolchain as CI.
- Run the proxy from a small Debian runtime image.
- Default the container to listen on `0.0.0.0:8765` and use device-code auth when startup needs a fresh login.
- Keep Codex credentials under a mounted `CODEX_HOME` so auth survives container restarts.

## Documentation

- Keep `README.md` as the default English document.
- Add `README.zh-CN.md` with equivalent Chinese instructions.
- Add a Compose example using `skybro/codex-auth-proxy:latest`, `CODEX_HOME=/data/codex`, and a Docker volume named `codex-home`.
- Explain that first startup on a fresh volume prints a device-code login flow in container logs.
- Keep security guidance explicit: bind locally by default in Compose and do not expose the proxy publicly.
