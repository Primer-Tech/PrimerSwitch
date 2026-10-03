use crate::{CliPaths, PlatformError, Result, Vault, files};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt;

pub struct ActiveSnapshot {
    pub credentials: Value,
    pub identity: Value,
    pub fingerprint: String,
}
impl fmt::Debug for ActiveSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveSnapshot")
            .field("credentials", &"[REDACTED]")
            .field("identity", &"[REDACTED]")
            .field("fingerprint", &self.fingerprint)
            .finish()
    }
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

    /// Runtime entry: rejects an external login/rotation since ownership verification.
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
        mut hook: impl FnMut(Boundary) -> Result<()>,
    ) -> Result<()> {
        let context = self.context()?;
        let _lock = vault.context_lock(&context)?;
        self.recover_locked(vault, &context)?;
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
            context: context.clone(),
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
        hook(Boundary::BeforeJournal)?;
        // Recheck after preflight/hook, before persisting snapshots or overwriting a journal.
        self.assert_original(&journal)?;
        let backup_name = format!("first-backup-{context}");
        if vault.load::<Journal>(&backup_name)?.is_none() {
            vault.save(&backup_name, &journal)?;
        }
        let journal_name = format!("switch-{context}");
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterJournal)?;
        self.assert_original(&journal)?;
        self.write_auth(journal.backend, Some(&journal.auth.after))?;
        journal.stage = Stage::AuthWritten;
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterAuth)?;
        let current = self.read_raw()?;
        if current.backend != journal.backend
            || current.auth.as_deref() != Some(journal.auth.after.as_slice())
            || current.config != journal.config.before
        {
            return Err(PlatformError::Conflict);
        }
        files::atomic_write(&self.paths.global_config_file, &journal.config.after)?;
        journal.stage = Stage::IdentityWritten;
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterIdentity)?;
        let current = self.read_raw()?;
        if !is_desired(&current, &journal) {
            return Err(PlatformError::Conflict);
        }
        // JSON readback validates both schema and the patched owner, without logging data.
        let verified = self.snapshot(&current)?;
        if verified.identity != *identity
            || verified.credentials.get("claudeAiOauth") != Some(oauth)
        {
            return Err(PlatformError::Conflict);
        }
        hook(Boundary::AfterVerification)?;
        journal.stage = Stage::Committed;
        vault.save(&journal_name, &journal)?;
        hook(Boundary::AfterCommit)?;
        hook(Boundary::BeforeCleanup)?;
        // A later external writer prevents even the successful transaction from being
        // reported as the selected active identity.
        if !is_desired(&self.read_raw()?, &journal) {
            return Err(PlatformError::Conflict);
        }
        vault.remove(&journal_name)?;
        Ok(())
    }

    pub fn recover(&self, vault: &Vault) -> Result<()> {
        let context = self.context()?;
        let _lock = vault.context_lock(&context)?;
        self.recover_locked(vault, &context)
    }

    fn recover_locked(&self, vault: &Vault, context: &str) -> Result<()> {
        let name = format!("switch-{context}");
        let Some(journal) = vault.load::<Journal>(&name)? else {
            return Ok(());
        };
        if journal.version != 1 || journal.context != context {
            return Err(PlatformError::Conflict);
        }
        let current = self.read_raw()?;
        // Full desired pair is a verified commit even if the stage write was interrupted.
        if is_desired(&current, &journal) {
            self.snapshot(&current)?;
            return vault.remove(&name);
        }
        if current.backend != journal.backend
            || !known(&current.auth, &journal.auth)
            || !known(&current.config, &journal.config)
        {
            return Err(PlatformError::Conflict);
        }
        // A committed pair changed subsequently: treat it as an external change rather
        // than restoring obsolete credentials (even if bytes match the old snapshot).
        if journal.stage == Stage::Committed {
            return Err(PlatformError::Conflict);
        }
        // Roll back only components still equal to this transaction's own write.
        if current.auth.as_deref() == Some(journal.auth.after.as_slice())
            && current.auth != journal.auth.before
        {
            let checked = self.read_raw()?;
            if checked.backend != journal.backend
                || checked.auth != current.auth
                || checked.config != current.config
            {
                return Err(PlatformError::Conflict);
            }
            self.write_auth(journal.backend, journal.auth.before.as_deref())?;
        }
        let after_auth = self.read_raw()?;
        if after_auth.backend != journal.backend
            || after_auth.auth != journal.auth.before
            || after_auth.config != current.config
        {
            return Err(PlatformError::Conflict);
        }
        if after_auth.config.as_deref() == Some(journal.config.after.as_slice())
            && after_auth.config != journal.config.before
        {
            match &journal.config.before {
                Some(bytes) => files::atomic_write(&self.paths.global_config_file, bytes)?,
                None => files::remove(&self.paths.global_config_file)?,
            }
        }
        self.assert_original(&journal)?;
        vault.remove(&name)
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
        let mut hash = Sha256::new();
        hash.update(self.paths.computed_context_id());
        hash.update(match raw.backend {
            Backend::File => b"file".as_slice(),
            #[cfg(target_os = "macos")]
            Backend::Keychain => b"keychain".as_slice(),
        });
        for part in [&raw.auth, &raw.config] {
            hash.update([u8::from(part.is_some())]);
            if let Some(bytes) = part {
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
        }
        Ok(ActiveSnapshot {
            credentials,
            identity,
            fingerprint: format!("{:x}", hash.finalize()),
        })
    }

    fn assert_original(&self, journal: &Journal) -> Result<()> {
        let current = self.read_raw()?;
        if current.backend != journal.backend
            || current.auth != journal.auth.before
            || current.config != journal.config.before
        {
            return Err(PlatformError::Conflict);
        }
        Ok(())
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
fn known(current: &Option<Vec<u8>>, component: &Component) -> bool {
    *current == component.before || current.as_deref() == Some(component.after.as_slice())
}
fn is_desired(current: &RawSnapshot, journal: &Journal) -> bool {
    current.backend == journal.backend
        && current.auth.as_deref() == Some(journal.auth.after.as_slice())
        && current.config.as_deref() == Some(journal.config.after.as_slice())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use core_foundation::base::{CFRelease, OSStatus, TCFType};
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework_sys::{
        base::{SecAccessRef, SecKeychainItemRef},
        keychain_item::SecKeychainItemDelete,
    };
    use std::ptr;

    // The high-level crate does not expose keychain item access ACLs. Preserve
    // the existing ACL while replacing the content, so a Claude item created
    // with `security add-generic-password -A` keeps its non-prompting access.
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecKeychainItemCopyAccess(
            item_ref: SecKeychainItemRef,
            access: *mut SecAccessRef,
        ) -> OSStatus;
        fn SecKeychainItemSetAccess(item_ref: SecKeychainItemRef, access: SecAccessRef)
        -> OSStatus;
    }

    // Static source inspection of installed public Claude 2.1.287, not native Mac
    // acceptance. Older username/service-only selectors are intentionally not guessed.
    const SERVICE: &str = "Claude Code-credentials";
    const ACCOUNT: &str = "claude-code-user";
    pub(super) fn read_current() -> Result<Option<Vec<u8>>> {
        let keychain = SecKeychain::default().map_err(|_| PlatformError::KeyUnavailable)?;
        match keychain.find_generic_password(SERVICE, ACCOUNT) {
            Ok((bytes, _)) => Ok(Some(bytes.to_owned())),
            Err(error) if error.code() == -25300 => {
                // The original Mac app used NSUserName. A legacy OAuth item is an
                // ambiguous CLI version/context, not permission to write a fallback file.
                let username = native_username()?;
                match keychain.find_generic_password(SERVICE, &username) {
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
        // Use an explicit handle to the user's default keychain. Updating the
        // existing item in-place is not durable on all supported macOS
        // versions (the Security.framework call can report success while a
        // fresh `security find-generic-password` still returns the old data).
        // Recreate the item through the same keychain-specific API that owns
        // the item instead. The old bytes stay in memory until the replacement
        // has been verified so a failed add can restore the previous value.
        let keychain = SecKeychain::default().map_err(|_| PlatformError::KeyUnavailable)?;
        let (old_password, item) = keychain
            .find_generic_password(SERVICE, ACCOUNT)
            .map_err(|_| PlatformError::Conflict)?;
        let old_bytes = old_password.to_owned();
        let access = copy_access(item.as_concrete_TypeRef())?;
        let delete_status = unsafe { SecKeychainItemDelete(item.as_concrete_TypeRef()) };
        drop(item);
        if delete_status != 0 {
            release_access(access);
            return Err(PlatformError::KeyUnavailable);
        }

        if keychain
            .add_generic_password(SERVICE, ACCOUNT, bytes)
            .is_err()
        {
            // The delete API is intentionally best-effort in the upstream
            // wrapper. Re-adding the old value gives the caller a usable
            // keychain item even when the replacement could not be created.
            restore_item(&keychain, &old_bytes, access);
            return Err(PlatformError::KeyUnavailable);
        }

        match keychain.find_generic_password(SERVICE, ACCOUNT) {
            Ok((current, replacement)) if current.as_ref() == bytes => {
                let status =
                    unsafe { SecKeychainItemSetAccess(replacement.as_concrete_TypeRef(), access) };
                drop(replacement);
                if status == 0 {
                    release_access(access);
                    Ok(())
                } else {
                    restore_item(&keychain, &old_bytes, access);
                    Err(PlatformError::KeyUnavailable)
                }
            }
            Ok((_, replacement)) => {
                delete_item(replacement);
                restore_item(&keychain, &old_bytes, access);
                Err(PlatformError::KeyUnavailable)
            }
            Err(_) => {
                restore_item(&keychain, &old_bytes, access);
                Err(PlatformError::KeyUnavailable)
            }
        }
    }

    fn copy_access(item: SecKeychainItemRef) -> Result<SecAccessRef> {
        let mut access = ptr::null_mut();
        let status = unsafe { SecKeychainItemCopyAccess(item, &mut access) };
        if status == 0 && !access.is_null() {
            Ok(access)
        } else {
            release_access(access);
            Err(PlatformError::KeyUnavailable)
        }
    }

    fn restore_item(keychain: &SecKeychain, bytes: &[u8], access: SecAccessRef) {
        if let Ok((_, item)) = keychain.find_generic_password(SERVICE, ACCOUNT) {
            delete_item(item);
        }
        if keychain
            .add_generic_password(SERVICE, ACCOUNT, bytes)
            .is_ok()
            && let Ok((_, item)) = keychain.find_generic_password(SERVICE, ACCOUNT)
        {
            let status = unsafe { SecKeychainItemSetAccess(item.as_concrete_TypeRef(), access) };
            drop(item);
            let _ = status;
        }
        release_access(access);
    }

    fn delete_item(item: impl TCFType<Ref = SecKeychainItemRef>) {
        let status = unsafe { SecKeychainItemDelete(item.as_concrete_TypeRef()) };
        drop(item);
        let _ = status;
    }

    fn release_access(access: SecAccessRef) {
        if !access.is_null() {
            unsafe { CFRelease(access.cast()) };
        }
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
    fn restart_after_each_durable_boundary_has_a_complete_pair() {
        for boundary in [
            Boundary::BeforeJournal,
            Boundary::AfterJournal,
            Boundary::AfterAuth,
            Boundary::AfterIdentity,
            Boundary::AfterVerification,
            Boundary::AfterCommit,
            Boundary::BeforeCleanup,
        ] {
            let (_root, store, vault) = fixture();
            let before = store.read().unwrap();
            let (credentials, identity) = target();
            let error = store.perform_switch(
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
            restarted.recover(&vault).unwrap();
            let recovered = restarted.read().unwrap();
            if matches!(
                boundary,
                Boundary::AfterIdentity
                    | Boundary::AfterVerification
                    | Boundary::AfterCommit
                    | Boundary::BeforeCleanup
            ) {
                assert_eq!(recovered.identity, identity);
                assert_eq!(
                    recovered.credentials["claudeAiOauth"],
                    credentials["claudeAiOauth"]
                );
            } else {
                assert_eq!(recovered.identity, before.identity);
                assert_eq!(recovered.credentials, before.credentials);
            }
            restarted.recover(&vault).unwrap();
        }
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
            let recovery = store.recover(&vault);
            if boundary == Boundary::BeforeJournal {
                assert!(recovery.is_ok());
            } else {
                assert!(matches!(recovery, Err(PlatformError::Conflict)));
            }
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
            .perform_switch(&credentials, &identity, None, &vault, |point| {
                if point == Boundary::AfterAuth {
                    Err(PlatformError::Io)
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        store.recover(&vault).unwrap();
        assert!(!store.paths.credentials_file.exists());
        assert!(!store.paths.global_config_file.exists());
        assert!(root.path().exists());
    }

    #[test]
    fn a_new_external_identity_blocks_recovery_and_preserves_the_journal() {
        let (_root, store, vault) = fixture();
        let (credentials, identity) = target();
        store
            .perform_switch(&credentials, &identity, None, &vault, |point| {
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
        assert!(matches!(
            store.recover(&vault),
            Err(PlatformError::Conflict)
        ));
        assert_eq!(
            std::fs::read(&store.paths.global_config_file).unwrap(),
            external
        );
        assert_eq!(
            std::fs::read(&store.paths.credentials_file).unwrap(),
            current_auth
        );
        assert!(
            vault
                .load::<Journal>(&format!("switch-{}", store.context().unwrap()))
                .unwrap()
                .is_some()
        );
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
        store.recover(&vault).unwrap();
        let recovered = store.read().unwrap();
        assert_eq!(recovered.fingerprint, original.fingerprint);
        drop(held);
        store.switch(&credentials, &identity, &vault).unwrap();
    }
}
