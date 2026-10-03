use crate::{CliPaths, PlatformError, Result, Vault, files};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt;

pub struct ActiveSnapshot {
    pub credentials: Value,
    pub identity: Value,
    /// Auth-relevant state only: the CLI OAuth blob plus the owner named by
    /// `oauthAccount`. Every other key in either file belongs to the CLI (startup
    /// counters, tips, project statistics, MCP tokens) and never changes it.
    pub fingerprint: String,
    /// Only the owner (account and organization) of the active login. A token
    /// rotation by the CLI keeps it; a login to another account or a logout changes it.
    pub identity_fingerprint: String,
}
impl fmt::Debug for ActiveSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveSnapshot")
            .field("credentials", &"[REDACTED]")
            .field("identity", &"[REDACTED]")
            .field("fingerprint", &self.fingerprint)
            .field("identity_fingerprint", &self.identity_fingerprint)
            .finish()
    }
}

/// What recovery found for a previously journaled switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No interrupted switch was pending.
    Clean,
    /// The interrupted switch had fully applied; only its journal was removed.
    Completed,
    /// The original login was still in place or was restored; the journal was removed.
    RolledBack,
    /// The current login matches neither side of the interrupted switch, so another
    /// writer owns it now. Nothing was written and the journal was retained under a
    /// set-aside name for diagnostics.
    SetAside,
}

pub struct ActiveStore {
    pub paths: CliPaths,
    native: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Backend {
    File,
    #[cfg(target_os = "macos")]
    Keychain,
}

struct RawSnapshot {
    auth: Option<Vec<u8>>,
    config: Option<Vec<u8>>,
    backend: Backend,
}

#[derive(Serialize, Deserialize)]
struct Component {
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Stage {
    Prepared,
    AuthWritten,
    IdentityWritten,
    Committed,
}

#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    context: String,
    backend: Backend,
    auth: Component,
    config: Component,
    stage: Stage,
}

impl Journal {
    /// The original and the desired login, derived from the recorded documents.
    fn logins(&self) -> Result<(Login, Login)> {
        Ok((
            Login::from_documents(self.auth.before.as_deref(), self.config.before.as_deref())?,
            Login::from_documents(Some(&self.auth.after), Some(&self.config.after))?,
        ))
    }
    fn original_identity(&self) -> Result<Option<Value>> {
        Ok(parse_object(self.config.before.as_deref())?
            .get("oauthAccount")
            .filter(|v| !v.is_null())
            .cloned())
    }
}

/// The only parts of the two CLI files a switch owns: the OAuth blob in the
/// credential document and the owner named by `oauthAccount` in the global
/// configuration. Deliberately not `Debug`: it holds tokens.
#[derive(Clone, PartialEq)]
struct Login {
    oauth: Option<Value>,
    owner: Owner,
}
#[derive(Clone, PartialEq, Eq)]
struct Owner {
    account: Option<String>,
    organization: Option<String>,
}
impl Login {
    fn of(raw: &RawSnapshot) -> Result<Self> {
        Self::from_documents(raw.auth.as_deref(), raw.config.as_deref())
    }
    fn from_documents(auth: Option<&[u8]>, config: Option<&[u8]>) -> Result<Self> {
        let auth = parse_object(auth)?;
        let config = parse_object(config)?;
        Ok(Self {
            oauth: auth.get("claudeAiOauth").filter(|v| !v.is_null()).cloned(),
            owner: Owner::of(config.get("oauthAccount")),
        })
    }
}
impl Owner {
    fn of(identity: Option<&Value>) -> Self {
        let field = |name: &str| {
            identity
                .and_then(|v| v.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        Self {
            account: field("accountUuid"),
            organization: field("organizationUuid"),
        }
    }
}

// The hook is private and production supplies a no-op. Fixture tests can interrupt
// at durable boundaries without compiling fault injection into the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    BeforeJournal,
    AfterJournal,
    AfterAuth,
    AfterIdentity,
    AfterVerification,
    AfterCommit,
    BeforeCleanup,
}

impl ActiveStore {
    pub fn new(paths: CliPaths) -> Self {
        Self {
            paths,
            native: true,
        }
    }
    pub fn file(paths: CliPaths) -> Self {
        Self {
            paths,
            native: false,
        }
    }
    pub fn paths(&self) -> &CliPaths {
        &self.paths
    }

    pub fn read(&self) -> Result<ActiveSnapshot> {
        let raw = self.read_raw()?;
        self.snapshot(&raw)
    }

    pub fn switch(&self, credentials: &Value, identity: &Value, vault: &Vault) -> Result<()> {
        self.perform_switch(credentials, identity, None, vault, |_| Ok(()))
    }

    /// Runtime entry: rejects an external login or token rotation since the caller
    /// captured `expected_fingerprint` (the auth-relevant `ActiveSnapshot` fingerprint).
    pub fn switch_checked(
        &self,
        credentials: &Value,
        identity: &Value,
        expected_fingerprint: &str,
        vault: &Vault,
    ) -> Result<()> {
        self.perform_switch(
            credentials,
            identity,
            Some(expected_fingerprint),
            vault,
            |_| Ok(()),
        )
    }

