use crate::{PlatformError, Result, files};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Serialize, de::DeserializeOwned};
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use std::path::Path;
use std::{fmt, path::PathBuf};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"PRMSV001";

pub struct Vault {
    directory: PathBuf,
    key: Zeroizing<[u8; 32]>,
}
impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Vault { key: [REDACTED] }")
    }
}

impl Vault {
    pub fn open(directory: PathBuf) -> Result<Self> {
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = directory;
            Err(PlatformError::UnsupportedSecretService)
        }
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            #[cfg(target_os = "linux")]
            crate::linux_vault::check_directory(&directory)?;
            files::private_dir(&directory)?;
            #[cfg(target_os = "linux")]
            let directory = directory.canonicalize()?;
            let _lock = files::Lock::acquire(&directory.join(".vault.lock"))?;
            let encrypted_files =
                std::fs::read_dir(&directory)?.try_fold(false, |found, entry| {
                    Ok::<_, std::io::Error>(
                        found || entry?.path().extension().is_some_and(|ext| ext == "vault"),
                    )
                })?;
            let key = load_native_key(&directory, encrypted_files)?;
            Ok(Self {
                directory,
                key: Zeroizing::new(key),
            })
        }
    }

    /// Fixture injection only. Production callers must use the OS-protected `open`.
    pub fn with_key(directory: PathBuf, key: [u8; 32]) -> Result<Self> {
        files::private_dir(&directory)?;
        Ok(Self {
            directory,
            key: Zeroizing::new(key),
        })
    }

    pub fn load<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>> {
        let path = self.record_path(name)?;
        let _lock = files::Lock::acquire(&self.directory.join(".vault.lock"))?;
        let Some(envelope) = files::read_optional(&path)? else {
            return Ok(None);
        };
        if envelope.len() < 8 + 24 + 16 || &envelope[..8] != MAGIC {
            return Err(PlatformError::Authentication);
        }
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_ref())
            .map_err(|_| PlatformError::Authentication)?;
        let aad = associated_data(name);
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&envelope[8..32]),
                    Payload {
                        msg: &envelope[32..],
                        aad: aad.as_bytes(),
                    },
                )
                .map_err(|_| PlatformError::Authentication)?,
        );
        // A schema parse failure preserves the authentic envelope as well.
        Ok(Some(serde_json::from_slice(&plaintext)?))
    }

    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let path = self.record_path(name)?;
        let _lock = files::Lock::acquire(&self.directory.join(".vault.lock"))?;
        // A damaged existing file must not be replaced as though it were an empty vault.
        if let Some(envelope) = files::read_optional(&path)? {
            self.authenticate(name, &envelope)?;
        }
        let plaintext = Zeroizing::new(serde_json::to_vec(value)?);
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_ref())
            .map_err(|_| PlatformError::Authentication)?;
        let mut nonce = [0u8; 24];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| PlatformError::KeyUnavailable)?;
        let aad = associated_data(name);
        let encrypted = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PlatformError::Authentication)?;
        let mut envelope = Vec::with_capacity(32 + encrypted.len());
        envelope.extend_from_slice(MAGIC);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&encrypted);
        files::atomic_write(&path, &envelope)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        let path = self.record_path(name)?;
        let _lock = files::Lock::acquire(&self.directory.join(".vault.lock"))?;
        if let Some(envelope) = files::read_optional(&path)? {
            self.authenticate(name, &envelope)?;
        }
        files::remove(&path)
    }

    fn authenticate(&self, name: &str, envelope: &[u8]) -> Result<()> {
        if envelope.len() < 48 || &envelope[..8] != MAGIC {
            return Err(PlatformError::Authentication);
        }
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_ref())
            .map_err(|_| PlatformError::Authentication)?;
        let aad = associated_data(name);
        let mut plaintext = cipher
            .decrypt(
                XNonce::from_slice(&envelope[8..32]),
                Payload {
                    msg: &envelope[32..],
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PlatformError::Authentication)?;
        plaintext.zeroize();
        Ok(())
    }

    fn record_path(&self, name: &str) -> Result<PathBuf> {
        // Win32 device names remain reserved even with the `.vault` extension.
        // Reject them on every platform so a portable fixture cannot create an
        // account filename that becomes unsafe after transfer to Windows.
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        if name.is_empty()
            || name.len() > 128
            || name.starts_with('.')
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            || name.contains("..")
            || reserved
        {
            return Err(PlatformError::UnsafePath);
        }
        Ok(self.directory.join(format!("{name}.vault")))
    }

    pub(crate) fn context_lock(&self, context: &str) -> Result<files::Lock> {
        if context.len() != 64 || !context.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(PlatformError::UnsafePath);
        }
        files::Lock::acquire(&self.directory.join(format!(".context-{context}.lock")))
    }
}

fn associated_data(name: &str) -> String {
    format!("com.primertech.primerswitch/vault/v1/{name}")
}

#[cfg(windows)]
fn load_native_key(directory: &Path, encrypted_files: bool) -> Result<[u8; 32]> {
    let path = directory.join("master-key.dpapi");
    match files::read_optional(&path)? {
        Some(bytes) => {
            let plaintext = Zeroizing::new(crate::protection::unprotect_key(&bytes)?);
            plaintext
                .as_slice()
                .try_into()
                .map_err(|_| PlatformError::KeyUnavailable)
        }
        None => {
            if encrypted_files {
                return Err(PlatformError::KeyLost);
            }
            let mut key = Zeroizing::new([0u8; 32]);
            OsRng
                .try_fill_bytes(key.as_mut())
                .map_err(|_| PlatformError::KeyUnavailable)?;
            let protected = crate::protection::protect_key(key.as_ref())?;
            files::atomic_write(&path, &protected)?;
            Ok(*key)
        }
    }
}

