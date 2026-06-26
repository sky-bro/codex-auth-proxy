use anyhow::Context;
use axum::body::Body;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Response;
use axum::http::StatusCode;
use axum::routing::get;
use axum::routing::post;
use clap::Args as ClapArgs;
use clap::Parser;
use clap::Subcommand;
use codex_auth_proxy::CODEX_MODELS_PATH;
use codex_auth_proxy::DEFAULT_CODEX_CLIENT_VERSION;
use codex_auth_proxy::DEFAULT_LISTEN;
use codex_auth_proxy::OPENAI_MODELS_PATH;
use codex_auth_proxy::OPENAI_RESPONSES_PATH;
use codex_auth_proxy::X_CLIENT_REQUEST_ID;
use codex_auth_proxy::X_CODEX_INSTALLATION_ID;
use codex_auth_proxy::bearer_is_authorized;
use codex_auth_proxy::codex_provider_headers;
use codex_auth_proxy::copy_response_headers;
use codex_auth_proxy::default_codex_home;
use codex_auth_proxy::models_url;
use codex_auth_proxy::normalize_responses_body;
use codex_auth_proxy::openai_models_body_from_codex_catalog;
use codex_auth_proxy::reject_unauthorized;
use codex_auth_proxy::resolve_installation_id;
use codex_auth_proxy::responses_url;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::CLIENT_ID;
use codex_login::ServerOptions;
use codex_login::default_client::build_reqwest_client;
use codex_login::logout_with_revoke;
use codex_login::run_device_code_login;
use codex_login::run_login_server;
use codex_model_provider::auth_provider_from_auth;
use codex_model_provider_info::CHATGPT_CODEX_BASE_URL;
use futures_util::TryStreamExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(version, about = "Local API proxy backed by Codex ChatGPT auth")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, env = "CODEX_PROXY_LISTEN", default_value = DEFAULT_LISTEN)]
    listen: SocketAddr,

    #[arg(long, env = "CODEX_PROXY_API_KEY")]
    api_key: Option<String>,

    #[arg(long, env = "CODEX_HOME")]
    codex_home: Option<PathBuf>,

    #[arg(long, env = "CODEX_PROXY_UPSTREAM_BASE_URL")]
    upstream_base_url: Option<String>,

    #[arg(
        long,
        env = "CODEX_PROXY_CODEX_CLIENT_VERSION",
        default_value = DEFAULT_CODEX_CLIENT_VERSION
    )]
    codex_client_version: String,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Log in with ChatGPT auth and write credentials under CODEX_HOME.
    Login(LoginCommand),
}

#[derive(Debug, ClapArgs)]
struct LoginCommand {
    #[arg(long, env = "CODEX_HOME")]
    codex_home: Option<PathBuf>,

    /// Use device-code login for remote or headless machines.
    #[arg(long)]
    device_auth: bool,
}

#[derive(Clone)]
struct AppState {
    auth_manager: Arc<AuthManager>,
    client: reqwest::Client,
    proxy_api_key: String,
    upstream_base_url: Option<String>,
    installation_id: String,
    codex_client_version: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    if let Some(command) = args.command {
        return match command {
            Command::Login(login) => run_login(login).await,
        };
    }

    run_proxy(args).await
}

async fn run_proxy(args: Args) -> anyhow::Result<()> {
    let codex_home = args.codex_home.unwrap_or_else(default_codex_home);
    let proxy_api_key = args
        .api_key
        .context("CODEX_PROXY_API_KEY is required unless using a subcommand")?;
    let installation_id = resolve_installation_id(&codex_home).with_context(|| {
        format!(
            "failed to resolve installation id in {}",
            codex_home.display()
        )
    })?;
    let auth_manager = AuthManager::shared(
        codex_home,
        false,
        AuthCredentialsStoreMode::Auto,
        None,
        None,
        AuthKeyringBackendKind::default(),
        None,
    )
    .await;

    let state = AppState {
        auth_manager,
        client: build_reqwest_client(),
        proxy_api_key,
        upstream_base_url: args.upstream_base_url,
        installation_id,
        codex_client_version: args.codex_client_version,
    };

    let app = axum::Router::new()
        .route("/healthz", get(healthz))
        .route(CODEX_MODELS_PATH, get(codex_models))
        .route(OPENAI_MODELS_PATH, get(openai_models))
        .route(OPENAI_RESPONSES_PATH, post(responses))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("failed to bind {}", args.listen))?;
    eprintln!("{}", startup_message(args.listen));
    tracing::info!("listening on http://{}", args.listen);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn startup_message(listen: SocketAddr) -> String {
    format!(
        "codex-auth-proxy listening on http://{listen}\n\
         endpoints:\n\
           GET /healthz\n\
           GET /models\n\
           GET /v1/models\n\
           POST /v1/responses"
    )
}

