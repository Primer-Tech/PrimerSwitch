//! Pinned, conservative CLI context qualification. Never exports source content.
use crate::codex_views::CodexReason;
use provider_codex::ConfigurationInspection;
use serde_json::Value;
use std::path::{Path, PathBuf};
use switcher_platform::codex::*;
use zeroize::Zeroizing;

pub(crate) struct ContextPaths {
    pub home: PathBuf,
    pub system_config: PathBuf,
    pub system_requirements: PathBuf,
    pub legacy_requirements: PathBuf,
    pub enforce_native_policy: bool,
    #[cfg(test)]
    pub project_ancestor_stop: Option<PathBuf>,
}
pub(crate) struct QualifiedContext {
    pub context: Option<CodexFileContext>,
    pub active_model: Option<String>,
}
impl ContextPaths {
    /// Credential variables in this environment. Interactive Codex ignores most of them
    /// while a ChatGPT login exists, but `codex exec` prefers CODEX_API_KEY and the
    /// others redirect sign-in, so the dashboard shows a warning instead of blocking.
    pub fn credential_overrides_present() -> bool {
        [
            "CODEX_API_KEY",
            "CODEX_ACCESS_TOKEN",
            "OPENAI_BASE_URL",
            "OPENAI_FEDERATION_RULE_ID",
            "OPENAI_IDENTITY_TOKEN_FILE",
            "OPENAI_WORKLOAD_IDENTITY_CONTEXT",
            "CODEX_APP_SERVER_LOGIN_CLIENT_ID",
            "CODEX_REFRESH_TOKEN_URL_OVERRIDE",
            "CODEX_REVOKE_TOKEN_URL_OVERRIDE",
            "CODEX_AUTHAPI_BASE_URL",
            "CODEX_INTERNAL_ORIGINATOR_OVERRIDE",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    }
    pub fn discover() -> Result<Self, CodexReason> {
        let home = match std::env::var_os("CODEX_HOME") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            Some(_) => return Err(CodexReason::UnsupportedStore),
            None => {
                let user = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .ok_or(CodexReason::UnsupportedStore)?;
                PathBuf::from(user).join(".codex")
            }
        };
        if !home.is_absolute() {
            return Err(CodexReason::UnsupportedStore);
        }
        let system = system_directory()?;
        Ok(Self {
            legacy_requirements: if cfg!(windows) {
                home.join("managed_config.toml")
            } else {
                system.join("managed_config.toml")
            },
            home,
            system_config: system.join("config.toml"),
            system_requirements: system.join("requirements.toml"),
            enforce_native_policy: true,
            #[cfg(test)]
            project_ancestor_stop: None,
        })
    }
    pub fn qualify(
        &self,
        executable: &Path,
        cwd: &Path,
        version: &str,
    ) -> Result<QualifiedContext, CodexReason> {
        if self.enforce_native_policy {
            macos_policy_is_unrestricted().map_err(|_| CodexReason::PolicyRestricted)?;
            // Check environment again, including changes since installation discovery.
            let current = Self::discover()?;
            if current.home != self.home {
                return Err(CodexReason::ExternalChange);
            }
        }
        #[cfg(unix)]
        if self.home.is_dir() {
            use std::os::unix::fs::PermissionsExt;
            let metadata =
                std::fs::metadata(&self.home).map_err(|_| CodexReason::UnsupportedStore)?;
            if metadata.permissions().mode() & 0o022 != 0 {
                return Err(CodexReason::UnsupportedStore);
            }
        }
        let mut sources = Vec::new();
        let mut capture = |role,
                           path: PathBuf|
         -> Result<Option<Zeroizing<Vec<u8>>>, CodexReason> {
            let (source, bytes) = CodexWatchedSource::capture(role, path).map_err(store_reason)?;
            if role == CodexSourceRole::Project && source.is_directory() {
                return Err(CodexReason::PolicyRestricted);
            }
            sources.push(source);
            Ok(bytes)
        };
        capture(CodexSourceRole::Executable, executable.to_owned())?;
        let system = parse_config(capture(
            CodexSourceRole::System,
            self.system_config.clone(),
        )?)?;
        validate_config(&system)?;
        for path in [&self.system_requirements, &self.legacy_requirements] {
            let config = parse_config(capture(CodexSourceRole::Managed, path.clone())?)?;
            // Managed exceptions are a separate compatibility gate, even if currently benign.
            if !config.as_table().is_some_and(|v| v.is_empty()) {
                return Err(CodexReason::PolicyRestricted);
            }
        }
        let user = parse_config(capture(
            CodexSourceRole::User,
            self.home.join("config.toml"),
        )?)?;
        validate_config(&user)?;
        // The owned cwd lives under the user's temp directory, so a project layer
        // (`.codex/config.toml` in an ancestor, e.g. a home that is a git repo) can
        // apply to the Codex children: hold it to the same rules as the user config.
        // The home's own CODEX_HOME/config.toml is the user layer validated above.
        let user_layer = self.home.join("config.toml").canonicalize().ok();
        for parent in cwd.ancestors() {
            let candidate = parent.join(".codex").join("config.toml");
            if candidate.canonicalize().ok().is_none_or(|path| Some(path) != user_layer)
                && let Some(bytes) = capture(CodexSourceRole::Project, candidate)?
            {
                validate_config(&parse_config(Some(bytes))?)?;
            }
            #[cfg(test)]
            if self.project_ancestor_stop.as_deref() == Some(parent) {
                break;
            }
        }
        sources.push(CodexWatchedSource::default_session());
        let (_, cloud) = CodexWatchedSource::capture(
            CodexSourceRole::Managed,
            self.home.join("cloud-config-bundle-cache.json"),
        )
        .map_err(store_reason)?;
        validate_cloud_cache(cloud.as_deref().map(Vec::as_slice))?;
        // Auth-scoped cloud cache changes are expected during a controlled Codex request.
        // Static config receipts remain fixed; the new cache and actual official effective
        // requirements are qualified again after shutdown before adopting any rotation.
        let context = if self.home.is_dir() {
            let (receipt, _) = CodexWatchedSource::capture(
                CodexSourceRole::Managed,
                self.home.join("cloud-config-bundle-cache.json"),
            )
            .map_err(store_reason)?;
            sources.push(receipt);
            Some(
                CodexFileContext::qualify(
                    self.home.clone(),
                    CodexContextEvidence {
                        version: version.into(),
                        schema: provider_codex::PINNED_SOURCE_COMMIT.into(),
                        storage: CodexStorageMode::File,
                        provider: CodexProviderMode::DefaultOpenAi,
                        policy: CodexPolicyState::Unrestricted,
                        sources,
                    },
                )
                .map_err(store_reason)?,
            )
        } else {
            if self.home.exists() {
                return Err(CodexReason::UnsupportedStore);
            }
            None
        };
        let active_model = user
            .get("model")
            .or_else(|| system.get("model"))
            .and_then(toml::Value::as_str)
            .and_then(safe_text);
        Ok(QualifiedContext {
            context,
            active_model,
        })
    }
}
fn parse_config(bytes: Option<Zeroizing<Vec<u8>>>) -> Result<toml::Value, CodexReason> {
    let Some(bytes) = bytes else {
        return Ok(toml::Value::Table(Default::default()));
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| CodexReason::PolicyRestricted)?;
    toml::from_str(text).map_err(|_| CodexReason::PolicyRestricted)
}
fn validate_config(value: &toml::Value) -> Result<(), CodexReason> {
    let table = value.as_table().ok_or(CodexReason::PolicyRestricted)?;
    // Codex must read ChatGPT sign-in from auth.json in this home.
    if table
        .get("cli_auth_credentials_store")
        .is_some_and(|v| v.as_str() != Some("file"))
    {
        return Err(CodexReason::UnsupportedStore);
    }
    if table
        .get("model_provider")
        .is_some_and(|v| v.as_str() != Some("openai"))
    {
        return Err(CodexReason::UnsupportedAuth);
    }
    if table
        .get("forced_login_method")
        .is_some_and(|v| v.as_str() != Some("chatgpt"))
    {
        return Err(CodexReason::PolicyRestricted);
    }
    // A forced workspace makes Codex sign out logins from any other workspace; a
    // default profile can change provider/login; custom endpoints change who
    // receives the subscription token.
    for key in [
        "forced_chatgpt_workspace_id",
        "profile",
        "openai_base_url",
        "chatgpt_base_url",
    ] {
        if table.contains_key(key) {
            return Err(CodexReason::PolicyRestricted);
        }
    }
    if table
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .is_some_and(|providers| providers.contains_key("openai"))
    {
        return Err(CodexReason::PolicyRestricted);
    }
    Ok(())
}
fn validate_cloud_cache(bytes: Option<&[u8]>) -> Result<(), CodexReason> {
    let Some(bytes) = bytes else {
        return Ok(());
    };
    let value: Value = serde_json::from_slice(bytes).map_err(|_| CodexReason::PolicyRestricted)?;
    let payload = &value["signed_payload"];
    if payload["version"] != 1 {
        return Err(CodexReason::PolicyRestricted);
    }
    let bundle = payload["bundle"]
        .as_object()
        .ok_or(CodexReason::PolicyRestricted)?;
    if bundle.len() != 2 {
        return Err(CodexReason::PolicyRestricted);
    }
    for key in ["config_toml", "requirements_toml"] {
        let section = bundle
            .get(key)
            .and_then(Value::as_object)
            .ok_or(CodexReason::PolicyRestricted)?;
        if section.len() != 1
            || !section
                .get("enterprise_managed")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            return Err(CodexReason::PolicyRestricted);
        }
    }
    // A cache is only a conservative denial signal, never independent identity or
    // policy proof. Codex itself checks its signature, scope, TTL and live bundle.
    Ok(())
}
pub(crate) fn validate_inspection(
    inspection: &ConfigurationInspection,
) -> Result<Option<String>, CodexReason> {
    let effective = inspection.effective_config();
    let config = effective
        .get("config")
        .and_then(Value::as_object)
        .ok_or(CodexReason::PolicyRestricted)?;
    for key in [
        "forced_login_method",
        "forced_chatgpt_workspace_id",
        "profile",
        "openai_base_url",
        "responses_api_metadata",
        "cloud",
        "experimental_thread_store",
        "experimental_thread_store_endpoint",
    ] {
        if config.get(key).is_some_and(|v| !v.is_null()) {
            return Err(CodexReason::PolicyRestricted);
        }
    }
    for key in ["model_providers", "profiles"] {
        if config
            .get(key)
            .is_some_and(|v| !v.is_null() && !v.as_object().is_some_and(serde_json::Map::is_empty))
        {
            return Err(CodexReason::PolicyRestricted);
        }
    }
    if config
        .get("cli_auth_credentials_store")
        .is_some_and(|v| !v.is_null() && v != "file")
        || config
            .get("model_provider")
            .is_some_and(|v| !v.is_null() && v != "openai")
    {
        return Err(CodexReason::PolicyRestricted);
    }
    if config
        .get("chatgpt_base_url")
        .is_some_and(|v| !v.is_null() && v != "https://chatgpt.com/backend-api/")
    {
        return Err(CodexReason::PolicyRestricted);
    }
    for (key, value) in config {
        if value.is_null() {
            continue;
        }
        if ![
            "cli_auth_credentials_store",
            "model_provider",
            "mcp_oauth_credentials_store",
            "mcp_enterprise_managed_auth",
            "mcp_oauth_callback_port",
            "mcp_oauth_callback_url",
            "allow_login_shell",
            "model_auto_compact_token_limit",
            "tool_output_token_limit",
            "model_auto_compact_token_limit_scope",
        ]
        .contains(&key.as_str())
            && (key.contains("credential")
                || key.contains("auth")
                || key.contains("login")
                || key.contains("api_key")
                || key.contains("token"))
        {
            return Err(CodexReason::PolicyRestricted);
        }
    }
    let requirements = inspection
        .requirements()
        .get("requirements")
        .ok_or(CodexReason::PolicyRestricted)?;
    if !requirements.is_null() {
        let object = requirements
            .as_object()
            .ok_or(CodexReason::PolicyRestricted)?;
        for key in [
            "model_provider",
            "model_providers",
            "allowedLoginMethods",
            "cliAuthCredentialsStore",
            "chatgptBaseUrl",
            "enforceResidency",
            "modelProvider",
            "modelProviders",
        ] {
            if object.get(key).is_some_and(|v| !v.is_null()) {
                return Err(CodexReason::PolicyRestricted);
            }
        }
        // Explicit unknown requirements fail closed; known non-auth restrictions
        // are left to the official client and never overridden.
        const KNOWN: &[&str] = &[
            "allowedApprovalPolicies",
            "allowedApprovalsReviewers",
            "allowedSandboxModes",
            "allowedWindowsSandboxImplementations",
            "allowedPermissionProfiles",
            "defaultPermissions",
            "allowedWebSearchModes",
            "allowManagedHooksOnly",
            "allowBrowserAndComputerUse",
            "allowAppshots",
            "allowRemoteControl",
            "computerUse",
            "browserUse",
            "inAppBrowser",
            "featureRequirements",
            "hooks",
            "network",
            "application",
            "additionalDeveloperInstructions",
            "modelProvider",
            "modelProviders",
            "allowedLoginMethods",
            "cliAuthCredentialsStore",
            "chatgptBaseUrl",
            "enforceResidency",
        ];
        for (key, value) in object {
            if value.is_null() {
                continue;
            }
            if !KNOWN.contains(&key.as_str()) {
                return Err(CodexReason::PolicyRestricted);
            }
            let valid = match key.as_str() {
                "allowedApprovalPolicies"
                | "allowedApprovalsReviewers"
                | "allowedSandboxModes"
                | "allowedWindowsSandboxImplementations"
                | "allowedWebSearchModes" => value
                    .as_array()
                    .is_some_and(|items| items.iter().all(|v| v.is_string() || v.is_object())),
                "allowedPermissionProfiles" | "featureRequirements" => value
                    .as_object()
                    .is_some_and(|items| items.values().all(Value::is_boolean)),
                "allowManagedHooksOnly"
                | "allowBrowserAndComputerUse"
                | "allowAppshots"
                | "allowRemoteControl" => value.is_boolean(),
                "defaultPermissions" | "additionalDeveloperInstructions" => value.is_string(),
                _ => value.is_object(),
            };
            if !valid {
                return Err(CodexReason::PolicyRestricted);
            }
        }
    }
    Ok(config
        .get("model")
        .and_then(Value::as_str)
        .and_then(safe_text))
}
pub(crate) fn safe_text(value: &str) -> Option<String> {
    (!value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}
pub(crate) fn store_reason(error: CodexStoreError) -> CodexReason {
    match error {
        CodexStoreError::UnsupportedContext => CodexReason::UnsupportedStore,
        CodexStoreError::UnsupportedAuth => CodexReason::UnsupportedAuth,
        CodexStoreError::ExternalChange => CodexReason::ExternalChange,
        CodexStoreError::Storage(_) => CodexReason::StoreConflict,
    }
}
#[cfg(windows)]
fn system_directory() -> Result<PathBuf, CodexReason> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
    };
    let mut pointer = std::ptr::null_mut();
    let result = unsafe {
        SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut pointer)
    };
    if result < 0 || pointer.is_null() {
        return Err(CodexReason::PolicyRestricted);
    }
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
        if length > 32768 {
            unsafe {
                CoTaskMemFree(pointer.cast());
            }
            return Err(CodexReason::PolicyRestricted);
        }
    }
    use std::os::windows::ffi::OsStringExt;
    let value =
        std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(pointer, length) });
    unsafe {
        CoTaskMemFree(pointer.cast());
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(CodexReason::PolicyRestricted);
    }
    Ok(path.join("OpenAI").join("Codex"))
}
#[cfg(not(windows))]
fn system_directory() -> Result<PathBuf, CodexReason> {
    Ok(PathBuf::from("/etc/codex"))
}

#[cfg(test)]
#[path = "codex_context_tests.rs"]
mod tests;
