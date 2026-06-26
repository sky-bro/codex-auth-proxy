use http::HeaderMap;
use http::HeaderValue;
use http::StatusCode;
use http::header;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8765";
pub const DEFAULT_AUTH_REFRESH_INTERVAL_SECS: u64 = 60;
pub const DEFAULT_CODEX_CLIENT_VERSION: &str = env!("CODEX_AUTH_PROXY_CODEX_CLIENT_VERSION");
pub const CODEX_MODELS_PATH: &str = "/models";
pub const CODEX_RESPONSES_PATH: &str = "/responses";
pub const OPENAI_MODELS_PATH: &str = "/v1/models";
pub const OPENAI_RESPONSES_PATH: &str = "/v1/responses";
pub const INSTALLATION_ID_FILENAME: &str = "installation_id";
pub const X_CODEX_INSTALLATION_ID: &str = "x-codex-installation-id";
pub const X_CLIENT_REQUEST_ID: &str = "x-client-request-id";

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub listen: String,
    pub proxy_api_key: String,
    pub codex_home: String,
    pub upstream_base_url: String,
}

pub fn responses_url(base_url: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), CODEX_RESPONSES_PATH)
}

pub fn models_url(base_url: &str, client_version: &str) -> String {
    format!(
        "{}{}?client_version={}",
        base_url.trim_end_matches('/'),
        CODEX_MODELS_PATH,
        client_version
    )
}

pub fn openai_models_body_from_codex_catalog(body: &[u8]) -> Result<Vec<u8>, String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid models JSON: {err}"))?;
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "models response must contain a models list".to_string())?;

    let data: Vec<Value> = models
        .iter()
        .filter_map(|model| model.get("slug").and_then(Value::as_str))
        .map(|slug| {
            json!({
                "id": slug,
                "object": "model",
                "created": 0,
                "owned_by": "openai"
            })
        })
        .collect();

    serde_json::to_vec(&json!({
        "object": "list",
        "data": data
    }))
    .map_err(|err| format!("failed to encode models JSON: {err}"))
}

pub fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

