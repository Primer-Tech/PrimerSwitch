//! Version-isolated Claude OAuth and usage contracts. No UI or credential storage.

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt, path::PathBuf, sync::Arc, time::Duration};
use switcher_core::{
    ResetOutcome, StoredAccount, UsageResponse, parse_reset_outcome, usage_from_headers,
    usage_from_limit_refusal,
};
use thiserror::Error;
use zeroize::Zeroizing;

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTHORIZE_URL: &str = "https://platform.claude.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage?cedar_ember=1&skip_spend=1";
pub const INFERENCE_URL: &str = "https://api.anthropic.com/v1/messages";
pub const BETA: &str = "oauth-2025-04-20";
pub const LOGIN_SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
pub const REFRESH_SCOPES: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
/// Source-compatible fallback, used only when public installation metadata is absent.
pub const FALLBACK_CLI_VERSION: &str = "2.1.259";

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ClientError {
    #[error("Could not connect to Anthropic. Try again later.")]
    Transport,
    #[error("Anthropic rejected this token. Sign in to this account again.")]
    Unauthorized,
    /// The token endpoint rejected the refresh token or code itself: retrying with
    /// the same grant cannot succeed, only a new sign-in can.
    #[error("Anthropic no longer accepts this sign-in. Sign in to this account again.")]
    InvalidGrant,
    #[error("Anthropic is temporarily limiting requests. Waiting before retrying.")]
    RateLimited { retry_after: Option<u64> },
    #[error("Unexpected response from Anthropic (HTTP {0}).")]
    Http(u16),
    #[error("Anthropic's response is missing required data.")]
    InvalidResponse,
    #[error("This code does not match the pending sign-in, or the sign-in has expired.")]
    InvalidLogin,
    #[error("The request identifier is invalid.")]
    InvalidIdentifier,
    #[error("The Claude Code version is invalid.")]
    InvalidVersion,
}

impl ClientError {
    /// Only fixed, reviewed error text is localized; provider response bodies are excluded.
    pub fn message(&self, language: &str) -> String {
        if language != "ro" {
            return self.to_string();
        }
        match self {
            Self::Transport => "Conexiunea cu Anthropic a eșuat. Reîncearcă mai târziu.".into(),
            Self::Unauthorized => "Token respins de Anthropic. Reautentifică acest cont.".into(),
            Self::InvalidGrant => {
                "Anthropic nu mai acceptă această autentificare. Autentifică din nou acest cont."
                    .into()
            }
            Self::RateLimited { .. } => {
                "Anthropic limitează temporar cererile. Așteptăm înainte de reîncercare.".into()
            }
            Self::Http(status) => format!("Răspuns neașteptat de la Anthropic (HTTP {status})."),
            Self::InvalidResponse => "Răspunsul Anthropic nu conține datele necesare.".into(),
            Self::InvalidLogin => {
                "Codul nu aparține acestei autentificări sau autentificarea a expirat.".into()
            }
            Self::InvalidIdentifier => "Identificatorul cererii nu este valid.".into(),
            Self::InvalidVersion => "Versiunea Claude Code nu este validă.".into(),
        }
    }
}

pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
    pub timeout_seconds: u64,
}
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("body", &"[REDACTED]")
            .finish()
    }
}
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}
impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body", &"[REDACTED]")
            .finish()
    }
}
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ClientError>;
}

