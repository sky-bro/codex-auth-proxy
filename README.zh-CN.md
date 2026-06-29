# codex-auth-proxy

[English](README.md) | 简体中文

这是一个小型本地 API 代理。它复用你已有的 Codex ChatGPT 登录状态，调用
Codex Responses 后端。

它提供：

- `GET /healthz`
- `GET /models`
- `GET /v1/models`
- `POST /v1/responses`
- `POST /v1/images/generations`
- `POST /v1/images/edits`

代理有自己的 bearer token。它不会把 Codex access token 暴露给调用方。

默认文档是英文版 [`README.md`](README.md)，本文件是对应的中文版本。

## 架构

![codex-auth-proxy architecture](docs/architecture.svg)

## Docker

Docker Hub 镜像是：

```text
skybro/codex-auth-proxy:latest
```

使用 Docker Compose 启动：

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
docker compose up -d
docker compose logs -f codex-auth-proxy
```

仓库里的 [`compose.yaml`](compose.yaml) 会把服务绑定到
`127.0.0.1:8765`，把 Codex 登录数据保存到名为 `codex-home` 的 Docker volume，
并在容器内设置 `CODEX_HOME=/data/codex`。

如果第一次启动时 `codex-home` volume 为空，代理会使用 device-code 登录。请根据容器
日志里打印的 URL 和 code 完成登录。登录凭据会写入挂载的 volume，容器重启后仍然可用。

手动运行 Docker：

```bash
docker run --rm -it \
  -p 127.0.0.1:8765:8765 \
  -e CODEX_PROXY_API_KEY="choose-a-local-secret" \
  -e CODEX_HOME=/data/codex \
  -v codex-auth-proxy-data:/data/codex \
  skybro/codex-auth-proxy:latest
```

## 本地构建

```bash
cargo build --release
```

本地构建容器镜像：

```bash
docker build -t codex-auth-proxy:local .
```

## 发布构建

GitHub Actions 会在 push、pull request 和手动 workflow 运行时构建 Linux x86_64、
macOS aarch64 和 Windows x86_64 二进制。

推送 `v0.1.0` 这类 tag 会把二进制压缩包发布为 GitHub Release assets。tag 构建也
会发布：

- `skybro/codex-auth-proxy:v0.1.0`
- `skybro/codex-auth-proxy:latest`

手动触发 workflow 会发布：

- `skybro/codex-auth-proxy:latest`

发布 Docker 镜像前，需要在 GitHub 仓库 secrets 中配置：

- `DOCKERHUB_USERNAME`
- `DOCKERHUB_TOKEN`

## 不使用 Docker 运行

先登录。在带本地浏览器的机器上可以使用浏览器登录：

```bash
codex-auth-proxy login
```

在远程或无头机器上，使用 device-code 登录：

```bash
codex-auth-proxy login --device-auth
```

如果想让这个代理和你日常 Codex 登录隔离，可以使用单独的 Codex home：

```bash
CODEX_HOME="$HOME/.codex-proxy-profile" codex-auth-proxy login --device-auth
```

启动代理时也会检查 Codex auth 是否可用。如果不可用，代理会先运行同样的登录流程，
然后继续启动。因此一台新机器只需要一个命令：

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
cargo run --release -- --listen 127.0.0.1:8765
```

远程或无头机器上，可以在代理启动时使用 device-code 登录：

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
cargo run --release -- --listen 127.0.0.1:8765 --device-auth
```

退出登录并清理已保存的 Codex auth：

```bash
codex-auth-proxy logout
```

选项可以通过 flag 或环境变量传入：

| Flag | Env | 默认值 |
| --- | --- | --- |
| `--listen` | `CODEX_PROXY_LISTEN` | `127.0.0.1:8765` |
| `--api-key` | `CODEX_PROXY_API_KEY` | 必填 |
| `--codex-home` | `CODEX_HOME` | `$HOME/.codex` |
| `--upstream-base-url` | `CODEX_PROXY_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` |
| `--codex-client-version` | `CODEX_PROXY_CODEX_CLIENT_VERSION` | Codex dependency tag version |
| `--auth-refresh-interval-secs` | `CODEX_PROXY_AUTH_REFRESH_INTERVAL_SECS` | `60` |
| `--device-auth` | - | `false` |

`--codex-client-version` 只会在获取 Codex model catalog 时发送。Codex 后端会根据最低
客户端版本过滤 `/models`，所以默认值来自当前 pin 住的 `openai/codex` git dependency
tag。

`--auth-refresh-interval-secs` 控制后台 Codex auth 刷新检查。代理会按这个间隔调用
Codex `AuthManager::auth()`，所以即使代理空闲，也会复用 Codex 的临近过期刷新行为。
设置为 `0` 可以关闭后台检查；请求时刷新和 upstream `401` 后的一次重试仍然保留。

使用同一个 `CODEX_HOME` 运行隔离 profile：

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
CODEX_HOME="$HOME/.codex-proxy-profile" \
  cargo run --release -- --listen 127.0.0.1:8765
```