pub fn resolve_installation_id(codex_home: &Path) -> io::Result<String> {
    let path = codex_home.join(INSTALLATION_ID_FILENAME);
    if let Ok(existing) = fs::read_to_string(&path) {
        let existing = existing.trim();
        if uuid::Uuid::parse_str(existing).is_ok() {
            return Ok(existing.to_string());
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(codex_home)?;
    fs::write(path, format!("{id}\n"))?;
    Ok(id)
}

pub fn bearer_is_authorized(headers: &HeaderMap, expected_key: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    value == format!("Bearer {expected_key}")
}

pub fn reject_unauthorized() -> (StatusCode, &'static str) {
    (
        StatusCode::UNAUTHORIZED,
        "missing or invalid proxy bearer token",
    )
}

pub fn upstream_headers(
    access_token: &str,
    account_id: Option<&str>,
    is_fedramp_account: bool,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let bearer = format!("Bearer {access_token}");
    if let Ok(value) = HeaderValue::from_str(&bearer) {
        headers.insert(header::AUTHORIZATION, value);
    }
    if let Some(account_id) = account_id
        && let Ok(value) = HeaderValue::from_str(account_id)
    {
        headers.insert("ChatGPT-Account-ID", value);
    }
    if is_fedramp_account {
        headers.insert("X-OpenAI-Fedramp", HeaderValue::from_static("true"));
    }
    headers
}

pub fn codex_provider_headers() -> HeaderMap {
    let provider = codex_model_provider_info::ModelProviderInfo::create_openai_provider(None);
    provider
        .to_api_provider(None)
        .map(|provider| provider.headers)
        .unwrap_or_default()
}

pub fn normalize_responses_body(body: &[u8], installation_id: &str) -> Result<Vec<u8>, String> {
    let mut value: Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid JSON body: {err}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "request body must be a JSON object".to_string())?;

    match object.get("input") {
        Some(Value::Array(_)) => {}
        Some(_) => return Err("input must be a list".to_string()),
        None => return Err("input is required".to_string()),
    }

    object.insert("store".to_string(), Value::Bool(false));
    object.insert("stream".to_string(), Value::Bool(true));

    match object.get_mut("client_metadata") {
        Some(Value::Object(metadata)) => {
            metadata.insert(X_CODEX_INSTALLATION_ID.to_string(), json!(installation_id));
        }
        Some(_) => return Err("client_metadata must be an object".to_string()),
        None => {
            object.insert(
                "client_metadata".to_string(),
                json!({ X_CODEX_INSTALLATION_ID: installation_id }),
            );
        }
    }

    serde_json::to_vec(&value).map_err(|err| format!("failed to encode JSON body: {err}"))
}

pub fn copy_response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut copied = HeaderMap::new();
    for name in [header::CONTENT_TYPE, header::CACHE_CONTROL] {
        if let Some(value) = headers.get(&name) {
            copied.insert(name, value.clone());
        }
    }
    for name in [
        "openai-processing-ms",
        "openai-organization",
        "openai-project",
        "x-request-id",
    ] {
        if let Some(value) = headers.get(name) {
            copied.insert(name, value.clone());
        }
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    #[test]
    fn responses_url_targets_codex_backend_responses_endpoint() {
        assert_eq!(
            responses_url("https://chatgpt.com/backend-api/codex/"),
            "https://chatgpt.com/backend-api/codex/responses"
        );
    }

    #[test]
    fn models_url_targets_codex_backend_models_endpoint_with_client_version() {
        assert_eq!(
            models_url(
                "https://chatgpt.com/backend-api/codex/",
                DEFAULT_CODEX_CLIENT_VERSION
            ),
            "https://chatgpt.com/backend-api/codex/models?client_version=0.142.2"
        );
    }

    #[test]
    fn openai_models_body_projects_codex_catalog_slugs() {
        let body = br#"{
            "models": [
                {
                    "slug": "gpt-test",
                    "display_name": "GPT Test",
                    "service_tiers": [{"id": "priority", "display_name": "Fast"}]
                },
                {
                    "slug": "gpt-other",
                    "display_name": "GPT Other"
                }
            ]
        }"#;

        let projected = openai_models_body_from_codex_catalog(body).expect("project catalog");
        let value: Value = serde_json::from_slice(&projected).expect("json");

        assert_eq!(value["object"], "list");
        assert_eq!(value["data"][0]["id"], "gpt-test");
        assert_eq!(value["data"][0]["object"], "model");
        assert_eq!(value["data"][0]["created"], 0);
        assert_eq!(value["data"][0]["owned_by"], "openai");
        assert_eq!(value["data"][1]["id"], "gpt-other");
    }

    #[test]
    fn bearer_authorization_requires_exact_proxy_key() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer local-secret"),
        );

        assert!(bearer_is_authorized(&headers, "local-secret"));
        assert!(!bearer_is_authorized(&headers, "wrong"));
    }

    #[test]
    fn upstream_headers_add_codex_account_routing_without_proxy_key() {
        let headers = upstream_headers("codex-token", Some("account-123"), true);

        assert_eq!(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer codex-token")
        );
        assert_eq!(
            headers
                .get("ChatGPT-Account-ID")
                .and_then(|v| v.to_str().ok()),
            Some("account-123")
        );
        assert_eq!(
            headers
                .get("X-OpenAI-Fedramp")
                .and_then(|v| v.to_str().ok()),
            Some("true")
        );
    }

    #[test]
    fn codex_provider_headers_include_client_version() {
        let headers = codex_provider_headers();

        assert!(headers.contains_key("version"));
    }

    #[test]
    fn resolve_installation_id_reuses_existing_uuid() {
        let dir = std::env::temp_dir().join(format!(
            "codex-auth-proxy-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let id = uuid::Uuid::new_v4().to_string();
        fs::write(dir.join(INSTALLATION_ID_FILENAME), format!("{id}\n")).expect("write id");

        assert_eq!(resolve_installation_id(&dir).expect("resolve id"), id);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn normalize_responses_body_forces_codex_required_fields() {
        let body = br#"{
            "model": "gpt-5.5",
            "store": true,
            "stream": false,
            "input": [{"role":"user","content":[{"type":"input_text","text":"hi"}]}],
            "client_metadata": {"existing": "kept"}
        }"#;

        let normalized = normalize_responses_body(body, "11111111-1111-4111-8111-111111111111")
            .expect("normalize body");
        let value: Value = serde_json::from_slice(&normalized).expect("json");

        assert_eq!(value["store"], false);
        assert_eq!(value["stream"], true);
        assert_eq!(value["client_metadata"]["existing"], "kept");
        assert_eq!(
            value["client_metadata"][X_CODEX_INSTALLATION_ID],
            "11111111-1111-4111-8111-111111111111"
        );
    }

    #[test]
    fn normalize_responses_body_rejects_string_input() {
        let err = normalize_responses_body(br#"{"model":"gpt-5.5","input":"hi"}"#, "id")
            .expect_err("string input should fail");

        assert_eq!(err, "input must be a list");
    }

    #[test]
    fn copied_response_headers_exclude_upstream_auth_and_hop_by_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert("x-request-id", HeaderValue::from_static("req_123"));

        let copied = copy_response_headers(&headers);

        assert_eq!(
            copied
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            copied.get("x-request-id").and_then(|v| v.to_str().ok()),
            Some("req_123")
        );
        assert!(!copied.contains_key(header::AUTHORIZATION));
        assert!(!copied.contains_key(header::CONNECTION));
    }
}
