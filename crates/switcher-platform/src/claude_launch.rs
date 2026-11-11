//! Launching an installed native Claude Code executable in its own console window.
//!
//! The renderer supplies no executable and no command line: the model is one of a few
//! fixed choices, the folder comes from the native dialog, and every other argument is
//! built here. Arguments are passed as a list, never through a shell, and carry no
//! secret. Credentials stay in the profile files selected by `CLAUDE_CONFIG_DIR`.

use crate::{CliPaths, PlatformError, Result, claude_profile::LaunchPlan};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

/// The model choices offered when launching. `Default` leaves the choice to Claude
/// Code and the shared user settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchModel {
    Default,
    Opus,
    Sonnet,
    Fable,
}

impl LaunchModel {
    /// The `--model` value. Aliases follow the newest release of their family; Fable
    /// uses its documented identifier.
    pub fn argument(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Opus => Some("opus"),
            Self::Sonnet => Some("sonnet"),
            Self::Fable => Some("claude-fable-5-2"),
        }
    }
}

/// Variables that select another authentication, provider, host-managed session or
/// nested-session state. Names are upper case; Windows compares case-insensitively.
const ALTERNATE_AUTH_VARIABLES: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SSE_PORT",
    "CLAUDE_CODE_ACCOUNT_UUID",
    "CLAUDE_CODE_USER_EMAIL",
    "CLAUDE_CODE_ORGANIZATION_UUID",
    "CLAUDE_CODE_API_KEY_HELPER_TTL_MS",
    "CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR",
    "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
    "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
    "CLAUDE_SESSION_INGRESS_TOKEN_FILE",
    "ANTHROPIC_PROFILE",
    "ANTHROPIC_CONFIG_DIR",
    "ANTHROPIC_IDENTITY_TOKEN",
    "ANTHROPIC_IDENTITY_TOKEN_FILE",
    "ANTHROPIC_FEDERATION_RULE_ID",
    "ANTHROPIC_ORGANIZATION_ID",
    "ANTHROPIC_WORKSPACE_ID",
    "ANTHROPIC_CUSTOM_HEADERS",
    "AWS_BEARER_TOKEN_BEDROCK",
];
/// Families of such variables, so a selector added by a newer Claude Code release is
/// not forwarded either.
const ALTERNATE_AUTH_PREFIXES: &[&str] = &[
    "ANTHROPIC_",
    "CLAUDE_CODE_USE_",
    "CLAUDE_CODE_SKIP_",
    "CLAUDE_CODE_OAUTH_",
    "CLAUDE_CODE_SESSION",
    "CLAUDE_CODE_REMOTE",
    "CLAUDE_CODE_HOST",
    "CLAUDE_CODE_PROVIDER",
    "CLAUDE_CODE_CUSTOM_OAUTH",
    "CLAUDE_CODE_API_KEY",
    "CLAUDE_SESSION",
    "CLAUDE_LOCAL_OAUTH",
    "CLAUDE_SECURESTORAGE",
    "AWS_BEARER_TOKEN_",
];

/// Model selectors are preferences, not authentication; they are left alone.
fn is_model_selector(upper: &str) -> bool {
    upper.starts_with("ANTHROPIC_") && (upper.ends_with("_MODEL") || upper.contains("_MODEL_"))
}

/// True when a variable of the launching process must not reach the new session. Every
/// other variable (PATH, proxies, tokens an MCP server needs) is inherited unchanged.
fn is_alternate_auth(name: &OsStr) -> bool {
    let upper = name.to_string_lossy().to_ascii_uppercase();
    if upper == "CLAUDE_CONFIG_DIR" {
        // Set explicitly to the profile.
        return false;
    }
    if CliPaths::context_variables().contains(&upper.as_str())
        || ALTERNATE_AUTH_VARIABLES.contains(&upper.as_str())
    {
        return true;
    }
    !is_model_selector(&upper)
        && ALTERNATE_AUTH_PREFIXES
            .iter()
            .any(|prefix| upper.starts_with(prefix))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub working_dir: PathBuf,
    pub env_set: Vec<(OsString, OsString)>,
}

