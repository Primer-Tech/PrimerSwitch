use super::{CodexFileContext, CodexResult, CodexStoreError, StoreGeneration};
use crate::Vault;
use zeroize::{Zeroize, Zeroizing};

pub struct CodexAuthSnapshot {
    pub generation: StoreGeneration,
    bytes: Option<Zeroizing<Vec<u8>>>,
}
impl CodexAuthSnapshot {
    pub fn auth_bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref().map(Vec::as_slice)
    }
}
impl std::fmt::Debug for CodexAuthSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexAuthSnapshot")
            .field("generation", &self.generation)
            .field("auth", &"[REDACTED]")
            .finish()
    }
}

/// The active FILE-mode login (`CODEX_HOME/auth.json`). Codex itself rewrites this
/// file on every token refresh; PrimerSwitch only ever replaces it atomically with a
/// complete saved payload after checking that it is still the generation it read.
pub struct CodexFileStore {
    context: CodexFileContext,
}
impl CodexFileStore {
    pub fn new(context: CodexFileContext) -> Self {
        Self { context }
    }
    pub fn context(&self) -> &CodexFileContext {
        &self.context
    }
    pub fn read(&self) -> CodexResult<CodexAuthSnapshot> {
        self.context.revalidate()?;
        let (bytes, generation) = super::generation::read(
            &self.context.home().join("auth.json"),
            self.context.context_id(),
            32 * 1024 * 1024,
            true,
        )?;
        Ok(CodexAuthSnapshot { generation, bytes })
    }
    /// Atomically install `incoming` as the active login if auth.json is still at
    /// `expected` (absent or present). Other files in the home are never touched; the
    /// payload is written exactly as saved, unknown fields included.
    pub fn replace(
        &self,
        incoming: &[u8],
        expected: &StoreGeneration,
        vault: &Vault,
    ) -> CodexResult<CodexAuthSnapshot> {
        validate_auth(incoming)?;
        let _lock = vault.context_lock(self.context.context_id())?;
        let current = self.read()?;
        if &current.generation != expected {
            return Err(CodexStoreError::ExternalChange);
        }
        let written = self.replace_checked(incoming, expected, current.auth_bytes().is_none())?;
        if written.auth_bytes() != Some(incoming) {
            return Err(CodexStoreError::ExternalChange);
        }
        Ok(written)
    }
    /// The 0.2.0 candidate kept an encrypted two-step selection journal. Its outgoing
    /// tokens were adopted into the saved record during preparation, so a leftover
    /// journal carries nothing that is not saved elsewhere. Returns whether one existed.
    pub fn discard_legacy_journal(&self, vault: &Vault) -> CodexResult<bool> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let name = self.journal_name();
        // An unreadable (tampered or foreign-key) leftover is removed as well.
        let existed = !matches!(vault.load::<serde_json::Value>(&name), Ok(None));
        if existed {
            vault.remove(&name)?;
        }
        Ok(existed)
    }
    fn replace_checked(
        &self,
        bytes: &[u8],
        expected: &StoreGeneration,
        originally_absent: bool,
    ) -> CodexResult<CodexAuthSnapshot> {
        use std::io::Write;
        let path = self.context.home().join("auth.json");
        crate::files::check_path(&path)?;
        let mut temp = tempfile::Builder::new()
            .prefix(".primerswitch-codex-")
            .tempfile_in(self.context.home())?;
        crate::protection::protect_file(temp.path(), false)?;
        temp.write_all(bytes)?;
        temp.as_file_mut().sync_all()?;
        let identity = super::generation::native_identity(temp.path())?;
        let staged = temp.into_temp_path();
        // Compare immediately before replacement. If originally absent, staging changes
        // parent timestamps; the caller's read proved absence and this checks it holds.
        let current = self.read()?;
        let expected_absence = originally_absent
            && current.auth_bytes().is_none()
            && current.generation.native_identity == expected.native_identity;
        if &current.generation != expected && !expected_absence {
            return Err(CodexStoreError::ExternalChange);
        }
        crate::files::check_path(&path)?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let source: Vec<u16> = staged.as_os_str().encode_wide().chain(Some(0)).collect();
            let dest: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // Codex holds auth.json open only while reading or writing it; retry a
            // short sharing violation instead of failing the switch.
            let mut attempts = 0;
            loop {
                let moved = unsafe {
                    MoveFileExW(
                        source.as_ptr(),
                        dest.as_ptr(),
                        (if originally_absent {
                            0
                        } else {
                            MOVEFILE_REPLACE_EXISTING
                        }) | MOVEFILE_WRITE_THROUGH,
                    )
                } != 0;
                if moved {
                    break;
                }
                attempts += 1;
                if attempts >= 20 {
                    return Err(crate::PlatformError::Io.into());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
                // Codex may have rotated the login while the move was blocked.
                let current = self.read()?;
                let still_absent = originally_absent
                    && current.auth_bytes().is_none()
                    && current.generation.native_identity == expected.native_identity;
                if &current.generation != expected && !still_absent {
                    return Err(CodexStoreError::ExternalChange);
                }
            }
        }
        #[cfg(not(windows))]
        if originally_absent {
            // Atomic no-replace creation. Never overwrite a newly created login.
            std::fs::hard_link(&staged, &path)?;
            std::fs::remove_file(&staged)?;
        } else {
            std::fs::rename(&staged, &path)?;
        }
        crate::protection::protect_file(&path, false)?;
        #[cfg(unix)]
        std::fs::File::open(self.context.home())?.sync_all()?;
        if super::generation::native_identity(&path)? != identity {
            return Err(CodexStoreError::ExternalChange);
        }
        let snapshot = self.read()?;
        if super::generation::native_identity(&path)? != identity {
            return Err(CodexStoreError::ExternalChange);
        }
        Ok(snapshot)
    }
    fn journal_name(&self) -> String {
        format!("codex-journal-{}", self.context.context_id())
    }
}