    fn perform_switch(
        &self,
        credentials: &Value,
        identity: &Value,
        expected: Option<&str>,
        vault: &Vault,
        hook: impl FnMut(Boundary) -> Result<()>,
    ) -> Result<()> {
        self.locked_switch(credentials, identity, expected, vault, hook, true)
    }

    fn locked_switch(
        &self,
        credentials: &Value,
        identity: &Value,
        expected: Option<&str>,
        vault: &Vault,
        mut hook: impl FnMut(Boundary) -> Result<()>,
        recover_on_error: bool,
    ) -> Result<()> {
        let context = self.context()?;
        let _lock = vault.context_lock(&context)?;
        self.recover_locked(vault, &context)?;
        let result = self.transaction(credentials, identity, expected, vault, &context, &mut hook);
        if result.is_err() && recover_on_error {
            // Resolve a failed switch at once: restore the original login when only this
            // switch's own writes are present, or set the journal aside when another
            // writer owns the login. The CLI is never left reading a mixed login.
            let _ = self.recover_locked(vault, &context);
        }
        result
    }

    fn transaction(
        &self,
        credentials: &Value,
        identity: &Value,
        expected: Option<&str>,
        vault: &Vault,
        context: &str,
        hook: &mut impl FnMut(Boundary) -> Result<()>,
    ) -> Result<()> {
        let before = self.read_raw()?;
        let snapshot = self.snapshot(&before)?;
        if expected.is_some_and(|fp| fp != snapshot.fingerprint) {
            return Err(PlatformError::Conflict);
        }
        let oauth = credentials
            .get("claudeAiOauth")
            .filter(|v| v.is_object())
            .ok_or(PlatformError::InvalidPayload)?;
        if oauth
            .get("accessToken")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
            || identity
                .get("accountUuid")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(PlatformError::InvalidPayload);
        }
        let mut auth = parse_object(before.auth.as_deref())?;
        auth["claudeAiOauth"] = oauth.clone();
        let mut config = parse_object(before.config.as_deref())?;
        config["oauthAccount"] = identity.clone();
        let mut journal = Journal {
            version: 1,
            context: context.to_owned(),
            backend: before.backend,
            auth: Component {
                before: before.auth,
                after: serde_json::to_vec_pretty(&auth)?,
            },
            config: Component {
                before: before.config,
                after: serde_json::to_vec_pretty(&config)?,
            },
            stage: Stage::Prepared,
        };
        let (original, desired) = journal.logins()?;
        hook(Boundary::BeforeJournal)?;
        // Recheck after preflight/hook, before persisting snapshots or overwriting a
        // journal. Only the login is compared: the CLI rewrites its other keys freely.
        if self.current_login(journal.backend)? != original {
            return Err(PlatformError::Conflict);
        }
        let backup_name = format!("first-backup-{context}");
        if vault.load::<Value>(&backup_name)?.is_none() {
            vault.save(&backup_name, &journal)?;
        }
        let journal_name = format!("switch-{context}");
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterJournal)?;
        // Each write patches this switch's key into the document as it is now, so keys
        // the CLI wrote meanwhile survive. It refuses if the login itself has moved.
        self.write_oauth(journal.backend, &original, desired.oauth.as_ref(), false)?;
        journal.stage = Stage::AuthWritten;
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterAuth)?;
        let auth_only = Login {
            oauth: desired.oauth.clone(),
            owner: original.owner.clone(),
        };
        self.write_identity(journal.backend, &auth_only, Some(identity), false)?;
        journal.stage = Stage::IdentityWritten;
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterIdentity)?;
        // JSON readback validates both schema and the patched owner, without logging data.
        let current = self.read_raw()?;
        self.snapshot(&current)?;
        if current.backend != journal.backend || Login::of(&current)? != desired {
            return Err(PlatformError::Conflict);
        }
        hook(Boundary::AfterVerification)?;
        journal.stage = Stage::Committed;
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterCommit)?;
        hook(Boundary::BeforeCleanup)?;
        // A later external writer prevents even the successful transaction from being
        // reported as the selected active identity.
        if self.current_login(journal.backend)? != desired {
            return Err(PlatformError::Conflict);
        }
        vault.remove(&journal_name)?;
        Ok(())
    }

    pub fn recover(&self, vault: &Vault) -> Result<Recovery> {
        let context = self.context()?;
        let _lock = vault.context_lock(&context)?;
        self.recover_locked(vault, &context)
    }

    fn recover_locked(&self, vault: &Vault, context: &str) -> Result<Recovery> {
        let name = format!("switch-{context}");
        let journal = match vault.load::<Journal>(&name) {
            Ok(Some(journal)) => journal,
            Ok(None) => return Ok(Recovery::Clean),
            // An authentic record this version cannot interpret is kept, never applied.
            Err(PlatformError::InvalidJson) => return set_aside(vault, &name),
            Err(error) => return Err(error),
        };
        if journal.version != 1 || journal.context != context {
            return set_aside(vault, &name);
        }
        let Ok((original, desired)) = journal.logins() else {
            return set_aside(vault, &name);
        };
        let current = self.read_raw()?;
        if current.backend != journal.backend {
            return set_aside(vault, &name);
        }
        // Unrelated keys the CLI rewrote since the journal was saved are irrelevant:
        // only the OAuth blob and the owner decide which side of the switch is present.
        let now = Login::of(&current)?;
        // Full desired pair is a verified commit even if the stage write was interrupted.
        if now == desired {
            self.snapshot(&current)?;
            vault.remove(&name)?;
            return Ok(Recovery::Completed);
        }
        // A committed pair changed subsequently: treat it as an external change rather
        // than restoring obsolete credentials (even if they match the old snapshot).
        if journal.stage == Stage::Committed {
            return set_aside(vault, &name);
        }
        let auth_only = Login {
            oauth: desired.oauth.clone(),
            owner: original.owner.clone(),
        };
        let identity_only = Login {
            oauth: original.oauth.clone(),
            owner: desired.owner.clone(),
        };
        // Roll back only a component still equal to this transaction's own write.
        if now == original {
        } else if now == auth_only {
            self.write_oauth(
                journal.backend,
                &auth_only,
                original.oauth.as_ref(),
                journal.auth.before.is_none(),
            )?;
        } else if now == identity_only {
            self.write_identity(
                journal.backend,
                &identity_only,
                journal.original_identity()?.as_ref(),
                journal.config.before.is_none(),
            )?;
        } else {
            return set_aside(vault, &name);
        }
        if self.current_login(journal.backend)? != original {
            return Err(PlatformError::Conflict);
        }
        vault.remove(&name)?;
        Ok(Recovery::RolledBack)
    }

    /// Replaces (or, with `None`, removes) the CLI OAuth blob in the credential
    /// document as it is now, only while the login still equals `expected`.
    fn write_oauth(
        &self,
        backend: Backend,
        expected: &Login,
        oauth: Option<&Value>,
        absent_originally: bool,
    ) -> Result<()> {
        let current = self.read_raw()?;
        if current.backend != backend || Login::of(&current)? != *expected {
            return Err(PlatformError::Conflict);
        }
        let mut document = parse_object(current.auth.as_deref())?;
        let empty = patch(&mut document, "claudeAiOauth", oauth)?;
        if empty && absent_originally {
            self.write_auth(backend, None)
        } else {
            self.write_auth(backend, Some(&serde_json::to_vec_pretty(&document)?))
        }
    }

    /// Replaces (or removes) `oauthAccount` in the global configuration as it is now,
    /// only while the login still equals `expected`.
    fn write_identity(
        &self,
        backend: Backend,
        expected: &Login,
        identity: Option<&Value>,
        absent_originally: bool,
    ) -> Result<()> {
        let current = self.read_raw()?;
        if current.backend != backend || Login::of(&current)? != *expected {
            return Err(PlatformError::Conflict);
        }
        let mut document = parse_object(current.config.as_deref())?;
        let empty = patch(&mut document, "oauthAccount", identity)?;
        if empty && absent_originally {
            files::remove(&self.paths.global_config_file)
        } else {
            files::atomic_write(
                &self.paths.global_config_file,
                &serde_json::to_vec_pretty(&document)?,
            )
        }
    }

    fn current_login(&self, backend: Backend) -> Result<Login> {
        let current = self.read_raw()?;
        if current.backend != backend {
            return Err(PlatformError::Conflict);
        }
        Login::of(&current)
    }

    fn context(&self) -> Result<String> {
        if self.native {
            self.paths.validate_subscription_mode()?;
        }
        files::check_path(&self.paths.credentials_file)?;
        files::check_path(&self.paths.global_config_file)?;
        // Recompute so externally constructed/mutated paths cannot reuse another context lock.
        Ok(self.paths.computed_context_id())
    }

    fn snapshot(&self, raw: &RawSnapshot) -> Result<ActiveSnapshot> {
        let credentials = parse_object(raw.auth.as_deref())?;
        let config = parse_object(raw.config.as_deref())?;
        let identity = config.get("oauthAccount").cloned().unwrap_or(Value::Null);
        if !identity.is_null() && !identity.is_object() {
            return Err(PlatformError::InvalidJson);
        }
        if credentials
            .get("claudeAiOauth")
            .is_some_and(|v| !v.is_null() && !v.is_object())
        {
            return Err(PlatformError::InvalidJson);
        }
        let mut owner = Sha256::new();
        owner.update(self.paths.computed_context_id());
        owner.update(match raw.backend {
            Backend::File => b"file".as_slice(),
            #[cfg(target_os = "macos")]
            Backend::Keychain => b"keychain".as_slice(),
        });
        let Owner {
            account,
            organization,
        } = Owner::of(Some(&identity));
        hash_text(&mut owner, account.as_deref());
        hash_text(&mut owner, organization.as_deref());
        let mut auth = owner.clone();
        match credentials.get("claudeAiOauth").filter(|v| !v.is_null()) {
            Some(oauth) => {
                auth.update([1]);
                hash_canonical(&mut auth, oauth);
            }
            None => auth.update([0]),
        }
        Ok(ActiveSnapshot {
            credentials,
            identity,
            fingerprint: format!("{:x}", auth.finalize()),
            identity_fingerprint: format!("{:x}", owner.finalize()),
        })
    }

    fn read_raw(&self) -> Result<RawSnapshot> {
        self.context()?;
        #[cfg(target_os = "macos")]
        if self.native
            && let Some(auth) = mac::read_current()?
        {
            return Ok(RawSnapshot {
                auth: Some(auth),
                config: files::read_optional(&self.paths.global_config_file)?,
                backend: Backend::Keychain,
            });
        }
        #[cfg(not(target_os = "macos"))]
        let _ = self.native;
        Ok(RawSnapshot {
            auth: files::read_optional(&self.paths.credentials_file)?,
            config: files::read_optional(&self.paths.global_config_file)?,
            backend: Backend::File,
        })
    }

    fn write_auth(&self, backend: Backend, bytes: Option<&[u8]>) -> Result<()> {
        match backend {
            Backend::File => match bytes {
                Some(bytes) => files::atomic_write(&self.paths.credentials_file, bytes),
                None => files::remove(&self.paths.credentials_file),
            },
            #[cfg(target_os = "macos")]
            Backend::Keychain => mac::update_current(bytes.ok_or(PlatformError::Conflict)?),
        }
    }
}