impl LaunchSpec {
    /// Whether the launcher drops this inherited environment variable.
    pub fn removes(&self, name: &OsStr) -> bool {
        is_alternate_auth(name)
    }
}

/// A project folder must be an existing absolute directory. It is used as the working
/// directory and never becomes part of a command line.
pub fn validate_launch_folder(folder: &Path) -> Result<PathBuf> {
    if !folder.is_absolute()
        || folder
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || !fs::metadata(folder).is_ok_and(|meta| meta.is_dir())
    {
        return Err(PlatformError::UnsafePath);
    }
    Ok(folder.to_path_buf())
}

/// Settings from the shared, project and profile layers that would select another
/// billing path or account. A launch stops with the setting's name (never its value)
/// instead of silently running on an API key, helper, provider route or other login.
fn validate_launch_settings(path: &Path, own_policy: bool) -> Result<()> {
    const BLOCKED_KEYS: [&str; 6] = [
        "apiKeyHelper",
        "awsAuthRefresh",
        "awsCredentialExport",
        "gcpAuthRefresh",
        "forceLoginMethod",
        "forceLoginOrgUUID",
    ];
    let bytes = match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= 4 * 1024 * 1024 => fs::read(path)?,
        Ok(meta) if meta.is_file() => return Err(PlatformError::InvalidJson),
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(PlatformError::Io),
    };
    let settings: Value = serde_json::from_slice(&bytes)?;
    let object = settings.as_object().ok_or(PlatformError::InvalidJson)?;
    for key in BLOCKED_KEYS {
        // The profile's own settings carry the login pins this launcher writes.
        let pinned = own_policy && matches!(key, "forceLoginMethod" | "forceLoginOrgUUID");
        if !pinned && object.contains_key(key) {
            return Err(PlatformError::UnsupportedSetting(key));
        }
    }
    if let Some(environment) = object.get("env") {
        let environment = environment.as_object().ok_or(PlatformError::InvalidJson)?;
        for name in environment.keys() {
            // The spawn spec overwrites the inherited CLAUDE_CONFIG_DIR, which is why the
            // process-environment check above lets it through. A settings layer applies
            // after the spawn, so here it would redirect the credential store: refuse it
            // in every spelling and every layer (the profile's own file included), and
            // name only the variable, never its value.
            if name.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR") {
                return Err(PlatformError::UnsupportedSetting("CLAUDE_CONFIG_DIR"));
            }
            if !is_alternate_auth(OsStr::new(name)) {
                continue;
            }
            let upper = name.to_ascii_uppercase();
            let fixed = CliPaths::context_variables()
                .iter()
                .chain(ALTERNATE_AUTH_VARIABLES)
                .find(|fixed| **fixed == upper);
            return Err(PlatformError::UnsupportedSetting(
                fixed.copied().unwrap_or("env"),
            ));
        }
    }
    Ok(())
}

pub fn build_launch_spec(
    executable: &Path,
    folder: &Path,
    model: LaunchModel,
    plan: &LaunchPlan,
) -> Result<LaunchSpec> {
    if !is_native_executable_name(executable) || !executable.is_absolute() {
        return Err(PlatformError::UnsafePath);
    }
    let folder = validate_launch_folder(folder)?;
    // Every settings layer the session will load, before anything is started.
    if let Some(shared) = &plan.settings {
        validate_launch_settings(shared, false)?;
    }
    validate_launch_settings(&plan.config_dir.join("settings.json"), true)?;
    validate_launch_settings(&folder.join(".claude").join("settings.json"), false)?;
    validate_launch_settings(&folder.join(".claude").join("settings.local.json"), false)?;
    let mut args: Vec<OsString> = Vec::new();
    if let Some(settings) = &plan.settings {
        args.push("--settings".into());
        args.push(settings.as_os_str().to_owned());
    }
    if let Some(mcp) = &plan.mcp_config {
        args.push("--mcp-config".into());
        args.push(mcp.as_os_str().to_owned());
    }
    if let Some(model) = model.argument() {
        args.push("--model".into());
        args.push(model.into());
    }
    Ok(LaunchSpec {
        executable: executable.to_path_buf(),
        args,
        working_dir: folder,
        env_set: vec![(
            OsString::from("CLAUDE_CONFIG_DIR"),
            plan.config_dir.as_os_str().to_owned(),
        )],
    })
}

