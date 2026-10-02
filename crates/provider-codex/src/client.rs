use crate::wire::{MAX_FRAME, SecretJson};
use crate::{AccountObservation, CodexError, CodexRateLimits, VerifiedCodexExecutable};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    ffi::OsString,
    fmt,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone)]
pub struct CodexContext {
    home: PathBuf,
    cwd: PathBuf,
    isolated: bool,
    env: Vec<(OsString, OsString)>,
}
impl fmt::Debug for CodexContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodexContext([REDACTED])")
    }
}
impl CodexContext {
    pub fn isolated(home: PathBuf, cwd: PathBuf) -> Result<Self, CodexError> {
        Self::new(home, cwd, true)
    }
    /// Caller must first qualify this FILE context and policy through the platform adapter.
    pub fn approved_file(home: PathBuf, cwd: PathBuf) -> Result<Self, CodexError> {
        Self::new(home, cwd, false)
    }
    fn new(home: PathBuf, cwd: PathBuf, isolated: bool) -> Result<Self, CodexError> {
        let home = std::fs::canonicalize(home).map_err(|_| CodexError::UnsafeContext)?;
        let cwd = std::fs::canonicalize(cwd).map_err(|_| CodexError::UnsafeContext)?;
        if !home.is_dir() || !cwd.is_dir() {
            return Err(CodexError::UnsafeContext);
        }
        if isolated
            && (home.join("auth.json").exists() || home.join("secrets/codex_auth.age").exists())
        {
            return Err(CodexError::UnsafeContext);
        }
        if isolated && !cwd.starts_with(&home) {
            return Err(CodexError::UnsafeContext);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Native Codex may create a traversable home (0755). Approved FILE
            // contexts rely on the platform's owner/path/private-auth receipt;
            // shared write access still invalidates that receipt's boundary.
            // Isolated homes and every owned cwd remain strictly private.
            for (path, forbidden) in [(&home, if isolated { 0o077 } else { 0o022 }), (&cwd, 0o077)]
            {
                if std::fs::metadata(path)
                    .map_err(|_| CodexError::UnsafeContext)?
                    .permissions()
                    .mode()
                    & forbidden
                    != 0
                {
                    return Err(CodexError::UnsafeContext);
                }
            }
        }
        let mut env = Vec::new();
        for name in [
            "PATH",
            "SystemRoot",
            "WINDIR",
            "SystemDrive",
            "TEMP",
            "TMP",
            "LANG",
            "LC_ALL",
        ] {
            if let Some(value) = std::env::var_os(name) {
                env.push((name.into(), value));
            }
        }
        env.push(("CODEX_HOME".into(), home.as_os_str().to_owned()));
        if isolated {
            for name in [
                "HOME",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_CACHE_HOME",
            ] {
                env.push((name.into(), home.as_os_str().to_owned()));
            }
        } else {
            for name in [
                "HOME",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_CACHE_HOME",
                "XDG_RUNTIME_DIR",
                "DBUS_SESSION_BUS_ADDRESS",
            ] {
                if let Some(value) = std::env::var_os(name) {
                    env.push((name.into(), value));
                }
            }
        }
        Ok(Self {
            home,
            cwd,
            isolated,
            env,
        })
    }
    pub fn home(&self) -> &std::path::Path {
        &self.home
    }
    pub(crate) fn is_isolated(&self) -> bool {
        self.isolated
    }
    pub(crate) fn configure(&self, command: &mut Command) {
        command
            .env_clear()
            .envs(self.env.iter().map(|(a, b)| (a, b)))
            .current_dir(&self.cwd);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
    }
}
pub struct BrowserLoginChallenge {
    url: Zeroizing<String>,
}
impl BrowserLoginChallenge {
    pub fn from_authorization_url(url: String) -> Result<Self, CodexError> {
        if url.len() > 16384
            || !url.starts_with("https://auth.openai.com/oauth/authorize?")
            || url.chars().any(char::is_control)
        {
            return Err(CodexError::Protocol);
        }
        Ok(Self {
            url: Zeroizing::new(url),
        })
    }