fn zeroize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => s.zeroize(),
        serde_json::Value::Array(items) => {
            for item in items {
                zeroize_json(item);
            }
        }
        serde_json::Value::Object(items) => {
            for item in items.values_mut() {
                zeroize_json(item);
            }
        }
        _ => (),
    }
}
/// Shape validation only. JWT routing claims never establish verified ownership.
fn validate_auth(bytes: &[u8]) -> CodexResult<()> {
    if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 {
        return Err(CodexStoreError::UnsupportedAuth);
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| CodexStoreError::UnsupportedAuth)?;
    let valid = (|| {
        let object = value.as_object()?;
        if object
            .get("auth_mode")
            .is_some_and(|v| !v.is_null() && v.as_str() != Some("chatgpt"))
        {
            return None;
        }
        for key in [
            "OPENAI_API_KEY",
            "agent_identity",
            "personal_access_token",
            "bedrock_api_key",
            "bedrock_access_keys",
        ] {
            if object.get(key).is_some_and(|v| !v.is_null()) {
                return None;
            }
        }
        let tokens = object.get("tokens")?.as_object()?;
        for key in ["id_token", "access_token", "refresh_token"] {
            if tokens.get(key)?.as_str()?.is_empty() {
                return None;
            }
        }
        if tokens
            .get("account_id")
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return None;
        }
        Some(())
    })()
    .is_some();
    zeroize_json(&mut value);
    if valid {
        Ok(())
    } else {
        Err(CodexStoreError::UnsupportedAuth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::{
        CodexContextEvidence, CodexPolicyState, CodexProviderMode, CodexSourceRole,
        CodexStorageMode, CodexWatchedSource,
    };
    use std::{fs, path::Path};

    const OLD: &[u8] = br#"{ "OPENAI_API_KEY":null,"tokens":{"id_token":"old-id-secret","access_token":"old-access-secret","refresh_token":"old-refresh-secret","account_id":null},"future":{"oldAccountOnly":true}}"#;
    const NEW: &[u8] = br#"{ "auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"new-id-secret","access_token":"new-access-secret","refresh_token":"new-refresh-secret","account_id":"new-workspace"},"future":{"incomingOnly":[1,2,3]}}"#;

    pub(crate) fn evidence(root: &Path, home: &Path) -> CodexContextEvidence {
        let sources_dir = root.join("sources");
        fs::create_dir_all(&sources_dir).unwrap();
        let mut sources = Vec::new();
        for role in [
            CodexSourceRole::Executable,
            CodexSourceRole::System,
            CodexSourceRole::Managed,
            CodexSourceRole::User,
            CodexSourceRole::Project,
        ] {
            let path = if role == CodexSourceRole::User {
                let path = home.join("config.toml");
                if !path.exists() {
                    fs::write(&path, b"model = \"fixture-model\"\n").unwrap();
                }
                path
            } else {
                sources_dir.join(format!("{role:?}"))
            };
            if role == CodexSourceRole::Executable {
                fs::write(&path, b"fake-native-executable").unwrap();
            }
            let (receipt, _) = CodexWatchedSource::capture(role, path).unwrap();
            sources.push(receipt);
        }
        sources.push(CodexWatchedSource::default_session());
        CodexContextEvidence {
            version: "0.160.0".into(),
            schema: "fixture".into(),
            storage: CodexStorageMode::File,
            provider: CodexProviderMode::DefaultOpenAi,
            policy: CodexPolicyState::Unrestricted,
            sources,
        }
    }
    fn fixture(initial: Option<&[u8]>) -> (tempfile::TempDir, CodexFileStore, Vault) {
        let root = crate::files::test_root();
        let home = root.path().join("codex-home");
        fs::create_dir(&home).unwrap();
        if let Some(bytes) = initial {
            crate::files::atomic_write(&home.join("auth.json"), bytes).unwrap();
        }
        let context =
            CodexFileContext::qualify(home.clone(), evidence(root.path(), &home)).unwrap();
        let vault = Vault::with_key(root.path().join("app-vault"), [47; 32]).unwrap();
        (root, CodexFileStore::new(context), vault)
    }

    #[test]
    fn exact_bytes_unknown_fields_and_other_home_files_survive_replacement() {
        let (_root, store, vault) = fixture(Some(OLD));
        let home = store.context.home().to_owned();
        let original_config = fs::read(home.join("config.toml")).unwrap();
        for name in ["history.jsonl", "mcp.json", "profiles.json"] {
            fs::write(home.join(name), b"unrelated-sentinel").unwrap();
        }
        let before = store.read().unwrap();
        let snapshot = store.replace(NEW, &before.generation, &vault).unwrap();
        assert_eq!(snapshot.auth_bytes(), Some(NEW));
        assert_ne!(snapshot.generation, before.generation);
        #[cfg(windows)]
        crate::protection::tests::assert_private_acl(&home.join("auth.json"), false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(home.join("auth.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        for name in ["history.jsonl", "mcp.json", "profiles.json"] {
            assert_eq!(fs::read(home.join(name)).unwrap(), b"unrelated-sentinel");
        }
        assert_eq!(fs::read(home.join("config.toml")).unwrap(), original_config);
        assert!(!format!("{snapshot:?}").contains("secret"));
        // No staging file is left behind.
        assert!(fs::read_dir(&home).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".primerswitch")
        }));
    }
    #[test]
    fn a_concurrent_external_write_is_never_overwritten() {
        let (_root, store, vault) = fixture(Some(OLD));
        let before = store.read().unwrap();
        // Codex refreshed the token between our read and the replacement.
        crate::files::atomic_write(&store.context.home().join("auth.json"), NEW).unwrap();
        assert!(matches!(
            store.replace(OLD, &before.generation, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        assert_eq!(store.read().unwrap().auth_bytes(), Some(NEW));
    }
    #[test]
    fn identical_bytes_rewritten_externally_are_a_new_generation() {
        let (_root, store, vault) = fixture(Some(OLD));
        let before = store.read().unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), OLD).unwrap();
        assert_ne!(store.read().unwrap().generation, before.generation);
        assert!(matches!(
            store.replace(NEW, &before.generation, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
    }
    #[test]
    fn missing_login_is_created_and_a_new_external_login_is_preserved() {
        let (_root, store, vault) = fixture(None);
        let absent = store.read().unwrap();
        assert!(absent.auth_bytes().is_none());
        let written = store.replace(NEW, &absent.generation, &vault).unwrap();
        assert_eq!(written.auth_bytes(), Some(NEW));

        let (_root, store, vault) = fixture(None);
        let absent = store.read().unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), OLD).unwrap();
        assert!(store.replace(NEW, &absent.generation, &vault).is_err());
        assert_eq!(store.read().unwrap().auth_bytes(), Some(OLD));
    }
    #[test]
    fn unsupported_payloads_are_never_written() {
        let (_root, store, vault) = fixture(Some(OLD));
        let before = store.read().unwrap();
        for payload in [
            br#"{"OPENAI_API_KEY":"sk-secret"}"#.as_slice(),
            br#"{"tokens":{"id_token":"","access_token":"a","refresh_token":"r"}}"#,
            br#"{"auth_mode":"apikey","tokens":{}}"#,
            b"not json",
        ] {
            assert!(matches!(
                store.replace(payload, &before.generation, &vault),
                Err(CodexStoreError::UnsupportedAuth)
            ));
        }
        assert_eq!(store.read().unwrap().auth_bytes(), Some(OLD));
    }
    #[test]
    fn config_changes_do_not_block_reading_the_login() {
        let (_root, store, vault) = fixture(Some(OLD));
        // Codex writes config.toml itself (trusted projects, model changes).
        fs::write(
            store.context.home().join("config.toml"),
            b"model = \"other\"\n[projects.'C:\\\\x']\ntrust_level = \"trusted\"\n",
        )
        .unwrap();
        let before = store.read().unwrap();
        assert!(store.replace(NEW, &before.generation, &vault).is_ok());
    }
    #[test]
    fn legacy_two_step_journal_is_discarded() {
        let (_root, store, vault) = fixture(Some(OLD));
        assert!(!store.discard_legacy_journal(&vault).unwrap());
        vault
            .save(&store.journal_name(), &serde_json::json!({"schema":1}))
            .unwrap();
        assert!(store.discard_legacy_journal(&vault).unwrap());
        assert!(
            vault
                .load::<serde_json::Value>(&store.journal_name())
                .unwrap()
                .is_none()
        );
        assert_eq!(store.read().unwrap().auth_bytes(), Some(OLD));
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_auth_is_denied_without_following_target() {
        let (root, store, vault) = fixture(None);
        let target = root.path().join("elsewhere.json");
        fs::write(&target, OLD).unwrap();
        std::os::unix::fs::symlink(&target, store.context.home().join("auth.json")).unwrap();
        assert!(store.read().is_err());
        let _ = vault;
        assert_eq!(fs::read(target).unwrap(), OLD);
    }
}