#[cfg(target_os = "linux")]
fn load_native_key(directory: &Path, encrypted_files: bool) -> Result<[u8; 32]> {
    crate::linux_vault::load_native_key(directory, encrypted_files)
}

#[cfg(target_os = "macos")]
fn load_native_key(directory: &Path, encrypted_files: bool) -> Result<[u8; 32]> {
    use security_framework::passwords::{get_generic_password, set_generic_password};
    use sha2::{Digest, Sha256};
    let account = format!(
        "vault-{:x}",
        Sha256::digest(directory.as_os_str().as_encoded_bytes())
    );
    let service = "com.primertech.primerswitch.vault.v1";
    let marker = directory.join("master-key.keychain");
    match get_generic_password(service, &account) {
        Ok(value) => {
            let value = Zeroizing::new(value);
            value
                .as_slice()
                .try_into()
                .map_err(|_| PlatformError::KeyUnavailable)
        }
        Err(error) if error.code() == -25300 => {
            if encrypted_files || files::read_optional(&marker)?.is_some() {
                return Err(PlatformError::KeyLost);
            }
            let mut key = Zeroizing::new([0u8; 32]);
            OsRng
                .try_fill_bytes(key.as_mut())
                .map_err(|_| PlatformError::KeyUnavailable)?;
            set_generic_password(service, &account, key.as_ref())
                .map_err(|_| PlatformError::KeyUnavailable)?;
            files::atomic_write(&marker, b"PrimerSwitch Keychain master key v1")?;
            Ok(*key)
        }
        Err(_) => Err(PlatformError::KeyUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn encrypted_round_trip_nonce_names_and_redaction() {
        let root = crate::files::test_root();
        let vault = Vault::with_key(root.path().join("vault"), [4; 32]).unwrap();
        let payload = json!({"token":"SENTINEL_TOKEN","unknown":{"field":true}});
        vault.save("account-a", &payload).unwrap();
        let first = std::fs::read(vault.record_path("account-a").unwrap()).unwrap();
        assert!(!first.windows(14).any(|w| w == b"SENTINEL_TOKEN"));
        vault.save("account-a", &payload).unwrap();
        let second = std::fs::read(vault.record_path("account-a").unwrap()).unwrap();
        assert_ne!(&first[8..32], &second[8..32]);
        assert_eq!(vault.load::<Value>("account-a").unwrap(), Some(payload));
        assert!(!format!("{vault:?}").contains("SENTINEL"));
        for name in [
            "../x", "a/b", "a\\b", ".hidden", "", "a..b", "CON", "nul.json", "COM1",
        ] {
            assert!(vault.save(name, &1).is_err());
        }
    }

    #[test]
    fn tampering_wrong_key_and_aad_preserve_files() {
        let root = crate::files::test_root();
        let directory = root.path().join("vault");
        let vault = Vault::with_key(directory.clone(), [1; 32]).unwrap();
        vault.save("a", &json!({"token":"secret"})).unwrap();
        let path = vault.record_path("a").unwrap();
        let original = std::fs::read(&path).unwrap();
        let wrong = Vault::with_key(directory.clone(), [2; 32]).unwrap();
        assert!(matches!(
            wrong.load::<Value>("a"),
            Err(PlatformError::Authentication)
        ));
        assert!(wrong.save("a", &1).is_err());
        assert!(wrong.remove("a").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::copy(&path, vault.record_path("b").unwrap()).unwrap();
        assert!(vault.load::<Value>("b").is_err());
        for offset in [0, 8, 32, original.len() - 1] {
            let mut damaged = original.clone();
            damaged[offset] ^= 1;
            std::fs::write(&path, &damaged).unwrap();
            assert!(vault.load::<Value>("a").is_err());
            assert!(vault.save("a", &1).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), damaged);
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_dpapi_round_trip_and_missing_key_are_safe() {
        let root = crate::files::test_root();
        let directory = root.path().join("native-vault");
        let vault = Vault::open(directory.clone()).unwrap();
        vault
            .save("native-test", &json!({"secret":"native-fixture-only"}))
            .unwrap();
        drop(vault);
        assert_eq!(
            Vault::open(directory.clone())
                .unwrap()
                .load::<Value>("native-test")
                .unwrap()
                .unwrap()["secret"],
            "native-fixture-only"
        );
        let protected = std::fs::read(directory.join("master-key.dpapi")).unwrap();
        assert!(protected.len() > 32);
        std::fs::remove_file(directory.join("master-key.dpapi")).unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyLost)
        ));
        assert!(!directory.join("master-key.dpapi").exists());
        assert!(directory.join("native-test.vault").exists());
    }

    #[cfg(windows)]
    #[test]
    fn damaged_dpapi_key_is_preserved_without_replacement() {
        let root = crate::files::test_root();
        let directory = root.path().join("native-vault");
        let vault = Vault::open(directory.clone()).unwrap();
        vault
            .save("native-test", &json!({"secret":"fixture-only"}))
            .unwrap();
        drop(vault);
        let path = directory.join("master-key.dpapi");
        let damaged = b"damaged-dpapi-key";
        std::fs::write(&path, damaged).unwrap();
        assert!(matches!(
            Vault::open(directory.clone()),
            Err(PlatformError::KeyUnavailable)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), damaged);
        assert!(directory.join("native-test.vault").exists());
    }
}
