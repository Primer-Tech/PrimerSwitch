#![cfg(target_os = "macos")]

use serde_json::{Value, json};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const SERVICE: &str = "Claude Code-credentials";
const ACCOUNT: &str = "claude-code-user";
const PASSWORD: &str = "PrimerSwitch-native-fixture-keychain-password";

fn security<I, S>(arguments: I) -> Result<Output, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("/usr/bin/security")
        .args(arguments)
        .output()
        .map_err(|error| format!("could not start security: {error}"))
}

fn security_ok<I, S>(arguments: I) -> Result<Vec<u8>, String>
where
    I: IntoIterator<Item = S> + Clone,
    S: AsRef<OsStr>,
{
    let output = security(arguments.clone())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "security command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn keychain_list() -> Result<Vec<String>, String> {
    let output = security_ok(["list-keychains", "-d", "user"])?;
    let paths = String::from_utf8_lossy(&output)
        .lines()
        .map(str::trim)
        .map(|line| line.trim_matches('"').to_owned())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("the macOS user keychain search list is empty".to_owned());
    }
    Ok(paths)
}

fn set_keychain_list(paths: &[String]) -> Result<(), String> {
    let mut arguments = vec![
        "list-keychains".to_owned(),
        "-d".to_owned(),
        "user".to_owned(),
        "-s".to_owned(),
    ];
    arguments.extend(paths.iter().cloned());
    security_ok(arguments).map(|_| ())
}

struct KeychainFixture {
    path: PathBuf,
    original_search_list: Vec<String>,
}

impl KeychainFixture {
    fn create(root: &Path) -> Result<Self, String> {
        let original_search_list = keychain_list()?;
        let path = root.join("primerswitch-fixture.keychain-db");
        let path_text = path.to_string_lossy().into_owned();
        security_ok(["create-keychain", "-p", PASSWORD, &path_text])?;
        let fixture = Self {
            path,
            original_search_list,
        };
        let setup = (|| {
            let path_text = fixture.path.to_string_lossy().into_owned();
            security_ok(["unlock-keychain", "-p", PASSWORD, &path_text])?;
            security_ok(["set-keychain-settings", "-lut", "3600", &path_text])?;
            set_keychain_list(&[path_text])?;
            Ok::<_, String>(())
        })();
        if let Err(error) = setup {
            drop(fixture);
            return Err(error);
        }
        Ok(fixture)
    }

    fn add_claude_item(&self, value: &Value) -> Result<(), String> {
        let path = self.path.to_string_lossy().into_owned();
        let password = serde_json::to_string(value).map_err(|error| error.to_string())?;
        security_ok([
            "add-generic-password",
            "-A",
            "-a",
            ACCOUNT,
            "-s",
            SERVICE,
            "-w",
            &password,
            &path,
        ])?;
        Ok(())
    }
}

impl Drop for KeychainFixture {
    fn drop(&mut self) {
        let _ = set_keychain_list(&self.original_search_list);
        let path = self.path.to_string_lossy().into_owned();
        let _ = security(["delete-keychain", &path]);
    }
}

fn write_report(path: Option<PathBuf>, checks: Value) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    let report = json!({
        "formatVersion": 1,
        "status": "passed",
        "scope": "Isolated macOS native Keychain and vault smoke; no provider/live account qualification",
        "checks": checks,
        "realCredentialsAccessed": false,
    });
    let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

#[test]
#[ignore = "runs only in the isolated hosted macOS qualification workflow"]
fn isolated_keychain_and_vault_round_trip() {
    if env::var("PRIMERSWITCH_MACOS_KEYCHAIN_QUALIFICATION").as_deref() != Ok("1") {
        return;
    }
    let result = (|| -> Result<Value, String> {
        let root = tempfile::tempdir().map_err(|error| error.to_string())?;
        // macOS exposes /var through a symlink to /private/var; the production
        // path guard deliberately rejects symlinked ancestors, so qualify using
        // the canonical temporary directory just as the native test helpers do.
        let root_path = root
            .path()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let home = root_path.join("fixture-home");
        fs::create_dir_all(home.join(".claude")).map_err(|error| error.to_string())?;
        let paths = switcher_platform::CliPaths::for_home(home.clone());
        fs::write(
            &paths.global_config_file,
            br#"{"oauthAccount":{"accountUuid":"fixture-owner-before"},"preferences":{"theme":"dark"}}"#,
        )
        .map_err(|error| error.to_string())?;

        let old_credentials = json!({
            "claudeAiOauth": {
                "accessToken": "fixture-old-access",
                "refreshToken": "fixture-old-refresh",
                "scopes": ["user:inference"]
            },
            "mcp": {"preserve": true}
        });
        let new_credentials = json!({
            "claudeAiOauth": {
                "accessToken": "fixture-new-access",
                "refreshToken": "fixture-new-refresh",
                "scopes": ["user:inference"],
                "futureField": 7
            },
            "mcp": "must-not-copy"
        });
        let new_identity = json!({
            "accountUuid": "fixture-owner-after",
            "emailAddress": "fixture@example.invalid"
        });
        let keychain = KeychainFixture::create(&root_path)?;
        keychain.add_claude_item(&old_credentials)?;
        let store = switcher_platform::ActiveStore::new(paths.clone());
        let before = store.read().map_err(|error| error.to_string())?;
        if before.credentials != old_credentials {
            return Err("native Keychain read did not return the fixture payload".to_owned());
        }

        let vault_directory = home.join(".primer-vault");
        let vault = switcher_platform::Vault::open(vault_directory.clone())
            .map_err(|error| error.to_string())?;
        let vault_payload = json!({"native": true, "sentinel": "fixture-vault-value"});
        vault
            .save("macos-native", &vault_payload)
            .map_err(|error| error.to_string())?;
        let reopened_vault = switcher_platform::Vault::open(vault_directory.clone())
            .map_err(|error| error.to_string())?;
        if reopened_vault
            .load::<Value>("macos-native")
            .map_err(|error| error.to_string())?
            != Some(vault_payload)
        {
            return Err("native Keychain vault key did not reopen the fixture record".to_owned());
        }
        if !vault_directory.join("master-key.keychain").is_file() {
            return Err("native Keychain vault marker was not created".to_owned());
        }

        store
            .switch(&new_credentials, &new_identity, &vault)
            .map_err(|error| error.to_string())?;
        let after = store.read().map_err(|error| error.to_string())?;
        if after.credentials != new_credentials || after.identity != new_identity {
            return Err("native Keychain switch did not persist the fixture payload".to_owned());
        }
        let config: Value = serde_json::from_slice(
            &fs::read(&paths.global_config_file).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if config.get("oauthAccount") != Some(&new_identity)
            || config.get("preferences") != Some(&json!({"theme": "dark"}))
        {
            return Err("native Keychain switch changed unrelated config fields".to_owned());
        }
        if paths.credentials_file.exists() {
            return Err("native macOS path unexpectedly created a credentials file".to_owned());
        }
        drop(keychain);
        Ok(json!({
            "temporaryKeychainCreated": true,
            "claudeKeychainReadWrite": true,
            "vaultKeychainRoundTrip": true,
            "unrelatedConfigPreserved": true,
            "fileFallbackAvoided": true,
        }))
    })();

    match result {
        Ok(checks) => {
            write_report(
                env::var_os("PRIMERSWITCH_MACOS_REPORT").map(PathBuf::from),
                checks,
            )
            .unwrap();
        }
        Err(error) => panic!("macOS native qualification failed: {error}"),
    }
}
