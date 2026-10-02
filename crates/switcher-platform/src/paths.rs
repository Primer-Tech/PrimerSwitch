use crate::{PlatformError, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct CliPaths {
    pub config_dir: PathBuf,
    pub credentials_file: PathBuf,
    pub global_config_file: PathBuf,
    pub settings_file: PathBuf,
    pub context_id: String,
}

impl CliPaths {
    /// Default Claude 2.1.287 context. Does not inspect or create any files.
    pub fn for_home(home: PathBuf) -> Self {
        let config_dir = home.join(".claude");
        let mut paths = Self {
            credentials_file: config_dir.join(".credentials.json"),
            global_config_file: home.join(".claude.json"),
            settings_file: config_dir.join("settings.json"),
            config_dir,
            context_id: String::new(),
        };
        paths.context_id = paths.computed_context_id();
        paths
    }

    pub fn discover() -> Result<Self> {
        Self::validate_environment(|name| env::var_os(name))?;
        let paths = Self::for_home(home_dir()?);
        paths.validate_subscription_mode()?;
        Ok(paths)
    }

    // Empty secure-storage override has distinct semantics in Claude. Global
    // identity resolution under overrides remains unqualified, so fail closed.
    fn validate_environment(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<()> {
        const CONTEXT: &[&str] = &[
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
            "CLAUDE_CODE_HOST_CREDS_FILE",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
            "CLAUDE_CODE_CUSTOM_OAUTH_URL",
            "CLAUDE_LOCAL_OAUTH_API_BASE",
            "CLAUDE_CODE_OAUTH_CLIENT_ID",
            "CLAUDE_CODE_HOST_GATEWAY_LINEAGE",
        ];
        if CONTEXT.iter().any(|name| get(name).is_some()) {
            return Err(PlatformError::UnsupportedContext);
        }
        Ok(())
    }

    pub fn default_model(&self) -> Result<Option<String>> {
        // The explicit user settings model takes precedence over the historical
        // global Claude configuration default. Only these model fields are exposed.
        for path in [&self.settings_file, &self.global_config_file] {
            let Some(bytes) = crate::files::read_optional(path)? else {
                continue;
            };
            let config: Value = serde_json::from_slice(&bytes)?;
            if !config.is_object() {
                return Err(PlatformError::InvalidJson);
            }
            if let Some(model) = config.get("model").and_then(Value::as_str) {
                return Ok(Some(model.to_owned()));
            }
        }
        Ok(None)
    }

    pub(crate) fn validate_subscription_mode(&self) -> Result<()> {
        let Some(bytes) = crate::files::read_optional(&self.settings_file)? else {
            return Ok(());
        };
        let settings: Value = serde_json::from_slice(&bytes)?;
        if !settings.is_object() {
            return Err(PlatformError::InvalidJson);
        }
        if settings.get("apiKeyHelper").is_some()
            || settings.get("forceLoginMethod").is_some()
            || settings.get("forceLoginOrgUUID").is_some()
        {
            return Err(PlatformError::UnsupportedContext);
        }
        if let Some(environment) = settings.get("env") {
            let object = environment.as_object().ok_or(PlatformError::InvalidJson)?;
            Self::validate_environment(|name| object.get(name).map(|_| std::ffi::OsString::new()))?;
        }
        Ok(())
    }

    pub(crate) fn computed_context_id(&self) -> String {
        let mut hash = Sha256::new();
        for path in [
            &self.config_dir,
            &self.credentials_file,
            &self.global_config_file,
        ] {
            // Windows treats path casing as equivalent. Keep those contexts on one
            // lock/journal even when the caller uses a differently cased home path.
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                // Preserve unpaired UTF-16 surrogates in the rare paths that have
                // them; lossy conversion must never merge different directories.
                let normalized: Vec<u16> = match path.to_str() {
                    Some(path) => path
                        .replace('/', "\\")
                        .to_lowercase()
                        .encode_utf16()
                        .collect(),
                    None => path
                        .as_os_str()
                        .encode_wide()
                        .map(|unit| match unit {
                            65..=90 => unit + 32,
                            47 => 92,
                            other => other,
                        })
                        .collect(),
                };
                for unit in normalized {
                    hash.update(unit.to_le_bytes());
                }
            }
            #[cfg(not(windows))]
            hash.update(path.as_os_str().as_encoded_bytes());
            hash.update([0]);
        }
        format!("{:x}", hash.finalize())
    }
}

fn home_dir() -> Result<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or(PlatformError::UnsupportedContext)
}

