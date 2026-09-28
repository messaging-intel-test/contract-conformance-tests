use anyhow::{bail, Context, Result};
use reqwest::{blocking::Client, redirect::Policy, Method, Url};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_URL: &str = "http://127.0.0.1:9587";
const PROTOCOL_VERSION: &str = "beamscale.desktop-daemon/v1";
const MIN_TOKEN_BYTES: usize = 24;
const MAX_TOKEN_BYTES: usize = 4096;
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
static IDEMPOTENCY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct DaemonClient {
    client: Client,
    base_url: String,
}

impl DaemonClient {
    pub fn load() -> Result<Self> {
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .context("build BeamScale daemon HTTP client")?;
        let configured = env::var("BMSCL_DAEMON_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
        let base_url = validate_daemon_url(&configured)?;

        return Ok(Self { client, base_url });
    }

    pub fn doctor(&self) -> Result<Value> {
        return self.request(Method::GET, "/v1/doctor", None);
    }

    pub fn status(&self) -> Result<Value> {
        return self.request(Method::GET, "/v1/status", None);
    }

    pub fn runtime_start(&self, project: &str) -> Result<Value> {
        let project = PathBuf::from(project)
            .canonicalize()
            .with_context(|| format!("resolve BeamScale project {project}"))?;
        return self.request(
            Method::POST,
            "/v1/runtime/start",
            Some(json!({"project_dir": project, "poll_ms": 250, "module": null})),
        );
    }

    pub fn runtime_stop(&self) -> Result<Value> {
        return self.request(Method::POST, "/v1/runtime/stop", None);
    }

    pub fn runtime_restart(&self) -> Result<Value> {
        return self.request(Method::POST, "/v1/runtime/restart", None);
    }

    pub fn tunnel_start(&self, name: &str, hostname: &str) -> Result<Value> {
        let name = name.trim();
        if name.is_empty() {
            bail!("tunnel name must not be empty");
        }
        if name.len() > 128 {
            bail!("tunnel name is too long");
        }
        let hostname = if hostname.trim().is_empty() {
            Value::Null
        } else {
            Value::String(hostname.trim().to_string())
        };
        return self.request(
            Method::POST,
            "/v1/tunnel/start",
            Some(json!({"name": name, "hostname": hostname, "config": null})),
        );
    }

    pub fn tunnel_stop(&self) -> Result<Value> {
        return self.request(Method::POST, "/v1/tunnel/stop", None);
    }

    pub fn tunnel_restart(&self) -> Result<Value> {
        return self.request(Method::POST, "/v1/tunnel/restart", None);
    }

    pub fn set_keep_awake(&self, enabled: bool) -> Result<Value> {
        return self.request(
            Method::PUT,
            "/v1/settings",
            Some(json!({"keep_alive_during_lock": enabled})),
        );
    }

    pub fn update_status(&self) -> Result<Value> {
        return self.request(Method::GET, "/v1/update/status", None);
    }

    pub fn update_apply(&self) -> Result<Value> {
        return self.request(Method::POST, "/v1/update/apply", None);
    }

    fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let token = daemon_token()?;
        let is_mutation = method != Method::GET && method != Method::HEAD;
        let mut builder = self
            .client
            .request(method, url)
            .bearer_auth(token)
            .header("x-ores-protocol-version", PROTOCOL_VERSION);
        if is_mutation {
            builder = builder.header("x-ores-idempotency-key", next_idempotency_key()?);
        }
        let builder = match body {
            Some(body) => {
                let encoded = serde_json::to_vec(&body).context("encode daemon request JSON")?;
                if encoded.len() > MAX_REQUEST_BYTES {
                    bail!("daemon request exceeds {MAX_REQUEST_BYTES} byte limit");
                }
                builder
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(encoded)
            }
            None => builder,
        };
        let mut response = builder.send().context("contact BeamScale desktop daemon")?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            bail!("daemon response exceeds {MAX_RESPONSE_BYTES} byte limit");
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take((MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .context("read daemon response")?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            bail!("daemon response exceeds {MAX_RESPONSE_BYTES} byte limit");
        }
        if !status.is_success() {
            bail!("daemon returned {status}: {}", String::from_utf8_lossy(&bytes));
        }
        if bytes.is_empty() {
            return Ok(json!({"ok": true}));
        }
        return serde_json::from_slice(&bytes).context("parse daemon JSON");
    }
}

fn validate_daemon_url(raw: &str) -> Result<String> {
    let parsed = Url::parse(raw).with_context(|| format!("parse BMSCL_DAEMON_URL={raw}"))?;
    if parsed.scheme() != "http" {
        bail!("BMSCL_DAEMON_URL must use http:// on numeric loopback");
    }
    if parsed.port().is_none() {
        bail!("BMSCL_DAEMON_URL must include an explicit loopback port");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("BMSCL_DAEMON_URL must not contain credentials");
    }
    let host = parsed
        .host_str()
        .context("BMSCL_DAEMON_URL must include a loopback host")?
        .to_ascii_lowercase();
    if host != "127.0.0.1" && host != "[::1]" {
        bail!("BMSCL_DAEMON_URL must use numeric loopback 127.0.0.1 or ::1");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() || parsed.path() != "/" {
        bail!("BMSCL_DAEMON_URL must be an origin without a path, query, or fragment");
    }
    return Ok(raw.trim_end_matches('/').to_string());
}

fn next_idempotency_key() -> Result<String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?;
    let sequence = IDEMPOTENCY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    return Ok(format!(
        "bmscl-desktop-app-{}-{}-{sequence}",
        std::process::id(),
        elapsed.as_nanos()
    ));
}

fn daemon_token() -> Result<String> {
    if let Ok(token) = env::var("BMSCL_DAEMON_TOKEN") {
        if !token.is_empty() {
            return validate_token(token, "BMSCL_DAEMON_TOKEN");
        }
    }
    let path = token_path()?;
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("inspect daemon token {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        bail!("daemon token {} must be a regular non-symlink file", path.display());
    }
    if metadata.len() > (MAX_TOKEN_BYTES + 2) as u64 {
        bail!("daemon token {} is too large", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("daemon token {} must not be accessible by group/other", path.display());
        }
    }
    let token = fs::read_to_string(&path)
        .with_context(|| format!("read daemon token {}", path.display()))?;
    return validate_token(trim_token_file_terminator(token), &path.display().to_string());
}

fn trim_token_file_terminator(mut token: String) -> String {
    while token.ends_with('\n') || token.ends_with('\r') {
        token.pop();
    }
    return token;
}

fn validate_token(token: String, source: &str) -> Result<String> {
    let bytes = token.as_bytes();
    if bytes.len() < MIN_TOKEN_BYTES || bytes.len() > MAX_TOKEN_BYTES {
        bail!("daemon token from {source} must contain {MIN_TOKEN_BYTES}..={MAX_TOKEN_BYTES} bytes");
    }
    if bytes.iter().any(|byte| *byte <= 0x20 || *byte == 0x7f) {
        bail!("daemon token from {source} must not contain whitespace or control bytes");
    }
    return Ok(token);
}

fn token_path() -> Result<PathBuf> {
    if let Ok(root) = env::var("BMSCL_DESKTOP_HOME") {
        let root = root.trim();
        if !root.is_empty() {
            return Ok(Path::new(root).join("token"));
        }
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("cannot locate home directory; set BMSCL_DESKTOP_HOME")?;
    return Ok(home.join(".beamscale").join("desktop-daemon").join("token"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_name_must_be_present_and_bounded() {
        let client = Client::builder().build().expect("client");
        let daemon = DaemonClient { client, base_url: DEFAULT_URL.to_string() };
        assert!(daemon.tunnel_start("", "").is_err());
        assert!(daemon.tunnel_start(&"x".repeat(129), "").is_err());
    }

    #[test]
    fn idempotency_keys_are_unique_within_process() {
        let first = next_idempotency_key().expect("first key");
        let second = next_idempotency_key().expect("second key");
        assert_ne!(first, second);
        assert!(first.starts_with(&format!("bmscl-desktop-app-{}-", std::process::id())));
    }

    #[test]
    fn daemon_token_policy_is_bounded_and_control_free() {
        assert!(validate_token("a".repeat(MIN_TOKEN_BYTES), "test").is_ok());
        assert!(validate_token("a".repeat(MIN_TOKEN_BYTES - 1), "test").is_err());
        assert!(validate_token("a".repeat(MAX_TOKEN_BYTES + 1), "test").is_err());
        assert!(validate_token(format!("{}\n", "a".repeat(MIN_TOKEN_BYTES)), "test").is_err());
    }

    #[test]
    fn daemon_url_must_be_numeric_loopback_http_origin() {
        assert!(validate_daemon_url("http://127.0.0.1:9587").is_ok());
        assert!(validate_daemon_url("http://[::1]:9587").is_ok());
        assert!(validate_daemon_url("http://localhost:9587").is_err());
        assert!(validate_daemon_url("http://127.0.0.1").is_err());
        assert!(validate_daemon_url("https://127.0.0.1:9587").is_err());
        assert!(validate_daemon_url("http://example.com:9587").is_err());
    }
}