    pub fn authorization_url(&self) -> &str {
        &self.url
    }
}
impl fmt::Debug for BrowserLoginChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrowserLoginChallenge([REDACTED])")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPoll {
    Pending,
    Completed,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    Cancelled,
    NotFound,
    AlreadyFinished,
}
struct PendingLogin {
    id: String,
    deadline: Instant,
    state: LoginPoll,
}
#[async_trait]
trait Transport: Send {
    async fn send(&mut self, bytes: &[u8]) -> Result<(), CodexError>;
    async fn recv(&mut self) -> Result<SecretJson, CodexError>;
    fn try_recv(&mut self) -> Result<Option<SecretJson>, CodexError>;
    fn abort(&mut self);
    async fn shutdown(&mut self) -> Result<(), CodexError>;
}
async fn read_frames<R: tokio::io::AsyncRead + Unpin>(
    stdout: &mut R,
    tx: mpsc::Sender<SecretJson>,
) -> Result<(), CodexError> {
    let mut frame = Zeroizing::new(Vec::new());
    let mut chunk = Zeroizing::new([0; 4096]);
    loop {
        let n = stdout
            .read(&mut *chunk)
            .await
            .map_err(|_| CodexError::ChildExited)?;
        if n == 0 {
            return Err(if frame.is_empty() {
                CodexError::ChildExited
            } else {
                CodexError::Protocol
            });
        }
        for byte in &chunk[..n] {
            if *byte == b'\n' {
                if frame.last() == Some(&b'\r') {
                    frame.pop();
                }
                let parsed = SecretJson::parse(&frame)?;
                frame.zeroize();
                tx.try_send(parsed).map_err(|_| CodexError::OutputLimit)?;
            } else {
                if frame.len() >= MAX_FRAME {
                    return Err(CodexError::OutputLimit);
                }
                frame.push(*byte);
            }
        }
    }
}
struct StdioTransport {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<SecretJson>,
    reader: JoinHandle<()>,
    failure: Arc<Mutex<Option<CodexError>>>,
}
impl StdioTransport {
    async fn spawn(
        executable: &VerifiedCodexExecutable,
        context: &CodexContext,
    ) -> Result<Self, CodexError> {
        let mut command = Command::new(executable.recheck()?);
        context.configure(&mut command);
        command
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| CodexError::SpawnFailed)?;
        let stdin = child.stdin.take().ok_or(CodexError::SpawnFailed)?;
        let mut stdout = child.stdout.take().ok_or(CodexError::SpawnFailed)?;
        let (tx, rx) = mpsc::channel(64);
        let failure = Arc::new(Mutex::new(None));
        let output_failure = Arc::clone(&failure);
        let reader = tokio::spawn(async move {
            let result = read_frames(&mut stdout, tx).await;
            if let Ok(mut error) = output_failure.lock() {
                *error = result.err();
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            rx,
            reader,
            failure,
        })
    }
    fn failure(&self) -> Result<(), CodexError> {
        if let Some(error) = *self.failure.lock().map_err(|_| CodexError::ChildExited)? {
            Err(error)
        } else {
            Ok(())
        }
    }
}
impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.reader.abort();
        let _ = self.child.start_kill();
    }
}
#[async_trait]
impl Transport for StdioTransport {
    async fn send(&mut self, bytes: &[u8]) -> Result<(), CodexError> {
        self.failure()?;
        self.stdin
            .as_mut()
            .ok_or(CodexError::ChildExited)?
            .write_all(bytes)
            .await
            .map_err(|_| CodexError::ChildExited)
    }
    async fn recv(&mut self) -> Result<SecretJson, CodexError> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| self.failure().err().unwrap_or(CodexError::ChildExited))
    }
    fn try_recv(&mut self) -> Result<Option<SecretJson>, CodexError> {
        match self.rx.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(mpsc::error::TryRecvError::Empty) => {
                self.failure()?;
                Ok(None)
            }
            Err(_) => Err(self.failure().err().unwrap_or(CodexError::ChildExited)),
        }
    }
    fn abort(&mut self) {
        self.stdin.take();
        self.reader.abort();
        let _ = self.child.start_kill();
    }
    async fn shutdown(&mut self) -> Result<(), CodexError> {
        self.stdin.take();
        self.reader.abort();
        match timeout(Duration::from_secs(2), self.child.wait()).await {
            Ok(result) => {
                result.map_err(|_| CodexError::ChildExited)?;
            }
            Err(_) => {
                let _ = self.child.start_kill();
                timeout(Duration::from_secs(5), self.child.wait())
                    .await
                    .map_err(|_| CodexError::Timeout)?
                    .map_err(|_| CodexError::ChildExited)?;
            }
        }
        Ok(())
    }
}
/// Private Rust inspection data; never serialize this object over IPC.
pub struct ConfigurationInspection {
    effective: SecretJson,
    requirements: SecretJson,
}
impl ConfigurationInspection {
    /// Construct from bounded fixed-method fixture responses; keeps raw data in Rust.
    pub fn from_response_bytes(effective: &[u8], requirements: &[u8]) -> Result<Self, CodexError> {
        let effective = SecretJson::parse(effective)?;
        let requirements = SecretJson::parse(requirements)?;
        if !effective.0["config"].is_object()
            || !effective.0["origins"].is_object()
            || !effective.0["layers"].is_array()
            || !requirements
                .0
                .get("requirements")
                .is_some_and(|v| v.is_null() || v.is_object())
        {
            return Err(CodexError::Protocol);
        }
        Ok(Self {
            effective,
            requirements,
        })
    }