fn set_aside(vault: &Vault, name: &str) -> Result<Recovery> {
    vault.set_aside(name)?;
    Ok(Recovery::SetAside)
}

/// Sets or removes one root key; returns whether the document is now empty.
fn patch(document: &mut Value, key: &str, value: Option<&Value>) -> Result<bool> {
    let map = document.as_object_mut().ok_or(PlatformError::InvalidJson)?;
    match value {
        Some(value) => {
            map.insert(key.to_owned(), value.clone());
        }
        None => {
            map.remove(key);
        }
    }
    Ok(map.is_empty())
}

fn hash_text(hash: &mut Sha256, text: Option<&str>) {
    match text {
        Some(text) => {
            hash.update([1]);
            hash.update((text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        }
        None => hash.update([0]),
    }
}

/// Key-order-independent digest, so a rewrite that only reorders the blob's keys is
/// not mistaken for a new token.
fn hash_canonical(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Null => hash.update([0]),
        Value::Bool(flag) => hash.update([1, u8::from(*flag)]),
        Value::Number(number) => {
            hash.update([2]);
            hash_text(hash, Some(&number.to_string()));
        }
        Value::String(text) => {
            hash.update([3]);
            hash_text(hash, Some(text));
        }
        Value::Array(items) => {
            hash.update([4]);
            hash.update((items.len() as u64).to_le_bytes());
            for item in items {
                hash_canonical(hash, item);
            }
        }
        Value::Object(map) => {
            hash.update([5]);
            hash.update((map.len() as u64).to_le_bytes());
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                hash_text(hash, Some(key));
                hash_canonical(hash, &map[key]);
            }
        }
    }
}