pub struct NetworkTransport {
    client: reqwest::Client,
}
impl NetworkTransport {
    pub fn new() -> Result<Self, ClientError> {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15));
        if let Some(path) = std::env::var_os("PRIMERSWITCH_CA_FILE") {
            let bytes = std::fs::read(path).map_err(|_| ClientError::Transport)?;
            let cert =
                reqwest::Certificate::from_pem(&bytes).map_err(|_| ClientError::Transport)?;
            builder = builder.add_root_certificate(cert);
        }
        Ok(Self {
            client: builder.build().map_err(|_| ClientError::Transport)?,
        })
    }
}
#[async_trait]
impl Transport for NetworkTransport {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ClientError> {
        let url = reqwest::Url::parse(&request.url).map_err(|_| ClientError::InvalidIdentifier)?;
        if url.scheme() != "https"
            || !matches!(
                url.host_str(),
                Some("api.anthropic.com" | "platform.claude.com")
            )
            || url.port_or_known_default() != Some(443)
        {
            return Err(ClientError::InvalidIdentifier);
        }
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| ClientError::InvalidIdentifier)?;
        let mut builder = self
            .client
            .request(method, url)
            .timeout(Duration::from_secs(request.timeout_seconds));
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }
        let mut response = builder.send().await.map_err(|_| ClientError::Transport)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(key, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (key.as_str().to_lowercase(), v.to_string()))
            })
            .collect();
        const MAX_BODY: usize = 1024 * 1024;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY as u64)
        {
            return Err(ClientError::InvalidResponse);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| ClientError::Transport)? {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(ClientError::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[derive(Clone)]
pub struct ClaudeClient {
    transport: Arc<dyn Transport>,
    user_agent: String,
}
impl ClaudeClient {
    pub fn new(version: &str) -> Result<Self, ClientError> {
        Self::with_transport(version, Arc::new(NetworkTransport::new()?))
    }
    pub fn with_transport(
        version: &str,
        transport: Arc<dyn Transport>,
    ) -> Result<Self, ClientError> {
        if !valid_version(version) {
            return Err(ClientError::InvalidVersion);
        }
        Ok(Self {
            transport,
            user_agent: format!("claude-cli/{version} (external, cli)"),
        })
    }
    fn request(
        &self,
        method: &'static str,
        url: &str,
        token: Option<&str>,
        body: Option<Value>,
        timeout_seconds: u64,
        beta: bool,
    ) -> HttpRequest {
        let mut headers = BTreeMap::from([
            ("content-type".into(), "application/json".into()),
            ("user-agent".into(), self.user_agent.clone()),
        ]);
        if let Some(token) = token {
            headers.insert("authorization".into(), format!("Bearer {token}"));
        }
        if beta {
            headers.insert("anthropic-beta".into(), BETA.into());
        }
        HttpRequest {
            method,
            url: url.into(),
            headers,
            body,
            timeout_seconds,
        }
    }
    async fn tokens(&self, body: Value) -> Result<TokenResponse, ClientError> {
        let response = self
            .transport
            .send(self.request("POST", TOKEN_URL, None, Some(body), 30, false))
            .await?;
        // OAuth reports a revoked or consumed grant as `invalid_grant`; this public
        // client has no secret, so a 401 here also means the grant was refused. Only
        // the fixed error code is inspected, never other response text.
        if response.status == 401
            || (response.status == 400 && response.body["error"] == "invalid_grant")
        {
            return Err(ClientError::InvalidGrant);
        }
        check_status(&response)?;
        TokenResponse::parse(&response.body)
    }
    pub async fn refresh_token(&self, token: &str) -> Result<TokenResponse, ClientError> {
        self.tokens(json!({"grant_type":"refresh_token","refresh_token":token,"client_id":CLIENT_ID,"scope":REFRESH_SCOPES})).await
    }
    pub async fn finish_login(
        &self,
        pending: &PendingLogin,
        pasted: &str,
        now: i64,
    ) -> Result<LoginResult, ClientError> {
        if now < pending.created_at - 30 || now - pending.created_at > 600 {
            return Err(ClientError::InvalidLogin);
        }
        let compact: Zeroizing<String> =
            Zeroizing::new(pasted.chars().filter(|c| !c.is_whitespace()).collect());
        let (code, returned_state) = compact.split_once('#').ok_or(ClientError::InvalidLogin)?;
        if code.is_empty()
            || code.len() > 4096
            || returned_state != pending.state
            || returned_state.contains('#')
        {
            return Err(ClientError::InvalidLogin);
        }
        let tokens = self.tokens(json!({"grant_type":"authorization_code","code":code,"state":returned_state,"redirect_uri":REDIRECT_URI,"client_id":CLIENT_ID,"code_verifier":pending.verifier.as_str()})).await?;
        let profile = self.profile(tokens.access_token.as_str()).await?;
        if profile
            .identity
            .get("accountUuid")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(ClientError::InvalidResponse);
        }
        let mut credentials = json!({"claudeAiOauth":{}});
        tokens.apply(&mut credentials, now);
        Ok(LoginResult {
            credentials,
            profile,
        })
    }
    pub async fn profile(&self, token: &str) -> Result<Profile, ClientError> {
        let mut request = self.request("GET", PROFILE_URL, Some(token), None, 15, false);
        request
            .headers
            .insert("cache-control".into(), "no-cache".into());
        let response = self.transport.send(request).await?;
        check_status(&response)?;
        Profile::parse(&response.body)
    }
    pub async fn metadata(&self, token: &str) -> Result<UsageResponse, ClientError> {
        let response = self
            .transport
            .send(self.request("GET", USAGE_URL, Some(token), None, 20, true))
            .await?;
        check_status(&response)?;
        let usage: UsageResponse =
            serde_json::from_value(response.body).map_err(|_| ClientError::InvalidResponse)?;
        if !usage.five_hour.utilization.is_finite()
            || !usage.seven_day.utilization.is_finite()
            || usage.five_hour.utilization < 0.0
            || usage.seven_day.utilization < 0.0
        {
            return Err(ClientError::InvalidResponse);
        }
        Ok(usage)
    }
    pub async fn inference(
        &self,
        token: &str,
        previous: Option<&UsageResponse>,
    ) -> Result<InferenceResult, ClientError> {
        let body = json!({"model":"claude-haiku-4-5-20251001","max_tokens":1,"system":[{"type":"text","text":"You are Claude Code, Anthropic's official CLI for Claude."}],"messages":[{"role":"user","content":"hi"}]});
        let mut request = self.request("POST", INFERENCE_URL, Some(token), Some(body), 30, true);
        request
            .headers
            .insert("anthropic-version".into(), "2023-06-01".into());
        request.headers.insert("x-app".into(), "cli".into());
        let response = self.transport.send(request).await?;
        if response.status == 429 {
            if let Some(usage) = usage_from_limit_refusal(&response.headers, previous) {
                let complete = has_both_window_headers(&response.headers);
                return Ok(InferenceResult {
                    usage,
                    complete,
                    accepted: false,
                    input_tokens: 0,
                    output_tokens: 0,
                });
            }
            return Err(rate_limit(&response));
        }
        check_status(&response)?;
        let usage = usage_from_headers(&response.headers).ok_or(ClientError::InvalidResponse)?;
        Ok(InferenceResult {
            usage,
            complete: true,
            accepted: true,
            input_tokens: response.body["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: response.body["usage"]["output_tokens"]
                .as_u64()
                .unwrap_or(0),
        })
    }
    pub async fn claim_reset(
        &self,
        org: &str,
        grant: &str,
        request_id: &str,
        token: &str,
    ) -> ResetOutcome {
        if !valid_id(org, 64, false) || !valid_id(grant, 40, true) || !valid_request_id(request_id)
        {
            return ResetOutcome::Unavailable {
                reason: Some("cerere invalidă".into()),
            };
        }
        let url = format!("https://api.anthropic.com/api/organizations/{org}/reset_rate_limits");
        let request = self.request(
            "POST",
            &url,
            Some(token),
            Some(json!({"program":"cedar_ember","grant_id":grant,"request_id":request_id})),
            25,
            true,
        );
        match self.transport.send(request).await {
            Ok(answer) => parse_reset_outcome(answer.status, &answer.body),
            Err(_) => ResetOutcome::Transport,
        }
    }
}

pub struct PendingLogin {
    pub id: String,
    pub url: String,
    pub created_at: i64,
    state: String,
    verifier: Zeroizing<String>,
}
impl fmt::Debug for PendingLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingLogin")
            .field("id", &self.id)
            .field("secrets", &"[REDACTED]")
            .finish()
    }
}
impl PendingLogin {
    pub fn new(now: i64) -> Self {
        let mut bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(bytes.as_mut());
        let verifier = Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref()));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = uuid::Uuid::new_v4().to_string();
        let mut url = reqwest::Url::parse(AUTHORIZE_URL).expect("constant authorization URL");
        url.query_pairs_mut().extend_pairs([
            ("code", "true"),
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT_URI),
            ("scope", LOGIN_SCOPES),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
        ]);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            url: url.into(),
            created_at: now,
            state,
            verifier,
        }
    }
}
pub struct TokenResponse {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_in: i64,
    pub scope: Option<String>,
}
impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("tokens", &"[REDACTED]")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}
impl TokenResponse {
    fn parse(value: &Value) -> Result<Self, ClientError> {
        let access = value["access_token"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or(ClientError::InvalidResponse)?;
        let expires_in = value["expires_in"].as_f64().unwrap_or(28800.0);
        if !expires_in.is_finite() || expires_in <= 0.0 || expires_in > 365.0 * 86400.0 {
            return Err(ClientError::InvalidResponse);
        }
        Ok(Self {
            access_token: Zeroizing::new(access.into()),
            refresh_token: value["refresh_token"]
                .as_str()
                .map(|v| Zeroizing::new(v.into())),
            expires_in: expires_in as i64,
            scope: value["scope"].as_str().map(str::to_owned),
        })
    }
    pub fn apply(&self, credentials: &mut Value, now: i64) {
        if !credentials.is_object() {
            *credentials = json!({});
        }
        if !credentials["claudeAiOauth"].is_object() {
            credentials["claudeAiOauth"] = json!({});
        }
        let oauth = &mut credentials["claudeAiOauth"];
        oauth["accessToken"] = json!(self.access_token.as_str());
        if let Some(refresh) = &self.refresh_token {
            oauth["refreshToken"] = json!(refresh.as_str());
        }
        oauth["expiresAt"] = json!(now.saturating_add(self.expires_in).saturating_mul(1000));
        if let Some(scope) = &self.scope {
            oauth["scopes"] = json!(scope.split_whitespace().collect::<Vec<_>>());
        }
    }
    pub fn apply_account(&self, account: &mut StoredAccount, now: i64) {
        self.apply(&mut account.credentials, now);
        account.saved_at = now;
        account.refresh_fail_at = None;
    }
}
pub struct Profile {
    pub identity: Value,
    pub subscription_status: Option<String>,
    pub subscription_started_at: Option<i64>,
    pub plan_tier: Option<String>,
}
impl Profile {
    fn parse(value: &Value) -> Result<Self, ClientError> {
        let account = &value["account"];
        let org = &value["organization"];
        let mut identity = json!({});
        for (target, aliases, source) in [
            ("accountUuid", vec!["uuid", "account_uuid"], account),
            ("emailAddress", vec!["email", "email_address"], account),
            ("fullName", vec!["full_name"], account),
            ("organizationUuid", vec!["uuid"], org),
            ("organizationName", vec!["name"], org),
            ("organizationRole", vec!["role"], org),
            ("organizationBillingType", vec!["billing_type"], org),
            ("organizationRateLimitTier", vec!["rate_limit_tier"], org),
        ] {
            if let Some(v) = aliases
                .iter()
                .find_map(|name| source.get(name).filter(|v| v.is_string()))
            {
                identity[target] = v.clone();
            }
        }
        if identity["accountUuid"].as_str().is_none() {
            return Err(ClientError::InvalidResponse);
        }
        Ok(Self {
            identity,
            subscription_status: org["subscription_status"].as_str().map(str::to_owned),
            subscription_started_at: switcher_core::parse_timestamp(
                &org["subscription_created_at"],
            ),
            plan_tier: org["rate_limit_tier"].as_str().map(str::to_owned),
        })
    }
    pub fn owner(&self) -> Option<&str> {
        self.identity["accountUuid"].as_str()
    }
    pub fn apply(&self, account: &mut StoredAccount, now: i64) {
        account.subscription_status = self.subscription_status.clone();
        account.subscription_started_at = self.subscription_started_at;
        account.plan_tier = self.plan_tier.clone();
        account.profile_checked_at = Some(now);
        account.identity_verified = true;
    }
}
pub struct LoginResult {
    pub credentials: Value,
    pub profile: Profile,
}
pub struct InferenceResult {
    pub usage: UsageResponse,
    pub complete: bool,
    pub accepted: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn check_status(response: &HttpResponse) -> Result<(), ClientError> {
    match response.status {
        200 => Ok(()),
        401 | 403 => Err(ClientError::Unauthorized),
        429 => Err(rate_limit(response)),
        status => Err(ClientError::Http(status)),
    }
}
fn rate_limit(response: &HttpResponse) -> ClientError {
    let retry_after = response.headers.get("retry-after").and_then(|v| {
        v.parse::<u64>().ok().or_else(|| {
            chrono::DateTime::parse_from_rfc2822(v)
                .ok()
                .map(|dt| (dt.timestamp() - chrono::Utc::now().timestamp()).max(0) as u64)
        })
    });
    ClientError::RateLimited { retry_after }
}
fn has_both_window_headers(headers: &BTreeMap<String, String>) -> bool {
    usage_from_headers(headers).is_some()
}
fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn valid_id(value: &str, max: usize, lowercase: bool) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|c| {
            c.is_ascii_alphanumeric() && (!lowercase || !c.is_ascii_uppercase())
                || c == b'-'
                || (lowercase && c == b'_')
        })
}
pub fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        && version.len() < 32
}
pub fn detect_cli_version() -> Option<String> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(appdata).join("npm/node_modules/@anthropic-ai/claude-code/package.json"),
        );
    }
    for root in [
        "/usr/local/lib/node_modules",
        "/usr/lib/node_modules",
        "/opt/homebrew/lib/node_modules",
    ] {
        candidates.push(PathBuf::from(root).join("@anthropic-ai/claude-code/package.json"));
    }
    if let Some(home) = &home {
        candidates
            .push(home.join(".claude/local/node_modules/@anthropic-ai/claude-code/package.json"));
    }
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && value["name"] == "@anthropic-ai/claude-code"
            && let Some(version) = value["version"].as_str().filter(|v| valid_version(v))
        {
            return Some(version.into());
        }
    }
    let mut binaries = vec![
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(home) = home {
        binaries.push(home.join(".local/bin/claude"));
        binaries.push(home.join(".local/bin/claude.exe"));
    }
    for binary in binaries {
        if let Ok(resolved) = binary.canonicalize() {
            if let Some(version) = resolved
                .file_name()
                .and_then(|v| v.to_str())
                .filter(|v| valid_version(v))
            {
                return Some(version.into());
            }
            #[cfg(windows)]
            if let Some(version) = native_binary_version(&resolved) {
                return Some(version);
            }
        }
    }
    None
}

