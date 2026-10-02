//! Application-owned Linux master keys. No provider/CLI credentials enter D-Bus.
//! Missing, locked or ambiguous entries are never repaired by rotating a key.
use crate::{PlatformError, Result, files};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use zeroize::Zeroizing;

const SERVICE: &str = "com.primertech.primerswitch.vault.v1";
const MARKER: &str = "master-key.secret-service";
const CONTENT_TYPE: &str = "application/octet-stream";
const GENERIC_SCHEMA: &str = "org.freedesktop.Secret.Generic";

fn diagnostic_stage(stage: &'static str) {
    #[cfg(test)]
    if std::env::var("PRIMERSWITCH_ISOLATED_KEYRING_TEST").as_deref() == Ok("1") {
        eprintln!("PRIMERSWITCH_STORAGE_STAGE={stage}");
    }
    #[cfg(not(test))]
    let _ = stage;
}

#[cfg(target_os = "linux")]
fn native_result<T>(
    result: std::result::Result<T, secret_service::Error>,
    stage: &'static str,
) -> Result<T> {
    result.map_err(|error| {
        diagnostic_stage(stage);
        #[cfg(test)]
        if std::env::var("PRIMERSWITCH_ISOLATED_KEYRING_TEST").as_deref() == Ok("1") {
            let category = match &error {
                secret_service::Error::Crypto(_) => "Crypto",
                secret_service::Error::Zbus(zbus::Error::MethodError(name, _, _)) => {
                    match name.as_str() {
                        "org.freedesktop.DBus.Error.AccessDenied" => "dbus_access_denied",
                        "org.freedesktop.DBus.Error.UnknownMethod" => "dbus_unknown_method",
                        "org.freedesktop.DBus.Error.UnknownObject" => "dbus_unknown_object",
                        "org.freedesktop.DBus.Error.ServiceUnknown" => "dbus_service_unknown",
                        "org.freedesktop.Secret.Error.NoSession" => "secret_no_session",
                        "org.freedesktop.Secret.Error.IsLocked" => "secret_locked",
                        _ => "other_dbus_error",
                    }
                }
                secret_service::Error::Zbus(_) => "Zbus",
                secret_service::Error::ZbusFdo(_) => "ZbusFdo",
                secret_service::Error::Zvariant(_) => "Zvariant",
                secret_service::Error::Locked => "Locked",
                secret_service::Error::NoResult => "NoResult",
                secret_service::Error::Prompt => "Prompt",
                secret_service::Error::PromptDisconnected => "PromptDisconnected",
                secret_service::Error::Unavailable => "Unavailable",
                _ => "Other",
            };
            eprintln!("PRIMERSWITCH_STORAGE_ERROR_CATEGORY={category}");
        }
        #[cfg(not(test))]
        let _ = error;
        PlatformError::KeyUnavailable
    })
}

fn namespace(directory: &Path) -> Result<String> {
    files::check_path(directory)?;
    let canonical = directory.canonicalize()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical.as_os_str().as_encoded_bytes())
    ))
}

fn attributes(namespace: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", SERVICE), ("vault", namespace)])
}

fn check_attributes(actual: &HashMap<String, String>, namespace: &str) -> Result<()> {
    // GNOME46.1 reloads a schema-less item from its type0 compatibility record
    // as Generic, then exposes that schema in Attributes. Only this documented
    // extra metadata is accepted; ownership still requires both exact fields.
    // Sources: GNOME46.1 pkcs11/secret-store/{gkm-secret-binary.c,
    // gkm-secret-compat.c,gkm-secret-fields.c} on github.com/GNOME/gnome-keyring.
    let schema = actual.get("xdg:schema");
    if actual.len() != 2 + usize::from(schema.is_some()) {
        diagnostic_stage("key_attribute_count");
        return Err(PlatformError::KeyUnavailable);
    }
    if schema.is_some_and(|value| value != GENERIC_SCHEMA) {
        diagnostic_stage("key_schema_value");
        return Err(PlatformError::KeyUnavailable);
    }
    if !attributes(namespace)
        .into_iter()
        .all(|(key, value)| actual.get(key).is_some_and(|v| v == value))
    {
        diagnostic_stage("key_attribute_ownership");
        return Err(PlatformError::KeyUnavailable);
    }
    Ok(())
}