fn parse_object(bytes: Option<&[u8]>) -> Result<Value> {
    let value = match bytes {
        Some(bytes) => serde_json::from_slice(bytes)?,
        None => json!({}),
    };
    if !value.is_object() {
        return Err(PlatformError::InvalidJson);
    }
    Ok(value)
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use security_framework::os::macos::passwords::find_generic_password;
    // Static source inspection of installed public Claude 2.1.287, not native Mac
    // acceptance. Older username/service-only selectors are intentionally not guessed.
    const SERVICE: &str = "Claude Code-credentials";
    const ACCOUNT: &str = "claude-code-user";
    pub(super) fn read_current() -> Result<Option<Vec<u8>>> {
        match find_generic_password(None, SERVICE, ACCOUNT) {
            Ok((bytes, _)) => Ok(Some(bytes.to_owned())),
            Err(error) if error.code() == -25300 => {
                // The original Mac app used NSUserName. A legacy OAuth item is an
                // ambiguous CLI version/context, not permission to write a fallback file.
                let username = native_username()?;
                match find_generic_password(None, SERVICE, &username) {
                    Ok((bytes, _)) => {
                        let value = parse_object(Some(bytes.as_ref()))?;
                        if value.get("claudeAiOauth").is_some() {
                            return Err(PlatformError::UnsupportedContext);
                        }
                        Ok(None)
                    }
                    Err(error) if error.code() == -25300 => Ok(None),
                    Err(_) => Err(PlatformError::KeyUnavailable),
                }
            }
            Err(error) if error.code() == -25291 => Ok(None),
            Err(_) => Err(PlatformError::KeyUnavailable),
        }
    }
    pub(super) fn update_current(bytes: &[u8]) -> Result<()> {
        // In-place native item update preserves its attributes and ACL. It cannot
        // upsert a new item if the CLI deletes the old one between read and write.
        let (_, mut item) =
            find_generic_password(None, SERVICE, ACCOUNT).map_err(|_| PlatformError::Conflict)?;
        item.set_password(bytes)
            .map_err(|_| PlatformError::KeyUnavailable)
    }
    fn native_username() -> Result<String> {
        use std::{ffi::CStr, mem::MaybeUninit, ptr};
        let mut buffer = vec![0u8; 16_384];
        let mut entry = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();
        unsafe {
            if libc::getpwuid_r(
                libc::geteuid(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            ) != 0
                || result.is_null()
            {
                return Err(PlatformError::UnsupportedContext);
            }
            CStr::from_ptr((*result).pw_name)
                .to_str()
                .map(str::to_owned)
                .map_err(|_| PlatformError::UnsupportedContext)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, ActiveStore, Vault) {
        let root = crate::files::test_root();
        let store = ActiveStore::file(CliPaths::for_home(root.path().join("home ș spaces")));
        std::fs::create_dir_all(&store.paths.config_dir).unwrap();
        std::fs::write(&store.paths.credentials_file, serde_json::to_vec(&json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"old-r","extension":"old-auth"},"mcp":{"preserve":true}})).unwrap()).unwrap();
        std::fs::write(&store.paths.global_config_file, serde_json::to_vec(&json!({"oauthAccount":{"accountUuid":"old-owner"},"preferences":{"theme":"dark"},"unknown":[1,2]})).unwrap()).unwrap();
        let vault = Vault::with_key(root.path().join("vault"), [42; 32]).unwrap();
        (root, store, vault)
    }
    fn target() -> (Value, Value) {
        (
            json!({"claudeAiOauth":{"accessToken":"NEW_SENTINEL_TOKEN","refreshToken":"new-r","futureAuthExtension":3},"mcp":"must-not-copy"}),
            json!({"accountUuid":"new-owner","emailAddress":"fixture@example.invalid","futureIdentity":true}),
        )
    }
    fn journal_name(store: &ActiveStore) -> String {
        format!("switch-{}", store.context().unwrap())
    }
    fn read_json(path: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }
    /// Rewrites a CLI file the way Claude Code does for its own bookkeeping.
    fn edit(path: &std::path::Path, change: impl FnOnce(&mut Value)) {
        let mut value = read_json(path);
        change(&mut value);
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    impl ActiveStore {
        /// Simulates a crash at `hook`: no in-process recovery runs, so the journal is
        /// left for a later restart exactly as a killed process would leave it.
        fn interrupted_switch(
            &self,
            credentials: &Value,
            identity: &Value,
            expected: Option<&str>,
            vault: &Vault,
            hook: impl FnMut(Boundary) -> Result<()>,
        ) -> Result<()> {
            self.locked_switch(credentials, identity, expected, vault, hook, false)
        }
    }

    #[test]
    fn switching_preserves_unrelated_json_and_first_backup() {
        let (_root, store, vault) = fixture();
        let old_auth = std::fs::read(&store.paths.credentials_file).unwrap();
        let (credentials, identity) = target();
        let fingerprint = store.read().unwrap().fingerprint;
        store
            .switch_checked(&credentials, &identity, &fingerprint, &vault)
            .unwrap();
        let snapshot = store.read().unwrap();
        assert_eq!(snapshot.credentials["mcp"], json!({"preserve":true}));
        assert_eq!(
            snapshot.credentials["claudeAiOauth"],
            credentials["claudeAiOauth"]
        );
        assert_eq!(snapshot.identity, identity);
        assert!(!format!("{snapshot:?}").contains("NEW_SENTINEL_TOKEN"));
        let config: Value =
            serde_json::from_slice(&std::fs::read(&store.paths.global_config_file).unwrap())
                .unwrap();
        assert_eq!(config["preferences"], json!({"theme":"dark"}));
        assert_eq!(config["unknown"], json!([1, 2]));
        let backup_name = format!("first-backup-{}", store.context().unwrap());
        let backup = vault.load::<Journal>(&backup_name).unwrap().unwrap();
        assert_eq!(backup.auth.before, Some(old_auth));
        store.switch(&credentials, &identity, &vault).unwrap();
        assert_eq!(
            vault
                .load::<Journal>(&backup_name)
                .unwrap()
                .unwrap()
                .auth
                .before,
            backup.auth.before
        );
        assert!(
            vault
                .load::<Journal>(&format!("switch-{}", store.context().unwrap()))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn corrupt_original_and_changed_preflight_are_preserved() {
        let (_root, store, vault) = fixture();
        let (credentials, identity) = target();
        let fingerprint = store.read().unwrap().fingerprint;
        std::fs::write(&store.paths.credentials_file, b"{invalid SECRET_SENTINEL").unwrap();
        assert!(matches!(
            store.switch(&credentials, &identity, &vault),
            Err(PlatformError::InvalidJson)
        ));
        assert_eq!(
            std::fs::read(&store.paths.credentials_file).unwrap(),
            b"{invalid SECRET_SENTINEL"
        );
        std::fs::write(&store.paths.credentials_file, b"{}").unwrap();
        assert!(matches!(
            store.switch_checked(&credentials, &identity, &fingerprint, &vault),
            Err(PlatformError::Conflict)
        ));
        assert_eq!(std::fs::read(&store.paths.credentials_file).unwrap(), b"{}");
    }
    #[test]
    fn fingerprints_follow_only_the_login_and_separate_rotation_from_owner() {
        let (_root, store, _vault) = fixture();
        let first = store.read().unwrap();
        // Claude Code's own bookkeeping: counters, tips, project stats, MCP tokens.
        edit(&store.paths.global_config_file, |c| {
            c["numStartups"] = json!(41);
            c["projects"] = json!({"fixture":{"lastCost":1.5}});
        });
        edit(&store.paths.credentials_file, |c| {
            c["mcpOAuth"] = json!({"fixture-server":{"accessToken":"mcp-rotated"}});
        });
        // Same blob written with its keys in another order.
        let mut auth = read_json(&store.paths.credentials_file);
        auth["claudeAiOauth"] =
            json!({"extension":"old-auth","refreshToken":"old-r","accessToken":"old"});
        std::fs::write(
            &store.paths.credentials_file,
            serde_json::to_vec_pretty(&auth).unwrap(),
        )
        .unwrap();
        let unrelated = store.read().unwrap();
        assert_eq!(unrelated.fingerprint, first.fingerprint);
        assert_eq!(unrelated.identity_fingerprint, first.identity_fingerprint);
        // Non-owner identity fields such as the email are not the owner either.
        edit(&store.paths.global_config_file, |c| {
            c["oauthAccount"]["emailAddress"] = json!("renamed@example.invalid");
        });
        assert_eq!(store.read().unwrap().fingerprint, first.fingerprint);
        // A CLI token rotation is an auth change for the same owner.
        edit(&store.paths.credentials_file, |c| {
            c["claudeAiOauth"]["accessToken"] = json!("rotated-by-cli");
        });
        let rotated = store.read().unwrap();
        assert_ne!(rotated.fingerprint, first.fingerprint);
        assert_eq!(rotated.identity_fingerprint, first.identity_fingerprint);
        // A login to another account changes the owner.
        edit(&store.paths.global_config_file, |c| {
            c["oauthAccount"] = json!({"accountUuid":"other-owner"});
        });
        let other = store.read().unwrap();
        assert_ne!(other.identity_fingerprint, first.identity_fingerprint);
        assert_ne!(other.fingerprint, rotated.fingerprint);
        assert!(!format!("{other:?}").contains("rotated-by-cli"));
    }
    #[test]
    fn unrelated_cli_writes_during_a_switch_are_kept_and_never_block_it() {
        for boundary in [
            Boundary::BeforeJournal,
            Boundary::AfterJournal,
            Boundary::AfterAuth,
            Boundary::AfterIdentity,
        ] {
            let (_root, store, vault) = fixture();
            let before = store.read().unwrap();
            let (credentials, identity) = target();
            store
                .perform_switch(
                    &credentials,
                    &identity,
                    Some(&before.fingerprint),
                    &vault,
                    |point| {
                        if point == boundary {
                            edit(&store.paths.global_config_file, |c| {
                                c["numStartups"] = json!(7)
                            });
                            edit(&store.paths.credentials_file, |c| {
                                c["mcpOAuth"] = json!({"fixture-server":"rotated"})
                            });
                        }
                        Ok(())
                    },
                )
                .unwrap();
            let config = read_json(&store.paths.global_config_file);
            assert_eq!(config["numStartups"], 7);
            assert_eq!(config["preferences"], json!({"theme":"dark"}));
            assert_eq!(config["oauthAccount"], identity);
            let auth = read_json(&store.paths.credentials_file);
            assert_eq!(auth["mcpOAuth"], json!({"fixture-server":"rotated"}));
            assert_eq!(auth["mcp"], json!({"preserve":true}));
            assert_eq!(auth["claudeAiOauth"], credentials["claudeAiOauth"]);
            assert!(
                vault
                    .load::<Journal>(&journal_name(&store))
                    .unwrap()
                    .is_none()
            );
            assert_eq!(store.recover(&vault).unwrap(), Recovery::Clean);
            // The next switch is not wedged by anything this one left behind.
            let fresh = store.read().unwrap();
            let (back, owner) = (
                json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"old-r"}}),
                json!({"accountUuid":"old-owner"}),
            );
            store
                .switch_checked(&back, &owner, &fresh.fingerprint, &vault)
                .unwrap();
            assert_eq!(read_json(&store.paths.global_config_file)["numStartups"], 7);
        }
    }
    #[test]
    fn restart_after_each_durable_boundary_has_a_complete_pair() {
        for (boundary, expected) in [
            (Boundary::BeforeJournal, Recovery::Clean),
            (Boundary::AfterJournal, Recovery::RolledBack),
            (Boundary::AfterAuth, Recovery::RolledBack),
            (Boundary::AfterIdentity, Recovery::Completed),
            (Boundary::AfterVerification, Recovery::Completed),
            (Boundary::AfterCommit, Recovery::Completed),
            (Boundary::BeforeCleanup, Recovery::Completed),
        ] {
            let (_root, store, vault) = fixture();
            let before = store.read().unwrap();
            let (credentials, identity) = target();
            let error = store.interrupted_switch(
                &credentials,
                &identity,
                Some(&before.fingerprint),
                &vault,
                |point| {
                    if point == boundary {
                        Err(PlatformError::Io)
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(error.is_err());
            // A newly constructed adapter must be able to recover the durable journal.
            let restarted = ActiveStore::file(store.paths.clone());
            assert_eq!(restarted.recover(&vault).unwrap(), expected);
            let recovered = restarted.read().unwrap();
            if expected == Recovery::Completed {
                assert_eq!(recovered.identity, identity);
                assert_eq!(
                    recovered.credentials["claudeAiOauth"],
                    credentials["claudeAiOauth"]
                );
            } else {
                assert_eq!(recovered.identity, before.identity);
                assert_eq!(recovered.credentials, before.credentials);
                assert_eq!(recovered.fingerprint, before.fingerprint);
            }
            assert_eq!(restarted.recover(&vault).unwrap(), Recovery::Clean);
        }
    }
    #[test]
    fn recovery_restores_the_login_through_unrelated_cli_writes() {
        let (_root, store, vault) = fixture();
        let before = store.read().unwrap();
        let (credentials, identity) = target();
        store
            .interrupted_switch(&credentials, &identity, None, &vault, |point| {
                if point == Boundary::AfterAuth {
                    Err(PlatformError::Io)
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        // The CLI keeps writing its own keys while the journal waits for a restart.
        edit(&store.paths.global_config_file, |c| {
            c["numStartups"] = json!(9)
        });
        edit(&store.paths.credentials_file, |c| {
            c["mcpOAuth"] = json!({"fixture-server":"kept"})
        });
        assert_eq!(store.recover(&vault).unwrap(), Recovery::RolledBack);
        let recovered = store.read().unwrap();
        assert_eq!(recovered.identity, before.identity);
        assert_eq!(
            recovered.credentials["claudeAiOauth"],
            before.credentials["claudeAiOauth"]
        );
        assert_eq!(
            recovered.credentials["mcpOAuth"],
            json!({"fixture-server":"kept"})
        );
        assert_eq!(read_json(&store.paths.global_config_file)["numStartups"], 9);
        assert!(
            vault
                .load::<Journal>(&journal_name(&store))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn a_failed_switch_restores_the_original_login_without_waiting_for_restart() {
        let (_root, store, vault) = fixture();
        let before = store.read().unwrap();
        let (credentials, identity) = target();
        let result = store.perform_switch(
            &credentials,
            &identity,
            Some(&before.fingerprint),
            &vault,
            |point| {
                if point == Boundary::AfterAuth {
                    Err(PlatformError::Io)
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(PlatformError::Io)));
        let now = store.read().unwrap();
        assert_eq!(now.identity, before.identity);
        assert_eq!(now.credentials, before.credentials);
        assert!(
            vault
                .load::<Journal>(&journal_name(&store))
                .unwrap()
                .is_none()
        );
        assert_eq!(store.recover(&vault).unwrap(), Recovery::Clean);
    }
    #[test]
    fn external_writer_at_every_boundary_is_never_rolled_back() {
        for boundary in [
            Boundary::BeforeJournal,
            Boundary::AfterJournal,
            Boundary::AfterAuth,
            Boundary::AfterIdentity,
            Boundary::AfterVerification,
            Boundary::AfterCommit,
        ] {
            let (_root, store, vault) = fixture();
            let before = store.read().unwrap();
            let (credentials, identity) = target();
            let external =
                br#"{"claudeAiOauth":{"accessToken":"external-newest"},"external":true}"#;
            let result = store.perform_switch(
                &credentials,
                &identity,
                Some(&before.fingerprint),
                &vault,
                |point| {
                    if point == boundary {
                        std::fs::write(&store.paths.credentials_file, external).unwrap();
                    }
                    Ok(())
                },
            );
            assert!(matches!(result, Err(PlatformError::Conflict)));
            assert_eq!(
                std::fs::read(&store.paths.credentials_file).unwrap(),
                external
            );
            // The journal the external login made inapplicable is retained under a
            // set-aside name instead of blocking every later switch and startup.
            let name = journal_name(&store);
            assert!(vault.load::<Journal>(&name).unwrap().is_none());
            assert_eq!(
                vault
                    .load::<Journal>(&format!("{name}-set-aside-1"))
                    .unwrap()
                    .is_some(),
                boundary != Boundary::BeforeJournal
            );
            assert_eq!(store.recover(&vault).unwrap(), Recovery::Clean);
            assert_eq!(
                std::fs::read(&store.paths.credentials_file).unwrap(),
                external
            );
        }
    }
    #[test]
    fn context_lock_refuses_concurrent_switch() {
        let (_root, store, vault) = fixture();
        let _lock = vault.context_lock(&store.context().unwrap()).unwrap();
        let (credentials, identity) = target();
        assert!(matches!(
            store.switch(&credentials, &identity, &vault),
            Err(PlatformError::Busy)
        ));
    }
    #[test]
    fn absent_original_recovers_absence_after_auth_only() {
        let (root, store, vault) = fixture();
        std::fs::remove_file(&store.paths.credentials_file).unwrap();
        std::fs::remove_file(&store.paths.global_config_file).unwrap();
        assert_eq!(store.read().unwrap().identity, Value::Null);
        let (credentials, identity) = target();
        store
            .interrupted_switch(&credentials, &identity, None, &vault, |point| {
                if point == Boundary::AfterAuth {
                    Err(PlatformError::Io)
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(store.recover(&vault).unwrap(), Recovery::RolledBack);
        assert!(!store.paths.credentials_file.exists());
        assert!(!store.paths.global_config_file.exists());
        assert!(root.path().exists());
    }

    #[test]
    fn a_new_external_identity_is_never_overwritten_and_the_journal_is_set_aside() {
        let (_root, store, vault) = fixture();
        let (credentials, identity) = target();
        store
            .interrupted_switch(&credentials, &identity, None, &vault, |point| {
                if point == Boundary::AfterAuth {
                    Err(PlatformError::Io)
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        let external = br#"{"oauthAccount":{"accountUuid":"external-owner"},"external":true}"#;
        std::fs::write(&store.paths.global_config_file, external).unwrap();
        let current_auth = std::fs::read(&store.paths.credentials_file).unwrap();
        assert_eq!(store.recover(&vault).unwrap(), Recovery::SetAside);
        assert_eq!(
            std::fs::read(&store.paths.global_config_file).unwrap(),
            external
        );
        assert_eq!(
            std::fs::read(&store.paths.credentials_file).unwrap(),
            current_auth
        );
        let name = journal_name(&store);
        assert!(vault.load::<Journal>(&name).unwrap().is_none());
        let retained = vault
            .load::<Journal>(&format!("{name}-set-aside-1"))
            .unwrap()
            .unwrap();
        assert_eq!(retained.stage, Stage::AuthWritten);
        // The retained record never blocks the next explicit switch.
        store.switch(&credentials, &identity, &vault).unwrap();
        assert_eq!(store.read().unwrap().identity, identity);
    }

    #[test]
    fn undecodable_or_foreign_journals_are_set_aside_instead_of_wedging_switches() {
        let (_root, store, vault) = fixture();
        let name = journal_name(&store);
        let original = std::fs::read(&store.paths.credentials_file).unwrap();
        vault
            .save(&name, &json!({"version":9,"fixture":"future format"}))
            .unwrap();
        assert_eq!(store.recover(&vault).unwrap(), Recovery::SetAside);
        assert_eq!(
            vault
                .load::<Value>(&format!("{name}-set-aside-1"))
                .unwrap()
                .unwrap()["version"],
            9
        );
        let foreign = Journal {
            version: 1,
            context: "another-context".into(),
            backend: Backend::File,
            auth: Component {
                before: None,
                after: b"{}".to_vec(),
            },
            config: Component {
                before: None,
                after: b"{}".to_vec(),
            },
            stage: Stage::Prepared,
        };
        vault.save(&name, &foreign).unwrap();
        assert_eq!(store.recover(&vault).unwrap(), Recovery::SetAside);
        assert!(
            vault
                .load::<Value>(&format!("{name}-set-aside-2"))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            std::fs::read(&store.paths.credentials_file).unwrap(),
            original
        );
        let (credentials, identity) = target();
        store.switch(&credentials, &identity, &vault).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn native_identity_replace_failure_recovers_the_original_pair() {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let (_root, store, vault) = fixture();
        let original = store.read().unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&store.paths.global_config_file)
            .unwrap();
        let (credentials, identity) = target();
        assert!(matches!(
            store.switch(&credentials, &identity, &vault),
            Err(PlatformError::Io)
        ));
        // The failed switch already restored the original login in-process.
        assert_eq!(store.read().unwrap().fingerprint, original.fingerprint);
        assert_eq!(store.recover(&vault).unwrap(), Recovery::Clean);
        let recovered = store.read().unwrap();
        assert_eq!(recovered.fingerprint, original.fingerprint);
        drop(held);
        store.switch(&credentials, &identity, &vault).unwrap();
    }
}
