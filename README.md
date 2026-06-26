# codex-auth-proxy

Small localhost API proxy that uses your existing Codex ChatGPT login to call the
Codex Responses backend.

It exposes:

- `GET /healthz`
- `GET /models`
- `GET /v1/models`
- `POST /v1/responses`

The proxy has its own bearer token. It never exposes the Codex access token to
callers.

## Architecture

![codex-auth-proxy architecture](docs/architecture.svg)

## Build

```bash
cargo build --release
```

## Release Builds

GitHub Actions builds Linux x86_64, macOS aarch64, and Windows x86_64 binaries
on pushes, pull requests, and manual workflow runs. Push a tag such as `v0.1.0`
to publish the same archives as GitHub Release assets.

## Run

Log in first. Browser login works on a machine with a local browser:

```bash
codex-auth-proxy login
```

On a remote or headless machine, use device-code login:

```bash
codex-auth-proxy login --device-auth
```

To isolate this proxy from your normal Codex login, use a separate Codex home:

```bash
CODEX_HOME="$HOME/.codex-proxy-profile" codex-auth-proxy login --device-auth
```

Starting the proxy also checks whether Codex auth is available. If it is not,
the proxy runs the same login flow first and then continues startup, so a fresh
machine only needs one command:

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
cargo run --release -- --listen 127.0.0.1:8765
```

Use device-code login during proxy startup on remote or headless machines:

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
cargo run --release -- --listen 127.0.0.1:8765 --device-auth
```

Log out and clear stored Codex auth:

```bash
codex-auth-proxy logout
```

Options can be passed as flags or environment variables:

| Flag | Env | Default |
| --- | --- | --- |
| `--listen` | `CODEX_PROXY_LISTEN` | `127.0.0.1:8765` |
| `--api-key` | `CODEX_PROXY_API_KEY` | required |
| `--codex-home` | `CODEX_HOME` | `$HOME/.codex` |
| `--upstream-base-url` | `CODEX_PROXY_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` |
| `--codex-client-version` | `CODEX_PROXY_CODEX_CLIENT_VERSION` | Codex dependency tag version |
| `--auth-refresh-interval-secs` | `CODEX_PROXY_AUTH_REFRESH_INTERVAL_SECS` | `60` |
| `--device-auth` | - | `false` |

`--codex-client-version` is sent only when fetching the Codex model catalog.
The Codex backend filters `/models` by minimum client version, so the default is
derived from the pinned `openai/codex` git dependency tag.

`--auth-refresh-interval-secs` controls a background Codex auth refresh check.
The proxy calls Codex `AuthManager::auth()` on that interval, so the same
near-expiry refresh behavior used by Codex also runs while the proxy is idle.
Set it to `0` to disable the background check; request-time refresh and one
retry after upstream `401` still remain enabled.

Run the proxy against the isolated Codex home with the same `CODEX_HOME`:

```bash
export CODEX_PROXY_API_KEY="choose-a-local-secret"
CODEX_HOME="$HOME/.codex-proxy-profile" \
  cargo run --release -- --listen 127.0.0.1:8765
```

## Call

List models in an OpenAI-compatible shape:

```bash
curl http://127.0.0.1:8765/v1/models \
  -H "authorization: Bearer choose-a-local-secret"
```

List the raw Codex model catalog, including Codex-specific metadata such as
service tiers and model capabilities:

```bash
curl http://127.0.0.1:8765/models \
  -H "authorization: Bearer choose-a-local-secret"
```

Text response:

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

Image understanding:

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

Image generation:

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

The Codex backend currently expects `input` to be a list, `store:false`, and
`stream:true` on this path. `store:false` asks the backend not to persist this
request as a saved ChatGPT/Codex turn; that is intentional for this local proxy.
Using `store:true` would only be better for a proxy that also exposes saved-turn
lifecycle behavior, such as later retrieval, continuation, and cleanup.
`stream:true` means the backend returns server-sent events instead of one
complete JSON response, matching this proxy's streaming pass-through behavior.

## Security Notes

- Bind to `127.0.0.1` by default.
- Use SSH tunneling for another machine instead of exposing this directly.
- Do not use your Codex access token as the proxy API key.
- Do not log request headers or `~/.codex/auth.json`.
- This is intended for a trusted personal environment, not as a shared public API
  gateway.
