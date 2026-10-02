use async_trait::async_trait;
use provider_claude::{
    ClaudeClient, ClientError, HttpRequest, HttpResponse, INFERENCE_URL, PROFILE_URL, Transport,
    USAGE_URL,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use switcher_platform::{ActiveStore, CliPaths, PlatformError, Vault};
use switcher_runtime::{Clock, RuntimeHandle};

const SENTINEL: &str = "DIAGNOSTIC_FIXTURE_TOKEN_SENTINEL";
const NOW: i64 = 1_800_000_000;
type DiagnosticResult<T> = Result<T, &'static str>;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> i64 {
        NOW
    }
}

#[derive(Default)]
struct FakeTransport {
    requests: AtomicUsize,
}
#[async_trait]
impl Transport for FakeTransport {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ClientError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        // No request is forwarded. Only known fixture routes have responses.
        match request.url.as_str() {
            PROFILE_URL => Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"account":{"uuid":"fixture-new-owner","email":"fixture@example.invalid"},"organization":{"uuid":"fixture-org","subscription_status":"active"}}),
            }),
            USAGE_URL => Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"five_hour":{"utilization":29,"resets_at":NOW+3600},"seven_day":{"utilization":57,"resets_at":NOW+86400},"unknownExtension":{"retained":true}}),
            }),
            INFERENCE_URL => Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::from([
                    (
                        "anthropic-ratelimit-unified-5h-utilization".into(),
                        "0.29".into(),
                    ),
                    (
                        "anthropic-ratelimit-unified-7d-utilization".into(),
                        "0.57".into(),
                    ),
                    (
                        "anthropic-ratelimit-unified-5h-reset".into(),
                        (NOW + 3600).to_string(),
                    ),
                    (
                        "anthropic-ratelimit-unified-7d-reset".into(),
                        (NOW + 86400).to_string(),
                    ),
                ]),
                body: json!({"usage":{"input_tokens":1,"output_tokens":1}}),
            }),
            _ => Err(ClientError::Transport),
        }
    }
}

fn require(condition: bool, error: &'static str) -> DiagnosticResult<()> {
    if condition { Ok(()) } else { Err(error) }
}