pub fn app_data_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|p| PathBuf::from(p).join("PrimerSwitch"))
            .filter(|p| p.is_absolute())
            .ok_or(PlatformError::UnsafePath)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(home_dir()?.join("Library/Application Support/PrimerSwitch"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        match env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            Some(value) => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(PlatformError::UnsafePath);
                }
                Ok(path.join("primerswitch"))
            }
            None => Ok(home_dir()?.join(".local/share/primerswitch")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_and_provider_modes_fail_closed() {
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
            "ANTHROPIC_API_KEY",
            "CLAUDE_CODE_USE_BEDROCK",
        ] {
            for value in ["", "custom", "0"] {
                assert!(
                    CliPaths::validate_environment(|n| (n == name).then(|| value.into())).is_err()
                );
            }
        }
        assert!(CliPaths::validate_environment(|_| None).is_ok());
    }
    #[test]
    fn distinct_unicode_contexts_and_model_only() {
        let root = crate::files::test_root();
        let paths = CliPaths::for_home(root.path().join("Unicode ș with spaces"));
        assert_ne!(
            paths.context_id,
            CliPaths::for_home(root.path().join("other")).context_id
        );
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(
            &paths.settings_file,
            br#"{"model":"sonnet","secret":"sentinel"}"#,
        )
        .unwrap();
        assert_eq!(paths.default_model().unwrap().as_deref(), Some("sonnet"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_casing_and_separator_aliases_share_one_context() {
        let first = CliPaths::for_home(PathBuf::from(r"C:\Users\Fixture"));
        let second = CliPaths::for_home(PathBuf::from("c:/users/fixture"));
        assert_eq!(first.context_id, second.context_id);
    }

    #[test]
    fn explicit_authentication_settings_are_not_silently_switched() {
        let root = crate::files::test_root();
        let paths = CliPaths::for_home(root.path().join("fixture-home"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        for settings in [
            serde_json::json!({"apiKeyHelper":"SECRET_HELPER_SENTINEL"}),
            serde_json::json!({"env":{"ANTHROPIC_API_KEY":"SECRET_KEY_SENTINEL"}}),
            serde_json::json!({"forceLoginOrgUUID":"managed-org"}),
        ] {
            let bytes = serde_json::to_vec(&settings).unwrap();
            std::fs::write(&paths.settings_file, &bytes).unwrap();
            let error = paths.validate_subscription_mode().unwrap_err();
            assert!(matches!(error, PlatformError::UnsupportedContext));
            assert!(!error.to_string().contains("SENTINEL"));
            assert_eq!(std::fs::read(&paths.settings_file).unwrap(), bytes);
        }
    }
    #[test]
    fn configured_model_prefers_settings_and_falls_back_to_global() {
        let root = crate::files::test_root();
        let paths = CliPaths::for_home(root.path().join("model-fixture"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(
            &paths.global_config_file,
            br#"{"model":"opus","unknown":"keep"}"#,
        )
        .unwrap();
        assert_eq!(paths.default_model().unwrap().as_deref(), Some("opus"));
        std::fs::write(
            &paths.settings_file,
            br#"{"model":"sonnet","unknown":"keep"}"#,
        )
        .unwrap();
        assert_eq!(paths.default_model().unwrap().as_deref(), Some("sonnet"));
        std::fs::write(&paths.settings_file, br#"{"unknown":"keep"}"#).unwrap();
        assert_eq!(paths.default_model().unwrap().as_deref(), Some("opus"));
        std::fs::remove_file(&paths.global_config_file).unwrap();
        assert_eq!(paths.default_model().unwrap(), None);
    }

    #[test]
    fn corrupted_model_configuration_is_preserved_and_reported() {
        let root = crate::files::test_root();
        let paths = CliPaths::for_home(root.path().join("model-fixture"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let damaged = b"{invalid CONFIG_SENTINEL";
        std::fs::write(&paths.global_config_file, damaged).unwrap();
        let error = paths.default_model().unwrap_err();
        assert!(matches!(error, PlatformError::InvalidJson));
        assert!(!error.to_string().contains("CONFIG_SENTINEL"));
        assert_eq!(std::fs::read(&paths.global_config_file).unwrap(), damaged);
        std::fs::write(&paths.settings_file, br#"{"model":"sonnet"}"#).unwrap();
        assert_eq!(paths.default_model().unwrap().as_deref(), Some("sonnet"));
        std::fs::write(&paths.settings_file, damaged).unwrap();
        assert!(matches!(
            paths.default_model(),
            Err(PlatformError::InvalidJson)
        ));
        assert_eq!(std::fs::read(&paths.settings_file).unwrap(), damaged);
        assert_eq!(std::fs::read(&paths.global_config_file).unwrap(), damaged);
    }
}