#[cfg(windows)]
fn native_binary_version(path: &std::path::Path) -> Option<String> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Only public executable version resources are inspected; no executable is launched.
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), ptr::null_mut());
        if size == 0 || size > 1024 * 1024 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let root = [b'\\' as u16, 0];
        let mut pointer = ptr::null_mut();
        let mut length = 0;
        if VerQueryValueW(
            data.as_ptr().cast(),
            root.as_ptr(),
            &mut pointer,
            &mut length,
        ) == 0
            || pointer.is_null()
            || length < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32
        {
            return None;
        }
        let info = ptr::read_unaligned(pointer.cast::<VS_FIXEDFILEINFO>());
        if info.dwSignature != 0xfeef04bd {
            return None;
        }
        let version = format!(
            "{}.{}.{}",
            info.dwProductVersionMS >> 16,
            info.dwProductVersionMS & 0xffff,
            info.dwProductVersionLS >> 16
        );
        valid_version(&version).then_some(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    struct Fake {
        replies: Mutex<VecDeque<HttpResponse>>,
        requests: Mutex<Vec<HttpRequest>>,
    }
    #[async_trait]
    impl Transport for Fake {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ClientError> {
            self.requests.lock().unwrap().push(request);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(ClientError::Transport)
        }
    }
    fn fake(replies: Vec<HttpResponse>) -> (ClaudeClient, Arc<Fake>) {
        let fake = Arc::new(Fake {
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(Vec::new()),
        });
        (
            ClaudeClient::with_transport("2.1.287", fake.clone()).unwrap(),
            fake,
        )
    }
    fn reply(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: BTreeMap::new(),
            body,
        }
    }
    #[test]
    fn pkce_matches_rfc_vector_and_debug_redacts() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let login = PendingLogin::new(100);
        assert_eq!(login.verifier.len(), 43);
        assert!(login.url.contains("code_challenge_method=S256"));
        assert!(!format!("{login:?}").contains(login.verifier.as_str()));
    }
    #[tokio::test]
    async fn mismatched_or_expired_login_never_sends() {
        let (client, transport) = fake(vec![]);
        let login = PendingLogin::new(100);
        assert!(matches!(
            client.finish_login(&login, "fake#wrong", 110).await,
            Err(ClientError::InvalidLogin)
        ));
        assert!(matches!(
            client.finish_login(&login, "fake", 701).await,
            Err(ClientError::InvalidLogin)
        ));
        assert!(transport.requests.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn missing_state_is_rejected_and_successful_exchange_uses_matching_pkce() {
        let (client, transport) = fake(vec![
            reply(
                200,
                json!({"access_token":"fixture-new-access","refresh_token":"fixture-new-refresh","expires_in":3600}),
            ),
            reply(
                200,
                json!({"account":{"uuid":"fixture-owner","email":"demo@example.invalid"},"organization":{"uuid":"fixture-org","rate_limit_tier":"max"}}),
            ),
        ]);
        let login = PendingLogin::new(100);
        assert!(matches!(
            client.finish_login(&login, "code-without-state", 101).await,
            Err(ClientError::InvalidLogin)
        ));
        assert!(transport.requests.lock().unwrap().is_empty());
        let answer = client
            .finish_login(&login, &format!("fixture-code#{}", login.state), 102)
            .await
            .unwrap();
        assert_eq!(answer.profile.owner(), Some("fixture-owner"));
        assert_eq!(
            answer.credentials["claudeAiOauth"]["refreshToken"],
            "fixture-new-refresh"
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body.as_ref().unwrap()["state"], login.state);
        assert_eq!(
            requests[0].body.as_ref().unwrap()["code_verifier"],
            login.verifier.as_str()
        );
        assert_eq!(requests[1].url, PROFILE_URL);
    }
    #[test]
    fn localized_errors_are_fixed_and_english_is_default() {
        assert_eq!(
            ClientError::Http(503).message("en"),
            "Unexpected response from Anthropic (HTTP 503)."
        );
        assert!(ClientError::Transport.message("ro").contains("eșuat"));
        assert_eq!(
            ClientError::Unauthorized.message("unsupported"),
            ClientError::Unauthorized.to_string()
        );
        assert!(valid_request_id("fixture_key_1"));
        assert!(!valid_request_id("bad/key"));
    }
    #[tokio::test]
    async fn refresh_preserves_extensions_and_prior_refresh_token() {
        let (client, transport) = fake(vec![reply(
            200,
            json!({"access_token":"sentinel-new-access","expires_in":3600,"scope":"user:profile user:inference"}),
        )]);
        let tokens = client.refresh_token("sentinel-refresh").await.unwrap();
        let mut credentials = json!({"other":{"keep":true},"claudeAiOauth":{"refreshToken":"sentinel-refresh","subscriptionType":"max"}});
        tokens.apply(&mut credentials, 100);
        assert_eq!(
            credentials["claudeAiOauth"]["refreshToken"],
            "sentinel-refresh"
        );
        assert_eq!(credentials["claudeAiOauth"]["expiresAt"], 3700000);
        assert_eq!(credentials["other"]["keep"], true);
        assert_eq!(credentials["claudeAiOauth"]["subscriptionType"], "max");
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].url, TOKEN_URL);
        assert_eq!(requests[0].body.as_ref().unwrap()["scope"], REFRESH_SCOPES);
        assert!(!format!("{:?}", requests[0]).contains("sentinel-refresh"));
        assert!(!format!("{tokens:?}").contains("sentinel-new-access"));
    }
    #[tokio::test]
    async fn rejected_refresh_grants_are_distinct_from_temporary_failures() {
        let (client, _) = fake(vec![
            reply(
                400,
                json!({"error":"invalid_grant","error_description":"secret-response-sentinel"}),
            ),
            reply(401, Value::Null),
            reply(400, json!({"error":"invalid_request"})),
            reply(503, Value::Null),
            reply(429, Value::Null),
        ]);
        for expected in [
            ClientError::InvalidGrant,
            ClientError::InvalidGrant,
            ClientError::Http(400),
            ClientError::Http(503),
            ClientError::RateLimited { retry_after: None },
        ] {
            let error = client.refresh_token("sentinel-refresh").await.unwrap_err();
            assert_eq!(error, expected);
            assert!(!error.to_string().contains("sentinel"));
        }
    }
    #[tokio::test]
    async fn generic_429_is_not_budget_reading() {
        let (client, _) = fake(vec![reply(
            429,
            json!({"error":{"message":"secret-response-sentinel"}}),
        )]);
        let err = client
            .inference("sentinel-access", None)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ClientError::RateLimited { .. }));
        assert!(!err.to_string().contains("secret-response-sentinel"));
    }
    #[tokio::test]
    async fn budget_refusal_accepts_exhaustion_without_status_and_mixed_case_status() {
        let mut exhausted = reply(429, Value::Null);
        exhausted.headers = BTreeMap::from([
            (
                "anthropic-ratelimit-unified-5h-utilization".into(),
                "1".into(),
            ),
            (
                "anthropic-ratelimit-unified-7d-utilization".into(),
                "0.42".into(),
            ),
        ]);
        let mut mixed_case = reply(429, Value::Null);
        mixed_case.headers = BTreeMap::from([
            (
                "Anthropic-Ratelimit-Unified-Status".into(),
                "Rejected".into(),
            ),
            (
                "Anthropic-Ratelimit-Unified-Representative-Claim".into(),
                "Five_Hour".into(),
            ),
            (
                "Anthropic-Ratelimit-Unified-5h-Utilization".into(),
                "0.42".into(),
            ),
            (
                "Anthropic-Ratelimit-Unified-7d-Utilization".into(),
                "0.2".into(),
            ),
        ]);
        let (client, _) = fake(vec![exhausted, mixed_case]);
        for expected_weekly in [42.0, 20.0] {
            let result = client.inference("fixture-access", None).await.unwrap();
            assert!(!result.accepted);
            assert!(result.complete);
            assert_eq!(result.usage.five_hour.utilization, 100.0);
            assert_eq!(result.usage.seven_day.utilization, expected_weekly);
        }
    }
    #[tokio::test]
    async fn budget_refusal_is_reading_but_missing_window_not_complete() {
        let mut response = reply(429, Value::Null);
        response.headers = BTreeMap::from([
            (
                "anthropic-ratelimit-unified-status".into(),
                "rejected".into(),
            ),
            (
                "anthropic-ratelimit-unified-representative-claim".into(),
                "seven_day".into(),
            ),
        ]);
        let previous = UsageResponse {
            five_hour: switcher_core::UsageWindow {
                utilization: 42.0,
                resets_at: None,
            },
            ..UsageResponse::default()
        };
        let (client, transport) = fake(vec![response]);
        let read = client
            .inference("sentinel-access", Some(&previous))
            .await
            .unwrap();
        assert!(read.usage.seven_day.utilization >= 100.0);
        assert_eq!(read.usage.five_hour.utilization, 42.0);
        assert!(!read.complete);
        assert!(!read.accepted);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].headers["x-app"], "cli");
        assert_eq!(requests[0].body.as_ref().unwrap()["max_tokens"], 1);
    }
    #[tokio::test]
    async fn malformed_reset_block_does_not_discard_valid_usage() {
        let (client, _) = fake(vec![reply(
            200,
            json!({"five_hour":{"utilization":29,"resets_at":"2026-10-03T00:00:00Z"},"seven_day":{"utilization":57,"resets_at":null},"cedar_ember":"invalid"}),
        )]);
        let usage = client.metadata("sentinel-access").await.unwrap();
        assert_eq!(usage.five_hour.utilization, 29.0);
        assert_eq!(usage.seven_day.utilization, 57.0);
        assert!(usage.cedar_ember.is_none());
    }
    #[tokio::test]
    async fn claim_reuses_request_key_and_rejects_path_injection() {
        let (client, transport) = fake(vec![reply(200, json!({"result":"already_used"}))]);
        assert!(matches!(
            client
                .claim_reset("org-a", "grant_a", "same-key-1", "sentinel-access")
                .await,
            ResetOutcome::AlreadyUsed
        ));
        assert!(matches!(
            client
                .claim_reset("../org-a", "grant_a", "same-key-1", "sentinel-access")
                .await,
            ResetOutcome::Unavailable { .. }
        ));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].body.as_ref().unwrap()["request_id"],
            "same-key-1"
        );
    }
    #[test]
    fn version_and_auth_identifier_validation() {
        assert!(valid_version("2.1.287"));
        assert!(!valid_version("2.1.287\r\nInjected:true"));
        assert!(!valid_id("UPPER_GRANT", 40, true));
    }
}