fn check_content_type(content_type: &str) -> Result<()> {
    // GNOME Keyring46.1 discards the incoming content type and unconditionally
    // returns text/plain even for arbitrary binary secret values. This is
    // transport metadata, not an encoding instruction: never decode the key.
    // https://raw.githubusercontent.com/GNOME/gnome-keyring/46.1/daemon/dbus/gkd-secret-secret.c
    if !matches!(content_type, CONTENT_TYPE | "text/plain") {
        diagnostic_stage("key_content_type_mismatch");
        return Err(PlatformError::KeyUnavailable);
    }
    Ok(())
}

fn check_search_result(unlocked: usize, locked: usize) -> Result<bool> {
    if locked != 0 || unlocked > 1 {
        return Err(PlatformError::KeyUnavailable);
    }
    Ok(unlocked == 1)
}

fn check_persistent_default(default: &str, session: Option<&str>) -> Result<()> {
    if default == "/" || session.is_some_and(|session| session == default) {
        return Err(PlatformError::KeyUnavailable);
    }
    Ok(())
}

trait KeyStore {
    /// Return precisely one unlocked, correctly attributed, binary 32-byte key,
    /// or None when no entry exists. Never implicitly unlock or prompt.
    fn read(&self, namespace: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    /// Check known availability before persisting a creation intent. This lets
    /// a user unlock an initially locked store and retry without losing a key.
    fn prepare_create(&self) -> Result<()>;
    /// Create without replacement, then confirm the sole entry by reading it.
    fn create(&self, namespace: &str, key: &[u8]) -> Result<()>;
}

fn fingerprint(key: &[u8]) -> Vec<u8> {
    format!(
        "PrimerSwitch Secret Service master key v1\n{:x}\n",
        Sha256::digest(key)
    )
    .into_bytes()
}

fn load_key(directory: &Path, encrypted_files: bool, store: &impl KeyStore) -> Result<[u8; 32]> {
    let namespace = namespace(directory)?;
    let marker_path = directory.join(MARKER);
    let marker = files::read_optional(&marker_path)?;
    if let Some(value) = store.read(&namespace)? {
        let key: [u8; 32] = value
            .as_slice()
            .try_into()
            .map_err(|_| PlatformError::KeyUnavailable)?;
        if let Some(marker) = marker {
            if marker != fingerprint(&key) {
                diagnostic_stage("key_marker_mismatch");
                return Err(PlatformError::KeyUnavailable);
            }
        } else {
            // An existing native key is safe to adopt; never replace it.
            files::atomic_write(&marker_path, &fingerprint(&key))?;
        }
        return Ok(key);
    }
    if encrypted_files || marker.is_some() {
        return Err(PlatformError::KeyLost);
    }
    store.prepare_create()?;
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng
        .try_fill_bytes(key.as_mut())
        .map_err(|_| PlatformError::KeyUnavailable)?;
    // Durable creation intent precedes the D-Bus mutation. If the outcome is
    // unknown, reopen either adopts this exact key or fails without minting one.
    files::atomic_write(&marker_path, &fingerprint(key.as_ref()))?;
    store.create(&namespace, key.as_ref())?;
    let confirmed = store.read(&namespace)?.ok_or_else(|| {
        diagnostic_stage("key_confirmation_missing");
        PlatformError::KeyUnavailable
    })?;
    if confirmed.as_slice() != key.as_ref() {
        diagnostic_stage("key_confirmation_mismatch");
        return Err(PlatformError::KeyUnavailable);
    }
    Ok(*key)
}

#[cfg(target_os = "linux")]
pub(crate) fn check_directory(directory: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    files::check_path(directory)?;
    let uid = unsafe { libc::geteuid() };
    for ancestor in directory.ancestors() {
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(PlatformError::Io),
        };
        if !metadata.is_dir() || (metadata.uid() != uid && metadata.uid() != 0) {
            return Err(PlatformError::UnsafePath);
        }
        // Shared sticky temporary directories are permitted; the vault itself
        // must be ours. Other writable ancestor directories cannot safely bind
        // a native key namespace to a persistent filesystem location.
        if (ancestor == directory && metadata.uid() != uid)
            || (ancestor != directory
                && metadata.mode() & 0o022 != 0
                && metadata.mode() & 0o1000 == 0)
        {
            return Err(PlatformError::UnsafePath);
        }
    }
    Ok(())
}