pub(super) async fn run() -> DiagnosticResult<Value> {
    let parent = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| "Could not resolve the disposable fixture directory")?;
    let root = tempfile::Builder::new()
        .prefix("primerswitch-diagnostic-")
        .tempdir_in(parent)
        .map_err(|_| "Could not create the disposable fixture directory")?;
    let paths = CliPaths::for_home(root.path().join("fixture home ș"));
    fs::create_dir_all(&paths.config_dir).map_err(|_| "Could not prepare fixture files")?;
    let auth = json!({"claudeAiOauth":{"accessToken":"fixture-old-token","refreshToken":"fixture-old-refresh","expiresAt":(NOW+3600)*1000},"mcp":{"unknownAuth":true}});
    let config = json!({"oauthAccount":{"accountUuid":"fixture-old-owner","organizationUuid":"fixture-org"},"preferences":{"theme":"dark"},"unknownConfig":[1,2,3]});
    fs::write(&paths.credentials_file, auth.to_string())
        .map_err(|_| "Could not prepare fixture files")?;
    fs::write(&paths.global_config_file, config.to_string())
        .map_err(|_| "Could not prepare fixture files")?;
    let directory = root.path().join("fixture vault");
    let vault = Vault::with_key(directory.clone(), [0x61; 32])
        .map_err(|_| "Fixture encryption initialization failed")?;
    let payload = json!({"secret":SENTINEL,"extension":{"future":true}});
    vault
        .save("diagnostic-roundtrip", &payload)
        .map_err(|_| "Fixture encryption save failed")?;
    let encrypted_path = directory.join("diagnostic-roundtrip.vault");
    let first = fs::read(&encrypted_path).map_err(|_| "Fixture ciphertext read failed")?;
    require(
        !first
            .windows(SENTINEL.len())
            .any(|bytes| bytes == SENTINEL.as_bytes()),
        "Fixture plaintext appeared in ciphertext",
    )?;
    require(
        vault
            .load::<Value>("diagnostic-roundtrip")
            .map_err(|_| "Fixture decrypt failed")?
            == Some(payload.clone()),
        "Fixture encryption round trip failed",
    )?;
    vault
        .save("diagnostic-roundtrip", &payload)
        .map_err(|_| "Fixture encryption save failed")?;
    let second = fs::read(&encrypted_path).map_err(|_| "Fixture ciphertext read failed")?;
    require(
        first.get(8..32) != second.get(8..32),
        "Fixture encryption reused a nonce",
    )?;
    fs::copy(&encrypted_path, directory.join("diagnostic-aad.vault"))
        .map_err(|_| "Fixture AAD preparation failed")?;
    require(
        matches!(
            vault.load::<Value>("diagnostic-aad"),
            Err(PlatformError::Authentication)
        ),
        "Fixture AAD substitution was accepted",
    )?;
    let wrong_key = Vault::with_key(directory.clone(), [0x62; 32])
        .map_err(|_| "Fixture wrong-key preparation failed")?;
    require(
        matches!(
            wrong_key.load::<Value>("diagnostic-roundtrip"),
            Err(PlatformError::Authentication)
        ),
        "Fixture wrong key was accepted",
    )?;
    let mut damaged = second;
    let last = damaged.last_mut().ok_or("Fixture ciphertext is empty")?;
    *last ^= 1;
    fs::write(&encrypted_path, &damaged).map_err(|_| "Fixture tamper preparation failed")?;
    require(
        matches!(
            vault.load::<Value>("diagnostic-roundtrip"),
            Err(PlatformError::Authentication)
        ),
        "Fixture tamper was accepted",
    )?;
    require(
        vault.save("diagnostic-roundtrip", &payload).is_err(),
        "Fixture tamper was overwritten",
    )?;
    require(
        fs::read(&encrypted_path).map_err(|_| "Fixture ciphertext read failed")? == damaged,
        "Fixture damaged evidence changed",
    )?;

    let active = ActiveStore::file(paths.clone());
    let before = active
        .read()
        .map_err(|_| "Fixture active-store read failed")?;
    let target = json!({"claudeAiOauth":{"accessToken":SENTINEL,"refreshToken":"FIXTURE_REFRESH_SENTINEL","expiresAt":(NOW+3600)*1000,"futureAuthExtension":42},"mcp":"do not copy this"});
    let identity = json!({"accountUuid":"fixture-new-owner","organizationUuid":"fixture-org","emailAddress":"fixture@example.invalid","futureIdentityExtension":true});
    active
        .switch_checked(&target, &identity, &before.fingerprint, &vault)
        .map_err(|_| "Fixture manual switch failed")?;
    let after = active
        .read()
        .map_err(|_| "Fixture active-store readback failed")?;
    require(
        after.credentials["claudeAiOauth"] == target["claudeAiOauth"] && after.identity == identity,
        "Fixture auth/identity pair did not match",
    )?;
    require(
        after.credentials["mcp"] == auth["mcp"],
        "Fixture unrelated auth fields changed",
    )?;
    let after_config: Value = serde_json::from_slice(
        &fs::read(&paths.global_config_file).map_err(|_| "Fixture config read failed")?,
    )
    .map_err(|_| "Fixture config parse failed")?;
    require(
        after_config["preferences"] == config["preferences"]
            && after_config["unknownConfig"] == config["unknownConfig"],
        "Fixture unrelated configuration changed",
    )?;
    require(
        matches!(
            active.switch_checked(&target, &identity, &before.fingerprint, &vault),
            Err(PlatformError::Conflict)
        ),
        "Fixture stale fingerprint was accepted",
    )?;
    active
        .recover(&vault)
        .map_err(|_| "Fixture clean restart recovery failed")?;
    let valid_config =
        fs::read(&paths.global_config_file).map_err(|_| "Fixture config read failed")?;
    fs::write(&paths.global_config_file, b"{corrupt fixture JSON")
        .map_err(|_| "Fixture corrupt JSON preparation failed")?;
    require(
        matches!(
            active.switch(&target, &identity, &vault),
            Err(PlatformError::InvalidJson)
        ),
        "Fixture corrupt JSON was accepted",
    )?;
    require(
        fs::read(&paths.global_config_file).map_err(|_| "Fixture corrupt JSON read failed")?
            == b"{corrupt fixture JSON",
        "Fixture corrupt original was overwritten",
    )?;
    fs::write(&paths.global_config_file, valid_config)
        .map_err(|_| "Fixture config restoration failed")?;

    let transport = Arc::new(FakeTransport::default());
    let client = ClaudeClient::with_transport("2.1.287", transport.clone())
        .map_err(|_| "Fixture fake client initialization failed")?;
    let inference = client
        .inference(SENTINEL, None)
        .await
        .map_err(|_| "Fixture inference adapter failed")?;
    require(
        inference.complete
            && inference.usage.five_hour.utilization == 29.0
            && inference.usage.seven_day.utilization == 57.0,
        "Fixture header precision changed",
    )?;
    let metadata = client
        .metadata(SENTINEL)
        .await
        .map_err(|_| "Fixture metadata adapter failed")?;
    require(
        metadata.five_hour.utilization == 29.0 && metadata.seven_day.utilization == 57.0,
        "Fixture metadata parsing failed",
    )?;
    let initial_requests = transport.requests.load(Ordering::Relaxed);
    let runtime = RuntimeHandle::from_parts(vault, active, client, Arc::new(FixedClock))
        .map_err(|_| "Fixture runtime initialization failed")?;
    require(
        transport.requests.load(Ordering::Relaxed) == initial_requests,
        "Fixture runtime startup sent a provider request",
    )?;
    let snapshot = runtime
        .import_current()
        .await
        .map_err(|_| "Fixture runtime import failed")?;
    require(
        snapshot.accounts.len() == 1 && snapshot.accounts[0].identity_verified,
        "Fixture runtime did not verify imported identity",
    )?;
    let redacted = serde_json::to_value(&snapshot)
        .map_err(|_| "Fixture redacted snapshot serialization failed")?;
    let serialized = redacted.to_string();
    require(
        !serialized.contains(SENTINEL)
            && !serialized.contains("FIXTURE_REFRESH_SENTINEL")
            && !has_secret_keys(&redacted),
        "Fixture credential data crossed the snapshot boundary",
    )?;
    // All public output fields are fixed labels, booleans and counts. No paths,
    // account identifiers, raw responses or credential-bearing values are returned.
    Ok(
        json!({"mode":"fixtures","ok":true,"networkRequests":0,"fakeTransportRequests":transport.requests.load(Ordering::Relaxed),"checks":[
            "encryptedRoundTrip","freshNonce","aadBinding","wrongKeyRejected","tamperPreserved",
            "manualSwitch","unknownJsonPreserved","staleFingerprintRejected","corruptJsonPreserved",
            "fakeProviderParsing","runtimeImportVerified","snapshotRedacted"
        ]}),
    )
}

fn has_secret_keys(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "credentials" | "accessToken" | "refreshToken" | "claudeAiOauth" | "oauthAccount"
            ) || has_secret_keys(value)
        }),
        Value::Array(array) => array.iter().any(has_secret_keys),
        _ => false,
    }
}