/// A native executable, not a script shim: `.cmd` and `.bat` files go through cmd.exe
/// and its argument parsing, which this launcher never uses.
fn is_native_executable_name(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// Looks for an installed native `claude.exe`. Relative `PATH` entries are ignored.
pub fn find_claude_executable_in(
    path_var: Option<&OsStr>,
    home: Option<&Path>,
    app_data: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path_var) = path_var {
        candidates.extend(
            std::env::split_paths(path_var)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join("claude.exe")),
        );
    }
    if let Some(home) = home {
        candidates.push(home.join(".local").join("bin").join("claude.exe"));
    }
    if let Some(app_data) = app_data {
        candidates.push(
            app_data
                .join("npm")
                .join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin")
                .join("claude.exe"),
        );
    }
    candidates.into_iter().find(|candidate| {
        candidate.is_absolute()
            && is_native_executable_name(candidate)
            && fs::metadata(candidate).is_ok_and(|meta| meta.is_file())
    })
}

/// Process launching, replaceable in tests. The production implementation starts the
/// executable in a new console window and returns without waiting for it.
pub trait ClaudeLauncher: Send + Sync {
    fn executable(&self) -> Option<PathBuf>;
    fn spawn(&self, spec: &LaunchSpec) -> io::Result<()>;
}

pub struct SystemLauncher;

impl ClaudeLauncher for SystemLauncher {
    fn executable(&self) -> Option<PathBuf> {
        if !cfg!(windows) {
            return None;
        }
        find_claude_executable_in(
            std::env::var_os("PATH").as_deref(),
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .as_deref(),
            std::env::var_os("APPDATA").map(PathBuf::from).as_deref(),
        )
    }

    #[cfg(windows)]
    fn spawn(&self, spec: &LaunchSpec) -> io::Result<()> {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let mut command = std::process::Command::new(&spec.executable);
        command
            .args(&spec.args)
            .current_dir(&spec.working_dir)
            .creation_flags(CREATE_NEW_CONSOLE);
        for (name, _) in std::env::vars_os() {
            if spec.removes(&name) {
                command.env_remove(&name);
            }
        }
        for (name, value) in &spec.env_set {
            command.env(name, value);
        }
        // Dropping the handle detaches: the session lives in its own window.
        drop(command.spawn()?);
        Ok(())
    }

    #[cfg(not(windows))]
    fn spawn(&self, _spec: &LaunchSpec) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_profile::SharedResources;

    fn plan(root: &Path, settings: bool, mcp: bool) -> LaunchPlan {
        LaunchPlan {
            config_dir: root.join("profile dir").join("11111111"),
            settings: settings.then(|| root.join("home & more").join("settings.json")),
            mcp_config: mcp.then(|| root.join("profile dir").join("shared-mcp.json")),
            shared: SharedResources::default(),
            instructions: false,
        }
    }

    fn project(root: &Path) -> PathBuf {
        let folder = root.join("project");
        fs::create_dir_all(folder.join(".claude")).unwrap();
        folder
    }