fn bounded_operation<T: Send + 'static>(
    timeout: std::time::Duration,
    operation: impl FnOnce() -> Result<T> + Send + 'static,
    cancel: impl FnOnce(),
) -> Result<T> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _ = sender.send(operation());
    });
    let result = receiver.recv_timeout(timeout).unwrap_or_else(|_| {
        diagnostic_stage("operation_timeout");
        Err(PlatformError::KeyUnavailable)
    });
    // Closing the shared D-Bus connection also ends a Prompt.Completed signal
    // iterator. Upstream maps that termination to PromptDisconnected. Join the
    // worker before returning so a timed-out key mutation cannot run detached.
    cancel();
    if worker.join().is_err() {
        return Err(PlatformError::KeyUnavailable);
    }
    result
}

#[cfg(target_os = "linux")]
pub(crate) fn load_native_key(directory: &Path, encrypted_files: bool) -> Result<[u8; 32]> {
    use secret_service::{EncryptionType, blocking::SecretService};
    use std::time::Duration;
    let connection = zbus::blocking::connection::Builder::session()
        .map_err(|_| {
            diagnostic_stage("bus_builder");
            PlatformError::KeyUnavailable
        })?
        .method_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| {
            diagnostic_stage("bus_connect");
            PlatformError::KeyUnavailable
        })?;
    let cancellation = connection.clone();
    let directory = directory.to_path_buf();
    let key = bounded_operation(
        Duration::from_secs(20),
        move || {
            // DH is mandatory. A rejected encrypted session fails; no plain retry.
            let service = native_result(
                SecretService::connect_with_existing(EncryptionType::Dh, connection),
                "encrypted_session",
            )?;
            load_key(&directory, encrypted_files, &NativeStore(&service)).map(Zeroizing::new)
        },
        move || {
            let _ = cancellation.close();
        },
    )?;
    Ok(*key)
}

#[cfg(target_os = "linux")]
struct NativeStore<'a>(&'a secret_service::blocking::SecretService<'a>);

#[cfg(target_os = "linux")]
impl<'a> NativeStore<'a> {
    fn persistent_default(&self) -> Result<secret_service::blocking::Collection<'a>> {
        let collection = native_result(self.0.get_default_collection(), "default_alias")?;
        let session = match self.0.get_collection_by_alias("session") {
            Ok(session) => Some(session),
            Err(secret_service::Error::NoResult) => None,
            Err(error) => return native_result(Err(error), "session_alias"),
        };
        check_persistent_default(
            collection.collection_path.as_str(),
            session.as_ref().map(|value| value.collection_path.as_str()),
        )
        .inspect_err(|_| diagnostic_stage("default_persistence"))?;
        native_result(collection.ensure_unlocked(), "default_unlock")?;
        Ok(collection)
    }
}