## 调用

以 OpenAI-compatible 格式列出模型：

```bash
curl http://127.0.0.1:8765/v1/models \
  -H "authorization: Bearer choose-a-local-secret"
```

列出原始 Codex model catalog，包括 service tier 和 model capability 等 Codex 专有元数据：

```bash
curl http://127.0.0.1:8765/models \
  -H "authorization: Bearer choose-a-local-secret"
```

文本响应：

```bash
curl http://127.0.0.1:8765/v1/responses \
  -H "authorization: Bearer choose-a-local-secret" \
  -H "content-type: application/json" \
  -d '{
    "model": "gpt-5.5",
    "store": false,
    "stream": true,
    "input": [
      {
        "role": "user",
        "content": [
          {
            "type": "input_text",
            "text": "Say hello in one sentence."
          }
        ]
      }
    ]
  }'
```

图像理解：

```bash
curl http://127.0.0.1:8765/v1/responses \
  -H "authorization: Bearer choose-a-local-secret" \
  -H "content-type: application/json" \
  -d '{
    "model": "gpt-5.5",
    "store": false,
    "stream": true,
    "input": [
      {
        "role": "user",
        "content": [
          {
            "type": "input_text",
            "text": "Describe this image in one sentence."
          },
          {
            "type": "input_image",
            "image_url": "data:image/png;base64,..."
          }
        ]
      }
    ]
  }'
```

图像生成：

```bash
curl http://127.0.0.1:8765/v1/images/generations \
  -H "authorization: Bearer choose-a-local-secret" \
  -H "content-type: application/json" \
  -d '{
    "model": "gpt-image-1",
    "prompt": "Draw a red circle on a white background.",
    "size": "1024x1024",
    "n": 1
  }'
```

图像编辑：

```bash
curl http://127.0.0.1:8765/v1/images/edits \
  -H "authorization: Bearer choose-a-local-secret" \
  -H "content-type: application/json" \
  -d '{
    "model": "gpt-image-2",
    "prompt": "Make this image look like a watercolor illustration.",
    "images": [{"image_url": "data:image/png;base64,..."}],
    "size": "auto"
  }'
```

通过 Responses 生成图像：

```bash
curl http://127.0.0.1:8765/v1/responses \
  -H "authorization: Bearer choose-a-local-secret" \
  -H "content-type: application/json" \
  -d '{
    "model": "gpt-5.5",
    "store": false,
    "stream": true,
    "input": [
      {
        "role": "user",
        "content": [
          {
            "type": "input_text",
            "text": "Draw a red circle on a white background."
          }
        ]
      }
    ],
    "tools": [
      {
        "type": "image_generation",
        "size": "1024x1024"
      }
    ]
  }'
```

Codex 后端目前要求这个路径上的 `input` 是 list，并且 `store:false`、
`stream:true`。`store:false` 表示请求不要被保存成 ChatGPT/Codex 历史 turn；这对这个
本地代理是刻意的。只有当代理也实现保存 turn 的后续读取、延续、清理等生命周期能力时，
`store:true` 才更合适。`stream:true` 表示后端返回 server-sent events，而不是完整的一次性
JSON 响应，这也和本代理的流式透传行为一致。

## 安全说明

- 默认绑定到 `127.0.0.1`。
- Compose 示例也绑定到 `127.0.0.1`。
- 允许浏览器 CORS 请求，因此本地 web 工具可以从另一个 localhost origin 调用代理。
  代理 bearer token 仍然是必需的。
- 如果要从另一台机器访问，优先使用 SSH tunneling，而不是直接暴露服务。
- 不要把 Codex access token 当作代理 API key。
- 不要记录请求 headers 或 `~/.codex/auth.json`。
- 本项目面向可信的个人环境，不适合作为共享的公网 API gateway。