    pub fn effective_config(&self) -> &Value {
        &self.effective.0
    }
    pub fn requirements(&self) -> &Value {
        &self.requirements.0
    }
}
impl fmt::Debug for ConfigurationInspection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfigurationInspection([REDACTED])")
    }
}

pub struct CodexClient {
    transport: Box<dyn Transport>,
    id: i64,
    pending: Option<PendingLogin>,
    early: VecDeque<SecretJson>,
    isolated: bool,
    dead: bool,
}
impl fmt::Debug for CodexClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodexClient([REDACTED])")
    }
}
impl CodexClient {
    pub async fn start_login(
        executable: &VerifiedCodexExecutable,
        context: CodexContext,
    ) -> Result<Self, CodexError> {
        if !context.isolated {
            return Err(CodexError::UnsafeContext);
        }
        Self::start(executable, context).await
    }
    pub async fn start_active(
        executable: &VerifiedCodexExecutable,
        context: CodexContext,
    ) -> Result<Self, CodexError> {
        if context.isolated {
            return Err(CodexError::UnsafeContext);
        }
        Self::start(executable, context).await
    }
    async fn start(
        executable: &VerifiedCodexExecutable,
        context: CodexContext,
    ) -> Result<Self, CodexError> {
        if context.isolated
            && (context.home.join("auth.json").exists()
                || context.home.join("secrets/codex_auth.age").exists())
        {
            return Err(CodexError::UnsafeContext);
        }
        let transport = Box::new(StdioTransport::spawn(executable, &context).await?);
        let mut client = Self {
            transport,
            id: 0,
            pending: None,
            early: VecDeque::new(),
            isolated: context.isolated,
            dead: false,
        };
        if let Err(error) = client.initialize(&context).await {
            client.dead = true;
            let _ = client.transport.shutdown().await;
            return Err(error);
        }
        Ok(client)
    }
    async fn initialize(&mut self, context: &CodexContext) -> Result<(), CodexError> {
        let response=self.request("initialize",json!({"clientInfo":{"name":"primerswitch","title":"PrimerSwitch","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false,"requestAttestation":false}}),10).await?;
        let home = response.0["codexHome"]
            .as_str()
            .ok_or(CodexError::Protocol)?;
        if std::path::Path::new(home) != context.home {
            return Err(CodexError::UnsafeContext);
        }
        let family = if cfg!(windows) { "windows" } else { "unix" };
        if response.0["platformFamily"] != family
            || response.0["platformOs"] != std::env::consts::OS
        {
            return Err(CodexError::Protocol);
        }
        self.notify("initialized", None).await?;
        Ok(())
    }
    async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), CodexError> {
        let mut frame = Zeroizing::new(
            serde_json::to_vec(&json!({"method":method,"params":params}))
                .map_err(|_| CodexError::Protocol)?,
        );
        frame.push(b'\n');
        timeout(Duration::from_secs(5), self.transport.send(&frame))
            .await
            .map_err(|_| CodexError::Timeout)?
    }
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        seconds: u64,
    ) -> Result<SecretJson, CodexError> {
        if self.dead {
            return Err(CodexError::ChildExited);
        }
        self.id = self.id.checked_add(1).ok_or(CodexError::Protocol)?;
        let id = self.id;
        let mut frame = Zeroizing::new(
            serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))
                .map_err(|_| CodexError::Protocol)?,
        );
        frame.push(b'\n');
        let result = timeout(Duration::from_secs(seconds), async {
            self.transport.send(&frame).await?;
            let mut notifications = 0;
            loop {
                let mut incoming = self.transport.recv().await?;
                if incoming.0.get("method").is_some() {
                    notifications += 1;
                    if notifications > 64 {
                        return Err(CodexError::OutputLimit);
                    }
                    self.notification(incoming)?;
                    continue;
                }
                if incoming.0["id"].as_i64() != Some(id)
                    || incoming.0.get("result").is_some() == incoming.0.get("error").is_some()
                {
                    return Err(CodexError::Protocol);
                }
                if let Some(error) = incoming.0.get("error") {
                    if error["code"].as_i64().is_none() || !error["message"].is_string() {
                        return Err(CodexError::Protocol);
                    }
                    return Err(CodexError::ServiceUnavailable);
                }
                return Ok(SecretJson(incoming.0["result"].take()));
            }
        })
        .await
        .unwrap_or(Err(CodexError::Timeout));
        if result.is_err() {
            self.dead = true;
            let _ = self.transport.shutdown().await;
        }
        result
    }
    fn notification(&mut self, frame: SecretJson) -> Result<(), CodexError> {
        if !frame.0["method"].is_string()
            || frame.0.get("id").is_some()
            || frame.0.get("result").is_some()
            || frame.0.get("error").is_some()
        {
            return Err(CodexError::Protocol);
        }
        if frame.0["method"] == "account/login/completed" {
            if let Some(pending) = &mut self.pending {
                if frame.0["params"]["loginId"].as_str() == Some(&pending.id) {
                    pending.state = match frame.0["params"]["success"].as_bool() {
                        Some(true) => LoginPoll::Completed,
                        Some(false) => LoginPoll::Failed,
                        None => return Err(CodexError::Protocol),
                    };
                }
            } else {
                if self.early.len() >= 64 {
                    return Err(CodexError::OutputLimit);
                }
                self.early.push_back(frame);
            }
        }
        Ok(())
    }
    /// Observe policy for this already approved context. Never a receipt for another home.
    pub async fn inspect_configuration(&mut self) -> Result<ConfigurationInspection, CodexError> {
        let effective = self
            .request("config/read", json!({"includeLayers":true}), 30)
            .await?;
        let requirements = self
            .request("configRequirements/read", Value::Null, 30)
            .await?;
        if !effective.0["config"].is_object()
            || !effective.0["origins"].is_object()
            || !effective.0["layers"].is_array()
            || !requirements
                .0
                .get("requirements")
                .is_some_and(|v| v.is_null() || v.is_object())
        {
            self.dead = true;
            let _ = self.transport.shutdown().await;
            return Err(CodexError::Protocol);
        }
        Ok(ConfigurationInspection {
            effective,
            requirements,
        })
    }
    pub async fn begin_browser_login(&mut self) -> Result<BrowserLoginChallenge, CodexError> {
        let result = self.begin_browser_login_inner().await;
        if result.is_err() {
            self.dead = true;
            let _ = self.transport.shutdown().await;
        }
        result
    }
    async fn begin_browser_login_inner(&mut self) -> Result<BrowserLoginChallenge, CodexError> {
        if !self.isolated || self.pending.is_some() {
            return Err(CodexError::UnsafeContext);
        }
        let response = self
            .request("account/login/start", json!({"type":"chatgpt"}), 30)
            .await?;
        if response.0["type"] != "chatgpt" {
            return Err(CodexError::Protocol);
        }
        let id = response.0["loginId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
            .ok_or(CodexError::Protocol)?
            .to_owned();
        let url = response.0["authUrl"]
            .as_str()
            .filter(|s| {
                s.len() <= 16384
                    && s.starts_with("https://auth.openai.com/oauth/authorize?")
                    && !s.chars().any(char::is_control)
            })
            .ok_or(CodexError::Protocol)?;
        let challenge = BrowserLoginChallenge {
            url: Zeroizing::new(url.to_owned()),
        };
        self.pending = Some(PendingLogin {
            id,
            deadline: Instant::now() + Duration::from_secs(600),
            state: LoginPoll::Pending,
        });
        while let Some(frame) = self.early.pop_front() {
            self.notification(frame)?;
        }
        Ok(challenge)
    }
    pub fn poll_login(&mut self) -> Result<LoginPoll, CodexError> {
        if self.dead {
            return Err(CodexError::ChildExited);
        }
        for _ in 0..64 {
            match self.transport.try_recv() {
                Ok(Some(frame)) => {
                    if let Err(error) = self.notification(frame) {
                        self.dead = true;
                        self.transport.abort();
                        return Err(error);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    self.dead = true;
                    self.transport.abort();
                    return Err(error);
                }
            }
        }
        let pending = self.pending.as_mut().ok_or(CodexError::LoginFailed)?;
        if pending.state == LoginPoll::Pending && Instant::now() >= pending.deadline {
            pending.state = LoginPoll::Failed;
            self.dead = true;
            self.transport.abort();
            return Err(CodexError::Timeout);
        }
        Ok(pending.state)
    }
    pub async fn cancel_login(&mut self) -> Result<CancelOutcome, CodexError> {
        let Some(pending) = self.pending.take() else {
            return Ok(CancelOutcome::AlreadyFinished);
        };
        if pending.state != LoginPoll::Pending {
            return Ok(CancelOutcome::AlreadyFinished);
        }
        let response = self
            .request("account/login/cancel", json!({"loginId":pending.id}), 5)
            .await?;
        match response.0["status"].as_str() {
            Some("canceled") => Ok(CancelOutcome::Cancelled),
            Some("notFound") => Ok(CancelOutcome::NotFound),
            _ => Err(CodexError::Protocol),
        }
    }
    pub async fn read_account(&mut self) -> Result<AccountObservation, CodexError> {
        let mut response = self
            .request("account/read", json!({"refreshToken":false}), 30)
            .await?;
        let parsed = serde_json::from_value(response.0.take()).map_err(|_| CodexError::Protocol);
        if parsed.is_err() {
            self.dead = true;
            let _ = self.transport.shutdown().await;
        }
        parsed
    }
    pub async fn read_rate_limits(&mut self) -> Result<CodexRateLimits, CodexError> {
        if self.isolated {
            return Err(CodexError::UnsafeContext);
        }
        let mut response = self
            .request(
                "account/rateLimits/read",
                json!({"supportsLunaReserve":false,"excludeResetCreditDetails":true}),
                30,
            )
            .await?;
        let parsed = serde_json::from_value(response.0.take()).map_err(|_| CodexError::Protocol);
        if parsed.is_err() {
            self.dead = true;
            let _ = self.transport.shutdown().await;
        }
        parsed
    }
    pub async fn shutdown(mut self) -> Result<(), CodexError> {
        self.dead = true;
        self.transport.shutdown().await
    }
}

/// Typed seam for the runtime owner and fake lifecycle fixtures. No raw RPC/token methods.
#[async_trait]
pub trait CodexService: Send {
    async fn inspect_configuration(&mut self) -> Result<ConfigurationInspection, CodexError>;
    async fn begin_browser_login(&mut self) -> Result<BrowserLoginChallenge, CodexError>;
    fn poll_login(&mut self) -> Result<LoginPoll, CodexError>;
    async fn cancel_login(&mut self) -> Result<CancelOutcome, CodexError>;
    async fn read_account(&mut self) -> Result<AccountObservation, CodexError>;
    async fn read_rate_limits(&mut self) -> Result<CodexRateLimits, CodexError>;
    async fn shutdown(&mut self) -> Result<(), CodexError>;
}
#[async_trait]
impl CodexService for CodexClient {
    async fn inspect_configuration(&mut self) -> Result<ConfigurationInspection, CodexError> {
        CodexClient::inspect_configuration(self).await
    }
    async fn begin_browser_login(&mut self) -> Result<BrowserLoginChallenge, CodexError> {
        CodexClient::begin_browser_login(self).await
    }
    fn poll_login(&mut self) -> Result<LoginPoll, CodexError> {
        CodexClient::poll_login(self)
    }
    async fn cancel_login(&mut self) -> Result<CancelOutcome, CodexError> {
        CodexClient::cancel_login(self).await
    }
    async fn read_account(&mut self) -> Result<AccountObservation, CodexError> {
        CodexClient::read_account(self).await
    }
    async fn read_rate_limits(&mut self) -> Result<CodexRateLimits, CodexError> {
        CodexClient::read_rate_limits(self).await
    }
    async fn shutdown(&mut self) -> Result<(), CodexError> {
        self.dead = true;
        self.transport.shutdown().await
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