async fn run_login(args: LoginCommand) -> anyhow::Result<()> {
    let codex_home = args.codex_home.unwrap_or_else(default_codex_home);
    let store_mode = AuthCredentialsStoreMode::Auto;
    let keyring_backend = AuthKeyringBackendKind::default();

    if let Err(err) = logout_with_revoke(&codex_home, store_mode, keyring_backend, None).await {
        tracing::warn!("failed to clear existing auth before login: {err}");
    }

    let opts = ServerOptions::new(
        codex_home,
        CLIENT_ID.to_string(),
        None,
        store_mode,
        keyring_backend,
        None,
    );

    if args.device_auth {
        run_device_code_login(opts)
            .await
            .context("failed to log in with device code")?;
    } else {
        let server = run_login_server(opts).context("failed to start login server")?;
        eprintln!(
            "Starting local login server on http://localhost:{}.\nIf your browser did not open, navigate to this URL to authenticate:\n\n{}\n\nOn a remote or headless machine? Use `codex-auth-proxy login --device-auth` instead.",
            server.actual_port, server.auth_url
        );
        server
            .block_until_done()
            .await
            .context("login server failed")?;
    }

    eprintln!("Successfully logged in");
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn codex_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !bearer_is_authorized(&headers, &state.proxy_api_key) {
        let (status, message) = reject_unauthorized();
        return Err((status, message.to_string()));
    }

    let first = forward_models(&state).await?;
    if first.status != StatusCode::UNAUTHORIZED {
        return response_from_bytes(first.status, first.headers, first.body);
    }

    state
        .auth_manager
        .refresh_token()
        .await
        .map_err(|err| (StatusCode::UNAUTHORIZED, err.to_string()))?;
    let second = forward_models(&state).await?;
    response_from_bytes(second.status, second.headers, second.body)
}

async fn openai_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !bearer_is_authorized(&headers, &state.proxy_api_key) {
        let (status, message) = reject_unauthorized();
        return Err((status, message.to_string()));
    }

    let first = forward_models(&state).await?;
    let upstream = if first.status == StatusCode::UNAUTHORIZED {
        state
            .auth_manager
            .refresh_token()
            .await
            .map_err(|err| (StatusCode::UNAUTHORIZED, err.to_string()))?;
        forward_models(&state).await?
    } else {
        first
    };

    if !upstream.status.is_success() {
        return response_from_bytes(upstream.status, upstream.headers, upstream.body);
    }

    let body = openai_models_body_from_codex_catalog(&upstream.body)
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    response_from_bytes(upstream.status, upstream.headers, Bytes::from(body))
}

async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !bearer_is_authorized(&headers, &state.proxy_api_key) {
        let (status, message) = reject_unauthorized();
        return Err((status, message.to_string()));
    }

    let body = Bytes::from(
        normalize_responses_body(&body, &state.installation_id)
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?,
    );

    let first = forward_responses(&state, body.clone()).await?;
    if first.status() != StatusCode::UNAUTHORIZED {
        return Ok(first);
    }

    state
        .auth_manager
        .refresh_token()
        .await
        .map_err(|err| (StatusCode::UNAUTHORIZED, err.to_string()))?;
    forward_responses(&state, body).await
}

struct UpstreamBytes {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

async fn forward_models(state: &AppState) -> Result<UpstreamBytes, (StatusCode, String)> {
    let auth = state.auth_manager.auth().await.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Codex is not logged in".to_string(),
        )
    })?;
    let base_url = state
        .upstream_base_url
        .clone()
        .unwrap_or_else(|| CHATGPT_CODEX_BASE_URL.to_string());
    let url = models_url(&base_url, &state.codex_client_version);
    let mut headers = codex_provider_headers();
    headers.extend(auth_provider_from_auth(&auth).to_auth_headers());
    let response = state
        .client
        .get(url)
        .headers(headers)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;

    let status = response.status();
    let headers = copy_response_headers(response.headers());
    let body = response
        .bytes()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;
    Ok(UpstreamBytes {
        status,
        headers,
        body,
    })
}

fn response_from_bytes(
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, (StatusCode, String)> {
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        if let Some(name) = name {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from(body))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn forward_responses(
    state: &AppState,
    body: Bytes,
) -> Result<Response<Body>, (StatusCode, String)> {
    let auth = state.auth_manager.auth().await.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Codex is not logged in".to_string(),
        )
    })?;
    let base_url = state
        .upstream_base_url
        .clone()
        .unwrap_or_else(|| CHATGPT_CODEX_BASE_URL.to_string());
    let url = responses_url(&base_url);
    let mut headers = codex_provider_headers();
    headers.extend(auth_provider_from_auth(&auth).to_auth_headers());
    let response = state
        .client
        .post(url)
        .headers(headers)
        .header("accept", "text/event-stream")
        .header("content-type", "application/json")
        .header(X_CODEX_INSTALLATION_ID, state.installation_id.as_str())
        .header(X_CLIENT_REQUEST_ID, uuid::Uuid::new_v4().to_string())
        .body(body)
        .send()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;

    let status = response.status();
    let headers = copy_response_headers(response.headers());
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        if let Some(name) = name {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn login_subcommand_does_not_require_proxy_api_key() {
        let args = Args::try_parse_from([
            "codex-auth-proxy",
            "login",
            "--device-auth",
            "--codex-home",
            "/tmp/codex-auth-proxy-login-test",
        ])
        .expect("login subcommand should parse without CODEX_PROXY_API_KEY");

        assert!(matches!(args.command, Some(Command::Login(login)) if login.device_auth));
    }

    #[test]
    fn proxy_uses_codex_client_version_for_models_catalog() {
        let args = Args::try_parse_from([
            "codex-auth-proxy",
            "--api-key",
            "local-secret",
            "--codex-client-version",
            "0.142.2",
        ])
        .expect("proxy args should parse");

        assert_eq!(args.codex_client_version, "0.142.2");
    }

    #[test]
    fn startup_message_shows_listen_url_and_endpoints() {
        let addr: SocketAddr = "127.0.0.1:8765".parse().expect("addr");

        let message = startup_message(addr);

        assert!(message.contains("http://127.0.0.1:8765"));
        assert!(message.contains("GET /v1/models"));
        assert!(message.contains("POST /v1/responses"));
    }
}
