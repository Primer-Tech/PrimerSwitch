use super::{CodexFileContext, CodexResult, CodexStoreError, CodexWriteGuard, StoreGeneration};
use crate::Vault;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
#[derive(Clone, Debug)]
pub struct PreparedCodexSwitch {
    context: String,
    transaction: String,
}
#[derive(Clone, Debug)]
pub struct PendingCodexSwitch {
    context: String,
    transaction: String,
    pub generation: StoreGeneration,
}
#[derive(Debug)]
pub enum CodexRecovery {
    Clean,
    CanceledPreparation,
    NeedsVerification(PendingCodexSwitch),
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Stage {
    Prepared,
    Written,
    Committed,
}
/// Provider-specific encrypted journal contains both exact opaque snapshots. No
/// plaintext backup, cross-account merge, or unknown-field reconstruction occurs.
#[derive(Serialize, Deserialize)]
struct Journal {
    schema: u32,
    context: String,
    transaction: String,
    stage: Stage,
    outgoing_generation: StoreGeneration,
    outgoing: Option<Vec<u8>>,
    incoming: Vec<u8>,
    written_generation: Option<StoreGeneration>,
    adopted: Option<Vec<u8>>,
}
impl Drop for Journal {
    fn drop(&mut self) {
        if let Some(bytes) = &mut self.outgoing {
            bytes.zeroize();
        }
        self.incoming.zeroize();
        if let Some(bytes) = &mut self.adopted {
            bytes.zeroize();
        }
    }
}

pub struct CodexFileStore {
    context: CodexFileContext,
    guard: Arc<dyn CodexWriteGuard>,
}
impl CodexFileStore {
    pub fn new(context: CodexFileContext, guard: Arc<dyn CodexWriteGuard>) -> Self {
        Self { context, guard }
    }
    pub fn context(&self) -> &CodexFileContext {
        &self.context
    }
    /// Read-only import still revalidates context. Runtime separately applies a
    /// quiescence guard before verification/refresh and before accepting a record.
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
    pub fn check_quiescent(&self) -> CodexResult<()> {
        self.context.revalidate()?;
        self.guard.check(&self.context)
    }
    pub fn prepare_switch(
        &self,
        auth: &[u8],
        expected: &StoreGeneration,
        vault: &Vault,
    ) -> CodexResult<PreparedCodexSwitch> {
        validate_auth(auth)?;
        let _lock = vault.context_lock(self.context.context_id())?;
        self.check_quiescent()?;
        if self.load_journal(vault)?.is_some() {
            return Err(CodexStoreError::ReconciliationRequired);
        }
        let current = self.read()?;
        if &current.generation != expected {
            return Err(CodexStoreError::ExternalChange);
        }
        if let Some(bytes) = current.auth_bytes() {
            validate_auth(bytes)?;
        }
        let mut random = [0u8; 16];
        OsRng.fill_bytes(&mut random);
        let transaction = super::generation::digest(&random);
        let journal = Journal {
            schema: 1,
            context: self.context.context_id().to_owned(),
            transaction: transaction.clone(),
            stage: Stage::Prepared,
            outgoing_generation: current.generation.clone(),
            outgoing: current.auth_bytes().map(<[u8]>::to_vec),
            incoming: auth.to_vec(),
            written_generation: None,
            adopted: None,
        };
        self.check_quiescent()?;
        if self.read()?.generation != journal.outgoing_generation {
            return Err(CodexStoreError::ExternalChange);
        }
        vault.save(&self.journal_name(), &journal)?;
        self.check_quiescent()?;
        if self.read()?.generation != journal.outgoing_generation {
            return Err(CodexStoreError::ExternalChange);
        }
        Ok(PreparedCodexSwitch {
            context: journal.context.clone(),
            transaction,
        })
    }
    pub fn commit_file(
        &self,
        prepared: &PreparedCodexSwitch,
        vault: &Vault,
    ) -> CodexResult<PendingCodexSwitch> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let mut journal = self
            .load_journal(vault)?
            .ok_or(CodexStoreError::InvalidReceipt)?;
        self.receipt(&prepared.context, &prepared.transaction, &journal)?;
        if journal.stage != Stage::Prepared {
            return Err(CodexStoreError::InvalidReceipt);
        }
        self.check_quiescent()?;
        if self.read()?.generation != journal.outgoing_generation {
            return Err(CodexStoreError::ExternalChange);
        }
        // The native rename is atomic, but not a kernel-wide conditional CAS. The
        // preceding generation check and repeated process guard bound the window.
        let current = self.replace_checked(
            &journal.incoming,
            &journal.outgoing_generation,
            journal.outgoing.is_none(),
        )?;
        if current.auth_bytes() != Some(journal.incoming.as_slice()) {
            return Err(CodexStoreError::ExternalChange);
        }
        journal.stage = Stage::Written;
        journal.written_generation = Some(current.generation.clone());
        vault.save(&self.journal_name(), &journal)?;
        self.check_quiescent()?;
        if self.read()?.generation != current.generation {
            return Err(CodexStoreError::ExternalChange);
        }
        Ok(PendingCodexSwitch {
            context: journal.context.clone(),
            transaction: journal.transaction.clone(),
            generation: current.generation,
        })
    }
    /// Call ONLY after independent provider identity/owner verification, durable
    /// saved-record rotation adoption, and shutdown of the owned refreshing client.
    /// The observed generation binds those verified bytes across the final await.
    pub fn finish_verified(
        &self,
        pending: &PendingCodexSwitch,
        observed_generation: &StoreGeneration,
        vault: &Vault,
    ) -> CodexResult<CodexAuthSnapshot> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let mut journal = self
            .load_journal(vault)?
            .ok_or(CodexStoreError::InvalidReceipt)?;
        self.receipt(&pending.context, &pending.transaction, &journal)?;
        if journal.stage != Stage::Written
            || journal.written_generation.as_ref() != Some(&pending.generation)
        {
            return Err(CodexStoreError::InvalidReceipt);
        }
        self.check_quiescent()?;
        let current = self.read()?;
        if &current.generation != observed_generation {
            return Err(CodexStoreError::ExternalChange);
        }
        let bytes = current
            .auth_bytes()
            .ok_or(CodexStoreError::UnsupportedAuth)?;
        validate_auth(bytes)?;
        // A Codex-owned refresh can change every token and its native generation.
        // Runtime proved its owner; preserve that exact payload as a new snapshot.
        journal.adopted = Some(bytes.to_vec());
        journal.stage = Stage::Committed;
        journal.written_generation = Some(current.generation.clone());
        vault.save(&self.journal_name(), &journal)?;
        self.check_quiescent()?;
        if self.read()?.generation != current.generation {
            return Err(CodexStoreError::ExternalChange);
        }
        vault.remove(&self.journal_name())?;
        Ok(current)
    }
    /// Explicit Rust-only reconciliation of the currently selected FILE login.
    /// Call ONLY after the user requests Import current, independent active backend
    /// identity/workspace and ordinary-usage proof succeeds, owner-bound opaque auth
    /// (including any rotation) is durably adopted in the saved record, and the owned
    /// child has shut down. Claims alone, journal recovery, startup discovery, and
    /// failed provider verification MUST NOT call this method. The observed native
    /// generation binds that proof across awaits. This method never writes auth.json,
    /// restores outgoing tokens, or erases an unauthenticated/damaged journal.
    pub fn reconcile_verified_current(
        &self,
        observed_generation: &StoreGeneration,
        vault: &Vault,
    ) -> CodexResult<CodexAuthSnapshot> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let mut journal = self.load_journal(vault)?;
        self.check_quiescent()?;
        let current = self.read()?;
        if &current.generation != observed_generation {
            return Err(CodexStoreError::ExternalChange);
        }
        let bytes = current
            .auth_bytes()
            .ok_or(CodexStoreError::UnsupportedAuth)?;
        validate_auth(bytes)?;
        if let Some(journal) = &mut journal {
            journal.adopted = Some(bytes.to_vec());
            journal.stage = Stage::Committed;
            journal.written_generation = Some(current.generation.clone());
            vault.save(&self.journal_name(), journal)?;
        }
        self.check_quiescent()?;
        if self.read()?.generation != current.generation {
            return Err(CodexStoreError::ExternalChange);
        }
        if journal.is_some() {
            vault.remove(&self.journal_name())?;
        }
        Ok(current)
    }
    /// Recovery never writes auth.json or replays outgoing tokens. An uncertain
    /// candidate requires fresh provider verification; external writes retain the
    /// encrypted journal and return a conflict for explicit reconciliation.
    pub fn recover(&self, vault: &Vault) -> CodexResult<CodexRecovery> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let Some(mut journal) = self.load_journal(vault)? else {
            return Ok(CodexRecovery::Clean);
        };
        self.check_quiescent()?;
        let current = self.read()?;
        match journal.stage {
            Stage::Prepared if current.generation == journal.outgoing_generation => {
                self.check_quiescent()?;
                if self.read()?.generation != current.generation {
                    return Err(CodexStoreError::ExternalChange);
                }
                vault.remove(&self.journal_name())?;
                Ok(CodexRecovery::CanceledPreparation)
            }
            Stage::Prepared if current.auth_bytes() == Some(journal.incoming.as_slice()) => {
                // Crash between replacement and journal update: cannot claim a
                // verified switch or distinguish a same-byte external writer.
                journal.stage = Stage::Written;
                journal.written_generation = Some(current.generation.clone());
                vault.save(&self.journal_name(), &journal)?;
                self.check_quiescent()?;
                if self.read()?.generation != current.generation {
                    return Err(CodexStoreError::ExternalChange);
                }
                Ok(CodexRecovery::NeedsVerification(PendingCodexSwitch {
                    context: journal.context.clone(),
                    transaction: journal.transaction.clone(),
                    generation: current.generation,
                }))
            }
            Stage::Written
                if journal.written_generation.as_ref() == Some(&current.generation)
                    && current.auth_bytes() == Some(journal.incoming.as_slice()) =>
            {
                Ok(CodexRecovery::NeedsVerification(PendingCodexSwitch {
                    context: journal.context.clone(),
                    transaction: journal.transaction.clone(),
                    generation: current.generation,
                }))
            }
            Stage::Committed
                if journal.written_generation.as_ref() == Some(&current.generation)
                    && current.auth_bytes() == journal.adopted.as_deref() =>
            {
                self.check_quiescent()?;
                if self.read()?.generation != current.generation {
                    return Err(CodexStoreError::ExternalChange);
                }
                vault.remove(&self.journal_name())?;
                Ok(CodexRecovery::Clean)
            }
            _ => Err(CodexStoreError::ExternalChange),
        }
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
        // Stage first, then compare immediately before replacement. If originally
        // absent, staging changes parent timestamps; the dedicated pre-stage read
        // already proved absence and this compare checks continued absence.
        self.check_quiescent()?;
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
            if unsafe {
                MoveFileExW(
                    source.as_ptr(),
                    dest.as_ptr(),
                    (if originally_absent {
                        0
                    } else {
                        MOVEFILE_REPLACE_EXISTING
                    }) | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(crate::PlatformError::Io.into());
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
    fn receipt(&self, context: &str, transaction: &str, journal: &Journal) -> CodexResult<()> {
        if context != self.context.context_id()
            || context != journal.context
            || transaction != journal.transaction
        {
            return Err(CodexStoreError::InvalidReceipt);
        }
        Ok(())
    }
    fn load_journal(&self, vault: &Vault) -> CodexResult<Option<Journal>> {
        let value: Option<Journal> = vault.load(&self.journal_name())?;
        if let Some(journal) = &value {
            if journal.schema != 1
                || journal.context != self.context.context_id()
                || journal.transaction.len() != 64
                || !journal.transaction.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(CodexStoreError::InvalidReceipt);
            }
            validate_auth(&journal.incoming)?;
            if let Some(bytes) = &journal.outgoing {
                validate_auth(bytes)?;
            }
            if let Some(bytes) = &journal.adopted {
                validate_auth(bytes)?;
            }
        }
        Ok(value)
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
        CodexStorageMode, CodexWatchedSource, PINNED_CODEX_SCHEMA, PINNED_CODEX_VERSION,
    };
    use std::{
        fs,
        path::Path,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    const OLD: &[u8] = br#"{ "OPENAI_API_KEY":null,"tokens":{"id_token":"old-id-secret","access_token":"old-access-secret","refresh_token":"old-refresh-secret","account_id":null},"future":{"oldAccountOnly":true}}"#;
    const NEW: &[u8] = br#"{ "auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"new-id-secret","access_token":"new-access-secret","refresh_token":"new-refresh-secret","account_id":"new-workspace"},"future":{"incomingOnly":[1,2,3]}}"#;
    const ROTATED: &[u8] = br#"{ "tokens":{"id_token":"rotated-id-secret","access_token":"rotated-access-secret","refresh_token":"rotated-refresh-secret"},"future":{"rotationExtension":true}}"#;
    type GuardAction = Box<dyn FnMut(usize, &CodexFileContext) -> CodexResult<()> + Send>;
    struct Guard {
        calls: AtomicUsize,
        action: Mutex<GuardAction>,
    }
    impl Guard {
        fn new(
            action: impl FnMut(usize, &CodexFileContext) -> CodexResult<()> + Send + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                action: Mutex::new(Box::new(action)),
            })
        }
    }
    impl CodexWriteGuard for Guard {
        fn check(&self, context: &CodexFileContext) -> CodexResult<()> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            (self.action.lock().unwrap())(call, context)
        }
    }
    fn evidence(root: &Path, home: &Path) -> CodexContextEvidence {
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
                fs::write(&path, b"fake-pinned-native-executable").unwrap();
            }
            let (receipt, _) = CodexWatchedSource::capture(role, path).unwrap();
            sources.push(receipt);
        }
        sources.push(CodexWatchedSource::default_session());
        CodexContextEvidence {
            version: PINNED_CODEX_VERSION.into(),
            schema: PINNED_CODEX_SCHEMA.into(),
            storage: CodexStorageMode::File,
            provider: CodexProviderMode::DefaultOpenAi,
            policy: CodexPolicyState::Unrestricted,
            sources,
        }
    }
    fn fixture(guard: Arc<dyn CodexWriteGuard>) -> (tempfile::TempDir, CodexFileStore, Vault) {
        let root = crate::files::test_root();
        let home = root.path().join("codex-home");
        fs::create_dir(&home).unwrap();
        crate::files::atomic_write(&home.join("auth.json"), OLD).unwrap();
        let context =
            CodexFileContext::qualify(home.clone(), evidence(root.path(), &home)).unwrap();
        let vault = Vault::with_key(root.path().join("app-vault"), [47; 32]).unwrap();
        (root, CodexFileStore::new(context, guard), vault)
    }
    fn quiet() -> Arc<Guard> {
        Guard::new(|_, _| Ok(()))
    }
    fn assert_original(store: &CodexFileStore) {
        assert_eq!(store.read().unwrap().auth_bytes(), Some(OLD));
    }

    #[test]
    fn exact_bytes_unknown_fields_and_other_home_files_survive_selection() {
        let (_root, store, vault) = fixture(quiet());
        let home = store.context.home();
        let original_config = fs::read(home.join("config.toml")).unwrap();
        for name in ["history.jsonl", "mcp.json", "profiles.json"] {
            fs::write(home.join(name), b"unrelated-sentinel").unwrap();
        }
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        assert_original(&store);
        let pending = store.commit_file(&prepared, &vault).unwrap();
        let snapshot = store
            .finish_verified(&pending, &pending.generation, &vault)
            .unwrap();
        assert_eq!(snapshot.auth_bytes(), Some(NEW));
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
        assert!(
            !String::from_utf8_lossy(snapshot.auth_bytes().unwrap()).contains("oldAccountOnly")
        );
        for name in ["history.jsonl", "mcp.json", "profiles.json"] {
            assert_eq!(fs::read(home.join(name)).unwrap(), b"unrelated-sentinel");
        }
        assert_eq!(fs::read(home.join("config.toml")).unwrap(), original_config);
        assert!(matches!(
            store.recover(&vault).unwrap(),
            CodexRecovery::Clean
        ));
        assert!(!format!("{snapshot:?}").contains("secret"));
    }
    #[test]
    fn snapshots_and_journal_are_authenticated_encrypted_and_provider_specific() {
        let (root, store, vault) = fixture(quiet());
        vault
            .save("runtime-state", &serde_json::json!({"claude":"unchanged"}))
            .unwrap();
        let claude_path = root.path().join("app-vault/runtime-state.vault");
        let claude = fs::read(&claude_path).unwrap();
        let before = store.read().unwrap();
        store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        let path = root
            .path()
            .join("app-vault")
            .join(format!("{}.vault", store.journal_name()));
        let mut bytes = fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("secret"));
        bytes[40] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            store.recover(&vault),
            Err(CodexStoreError::Storage(
                crate::PlatformError::Authentication
            ))
        ));
        assert_original(&store);
        assert_eq!(fs::read(claude_path).unwrap(), claude);
    }
    #[test]
    fn identical_bytes_replacement_is_a_different_generation() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), OLD).unwrap();
        assert_ne!(store.read().unwrap().generation, before.generation);
        assert!(matches!(
            store.prepare_switch(NEW, &before.generation, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        assert_original(&store);
    }
    #[test]
    fn process_visibility_and_running_client_denials_never_write() {
        for visibility in [false, true] {
            let (_root, store, vault) = fixture(Guard::new(move |_, _| {
                Err(if visibility {
                    CodexStoreError::ProcessVisibility
                } else {
                    CodexStoreError::RunningClients
                })
            }));
            let before = store.read().unwrap();
            assert!(
                store
                    .prepare_switch(NEW, &before.generation, &vault)
                    .is_err()
            );
            assert_original(&store);
            assert!(store.load_journal(&vault).unwrap().is_none());
        }
    }
    #[test]
    fn process_appearance_after_write_retains_reconciliation_and_never_rolls_back() {
        let (_root, store, vault) = fixture(Guard::new(|call, _| {
            if call == 6 {
                Err(CodexStoreError::RunningClients)
            } else {
                Ok(())
            }
        }));
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        assert!(matches!(
            store.commit_file(&prepared, &vault),
            Err(CodexStoreError::RunningClients)
        ));
        assert_eq!(store.read().unwrap().auth_bytes(), Some(NEW));
        assert!(matches!(
            store.recover(&vault).unwrap(),
            CodexRecovery::NeedsVerification(_)
        ));
    }
    #[test]
    fn external_change_before_and_after_mutation_is_preserved() {
        for boundary in [3, 4, 6, 8] {
            let (_root, store, vault) = fixture(Guard::new(move |call, context| {
                if call == boundary {
                    crate::files::atomic_write(&context.home().join("auth.json"), ROTATED)?;
                }
                Ok(())
            }));
            let before = store.read().unwrap();
            let result = store
                .prepare_switch(NEW, &before.generation, &vault)
                .and_then(|prepared| store.commit_file(&prepared, &vault))
                .and_then(|pending| store.finish_verified(&pending, &pending.generation, &vault));
            assert!(
                matches!(result, Err(CodexStoreError::ExternalChange)),
                "boundary {boundary}"
            );
            assert_eq!(store.read().unwrap().auth_bytes(), Some(ROTATED));
            assert!(matches!(
                store.recover(&vault),
                Err(CodexStoreError::ExternalChange)
            ));
        }
    }
    #[test]
    fn explicit_verified_external_current_retires_journal_without_auth_mutation() {
        for after_write in [false, true] {
            let (_root, store, vault) = fixture(quiet());
            let before = store.read().unwrap();
            let prepared = store
                .prepare_switch(NEW, &before.generation, &vault)
                .unwrap();
            if after_write {
                store.commit_file(&prepared, &vault).unwrap();
            }
            crate::files::atomic_write(&store.context.home().join("auth.json"), ROTATED).unwrap();
            assert!(matches!(
                store.recover(&vault),
                Err(CodexStoreError::ExternalChange)
            ));
            let verified = store.read().unwrap();
            let adopted = store
                .reconcile_verified_current(&verified.generation, &vault)
                .unwrap();
            assert_eq!(adopted.auth_bytes(), Some(ROTATED));
            assert_eq!(adopted.generation, verified.generation);
            assert!(store.load_journal(&vault).unwrap().is_none());
            assert_eq!(store.read().unwrap().generation, verified.generation);
        }
    }
    #[test]
    fn explicit_reconciliation_without_journal_still_checks_managed_auth_and_generation() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        assert_eq!(
            store
                .reconcile_verified_current(&before.generation, &vault)
                .unwrap()
                .generation,
            before.generation
        );
        crate::files::atomic_write(&store.context.home().join("auth.json"), OLD).unwrap();
        assert!(matches!(
            store.reconcile_verified_current(&before.generation, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        crate::files::atomic_write(
            &store.context.home().join("auth.json"),
            br#"{"auth_mode":"apikey","OPENAI_API_KEY":"fixture-api-secret"}"#,
        )
        .unwrap();
        let api = store.read().unwrap();
        assert!(matches!(
            store.reconcile_verified_current(&api.generation, &vault),
            Err(CodexStoreError::UnsupportedAuth)
        ));
        assert!(store.load_journal(&vault).unwrap().is_none());
    }
    #[test]
    fn stale_generation_or_guard_denial_preserves_pending_journal() {
        for deny_guard in [false, true] {
            let (root, store, vault) = fixture(Guard::new(move |call, _| {
                if deny_guard && call == 4 {
                    Err(CodexStoreError::ProcessVisibility)
                } else {
                    Ok(())
                }
            }));
            let before = store.read().unwrap();
            store
                .prepare_switch(NEW, &before.generation, &vault)
                .unwrap();
            crate::files::atomic_write(&store.context.home().join("auth.json"), ROTATED).unwrap();
            let path = root
                .path()
                .join("app-vault")
                .join(format!("{}.vault", store.journal_name()));
            let encrypted_before = fs::read(&path).unwrap();
            let observed = if deny_guard {
                store.read().unwrap().generation
            } else {
                before.generation
            };
            assert!(matches!(
                store.reconcile_verified_current(&observed, &vault),
                Err(CodexStoreError::ProcessVisibility | CodexStoreError::ExternalChange)
            ));
            assert_eq!(fs::read(path).unwrap(), encrypted_before);
            assert_eq!(store.read().unwrap().auth_bytes(), Some(ROTATED));
        }
    }
    #[test]
    fn external_change_after_reconciliation_commit_keeps_authenticated_journal() {
        let (_root, store, vault) = fixture(Guard::new(|call, context| {
            if call == 5 {
                crate::files::atomic_write(&context.home().join("auth.json"), OLD)?;
            }
            Ok(())
        }));
        let before = store.read().unwrap();
        store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), ROTATED).unwrap();
        let verified = store.read().unwrap();
        assert!(matches!(
            store.reconcile_verified_current(&verified.generation, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        let journal = store.load_journal(&vault).unwrap().unwrap();
        assert!(journal.stage == Stage::Committed);
        assert_eq!(journal.adopted.as_deref(), Some(ROTATED));
        assert_eq!(store.read().unwrap().auth_bytes(), Some(OLD));
        assert!(matches!(
            store.recover(&vault),
            Err(CodexStoreError::ExternalChange)
        ));
    }
    #[test]
    fn reconciliation_authenticates_and_preserves_tampered_journal() {
        let (root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), ROTATED).unwrap();
        let path = root
            .path()
            .join("app-vault")
            .join(format!("{}.vault", store.journal_name()));
        let mut damaged = fs::read(&path).unwrap();
        damaged[40] ^= 1;
        fs::write(&path, &damaged).unwrap();
        let verified = store.read().unwrap();
        assert!(matches!(
            store.reconcile_verified_current(&verified.generation, &vault),
            Err(CodexStoreError::Storage(
                crate::PlatformError::Authentication
            ))
        ));
        assert_eq!(fs::read(path).unwrap(), damaged);
        assert_eq!(store.read().unwrap().generation, verified.generation);
    }
    #[test]
    fn stale_receipts_and_context_lock_cannot_write() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        let stale = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        store.recover(&vault).unwrap();
        let fresh = store
            .prepare_switch(ROTATED, &before.generation, &vault)
            .unwrap();
        assert!(matches!(
            store.commit_file(&stale, &vault),
            Err(CodexStoreError::InvalidReceipt)
        ));
        assert_original(&store);
        let lock = vault.context_lock(store.context.context_id()).unwrap();
        assert!(matches!(
            store.commit_file(&fresh, &vault),
            Err(CodexStoreError::Storage(crate::PlatformError::Busy))
        ));
        drop(lock);
        store.commit_file(&fresh, &vault).unwrap();
        assert_eq!(store.read().unwrap().auth_bytes(), Some(ROTATED));
    }
    #[test]
    fn encrypted_prepare_restart_cancels_without_touching_active_auth() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        let restarted = CodexFileStore::new(store.context.clone(), quiet());
        assert!(matches!(
            restarted.recover(&vault).unwrap(),
            CodexRecovery::CanceledPreparation
        ));
        assert_original(&restarted);
        assert_eq!(restarted.read().unwrap().generation, before.generation);
    }
    #[test]
    fn crash_between_atomic_replace_and_journal_update_requires_verification() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), NEW).unwrap();
        let CodexRecovery::NeedsVerification(pending) = store.recover(&vault).unwrap() else {
            panic!("must verify");
        };
        assert_eq!(store.read().unwrap().auth_bytes(), Some(NEW));
        store
            .finish_verified(&pending, &pending.generation, &vault)
            .unwrap();
    }
    #[test]
    fn same_byte_external_replacement_after_write_is_not_automatically_adopted() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        store.commit_file(&prepared, &vault).unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), NEW).unwrap();
        assert!(matches!(
            store.recover(&vault),
            Err(CodexStoreError::ExternalChange)
        ));
        assert_eq!(store.read().unwrap().auth_bytes(), Some(NEW));
    }
    #[test]
    fn owner_verified_rotation_is_adopted_exactly_and_stale_observation_rejected() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        let pending = store.commit_file(&prepared, &vault).unwrap();
        crate::files::atomic_write(&store.context.home().join("auth.json"), ROTATED).unwrap();
        assert!(matches!(
            store.finish_verified(&pending, &pending.generation, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        let observed = store.read().unwrap();
        let final_snapshot = store
            .finish_verified(&pending, &observed.generation, &vault)
            .unwrap();
        assert_eq!(final_snapshot.auth_bytes(), Some(ROTATED));
    }
    #[test]
    fn committed_cleanup_restart_never_replays_outgoing_tokens() {
        let (_root, store, vault) = fixture(Guard::new(|call, _| {
            if call == 8 {
                Err(CodexStoreError::RunningClients)
            } else {
                Ok(())
            }
        }));
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        let pending = store.commit_file(&prepared, &vault).unwrap();
        assert!(
            store
                .finish_verified(&pending, &pending.generation, &vault)
                .is_err()
        );
        assert!(matches!(
            store.recover(&vault).unwrap(),
            CodexRecovery::Clean
        ));
        assert_eq!(store.read().unwrap().auth_bytes(), Some(NEW));
    }
    #[test]
    fn unsupported_payloads_and_corrupt_existing_json_are_preserved() {
        let (_root, store, vault) = fixture(quiet());
        let before = store.read().unwrap();
        for bytes in [b"not-json".as_slice(), br#"{"auth_mode":"apikey","OPENAI_API_KEY":"secret"}"#, br#"{"auth_mode":"chatgptAuthTokens","tokens":{}}"#,
            br#"{"tokens":{"id_token":"i","access_token":"a","refresh_token":"r"},"agent_identity":"jwt"}"#] {
            assert!(matches!(store.prepare_switch(bytes, &before.generation, &vault), Err(CodexStoreError::UnsupportedAuth)));
        }
        assert_original(&store);
        crate::files::atomic_write(&store.context.home().join("auth.json"), b"{corrupt").unwrap();
        let corrupt = store.read().unwrap();
        assert!(matches!(
            store.prepare_switch(NEW, &corrupt.generation, &vault),
            Err(CodexStoreError::UnsupportedAuth)
        ));
        assert_eq!(
            store.read().unwrap().auth_bytes(),
            Some(b"{corrupt".as_slice())
        );
    }
    #[test]
    fn disappearance_of_existing_auth_during_staging_is_a_conflict() {
        let (_root, store, vault) = fixture(Guard::new(|call, context| {
            if call == 5 {
                crate::files::remove(&context.home().join("auth.json"))?;
            }
            Ok(())
        }));
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        assert!(matches!(
            store.commit_file(&prepared, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        assert!(store.read().unwrap().auth_bytes().is_none());
        assert!(store.load_journal(&vault).unwrap().is_some());
    }
    #[test]
    fn external_login_created_during_initial_absent_staging_is_preserved() {
        let (_root, store, vault) = fixture(Guard::new(|call, context| {
            if call == 5 {
                crate::files::atomic_write(&context.home().join("auth.json"), ROTATED)?;
            }
            Ok(())
        }));
        crate::files::remove(&store.context.home().join("auth.json")).unwrap();
        let before = store.read().unwrap();
        let prepared = store
            .prepare_switch(NEW, &before.generation, &vault)
            .unwrap();
        assert!(matches!(
            store.commit_file(&prepared, &vault),
            Err(CodexStoreError::ExternalChange)
        ));
        assert_eq!(store.read().unwrap().auth_bytes(), Some(ROTATED));
    }
    #[test]
    fn watched_directory_project_markers_are_revalidated() {
        let root = crate::files::test_root();
        let marker = root.path().join(".git");
        fs::create_dir(&marker).unwrap();
        let (receipt, bytes) =
            CodexWatchedSource::capture(CodexSourceRole::Project, marker.clone()).unwrap();
        assert!(bytes.is_none());
        assert!(receipt.is_directory());
        receipt.revalidate().unwrap();
        fs::rename(&marker, root.path().join("old-project-marker")).unwrap();
        fs::create_dir(&marker).unwrap();
        assert!(matches!(
            receipt.revalidate(),
            Err(CodexStoreError::ExternalChange)
        ));
    }
    #[test]
    fn missing_active_auth_is_supported_without_touching_the_home() {
        let (_root, store, vault) = fixture(quiet());
        crate::files::remove(&store.context.home().join("auth.json")).unwrap();
        let absent = store.read().unwrap();
        assert!(absent.auth_bytes().is_none());
        let prepared = store
            .prepare_switch(NEW, &absent.generation, &vault)
            .unwrap();
        let pending = store.commit_file(&prepared, &vault).unwrap();
        store
            .finish_verified(&pending, &pending.generation, &vault)
            .unwrap();
        assert_eq!(store.read().unwrap().auth_bytes(), Some(NEW));
    }
    #[test]
    fn context_modes_version_unknown_policy_and_incomplete_layers_are_denied() {
        let root = crate::files::test_root();
        let home = root.path().join("home");
        fs::create_dir(&home).unwrap();
        for mutate in 0..10 {
            let mut proof = evidence(root.path(), &home);
            match mutate {
                0 => proof.version = "0.159.0".into(),
                1 => proof.schema = "unknown".into(),
                2 => proof.storage = CodexStorageMode::Auto,
                3 => proof.storage = CodexStorageMode::Keyring,
                4 => proof.storage = CodexStorageMode::Ephemeral,
                5 => proof.provider = CodexProviderMode::Custom,
                6 => proof.policy = CodexPolicyState::Unknown,
                7 => {
                    proof.sources.remove(2);
                }
                8 => proof.policy = CodexPolicyState::Restricted,
                _ => {
                    let wrong = root.path().join("wrong-user-config.toml");
                    fs::write(&wrong, b"model = \"fixture-model\"").unwrap();
                    let (receipt, _) =
                        CodexWatchedSource::capture(CodexSourceRole::User, wrong).unwrap();
                    proof
                        .sources
                        .retain(|source| source.role() != CodexSourceRole::User);
                    proof.sources.push(receipt);
                }
            }
            assert!(matches!(
                CodexFileContext::qualify(home.clone(), proof),
                Err(CodexStoreError::UnsupportedContext)
            ));
        }
    }
    #[test]
    fn authoritative_source_replacement_or_creation_invalidates_receipt() {
        let (root, store, _vault) = fixture(quiet());
        let executable = root.path().join("sources/Executable");
        crate::files::atomic_write(&executable, b"fake-pinned-native-executable").unwrap();
        assert!(matches!(store.read(), Err(CodexStoreError::ExternalChange)));
        let (root, store, _vault) = fixture(quiet());
        fs::write(root.path().join("sources/Managed"), b"forced policy").unwrap();
        assert!(matches!(
            store.check_quiescent(),
            Err(CodexStoreError::ExternalChange)
        ));
    }
    #[cfg(unix)]
    #[test]
    fn symlink_auth_or_context_is_denied_without_following_target() {
        use std::os::unix::fs::symlink;
        let (root, store, _vault) = fixture(quiet());
        let outside = root.path().join("outside");
        fs::write(&outside, OLD).unwrap();
        fs::remove_file(store.context.home().join("auth.json")).unwrap();
        symlink(&outside, store.context.home().join("auth.json")).unwrap();
        assert!(matches!(
            store.read(),
            Err(CodexStoreError::Storage(crate::PlatformError::UnsafePath))
        ));
        assert_eq!(fs::read(outside).unwrap(), OLD);
    }
}