    #[test]
    fn models_map_to_fixed_arguments_and_default_leaves_the_choice_alone() {
        assert_eq!(LaunchModel::Default.argument(), None);
        assert_eq!(LaunchModel::Opus.argument(), Some("opus"));
        assert_eq!(LaunchModel::Sonnet.argument(), Some("sonnet"));
        assert_eq!(LaunchModel::Fable.argument(), Some("claude-fable-5-2"));
        // The renderer can only name a model; an arbitrary value does not deserialize.
        assert_eq!(
            serde_json::from_str::<LaunchModel>("\"fable\"").unwrap(),
            LaunchModel::Fable
        );
        for bad in [
            "\"--dangerously-skip-permissions\"",
            "\"Opus\"",
            "\"\"",
            "1",
        ] {
            assert!(serde_json::from_str::<LaunchModel>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hostile_paths_stay_single_arguments() {
        let root = crate::files::test_root();
        // Spaces, quotes, shell and batch metacharacters, percent signs and Unicode.
        let folder_name = if cfg!(windows) {
            "Proiect ș \u{201c}x\u{201d} & (a) %PATH% ^ ! ; ,"
        } else {
            "Proiect ș \"x\" 'y' $(z) `w` & ; | > < * ?"
        };
        let folder = root.path().join(folder_name);
        fs::create_dir_all(&folder).unwrap();
        let executable = root.path().join("claude.exe");
        fs::write(&executable, b"").unwrap();
        let plan = plan(root.path(), true, true);
        let spec = build_launch_spec(&executable, &folder, LaunchModel::Sonnet, &plan).unwrap();
        assert_eq!(spec.working_dir, folder);
        assert_eq!(spec.executable, executable);
        assert_eq!(
            spec.args,
            vec![
                OsString::from("--settings"),
                plan.settings.clone().unwrap().into_os_string(),
                OsString::from("--mcp-config"),
                plan.mcp_config.clone().unwrap().into_os_string(),
                OsString::from("--model"),
                OsString::from("sonnet"),
            ]
        );
        // The chosen folder is a working directory, not an argument.
        assert!(spec.args.iter().all(|arg| arg != folder.as_os_str()));
        assert_eq!(
            spec.env_set,
            vec![(
                OsString::from("CLAUDE_CONFIG_DIR"),
                plan.config_dir.clone().into_os_string()
            )]
        );
        // Default model, no shared files: no extra arguments at all.
        let bare = build_launch_spec(
            &executable,
            &folder,
            LaunchModel::Default,
            &self::plan(root.path(), false, false),
        )
        .unwrap();
        assert!(bare.args.is_empty());
    }

    #[test]
    fn alternate_authentication_and_host_session_state_never_reaches_the_session() {
        let spec = LaunchSpec {
            executable: PathBuf::from("claude.exe"),
            args: vec![],
            working_dir: PathBuf::new(),
            env_set: vec![],
        };
        for removed in [
            "ANTHROPIC_API_KEY",
            "anthropic_auth_token",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_PROFILE",
            "ANTHROPIC_CONFIG_DIR",
            "ANTHROPIC_FEDERATION_RULE_ID",
            "ANTHROPIC_IDENTITY_TOKEN_FILE",
            "ANTHROPIC_SOME_FUTURE_SELECTOR",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_ANYTHING_NEW",
            "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
            "CLAUDE_CODE_HOST_CREDS_FILE",
            "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
            "CLAUDE_CODE_REMOTE_SESSION_ID",
            "CLAUDE_SESSION_INGRESS_TOKEN_FILE",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "AWS_BEARER_TOKEN_BEDROCK",
        ] {
            assert!(spec.removes(OsStr::new(removed)), "{removed}");
        }
        // Everything unrelated stays, including what an MCP server may need, model
        // preferences and the profile directory that is set explicitly.
        for kept in [
            "PATH",
            "USERPROFILE",
            "HTTPS_PROXY",
            "GITHUB_TOKEN",
            "AWS_PROFILE",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        ] {
            assert!(!spec.removes(OsStr::new(kept)), "{kept}");
        }
    }

    #[test]
    fn shared_project_and_profile_settings_cannot_switch_the_billing_path() {
        let root = crate::files::test_root();
        let folder = project(root.path());
        let executable = root.path().join("claude.exe");
        let plan = plan(root.path(), false, false);
        fs::create_dir_all(&plan.config_dir).unwrap();
        let launch =
            |plan: &LaunchPlan| build_launch_spec(&executable, &folder, LaunchModel::Default, plan);
        assert!(launch(&plan).is_ok());
        let project_local = folder.join(".claude").join("settings.local.json");
        let project_shared = folder.join(".claude").join("settings.json");
        let profile_settings = plan.config_dir.join("settings.json");
        for (path, content, expected) in [
            (
                &project_local,
                r#"{"apiKeyHelper":"SECRET_SENTINEL"}"#,
                "apiKeyHelper",
            ),
            (
                &project_shared,
                r#"{"env":{"ANTHROPIC_API_KEY":"SECRET_SENTINEL"}}"#,
                "ANTHROPIC_API_KEY",
            ),
            (
                &project_shared,
                r#"{"env":{"anthropic_base_url":"https://example.invalid"}}"#,
                "ANTHROPIC_BASE_URL",
            ),
            (
                &project_shared,
                r#"{"env":{"ANTHROPIC_SOME_FUTURE_SELECTOR":"x"}}"#,
                "env",
            ),
            (
                &project_shared,
                r#"{"forceLoginMethod":"console"}"#,
                "forceLoginMethod",
            ),
            (
                &project_shared,
                r#"{"forceLoginOrgUUID":"other"}"#,
                "forceLoginOrgUUID",
            ),
            (
                &profile_settings,
                r#"{"awsAuthRefresh":"x"}"#,
                "awsAuthRefresh",
            ),
            (
                &profile_settings,
                r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#,
                "CLAUDE_CODE_USE_BEDROCK",
            ),
            // The launcher sets the profile directory itself. No settings layer may
            // redirect it afterwards, in any spelling, including the profile's own file.
            (
                &project_shared,
                r#"{"env":{"CLAUDE_CONFIG_DIR":"C:/redirect/SECRET_SENTINEL"}}"#,
                "CLAUDE_CONFIG_DIR",
            ),
            (
                &project_local,
                r#"{"env":{"CLAUDE_CONFIG_DIR":"C:/redirect/SECRET_SENTINEL"}}"#,
                "CLAUDE_CONFIG_DIR",
            ),
            (
                &profile_settings,
                r#"{"env":{"CLAUDE_CONFIG_DIR":"C:/redirect/SECRET_SENTINEL"}}"#,
                "CLAUDE_CONFIG_DIR",
            ),
            (
                &project_shared,
                r#"{"env":{"Claude_Config_Dir":"SECRET_SENTINEL"}}"#,
                "CLAUDE_CONFIG_DIR",
            ),
            (
                &project_local,
                r#"{"env":{"claude_config_dir":"SECRET_SENTINEL","MY_TOOL":"1"}}"#,
                "CLAUDE_CONFIG_DIR",
            ),
        ] {
            fs::write(path, content).unwrap();
            let error = launch(&plan).unwrap_err();
            assert!(
                matches!(error, PlatformError::UnsupportedSetting(name) if name == expected),
                "{content}"
            );
            assert!(!error.to_string().contains("SENTINEL"));
            fs::remove_file(path).unwrap();
        }
        // The pins this launcher writes into the profile are not a conflict, and
        // ordinary project settings (including unrelated variables) are fine.
        fs::write(
            &profile_settings,
            r#"{"forceLoginMethod":"claudeai","forceLoginOrgUUID":"org"}"#,
        )
        .unwrap();
        fs::write(&project_shared, r#"{"env":{"MY_TOOL":"1"},"model":"opus"}"#).unwrap();
        assert!(launch(&plan).is_ok());
        // A shared user settings file is checked as well.
        let mut with_shared = plan.clone();
        let shared = root.path().join("shared-settings.json");
        fs::write(&shared, r#"{"apiKeyHelper":"x"}"#).unwrap();
        with_shared.settings = Some(shared);
        assert!(matches!(
            launch(&with_shared),
            Err(PlatformError::UnsupportedSetting("apiKeyHelper"))
        ));
        // The shared user file may not redirect the profile directory either, in any
        // spelling, and the value is never reported.
        for spelling in [
            "CLAUDE_CONFIG_DIR",
            "Claude_Config_Dir",
            "claude_config_dir",
        ] {
            let content = format!(r#"{{"env":{{"{spelling}":"SECRET_SENTINEL"}}}}"#);
            fs::write(with_shared.settings.as_ref().unwrap(), content).unwrap();
            let error = launch(&with_shared).unwrap_err();
            assert!(
                matches!(
                    error,
                    PlatformError::UnsupportedSetting("CLAUDE_CONFIG_DIR")
                ),
                "{spelling}"
            );
            assert!(error.to_string().contains("CLAUDE_CONFIG_DIR"));
            assert!(!error.to_string().contains("SENTINEL"));
        }
        // An unreadable settings file cannot be proven safe.
        fs::write(&project_local, b"{not json").unwrap();
        assert!(matches!(launch(&plan), Err(PlatformError::InvalidJson)));
    }

    #[test]
    fn only_an_absolute_native_executable_and_an_existing_folder_are_accepted() {
        let root = crate::files::test_root();
        let folder = project(root.path());
        let plan = plan(root.path(), false, false);
        let exe = root.path().join("claude.exe");
        for shim in ["claude.cmd", "claude.bat", "claude.ps1", "claude"] {
            assert!(
                build_launch_spec(
                    &root.path().join(shim),
                    &folder,
                    LaunchModel::Default,
                    &plan
                )
                .is_err(),
                "{shim}"
            );
        }
        assert!(
            build_launch_spec(
                Path::new("claude.exe"),
                &folder,
                LaunchModel::Default,
                &plan
            )
            .is_err()
        );
        let file = root.path().join("a-file");
        fs::write(&file, b"x").unwrap();
        // `PathBuf::join` normalizes a verbatim Windows root, so the parent segment is
        // written into the plain (non-verbatim) path text instead.
        let with_parent = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\..\Windows")
        } else {
            PathBuf::from("/usr/../usr")
        };
        for bad in [
            root.path().join("missing"),
            file,
            PathBuf::from("relative"),
            with_parent,
        ] {
            assert!(
                matches!(
                    build_launch_spec(&exe, &bad, LaunchModel::Default, &plan),
                    Err(PlatformError::UnsafePath)
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn executable_discovery_ignores_relative_entries_and_script_shims() {
        let root = crate::files::test_root();
        let bin = root.path().join("bin");
        let shim_dir = root.path().join("shims");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&shim_dir).unwrap();
        fs::write(shim_dir.join("claude.cmd"), b"").unwrap();
        let join = |dirs: &[&Path]| std::env::join_paths(dirs.iter().copied()).unwrap();
        assert_eq!(
            find_claude_executable_in(Some(&join(&[&shim_dir, Path::new("relative")])), None, None),
            None
        );
        // A directory named like the executable is not an installation.
        fs::create_dir_all(bin.join("claude.exe")).unwrap();
        assert_eq!(
            find_claude_executable_in(Some(&join(&[&bin])), None, None),
            None
        );
        fs::remove_dir(bin.join("claude.exe")).unwrap();
        fs::write(bin.join("claude.exe"), b"").unwrap();
        assert_eq!(
            find_claude_executable_in(Some(&join(&[&shim_dir, &bin])), None, None),
            Some(bin.join("claude.exe"))
        );
        // The native installer location is used when PATH has nothing.
        let home = root.path().join("user");
        fs::create_dir_all(home.join(".local/bin")).unwrap();
        fs::write(home.join(".local/bin/claude.exe"), b"").unwrap();
        assert_eq!(
            find_claude_executable_in(None, Some(&home), None),
            Some(home.join(".local/bin/claude.exe"))
        );
        assert_eq!(find_claude_executable_in(None, None, None), None);
    }
}