#[cfg(target_os = "linux")]
impl KeyStore for NativeStore<'_> {
    fn read(&self, namespace: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let found = native_result(self.0.search_items(attributes(namespace)), "key_search")?;
        check_search_result(found.unlocked.len(), found.locked.len())
            .inspect_err(|_| diagnostic_stage("key_search_ambiguous"))?;
        let Some(item) = found.unlocked.first() else {
            return Ok(None);
        };
        // SearchItems spans every collection. Prove membership in the current
        // persistent default through Items, not implementation-specific paths.
        let collection = self.persistent_default()?;
        let members = native_result(collection.get_all_items(), "key_membership_list")?;
        if members
            .iter()
            .filter(|member| member.item_path == item.item_path)
            .count()
            != 1
        {
            diagnostic_stage("key_membership");
            return Err(PlatformError::KeyUnavailable);
        }
        let actual = native_result(item.get_attributes(), "key_attributes")?;
        check_attributes(&actual, namespace)?;
        if native_result(item.is_locked(), "key_locked")? {
            diagnostic_stage("key_item_locked");
            return Err(PlatformError::KeyUnavailable);
        }
        let content_type = native_result(item.get_secret_content_type(), "key_content_type")?;
        check_content_type(&content_type)?;
        let value = Zeroizing::new(native_result(item.get_secret(), "key_secret")?);
        if value.len() != 32 {
            diagnostic_stage("key_length");
            return Err(PlatformError::KeyUnavailable);
        }
        Ok(Some(value))
    }

    fn prepare_create(&self) -> Result<()> {
        self.persistent_default().map(|_| ())
    }

    fn create(&self, namespace: &str, key: &[u8]) -> Result<()> {
        if self.read(namespace)?.is_some() {
            return Err(PlatformError::KeyUnavailable);
        }
        // Persistent default collection only. A session collection would lose
        // the key on logout and must never be used as an automatic fallback.
        let collection = self.persistent_default()?;
        native_result(
            collection.create_item(
                "PrimerSwitch vault master key",
                attributes(namespace),
                key,
                false,
                CONTENT_TYPE,
            ),
            "key_create_item",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct FakeStore {
        value: RefCell<Option<Vec<u8>>>,
        reads: Cell<u32>,
        creates: Cell<u32>,
        unavailable: Cell<bool>,
        uncertain_create: Cell<bool>,
        lose_after_create: Cell<bool>,
        creation_unavailable: Cell<bool>,
    }
    impl KeyStore for FakeStore {
        fn read(&self, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
            self.reads.set(self.reads.get() + 1);
            if self.unavailable.get() {
                return Err(PlatformError::KeyUnavailable);
            }
            Ok(self.value.borrow().clone().map(Zeroizing::new))
        }
        fn prepare_create(&self) -> Result<()> {
            if self.creation_unavailable.get() {
                return Err(PlatformError::KeyUnavailable);
            }
            Ok(())
        }
        fn create(&self, _: &str, key: &[u8]) -> Result<()> {
            self.creates.set(self.creates.get() + 1);
            if !self.lose_after_create.get() {
                *self.value.borrow_mut() = Some(key.to_vec());
            }
            if self.uncertain_create.get() {
                return Err(PlatformError::KeyUnavailable);
            }
            Ok(())
        }
    }
    fn directory(root: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let directory = root.path().join(name);
        files::private_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn linux_fake_metadata_accepts_documented_gnome_normalization_and_keeps_ownership_exact() {
        let namespace = "isolated-fixture";
        let original: HashMap<String, String> = attributes(namespace)
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        check_attributes(&original, namespace).unwrap();
        check_content_type(CONTENT_TYPE).unwrap();
        check_content_type("text/plain").unwrap();
        for content_type in [
            "",
            "text/plain; charset=utf-8",
            "application/json",
            "malformed",
        ] {
            assert!(matches!(
                check_content_type(content_type),
                Err(PlatformError::KeyUnavailable)
            ));
        }
        let mut restored = original.clone();
        restored.insert("xdg:schema".into(), GENERIC_SCHEMA.into());
        check_attributes(&restored, namespace).unwrap();
        let mut wrong_schema = restored.clone();
        wrong_schema.insert("xdg:schema".into(), "unexpected".into());
        assert!(matches!(
            check_attributes(&wrong_schema, namespace),
            Err(PlatformError::KeyUnavailable)
        ));
        for baseline in [&original, &restored] {
            let mut extra = baseline.clone();
            extra.insert("unexpected-attribute".into(), "unexpected".into());
            assert!(matches!(
                check_attributes(&extra, namespace),
                Err(PlatformError::KeyUnavailable)
            ));
        }
        for key in ["application", "vault"] {
            let mut wrong = original.clone();
            wrong.insert(key.into(), "different-owner".into());
            assert!(matches!(
                check_attributes(&wrong, namespace),
                Err(PlatformError::KeyUnavailable)
            ));
            let mut missing = original.clone();
            missing.remove(key);
            assert!(check_attributes(&missing, namespace).is_err());
        }
    }

    #[test]
    fn linux_fake_watchdog_cancels_and_joins_pending_operation() {
        use std::sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let pending = gate.clone();
        let joined = Arc::new(AtomicBool::new(false));
        let completed = joined.clone();
        let result: Result<()> = bounded_operation(
            std::time::Duration::from_millis(10),
            move || {
                let (lock, signal) = &*pending;
                let mut cancelled = lock.lock().unwrap();
                while !*cancelled {
                    cancelled = signal.wait(cancelled).unwrap();
                }
                completed.store(true, Ordering::SeqCst);
                Err(PlatformError::KeyUnavailable)
            },
            move || {
                let (lock, signal) = &*gate;
                *lock.lock().unwrap() = true;
                signal.notify_all();
            },
        );
        assert!(matches!(result, Err(PlatformError::KeyUnavailable)));
        assert!(joined.load(Ordering::SeqCst));
    }

    #[test]
    fn linux_fake_watchdog_completed_operation_retains_result() {
        assert_eq!(
            bounded_operation(std::time::Duration::from_secs(1), || Ok(17), || {}).unwrap(),
            17
        );
    }

    #[test]
    fn linux_fake_ephemeral_default_is_not_a_persistent_key_store() {
        assert!(check_persistent_default("/collection/login", Some("/collection/session")).is_ok());
        assert!(check_persistent_default("/provider/arbitrary-path", None).is_ok());
        assert!(matches!(
            check_persistent_default("/opaque/ephemeral", Some("/opaque/ephemeral")),
            Err(PlatformError::KeyUnavailable)
        ));
        assert!(matches!(
            check_persistent_default("/", None),
            Err(PlatformError::KeyUnavailable)
        ));
    }

    #[test]
    fn linux_fake_locked_or_duplicate_searches_are_never_selected() {
        assert!(!check_search_result(0, 0).unwrap());
        assert!(check_search_result(1, 0).unwrap());
        for (unlocked, locked) in [(0, 1), (1, 1), (2, 0), (2, 1), (0, 2)] {
            assert!(matches!(
                check_search_result(unlocked, locked),
                Err(PlatformError::KeyUnavailable)
            ));
        }
    }

    #[test]
    fn linux_fake_key_reopens_and_canonical_namespaces_are_stable() {
        let root = files::test_root();
        let directory = directory(&root, "vault");
        let store = FakeStore::default();
        let key = load_key(&directory, false, &store).unwrap();
        assert_eq!(load_key(&directory.join("."), false, &store).unwrap(), key);
        assert_eq!(store.creates.get(), 1);
        assert_eq!(
            namespace(&directory).unwrap(),
            namespace(&directory.join(".")).unwrap()
        );
        let other = self::directory(&root, "other");
        assert_ne!(namespace(&directory).unwrap(), namespace(&other).unwrap());
        let vault = crate::Vault::with_key(directory.clone(), key).unwrap();
        vault
            .save("fixture", &serde_json::json!({"secret":"FIXTURE_SENTINEL"}))
            .unwrap();
        let reopened = crate::Vault::with_key(
            directory.clone(),
            load_key(&directory, true, &store).unwrap(),
        )
        .unwrap();
        assert_eq!(
            reopened
                .load::<serde_json::Value>("fixture")
                .unwrap()
                .unwrap()["secret"],
            "FIXTURE_SENTINEL"
        );
        assert!(
            !String::from_utf8_lossy(&std::fs::read(directory.join(MARKER)).unwrap())
                .contains("FIXTURE_SENTINEL")
        );
    }

    #[test]
    fn linux_fake_missing_unavailable_and_malformed_keys_preserve_ciphertext() {
        let root = files::test_root();
        let directory = directory(&root, "vault");
        let vault = crate::Vault::with_key(directory.clone(), [19; 32]).unwrap();
        vault
            .save("fixture", &serde_json::json!({"secret":"fixture-only"}))
            .unwrap();
        let path = directory.join("fixture.vault");
        let before = std::fs::read(&path).unwrap();
        let store = FakeStore::default();
        assert!(matches!(
            load_key(&directory, true, &store),
            Err(PlatformError::KeyLost)
        ));
        assert_eq!(store.creates.get(), 0);
        store.unavailable.set(true);
        let error = load_key(&directory, true, &store).unwrap_err();
        assert!(matches!(error, PlatformError::KeyUnavailable));
        assert!(!format!("{error:?} {error}").contains("fixture-only"));
        store.unavailable.set(false);
        *store.value.borrow_mut() = Some(vec![1; 31]);
        assert!(matches!(
            load_key(&directory, true, &store),
            Err(PlatformError::KeyUnavailable)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(store.creates.get(), 0);
        assert!(!directory.join(MARKER).exists());
    }

    #[test]
    fn linux_fake_uncertain_creation_recovers_same_key_or_fails_without_rotation() {
        let root = files::test_root();
        let directory = directory(&root, "vault");
        let store = FakeStore::default();
        store.uncertain_create.set(true);
        assert!(load_key(&directory, false, &store).is_err());
        let original = store.value.borrow().clone().unwrap();
        store.uncertain_create.set(false);
        assert_eq!(
            load_key(&directory, false, &store).unwrap().as_slice(),
            original
        );
        *store.value.borrow_mut() = None;
        assert!(matches!(
            load_key(&directory, false, &store),
            Err(PlatformError::KeyLost)
        ));
        assert_eq!(store.creates.get(), 1);
        *store.value.borrow_mut() = Some(vec![7; 32]);
        assert!(matches!(
            load_key(&directory, false, &store),
            Err(PlatformError::KeyUnavailable)
        ));
        assert_eq!(store.creates.get(), 1);
        std::fs::write(directory.join(MARKER), b"malformed").unwrap();
        assert!(load_key(&directory, false, &store).is_err());
        assert_eq!(std::fs::read(directory.join(MARKER)).unwrap(), b"malformed");
    }

    #[test]
    fn linux_fake_initial_locked_or_missing_collection_can_be_retried() {
        let root = files::test_root();
        let directory = directory(&root, "vault");
        let store = FakeStore::default();
        store.creation_unavailable.set(true);
        assert!(matches!(
            load_key(&directory, false, &store),
            Err(PlatformError::KeyUnavailable)
        ));
        assert_eq!(store.creates.get(), 0);
        assert!(!directory.join(MARKER).exists());
        store.creation_unavailable.set(false);
        assert!(load_key(&directory, false, &store).is_ok());
        assert_eq!(store.creates.get(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_unsafe_ancestors_and_namespace_symlinks_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = files::test_root();
        let parent = directory(&root, "parent");
        let directory = parent.join("vault");
        files::private_dir(&directory).unwrap();
        check_directory(&directory).unwrap();
        let link = root.path().join("alias");
        symlink(&directory, &link).unwrap();
        assert!(matches!(namespace(&link), Err(PlatformError::UnsafePath)));
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            check_directory(&directory),
            Err(PlatformError::UnsafePath)
        ));
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        check_directory(&directory).unwrap();
    }

    #[test]
    fn linux_fake_unconfirmed_creation_never_returns_an_unpersisted_key() {
        let root = files::test_root();
        let directory = directory(&root, "vault");
        let store = FakeStore::default();
        store.lose_after_create.set(true);
        assert!(matches!(
            load_key(&directory, false, &store),
            Err(PlatformError::KeyUnavailable)
        ));
        assert!(matches!(
            load_key(&directory, false, &store),
            Err(PlatformError::KeyLost)
        ));
        assert_eq!(store.creates.get(), 1);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod native_tests {
    use super::*;
    use crate::Vault;
    use secret_service::{EncryptionType, blocking::SecretService};
    use serde_json::{Value, json};
    use std::path::PathBuf;

    fn isolated_home() -> PathBuf {
        assert_eq!(
            std::env::var("PRIMERSWITCH_ISOLATED_KEYRING_TEST").as_deref(),
            Ok("1"),
            "native fixtures require the isolated harness"
        );
        let home = PathBuf::from(std::env::var_os("HOME").expect("temporary HOME required"))
            .canonicalize()
            .unwrap();
        let expected = PathBuf::from(
            std::env::var_os("PRIMERSWITCH_ISOLATED_HOME").expect("isolation HOME marker required"),
        )
        .canonicalize()
        .unwrap();
        assert_eq!(home, expected);
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert!(
            home.starts_with(&temp) && home != temp,
            "fixture HOME must be inside the temporary directory"
        );
        assert_eq!(
            std::fs::read(home.join(".primerswitch-isolated-keyring")).unwrap(),
            b"PrimerSwitch isolated keyring test v1\n"
        );
        let bus =
            std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("isolated dbus-run-session required");
        assert!(bus.starts_with("unix:path="));
        assert_eq!(
            std::env::var("PRIMERSWITCH_TEST_DBUS_ADDRESS").unwrap(),
            bus
        );
        check_directory(&home).unwrap();
        home
    }

    struct AliasRestore<'a> {
        proxy: zbus::blocking::Proxy<'a>,
        prior: zbus::zvariant::OwnedObjectPath,
    }
    impl AliasRestore<'_> {
        fn restore(&self) -> zbus::Result<()> {
            self.proxy.call("SetAlias", &("default", &self.prior))
        }
    }
    impl Drop for AliasRestore<'_> {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    struct Cleanup<'a> {
        service: &'a SecretService<'a>,
        namespace: String,
    }
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            if let Ok(found) = self.service.search_items(attributes(&self.namespace)) {
                for item in found.unlocked {
                    let _ = item.delete();
                }
            }
        }
    }

    #[test]
    #[ignore = "requires explicit temporary HOME and isolated dbus/gnome-keyring harness"]
    fn linux_secret_service_native() {
        let home = isolated_home(); // Must run before even connecting to D-Bus.
        let root = tempfile::Builder::new()
            .prefix("native-vault-")
            .tempdir_in(&home)
            .unwrap();
        let directory = root.path().join("vault");
        files::private_dir(&directory).unwrap();
        let namespace = namespace(&directory).unwrap();
        let service = SecretService::connect(EncryptionType::Dh).unwrap();
        let cleanup = Cleanup {
            service: &service,
            namespace: namespace.clone(),
        };
        let vault = Vault::open(directory.clone()).unwrap();
        vault
            .save("fixture", &json!({"secret":"NATIVE_FIXTURE_ONLY"}))
            .unwrap();
        drop(vault);
        let original = std::fs::read(directory.join("fixture.vault")).unwrap();
        let marker = std::fs::read(directory.join(MARKER)).unwrap();
        let key = NativeStore(&service).read(&namespace).unwrap().unwrap();
        let reopened = Vault::open(directory.join(".")).unwrap();
        assert_eq!(
            reopened.load::<Value>("fixture").unwrap().unwrap()["secret"],
            "NATIVE_FIXTURE_ONLY"
        );
        drop(reopened);
        let collection = service.get_default_collection().unwrap();
        let duplicate = collection
            .create_item(
                "isolated duplicate fixture",
                attributes(&namespace),
                key.as_slice(),
                false,
                CONTENT_TYPE,
            )
            .unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        duplicate.delete().unwrap();
        let found = service.search_items(attributes(&namespace)).unwrap();
        let item = &found.unlocked[0];
        item.set_secret(b"malformed-native-fixture", CONTENT_TYPE)
            .unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        assert_eq!(item.get_secret().unwrap(), b"malformed-native-fixture");
        // GNOME returns text/plain for secrets originally written as binary.
        assert_eq!(item.get_secret_content_type().unwrap(), "text/plain");
        item.set_secret(key.as_slice(), "text/plain").unwrap();
        assert!(Vault::open(directory.clone()).is_ok());
        item.set_secret(key.as_slice(), CONTENT_TYPE).unwrap();
        // Only GNOME's known reload schema is compatible; no other item metadata
        // may turn a matching search result into a valid application-owned key.
        item.set_attributes(HashMap::from([
            ("application", SERVICE),
            ("vault", namespace.as_str()),
            ("xdg:schema", "unexpected-fixture-schema"),
        ]))
        .unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        item.set_attributes(HashMap::from([
            ("application", SERVICE),
            ("vault", namespace.as_str()),
            ("xdg:schema", GENERIC_SCHEMA),
            ("unexpected-attribute", "fixture-only"),
        ]))
        .unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        item.set_attributes(HashMap::from([
            ("application", SERVICE),
            ("vault", namespace.as_str()),
            ("xdg:schema", GENERIC_SCHEMA),
        ]))
        .unwrap();
        assert!(Vault::open(directory.clone()).is_ok());
        item.set_secret(&[23; 32], CONTENT_TYPE).unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        item.set_secret(key.as_slice(), CONTENT_TYPE).unwrap();
        item.delete().unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyLost)
        ));
        assert_eq!(
            std::fs::read(directory.join("fixture.vault")).unwrap(),
            original
        );
        assert_eq!(std::fs::read(directory.join(MARKER)).unwrap(), marker);
        assert!(NativeStore(&service).read(&namespace).unwrap().is_none());
        drop(cleanup); // Remove any own fixture items before locking collection.
        collection.lock().unwrap();
        let locked = root.path().join("locked-vault");
        assert!(matches!(
            Vault::open(locked.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        assert!(
            service
                .search_items(attributes(&super::namespace(&locked).unwrap()))
                .unwrap()
                .unlocked
                .is_empty()
        );
    }

    #[test]
    #[ignore = "requires isolated native keyring; temporarily changes only its default alias"]
    fn linux_secret_service_session_alias_native() {
        let home = isolated_home();
        let root = tempfile::Builder::new()
            .prefix("session-alias-")
            .tempdir_in(&home)
            .unwrap();
        let directory = root.path().join("persistent");
        let vault = Vault::open(directory.clone()).unwrap();
        vault
            .save("fixture", &json!({"secret":"SESSION_ALIAS_FIXTURE_ONLY"}))
            .unwrap();
        drop(vault);
        let ciphertext = std::fs::read(directory.join("fixture.vault")).unwrap();
        let marker = std::fs::read(directory.join(MARKER)).unwrap();
        let service = SecretService::connect(EncryptionType::Dh).unwrap();
        let cleanup = Cleanup {
            service: &service,
            namespace: namespace(&directory).unwrap(),
        };
        let prior = service.get_default_collection().unwrap();
        let session = service.get_collection_by_alias("session").unwrap();
        let connection = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.secrets",
            "/org/freedesktop/secrets",
            "org.freedesktop.Secret.Service",
        )
        .unwrap();
        let restore = AliasRestore {
            proxy,
            prior: prior.collection_path.clone(),
        };
        let _: () = restore
            .proxy
            .call("SetAlias", &("default", &session.collection_path))
            .unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        let fresh = root.path().join("new-vault");
        assert!(matches!(
            Vault::open(fresh.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        assert!(!fresh.join(MARKER).exists());
        assert_eq!(
            std::fs::read(directory.join("fixture.vault")).unwrap(),
            ciphertext
        );
        assert_eq!(std::fs::read(directory.join(MARKER)).unwrap(), marker);
        // Even an exact entry in the ephemeral default must not be adopted.
        let ephemeral_namespace = namespace(&fresh).unwrap();
        let ephemeral = session
            .create_item(
                "isolated ephemeral fixture",
                attributes(&ephemeral_namespace),
                &[29; 32],
                false,
                CONTENT_TYPE,
            )
            .unwrap();
        assert!(matches!(
            Vault::open(fresh.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        assert!(!fresh.join(MARKER).exists());
        ephemeral.delete().unwrap();
        restore.restore().unwrap();
        assert!(Vault::open(directory.clone()).is_ok());
        drop(restore);
        drop(cleanup);
    }

    #[test]
    #[ignore = "requires isolated D-Bus with Secret Service activation disabled"]
    fn linux_secret_service_missing_native() {
        let home = isolated_home();
        assert_eq!(
            std::env::var("PRIMERSWITCH_NATIVE_EXPECT_SERVICE").as_deref(),
            Ok("missing")
        );
        let root = tempfile::Builder::new()
            .prefix("missing-service-")
            .tempdir_in(&home)
            .unwrap();
        let directory = root.path().join("vault");
        let fixture = Vault::with_key(directory.clone(), [31; 32]).unwrap();
        fixture
            .save("fixture", &json!({"secret":"MISSING_SERVICE_FIXTURE_ONLY"}))
            .unwrap();
        drop(fixture);
        let ciphertext = std::fs::read(directory.join("fixture.vault")).unwrap();
        let error = Vault::open(directory.clone()).unwrap_err();
        assert!(matches!(error, PlatformError::KeyUnavailable));
        assert!(!format!("{error:?} {error}").contains("MISSING_SERVICE_FIXTURE_ONLY"));
        assert_eq!(
            std::fs::read(directory.join("fixture.vault")).unwrap(),
            ciphertext
        );
        assert!(!directory.join(MARKER).exists());
    }

    /// The harness invokes this in two separate test processes with the same
    /// fresh random directory, optionally restarting its isolated keyring daemon
    /// between phases. The create phase deliberately retains only its fixture.
    #[test]
    #[ignore = "two-phase isolated process/keyring persistence fixture"]
    fn linux_secret_service_process_native() {
        let home = isolated_home();
        let directory = PathBuf::from(
            std::env::var_os("PRIMERSWITCH_NATIVE_VAULT_DIR")
                .expect("fresh random fixture directory required"),
        );
        assert!(directory.is_absolute() && directory.starts_with(&home) && directory != home);
        files::check_path(&directory).unwrap();
        let phase = std::env::var("PRIMERSWITCH_NATIVE_PHASE").unwrap();
        match phase.as_str() {
            "create" => {
                assert!(
                    !directory.exists(),
                    "create phase must use a fresh namespace"
                );
                let vault = Vault::open(directory).unwrap();
                vault
                    .save(
                        "process-fixture",
                        &json!({"secret":"PROCESS_RESTART_FIXTURE_ONLY"}),
                    )
                    .unwrap();
            }
            "reopen" => {
                let vault = Vault::open(directory.clone()).unwrap();
                assert_eq!(
                    vault.load::<Value>("process-fixture").unwrap().unwrap()["secret"],
                    "PROCESS_RESTART_FIXTURE_ONLY"
                );
                let namespace = namespace(&directory).unwrap();
                let service = SecretService::connect(EncryptionType::Dh).unwrap();
                let found = service.search_items(attributes(&namespace)).unwrap();
                assert_eq!(found.unlocked.len(), 1);
                let reloaded_attributes = found.unlocked[0].get_attributes().unwrap();
                assert_eq!(
                    reloaded_attributes.get("xdg:schema").map(String::as_str),
                    Some(GENERIC_SCHEMA)
                );
                check_attributes(&reloaded_attributes, &namespace).unwrap();
                let cleanup = Cleanup {
                    service: &service,
                    namespace,
                };
                drop(cleanup);
                let ciphertext = std::fs::read(directory.join("process-fixture.vault")).unwrap();
                assert!(matches!(
                    Vault::open(directory.clone()),
                    Err(PlatformError::KeyLost)
                ));
                assert_eq!(
                    std::fs::read(directory.join("process-fixture.vault")).unwrap(),
                    ciphertext
                );
            }
            _ => panic!("unknown isolated fixture phase"),
        }
    }
}
