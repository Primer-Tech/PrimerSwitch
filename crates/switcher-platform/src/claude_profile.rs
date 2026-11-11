//! Dedicated Claude Code profiles: one isolated `CLAUDE_CONFIG_DIR` per saved account.
//!
//! A profile owns only identity and account policy: its own `.credentials.json`, the
//! `oauthAccount` of its own `.claude.json`, and a `settings.json` that pins the login to
//! the account's workspace. Everything the user curates is shared with the ordinary Claude
//! configuration, either by directory links (history and project memory, skills, commands,
//! agents, plugins, rules, hooks) or by reading the original at launch (user settings
//! through `--settings`, MCP definitions through a derived private file, user instructions
//! through an import line). The whole configuration directory is never linked and
//! `oauthAccount` metadata is never shared. No provider request happens here, and the
//! global login, its credentials and its configuration are never written.
//!
//! Profile paths use this explicit type. `CliPaths` deliberately rejects authentication
//! context overrides, and global operations keep that restriction.

use crate::{CliPaths, PlatformError, Result, files};
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Conversation history and project memory. These are always created on the shared
/// side, and a launch refuses to continue when the profile holds its own copy, so memory
/// never forks silently.
pub const HISTORY_DIRECTORIES: &[&str] = &["projects", "todos", "file-history"];

/// Resource directories shared with the ordinary configuration by a directory link.
pub const SHARED_DIRECTORIES: &[&str] = &[
    "projects",
    "todos",
    "file-history",
    "skills",
    "commands",
    "agents",
    "plugins",
    "rules",
    "hooks",
    "output-styles",
    // Helper scripts that shared hooks and skills call.
    "scripts",
];

const MEMORY_MARKER: &str =
    "<!-- PrimerSwitch: shared user instructions. Managed file; edit the original CLAUDE.md. -->";

#[derive(Clone, Debug)]
pub struct ClaudeProfilePaths {
    pub config_dir: PathBuf,
    pub credentials_file: PathBuf,
    pub config_file: PathBuf,
    pub settings_file: PathBuf,
    pub memory_file: PathBuf,
    pub mcp_file: PathBuf,
}

impl ClaudeProfilePaths {
    /// `profiles_dir/<account id>`. The id names a directory, so only a plain stable
    /// identifier is accepted.
    pub fn new(profiles_dir: &Path, account_id: &str) -> Result<Self> {
        if !valid_profile_id(account_id) {
            return Err(PlatformError::UnsafePath);
        }
        let config_dir = profiles_dir.join(account_id);
        Ok(Self {
            credentials_file: config_dir.join(".credentials.json"),
            config_file: config_dir.join(".claude.json"),
            settings_file: config_dir.join("settings.json"),
            memory_file: config_dir.join("CLAUDE.md"),
            mcp_file: config_dir.join("shared-mcp.json"),
            config_dir,
        })
    }
}

fn valid_profile_id(id: &str) -> bool {
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !id.starts_with('-')
        && !RESERVED.contains(&id.to_ascii_lowercase().as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileState {
    /// No usable authorization yet (first use, or the profile was signed out).
    NeedsLogin,
    /// The profile is signed in as a different account or workspace.
    IdentityChanged,
    /// Signed in as the expected account and workspace.
    Ready,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedResources {
    /// Directories now linked to the ordinary configuration.
    pub linked: Vec<&'static str>,
    /// Real directories with content that were left alone instead of being replaced.
    pub separate: Vec<&'static str>,
}

impl SharedResources {
    /// The profile holds its own history or project memory next to the shared one.
    pub fn splits_history(&self) -> bool {
        self.separate
            .iter()
            .any(|name| HISTORY_DIRECTORIES.contains(name))
    }
}

/// The profile's own access grant, only for checking its owner with the provider. It
/// is never refreshed, copied or stored elsewhere by PrimerSwitch.
pub struct ProfileCredential {
    pub access_token: zeroize::Zeroizing<String>,
    pub expires_at_ms: Option<i64>,
}
impl std::fmt::Debug for ProfileCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileCredential")
            .field("access_token", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}
impl ProfileCredential {
    /// True while the token can be sent as is (two minutes of margin). An expired one
    /// belongs to the Claude Code session that renews it.
    pub fn is_current(&self, now: i64) -> bool {
        self.expires_at_ms
            .is_some_and(|expires| now < expires / 1000 - 120)
    }
}

#[derive(Clone, Debug)]
pub struct LaunchPlan {
    pub config_dir: PathBuf,
    /// The ordinary user settings file, loaded as an additional settings source.
    pub settings: Option<PathBuf>,
    /// Private derived file with the ordinary MCP server definitions.
    pub mcp_config: Option<PathBuf>,
    pub shared: SharedResources,
    /// The profile's `CLAUDE.md` imports the ordinary one.
    pub instructions: bool,
}

pub struct ClaudeProfile {
    paths: ClaudeProfilePaths,
    common: CliPaths,
}

impl ClaudeProfile {
    pub fn new(profiles_dir: &Path, account_id: &str, common: CliPaths) -> Result<Self> {
        Ok(Self {
            paths: ClaudeProfilePaths::new(profiles_dir, account_id)?,
            common,
        })
    }

    pub fn paths(&self) -> &ClaudeProfilePaths {
        &self.paths
    }

    /// Reads only the profile's own files. A profile that names another account or
    /// workspace is never reported as ready.
    pub fn state(&self, account_uuid: &str, organization_uuid: &str) -> Result<ProfileState> {
        let Some(config) = read_object(&self.paths.config_file)? else {
            return Ok(ProfileState::NeedsLogin);
        };
        let Some(owner) = config.get("oauthAccount").filter(|v| v.is_object()) else {
            return Ok(ProfileState::NeedsLogin);
        };
        let field = |name: &str| owner.get(name).and_then(Value::as_str);
        match (field("accountUuid"), field("organizationUuid")) {
            (Some(account), Some(organization))
                if account == account_uuid && organization == organization_uuid => {}
            (None, _) | (_, None) => return Ok(ProfileState::NeedsLogin),
            _ => return Ok(ProfileState::IdentityChanged),
        }
        let Some(credentials) = read_object(&self.paths.credentials_file)? else {
            return Ok(ProfileState::NeedsLogin);
        };
        let refreshable = credentials
            .get("claudeAiOauth")
            .and_then(|oauth| oauth.get("refreshToken"))
            .and_then(Value::as_str)
            .is_some_and(|token| !token.is_empty());
        Ok(if refreshable {
            ProfileState::Ready
        } else {
            ProfileState::NeedsLogin
        })
    }

    /// The profile's current access grant, when it has one.
    pub fn credential(&self) -> Result<Option<ProfileCredential>> {
        let Some(credentials) = read_object(&self.paths.credentials_file)? else {
            return Ok(None);
        };
        let Some(oauth) = credentials.get("claudeAiOauth") else {
            return Ok(None);
        };
        let token = oauth
            .get("accessToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty());
        Ok(token.map(|token| ProfileCredential {
            access_token: zeroize::Zeroizing::new(token.to_owned()),
            expires_at_ms: oauth.get("expiresAt").and_then(Value::as_i64),
        }))
    }

    /// Stores an independently issued authorization. Credentials are written first and
    /// the identity last, because `state` treats the identity as the commit marker.
    /// Unknown JSON in both profile files is preserved.
    pub fn authorize(&self, credentials: &Value, identity: &Value) -> Result<()> {
        let oauth = credentials
            .get("claudeAiOauth")
            .filter(|v| v.is_object())
            .ok_or(PlatformError::InvalidPayload)?;
        let token = |name: &str| {
            oauth
                .get(name)
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
        };
        let owner = |name: &str| {
            identity
                .get(name)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        };
        let (Some(_), Some(organization)) = (owner("accountUuid"), owner("organizationUuid"))
        else {
            return Err(PlatformError::InvalidPayload);
        };
        if !token("accessToken") || !token("refreshToken") {
            return Err(PlatformError::InvalidPayload);
        }
        self.ensure_directories()?;
        let mut auth = read_object(&self.paths.credentials_file)?.unwrap_or_default();
        auth.insert("claudeAiOauth".into(), oauth.clone());
        write_object(&self.paths.credentials_file, &auth)?;
        self.write_policy(organization)?;
        let mut config = read_object(&self.paths.config_file)?.unwrap_or_default();
        config.insert("oauthAccount".into(), identity.clone());
        config.insert("hasCompletedOnboarding".into(), Value::Bool(true));
        write_object(&self.paths.config_file, &config)
    }

    /// Refreshes everything derived from the ordinary configuration and returns what
    /// the launch needs. Refuses a profile that is not signed in as the expected owner.
    pub fn prepare(&self, account_uuid: &str, organization_uuid: &str) -> Result<LaunchPlan> {
        self.prepare_inner(account_uuid, organization_uuid, None)
    }

    /// Like `prepare`, and also carries the ordinary configuration's local-scope MCP
    /// definitions and approval choices for exactly this project folder.
    pub fn prepare_for_folder(
        &self,
        account_uuid: &str,
        organization_uuid: &str,
        folder: &Path,
    ) -> Result<LaunchPlan> {
        self.prepare_inner(account_uuid, organization_uuid, Some(folder))
    }

    fn prepare_inner(
        &self,
        account_uuid: &str,
        organization_uuid: &str,
        folder: Option<&Path>,
    ) -> Result<LaunchPlan> {
        if self.state(account_uuid, organization_uuid)? != ProfileState::Ready {
            return Err(PlatformError::InvalidPayload);
        }
        // API keys, helpers and provider routing in the shared settings would defeat
        // the subscription login this profile pins.
        self.common.validate_subscription_mode()?;
        self.ensure_directories()?;
        self.write_policy(organization_uuid)?;
        let mcp_config = self.write_shared_mcp(folder)?;
        let instructions = self.link_instructions()?;
        let shared = self.link_resources()?;
        Ok(LaunchPlan {
            config_dir: self.paths.config_dir.clone(),
            settings: existing_file(&self.common.settings_file)?,
            mcp_config,
            shared,
            instructions,
        })
    }

    /// Deletes the profile. Links are removed first so the shared real files can never
    /// be reached through them.
    pub fn remove(&self) -> Result<()> {
        let root = &self.paths.config_dir;
        files::check_path(root)?;
        match fs::symlink_metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(PlatformError::UnsafePath),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(PlatformError::Io),
        }
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink() {
                remove_link(&entry.path())?;
            }
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    fn ensure_directories(&self) -> Result<()> {
        let parent = self
            .paths
            .config_dir
            .parent()
            .ok_or(PlatformError::UnsafePath)?;
        files::private_dir(parent)?;
        files::private_dir(&self.paths.config_dir)
    }

    fn write_policy(&self, organization_uuid: &str) -> Result<()> {
        let mut settings = read_object(&self.paths.settings_file)?.unwrap_or_default();
        settings.insert("forceLoginMethod".into(), json!("claudeai"));
        settings.insert("forceLoginOrgUUID".into(), json!(organization_uuid));
        write_object_if_changed(&self.paths.settings_file, &settings)
    }

    /// MCP server definitions of the ordinary configuration: user scope, overridden by
    /// the chosen folder's local scope (the more specific one wins, as in Claude Code).
    /// Only definitions and the folder's approval choices are carried. Account
    /// metadata, MCP OAuth logins and the rest of the project record stay behind, and
    /// the ordinary `.claude.json` is only read.
    fn write_shared_mcp(&self, folder: Option<&Path>) -> Result<Option<PathBuf>> {
        let config = read_object(&self.common.global_config_file)?;
        let mut servers = match config.as_ref().and_then(|c| c.get("mcpServers")) {
            Some(Value::Object(servers)) => servers.clone(),
            _ => Map::new(),
        };
        if let (Some(config), Some(folder)) = (config.as_ref(), folder)
            && let Some((key, project)) = project_entry(config, folder)
        {
            if let Some(Value::Object(local)) = project.get("mcpServers") {
                for (name, definition) in local {
                    servers.insert(name.clone(), definition.clone());
                }
            }
            self.seed_project_choices(key, project)?;
        }
        if servers.is_empty() {
            files::remove(&self.paths.mcp_file)?;
            return Ok(None);
        }
        let mut document = Map::new();
        document.insert("mcpServers".into(), Value::Object(servers));
        write_object_if_changed(&self.paths.mcp_file, &document)?;
        Ok(Some(self.paths.mcp_file.clone()))
    }

    /// Approval choices for `.mcp.json` servers are remembered per project. They are
    /// copied into the profile's own record only where it has no choice yet, so a choice
    /// made inside the profile is never overwritten by an older ordinary one.
    fn seed_project_choices(&self, key: &str, project: &Map<String, Value>) -> Result<()> {
        const CHOICES: [&str; 4] = [
            "enabledMcpjsonServers",
            "disabledMcpjsonServers",
            "disabledMcpServers",
            "enableAllProjectMcpServers",
        ];
        let wanted: Vec<(&str, &Value)> = CHOICES
            .iter()
            .filter_map(|name| project.get(*name).map(|value| (*name, value)))
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let mut config = read_object(&self.paths.config_file)?.unwrap_or_default();
        let projects = config
            .entry("projects")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or(PlatformError::InvalidJson)?;
        let entry = projects
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or(PlatformError::InvalidJson)?;
        let mut changed = false;
        for (name, value) in wanted {
            if !entry.contains_key(name) {
                entry.insert(name.to_owned(), value.clone());
                changed = true;
            }
        }
        if changed {
            write_object(&self.paths.config_file, &config)?;
        }
        Ok(())
    }

    fn link_instructions(&self) -> Result<bool> {
        let source = self.common.config_dir.join("CLAUDE.md");
        let existing = files::read_optional(&self.paths.memory_file)?;
        // A CLAUDE.md the user wrote in the profile is never replaced.
        if existing
            .as_deref()
            .is_some_and(|bytes| !bytes.starts_with(MEMORY_MARKER.as_bytes()))
        {
            return Ok(false);
        }
        // A present source that cannot be written as an import is an explicit error,
        // never a silently lost memory.
        let import = existing_file(&source)?
            .map(|path| memory_import(&path))
            .transpose()?;
        let Some(import) = import else {
            files::remove(&self.paths.memory_file)?;
            return Ok(false);
        };
        let text = format!("{MEMORY_MARKER}\n@{import}\n");
        if existing.as_deref() != Some(text.as_bytes()) {
            files::atomic_write(&self.paths.memory_file, text.as_bytes())?;
        }
        Ok(true)
    }

    fn link_resources(&self) -> Result<SharedResources> {
        let mut shared = SharedResources::default();
        for name in SHARED_DIRECTORIES.iter().copied() {
            let target = self.common.config_dir.join(name);
            let link = self.paths.config_dir.join(name);
            // The shared side always exists, so a resource first created later (a plugin
            // install, a new skill) lands in the shared store instead of a copy that only
            // this profile sees. Content already there is never touched.
            match fs::metadata(&target) {
                Ok(meta) if meta.is_dir() => {}
                // A file (or anything else) where a directory belongs is an explicit
                // error, not a silently unshared resource.
                Ok(_) => return Err(PlatformError::UnsafePath),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    files::check_path(&target)?;
                    fs::create_dir_all(&target)?;
                }
                Err(_) => return Err(PlatformError::Io),
            }
            match fs::symlink_metadata(&link) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    create_link(&link, &target)?;
                    shared.linked.push(name);
                }
                Err(_) => return Err(PlatformError::Io),
                Ok(meta) if meta.file_type().is_symlink() => {
                    if !same_directory(&link, &target) {
                        remove_link(&link)?;
                        create_link(&link, &target)?;
                    }
                    shared.linked.push(name);
                }
                Ok(meta) if meta.is_dir() => {
                    // Claude Code may have created an empty directory before the shared
                    // one existed. Anything with content stays separate; it is never
                    // merged or deleted silently.
                    if fs::read_dir(&link)?.next().is_none() {
                        fs::remove_dir(&link)?;
                        create_link(&link, &target)?;
                        shared.linked.push(name);
                    } else {
                        shared.separate.push(name);
                    }
                }
                Ok(_) => shared.separate.push(name),
            }
        }
        Ok(shared)
    }
}

/// How Claude Code keys a project record: the working directory with forward slashes
/// (Windows), no trailing separator and no verbatim prefix. Windows paths compare
/// without regard to case.
fn normalize_project_key(text: &str) -> String {
    let mut text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_owned()
    };
    if let Some(rest) = text.strip_prefix("//?/") {
        text = rest.to_owned();
    }
    while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
        text.pop();
    }
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// The ordinary configuration's record for exactly this folder: never a parent, a child
/// or a sibling whose name merely starts the same way.
fn project_entry<'a>(
    config: &'a Map<String, Value>,
    folder: &Path,
) -> Option<(&'a str, &'a Map<String, Value>)> {
    let wanted = normalize_project_key(&folder.to_string_lossy());
    config
        .get("projects")?
        .as_object()?
        .iter()
        .find(|(key, _)| normalize_project_key(key) == wanted)
        .and_then(|(key, value)| Some((key.as_str(), value.as_object()?)))
}

/// The `@` import form of a CLAUDE.md path, as Claude Code's memory documentation
/// describes it: a space is written as a backslash and a space (quotes do not work),
/// the path ends at any other whitespace, and Windows paths use forward slashes. A path
/// that cannot be written without changing its meaning is refused instead of being
/// imported wrongly or dropped.
fn memory_import(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or(PlatformError::UnsafePath)?;
    let mut text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_owned()
    };
    if cfg!(windows) {
        if let Some(rest) = text.strip_prefix("//?/UNC/") {
            text = format!("//{rest}");
        } else if let Some(rest) = text.strip_prefix("//?/") {
            text = rest.to_owned();
        }
    }
    let mut import = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            ' ' => import.push_str("\\ "),
            // A backslash would read as an escape and a backtick starts a code span.
            '\\' | '`' => return Err(PlatformError::UnsafePath),
            c if c.is_whitespace() || c.is_control() => return Err(PlatformError::UnsafePath),
            c => import.push(c),
        }
    }
    Ok(import)
}

fn existing_file(path: &Path) -> Result<Option<PathBuf>> {
    files::check_path(path)?;
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(path.to_path_buf())),
        Ok(_) => Err(PlatformError::UnsafePath),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(PlatformError::Io),
    }
}

fn read_object(path: &Path) -> Result<Option<Map<String, Value>>> {
    match files::read_optional(path)? {
        None => Ok(None),
        Some(bytes) => match serde_json::from_slice::<Value>(&bytes)? {
            Value::Object(map) => Ok(Some(map)),
            _ => Err(PlatformError::InvalidJson),
        },
    }
}

fn write_object(path: &Path, value: &Map<String, Value>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    files::atomic_write(path, &bytes)
}

fn write_object_if_changed(path: &Path, value: &Map<String, Value>) -> Result<()> {
    if read_object(path)?.as_ref() == Some(value) {
        return Ok(());
    }
    write_object(path, value)
}

fn same_directory(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

/// Removes a link itself, never what it points to.
fn remove_link(path: &Path) -> Result<()> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(_) => fs::remove_file(path).map_err(Into::into),
    }
}

#[cfg(unix)]
fn create_link(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

/// A directory junction needs no privilege on Windows. The target is resolved first so
/// the junction never points through another link.
#[cfg(windows)]
fn create_link(link: &Path, target: &Path) -> Result<()> {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::{
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
        System::IO::DeviceIoControl,
    };
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    let resolved = fs::canonicalize(target)?;
    let mut name: Vec<u16> = resolved.as_os_str().encode_wide().collect();
    // `\\?\C:\x` becomes the NT form `\??\C:\x`.
    if name.len() < 5 || name[..4] != [92, 92, 63, 92] {
        return Err(PlatformError::UnsafePath);
    }
    name[1] = 63;
    let name_bytes = u16::try_from(name.len() * 2).map_err(|_| PlatformError::UnsafePath)?;
    // Four u16 fields, the substitute name and the empty print name, each NUL terminated.
    let data_length = 8u16
        .checked_add(name_bytes)
        .and_then(|n| n.checked_add(4))
        .ok_or(PlatformError::UnsafePath)?;
    let mut buffer: Vec<u8> = Vec::with_capacity(8 + data_length as usize);
    buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&data_length.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes()); // substitute name offset
    buffer.extend_from_slice(&name_bytes.to_le_bytes());
    buffer.extend_from_slice(&(name_bytes + 2).to_le_bytes()); // print name offset
    buffer.extend_from_slice(&0u16.to_le_bytes()); // print name length
    for unit in &name {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }
    buffer.extend_from_slice(&[0, 0, 0, 0]);
    fs::create_dir(link)?;
    let result = (|| {
        let handle = fs::OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(link)?;
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir(link);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "claude_profile_sharing_tests.rs"]
mod sharing_tests;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const ACCOUNT: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";

    pub(crate) struct Fixture {
        pub root: tempfile::TempDir,
        pub common: CliPaths,
        pub profiles: PathBuf,
    }

    pub(crate) fn fixture() -> Fixture {
        let root = files::test_root();
        let common = CliPaths::for_home(root.path().join("home"));
        fs::create_dir_all(&common.config_dir).unwrap();
        let profiles = root.path().join("data").join("claude-profiles");
        Fixture {
            root,
            common,
            profiles,
        }
    }

    pub(crate) fn credentials(marker: &str) -> Value {
        json!({"claudeAiOauth":{"accessToken":format!("access-{marker}"),"refreshToken":format!("refresh-{marker}"),"expiresAt":1,"scopes":["user:profile"]}})
    }

    pub(crate) fn identity(account: &str, organization: &str) -> Value {
        json!({"accountUuid":account,"organizationUuid":organization,"emailAddress":"owner@example.invalid"})
    }

    pub(crate) fn profile(fixture: &Fixture) -> ClaudeProfile {
        ClaudeProfile::new(&fixture.profiles, ACCOUNT, fixture.common.clone()).unwrap()
    }

    /// Spaces are escaped in an import; other whitespace, control characters, backticks
    /// (and backslashes outside Windows) cannot be written.
    fn importable(fixture: &Fixture) -> bool {
        !fixture
            .common
            .config_dir
            .to_string_lossy()
            .chars()
            .any(|c| {
                (c != ' ' && (c.is_whitespace() || c.is_control()))
                    || c == '`'
                    || (!cfg!(windows) && c == '\\')
            })
    }

    #[test]
    fn profile_identifiers_must_be_plain_directory_names() {
        let base = Path::new("profiles");
        assert!(ClaudeProfilePaths::new(base, ACCOUNT).is_ok());
        for bad in [
            "", "..", "a/b", "a\\b", "a:b", "a.b", "con", "NUL", "-x", "a b", "é",
        ] {
            assert!(
                matches!(
                    ClaudeProfilePaths::new(base, bad),
                    Err(PlatformError::UnsafePath)
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn authorization_is_profile_only_and_state_requires_the_exact_owner() {
        let f = fixture();
        fs::write(
            &f.common.credentials_file,
            br#"{"claudeAiOauth":{"accessToken":"GLOBAL_SENTINEL"},"mcpOAuth":{"keep":1}}"#,
        )
        .unwrap();
        fs::write(
            &f.common.global_config_file,
            br#"{"oauthAccount":{"accountUuid":"other"},"projects":{"p":1}}"#,
        )
        .unwrap();
        let global_before = (
            fs::read(&f.common.credentials_file).unwrap(),
            fs::read(&f.common.global_config_file).unwrap(),
        );
        let profile = profile(&f);
        assert_eq!(
            profile.state(ACCOUNT, "org").unwrap(),
            ProfileState::NeedsLogin
        );
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        assert_eq!(profile.state(ACCOUNT, "org").unwrap(), ProfileState::Ready);
        // Account and workspace are both enforced.
        assert_eq!(
            profile.state(OTHER, "org").unwrap(),
            ProfileState::IdentityChanged
        );
        assert_eq!(
            profile.state(ACCOUNT, "other-org").unwrap(),
            ProfileState::IdentityChanged
        );
        assert_eq!(
            global_before,
            (
                fs::read(&f.common.credentials_file).unwrap(),
                fs::read(&f.common.global_config_file).unwrap()
            )
        );
        let config: Value =
            serde_json::from_slice(&fs::read(&profile.paths().config_file).unwrap()).unwrap();
        assert_eq!(config["oauthAccount"]["accountUuid"], ACCOUNT);
        assert!(config.get("projects").is_none());
        let auth: Value =
            serde_json::from_slice(&fs::read(&profile.paths().credentials_file).unwrap()).unwrap();
        assert_eq!(auth["claudeAiOauth"]["refreshToken"], "refresh-one");
        assert!(!auth.to_string().contains("GLOBAL_SENTINEL"));
        let policy: Value =
            serde_json::from_slice(&fs::read(&profile.paths().settings_file).unwrap()).unwrap();
        assert_eq!(policy["forceLoginMethod"], "claudeai");
        assert_eq!(policy["forceLoginOrgUUID"], "org");
        #[cfg(windows)]
        {
            crate::protection::tests::assert_private_acl(&profile.paths().config_dir, true);
            crate::protection::tests::assert_private_acl(&profile.paths().credentials_file, false);
        }
    }

    #[test]
    fn incomplete_or_foreign_authorization_never_reads_as_ready() {
        let f = fixture();
        let profile = profile(&f);
        fs::create_dir_all(&profile.paths().config_dir).unwrap();
        // Identity without credentials, and credentials without a refresh grant.
        fs::write(
            &profile.paths().config_file,
            serde_json::to_vec(&json!({"oauthAccount":identity(ACCOUNT,"org")})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            profile.state(ACCOUNT, "org").unwrap(),
            ProfileState::NeedsLogin
        );
        fs::write(
            &profile.paths().credentials_file,
            br#"{"claudeAiOauth":{"accessToken":"a"}}"#,
        )
        .unwrap();
        assert_eq!(
            profile.state(ACCOUNT, "org").unwrap(),
            ProfileState::NeedsLogin
        );
        // Invalid payloads are rejected before anything is written.
        let fresh = fixture();
        let fresh_profile = self::profile(&fresh);
        for (credentials, identity) in [
            (json!({}), identity(ACCOUNT, "org")),
            (
                json!({"claudeAiOauth":{"accessToken":"a"}}),
                identity(ACCOUNT, "org"),
            ),
            (credentials("x"), json!({"accountUuid":ACCOUNT})),
            (credentials("x"), json!({"organizationUuid":"org"})),
        ] {
            assert!(matches!(
                fresh_profile.authorize(&credentials, &identity),
                Err(PlatformError::InvalidPayload)
            ));
        }
        assert!(!fresh.profiles.exists());
    }

    #[test]
    fn reauthorization_replaces_the_grant_and_preserves_unknown_profile_json() {
        let f = fixture();
        let profile = profile(&f);
        fs::create_dir_all(&profile.paths().config_dir).unwrap();
        fs::write(
            &profile.paths().config_file,
            br#"{"customSetting":{"keep":true},"oauthAccount":{"accountUuid":"old","organizationUuid":"org"}}"#,
        )
        .unwrap();
        fs::write(
            &profile.paths().credentials_file,
            br#"{"mcpOAuth":{"server":"token"},"claudeAiOauth":{"refreshToken":"old"}}"#,
        )
        .unwrap();
        fs::write(
            &profile.paths().settings_file,
            br#"{"theme":"dark","forceLoginOrgUUID":"stale"}"#,
        )
        .unwrap();
        assert_eq!(
            profile.state(ACCOUNT, "org").unwrap(),
            ProfileState::IdentityChanged
        );
        profile
            .authorize(&credentials("new"), &identity(ACCOUNT, "org"))
            .unwrap();
        let read =
            |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
        assert_eq!(
            read(&profile.paths().config_file)["customSetting"]["keep"],
            true
        );
        let auth = read(&profile.paths().credentials_file);
        assert_eq!(auth["mcpOAuth"]["server"], "token");
        assert_eq!(auth["claudeAiOauth"]["refreshToken"], "refresh-new");
        let settings = read(&profile.paths().settings_file);
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["forceLoginOrgUUID"], "org");
        assert_eq!(profile.state(ACCOUNT, "org").unwrap(), ProfileState::Ready);
    }

    #[test]
    fn prepare_refuses_a_profile_that_is_not_the_expected_owner() {
        let f = fixture();
        let profile = profile(&f);
        assert!(profile.prepare(ACCOUNT, "org").is_err());
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        assert!(profile.prepare(OTHER, "org").is_err());
        assert!(profile.prepare(ACCOUNT, "elsewhere").is_err());
        assert!(profile.prepare(ACCOUNT, "org").is_ok());
    }

    #[test]
    fn prepare_shares_resources_by_link_but_never_the_configuration_directory() {
        let f = fixture();
        let common = &f.common;
        fs::create_dir_all(common.config_dir.join("skills/demo")).unwrap();
        fs::write(common.config_dir.join("skills/demo/SKILL.md"), b"skill").unwrap();
        fs::write(common.config_dir.join("CLAUDE.md"), b"instructions").unwrap();
        fs::write(
            &common.settings_file,
            br#"{"model":"sonnet","hooks":{"keep":true}}"#,
        )
        .unwrap();
        fs::write(
            &common.global_config_file,
            br#"{"oauthAccount":{"accountUuid":"global"},"mcpServers":{"docs":{"command":"docs","env":{"KEY":"PRIVATE_SENTINEL"}}},"projects":{"p":{}}}"#,
        )
        .unwrap();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        // A real, non-empty directory in the profile is never replaced.
        fs::create_dir_all(profile.paths().config_dir.join("agents")).unwrap();
        fs::create_dir_all(common.config_dir.join("agents")).unwrap();
        fs::write(profile.paths().config_dir.join("agents/own.md"), b"own").unwrap();
        let plan = profile.prepare(ACCOUNT, "org").unwrap();
        assert!(plan.shared.linked.contains(&"projects"));
        assert!(plan.shared.linked.contains(&"skills"));
        assert_eq!(plan.shared.separate, vec!["agents"]);
        // Absent at the first launch, yet still shared: created on the common side.
        assert!(plan.shared.linked.contains(&"commands"));
        assert!(common.config_dir.join("commands").is_dir());
        assert_eq!(
            plan.settings.as_deref(),
            Some(common.settings_file.as_path())
        );
        assert_eq!(plan.instructions, importable(&f));
        // The shared directories are the real ones, so history and memory never fork.
        assert_eq!(
            fs::read(profile.paths().config_dir.join("skills/demo/SKILL.md")).unwrap(),
            b"skill"
        );
        fs::create_dir_all(profile.paths().config_dir.join("projects/slug/memory")).unwrap();
        fs::write(
            profile
                .paths()
                .config_dir
                .join("projects/slug/memory/note.md"),
            b"memory",
        )
        .unwrap();
        assert_eq!(
            fs::read(common.config_dir.join("projects/slug/memory/note.md")).unwrap(),
            b"memory"
        );
        // Identity stays separate: the configuration directory and its auth files are
        // real, and no global account metadata is copied.
        let root_meta = fs::symlink_metadata(&profile.paths().config_dir).unwrap();
        assert!(!root_meta.file_type().is_symlink());
        for file in [
            &profile.paths().credentials_file,
            &profile.paths().config_file,
        ] {
            assert!(!fs::symlink_metadata(file).unwrap().file_type().is_symlink());
        }
        let config = fs::read_to_string(&profile.paths().config_file).unwrap();
        assert!(!config.contains("global") && !config.contains("PRIVATE_SENTINEL"));
        // MCP definitions are derived (unknown JSON preserved) into a private file.
        let mcp_path = plan.mcp_config.clone().unwrap();
        let mcp: Value = serde_json::from_slice(&fs::read(&mcp_path).unwrap()).unwrap();
        assert_eq!(mcp["mcpServers"]["docs"]["env"]["KEY"], "PRIVATE_SENTINEL");
        #[cfg(windows)]
        crate::protection::tests::assert_private_acl(&mcp_path, false);
        if importable(&f) {
            let memory = fs::read_to_string(&profile.paths().memory_file).unwrap();
            assert!(memory.starts_with(MEMORY_MARKER) && memory.contains("\n@"));
            assert!(memory.contains("CLAUDE.md"));
        }
        // The ordinary configuration was not modified beyond creating shared targets.
        assert_eq!(
            fs::read(&common.global_config_file).unwrap(),
            br#"{"oauthAccount":{"accountUuid":"global"},"mcpServers":{"docs":{"command":"docs","env":{"KEY":"PRIVATE_SENTINEL"}}},"projects":{"p":{}}}"#
        );
        // Running it again is idempotent.
        let again = profile.prepare(ACCOUNT, "org").unwrap();
        assert_eq!(again.shared, plan.shared);
    }

    #[test]
    fn subscription_blocking_shared_settings_stop_the_launch() {
        let f = fixture();
        fs::write(
            &f.common.settings_file,
            br#"{"env":{"ANTHROPIC_API_KEY":"SECRET_SENTINEL"}}"#,
        )
        .unwrap();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        let error = profile.prepare(ACCOUNT, "org").unwrap_err();
        assert!(matches!(
            error,
            PlatformError::UnsupportedSetting("ANTHROPIC_API_KEY")
        ));
        assert!(!error.to_string().contains("SENTINEL"));
    }

    #[test]
    fn derived_files_follow_the_ordinary_configuration_and_never_overwrite_user_files() {
        let f = fixture();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        // Without servers or instructions there is nothing to load.
        let plan = profile.prepare(ACCOUNT, "org").unwrap();
        assert!(plan.mcp_config.is_none() && plan.settings.is_none() && !plan.instructions);
        assert!(!profile.paths().mcp_file.exists());
        // A stale derived file disappears when the servers do.
        fs::write(&f.common.global_config_file, br#"{"mcpServers":{"a":{}}}"#).unwrap();
        assert!(
            profile
                .prepare(ACCOUNT, "org")
                .unwrap()
                .mcp_config
                .is_some()
        );
        fs::write(&f.common.global_config_file, br#"{"mcpServers":{}}"#).unwrap();
        assert!(
            profile
                .prepare(ACCOUNT, "org")
                .unwrap()
                .mcp_config
                .is_none()
        );
        assert!(!profile.paths().mcp_file.exists());
        // A managed CLAUDE.md follows the original; a user's own file is left alone.
        fs::write(f.common.config_dir.join("CLAUDE.md"), b"x").unwrap();
        assert_eq!(
            profile.prepare(ACCOUNT, "org").unwrap().instructions,
            importable(&f)
        );
        fs::remove_file(f.common.config_dir.join("CLAUDE.md")).unwrap();
        assert!(!profile.prepare(ACCOUNT, "org").unwrap().instructions);
        assert!(!profile.paths().memory_file.exists());
        fs::write(&profile.paths().memory_file, b"my own rules").unwrap();
        fs::write(f.common.config_dir.join("CLAUDE.md"), b"x").unwrap();
        assert!(!profile.prepare(ACCOUNT, "org").unwrap().instructions);
        assert_eq!(
            fs::read(&profile.paths().memory_file).unwrap(),
            b"my own rules"
        );
    }

    #[test]
    fn a_wrong_or_stale_link_is_repaired_without_touching_either_directory() {
        let f = fixture();
        let elsewhere = f.root.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("keep.md"), b"keep").unwrap();
        fs::create_dir_all(f.common.config_dir.join("projects")).unwrap();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        create_link(&profile.paths().config_dir.join("projects"), &elsewhere).unwrap();
        let plan = profile.prepare(ACCOUNT, "org").unwrap();
        assert!(plan.shared.linked.contains(&"projects"));
        assert!(same_directory(
            &profile.paths().config_dir.join("projects"),
            &f.common.config_dir.join("projects")
        ));
        assert_eq!(fs::read(elsewhere.join("keep.md")).unwrap(), b"keep");
    }

    #[test]
    fn helper_scripts_are_shared_like_the_other_resources() {
        let f = fixture();
        fs::create_dir_all(f.common.config_dir.join("scripts")).unwrap();
        fs::write(
            f.common.config_dir.join("scripts/helper.ps1"),
            b"echo shared",
        )
        .unwrap();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        let plan = profile.prepare(ACCOUNT, "org").unwrap();
        assert!(plan.shared.linked.contains(&"scripts"));
        assert_eq!(
            fs::read(profile.paths().config_dir.join("scripts/helper.ps1")).unwrap(),
            b"echo shared"
        );
    }

    /// The path as Claude Code records it: no verbatim prefix, and on Windows forward
    /// slashes.
    fn recorded(path: &Path) -> String {
        let text = path.to_string_lossy().into_owned();
        let text = text
            .strip_prefix(r"\\?\")
            .map(str::to_owned)
            .unwrap_or(text);
        if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text
        }
    }

    #[test]
    fn project_local_mcp_definitions_follow_the_exact_chosen_folder_only() {
        let f = fixture();
        let work = f.root.path().join("work");
        let (folder, sibling) = (work.join("proj"), work.join("proj2"));
        fs::create_dir_all(&folder).unwrap();
        let recorded_folder = if cfg!(windows) {
            recorded(&folder).to_lowercase()
        } else {
            recorded(&folder)
        };
        let mut projects = Map::new();
        projects.insert(
            recorded_folder.clone(),
            json!({
                "mcpServers": {
                    "local": {"command":"local","env":{"KEY":"PRIVATE_SENTINEL"}},
                    "shared": {"command":"local-wins"}
                },
                "enabledMcpjsonServers": ["repo"],
                "disabledMcpServers": ["docs"],
                "allowedTools": ["Bash"],
                "history": [{"display":"PRIVATE_PROMPT"}]
            }),
        );
        projects.insert(recorded(&sibling), json!({"mcpServers":{"sibling":{}}}));
        projects.insert(recorded(&work), json!({"mcpServers":{"parent":{}}}));
        let global = serde_json::to_vec(&json!({
            "oauthAccount": {"accountUuid": "global-account"},
            "mcpServers": {"docs": {"command":"docs"}, "shared": {"command":"global"}},
            "projects": Value::Object(projects)
        }))
        .unwrap();
        fs::write(&f.common.global_config_file, &global).unwrap();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        let read =
            |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
        let exported = |plan: &LaunchPlan| -> Vec<String> {
            let document = read(plan.mcp_config.as_ref().unwrap());
            let mut names: Vec<String> = document["mcpServers"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            names.sort();
            names
        };
        // Without a folder only the user-scope servers are exported.
        assert_eq!(
            exported(&profile.prepare(ACCOUNT, "org").unwrap()),
            ["docs", "shared"]
        );
        assert_eq!(
            read(&profile.paths().mcp_file)["mcpServers"]["shared"]["command"],
            "global"
        );
        // The chosen folder adds its local servers; the more specific definition wins.
        let plan = profile.prepare_for_folder(ACCOUNT, "org", &folder).unwrap();
        assert_eq!(exported(&plan), ["docs", "local", "shared"]);
        let mcp = read(plan.mcp_config.as_ref().unwrap());
        assert_eq!(mcp["mcpServers"]["shared"]["command"], "local-wins");
        assert_eq!(mcp["mcpServers"]["local"]["env"]["KEY"], "PRIVATE_SENTINEL");
        #[cfg(windows)]
        crate::protection::tests::assert_private_acl(plan.mcp_config.as_ref().unwrap(), false);
        // Only approval choices reach the profile's own record: no history, tools,
        // definitions or account metadata of the ordinary configuration.
        let config = read(&profile.paths().config_file);
        let entry = &config["projects"][recorded_folder.as_str()];
        assert_eq!(entry["enabledMcpjsonServers"], json!(["repo"]));
        assert_eq!(entry["disabledMcpServers"], json!(["docs"]));
        for absent in ["mcpServers", "allowedTools", "history"] {
            assert!(entry.get(absent).is_none(), "{absent}");
        }
        assert_eq!(config["oauthAccount"]["accountUuid"], ACCOUNT);
        assert_eq!(config["projects"].as_object().unwrap().len(), 1);
        let on_disk = fs::read_to_string(&profile.paths().config_file).unwrap();
        assert!(!on_disk.contains("PRIVATE_PROMPT") && !on_disk.contains("global-account"));
        // The ordinary configuration is untouched, and a choice made inside the profile
        // is not overwritten by the older ordinary one.
        assert_eq!(fs::read(&f.common.global_config_file).unwrap(), global);
        let mut own = config.clone();
        own["projects"][recorded_folder.as_str()]["enabledMcpjsonServers"] = json!(["chosen"]);
        fs::write(
            &profile.paths().config_file,
            serde_json::to_vec(&own).unwrap(),
        )
        .unwrap();
        profile.prepare_for_folder(ACCOUNT, "org", &folder).unwrap();
        assert_eq!(
            read(&profile.paths().config_file)["projects"][recorded_folder.as_str()]["enabledMcpjsonServers"],
            json!(["chosen"])
        );
        // A folder with no record, and a similar name, get only the user-scope servers.
        for other in [sibling.clone(), work.join("pro")] {
            fs::create_dir_all(&other).unwrap();
            let plan = profile.prepare_for_folder(ACCOUNT, "org", &other).unwrap();
            let names = exported(&plan);
            assert!(!names.contains(&"local".to_owned()), "{other:?}");
            assert_eq!(names.contains(&"sibling".to_owned()), other == sibling);
        }
    }

    #[test]
    fn project_records_match_across_slash_case_and_prefix_spellings() {
        let f = fixture();
        let folder = f.root.path().join("Work Folder").join("Proj");
        fs::create_dir_all(&folder).unwrap();
        let base = recorded(&folder);
        let mut spellings = vec![format!("{base}/")];
        if cfg!(windows) {
            spellings.push(base.to_lowercase());
            spellings.push(base.replace('/', "\\"));
            spellings.push(base.to_uppercase());
        } else {
            spellings.push(base.clone());
        }
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        for key in spellings {
            let mut projects = Map::new();
            projects.insert(key.clone(), json!({"mcpServers":{"local":{}}}));
            fs::write(
                &f.common.global_config_file,
                serde_json::to_vec(&json!({"projects": Value::Object(projects)})).unwrap(),
            )
            .unwrap();
            let plan = profile.prepare_for_folder(ACCOUNT, "org", &folder).unwrap();
            assert!(plan.mcp_config.is_some(), "{key}");
        }
        // On a case-sensitive file system a different case is a different folder.
        if !cfg!(windows) {
            let mut projects = Map::new();
            projects.insert(base.to_uppercase(), json!({"mcpServers":{"local":{}}}));
            fs::write(
                &f.common.global_config_file,
                serde_json::to_vec(&json!({"projects": Value::Object(projects)})).unwrap(),
            )
            .unwrap();
            assert!(
                profile
                    .prepare_for_folder(ACCOUNT, "org", &folder)
                    .unwrap()
                    .mcp_config
                    .is_none()
            );
        }
    }

    #[test]
    fn a_profile_with_its_own_history_is_reported_instead_of_forking_memory() {
        let f = fixture();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        let own = profile.paths().config_dir.join("projects/slug/memory");
        fs::create_dir_all(&own).unwrap();
        fs::write(own.join("note.md"), b"private profile memory").unwrap();
        // The shared side exists although it was empty, so a later session cannot fork.
        let plan = profile.prepare(ACCOUNT, "org").unwrap();
        assert!(plan.shared.splits_history());
        assert_eq!(plan.shared.separate, vec!["projects"]);
        assert!(plan.shared.linked.contains(&"todos"));
        assert!(f.common.config_dir.join("projects").is_dir());
        // Nothing was merged, moved or deleted.
        assert_eq!(
            fs::read(own.join("note.md")).unwrap(),
            b"private profile memory"
        );
        assert!(!SharedResources::default().splits_history());
    }

    #[test]
    fn the_profile_credential_is_exposed_only_for_owner_checks_and_redacted() {
        let f = fixture();
        let profile = profile(&f);
        assert!(profile.credential().unwrap().is_none());
        let mut grant = credentials("one");
        grant["claudeAiOauth"]["expiresAt"] = json!(10_000_000_i64);
        profile
            .authorize(&grant, &identity(ACCOUNT, "org"))
            .unwrap();
        let credential = profile.credential().unwrap().unwrap();
        assert_eq!(credential.access_token.as_str(), "access-one");
        assert!(!format!("{credential:?}").contains("access-one"));
        // Two minutes of margin before the stated expiry; an expired one is never used.
        assert!(credential.is_current(10_000 - 121));
        assert!(!credential.is_current(10_000 - 119));
        assert!(!credential.is_current(20_000));
    }

    #[test]
    fn removing_a_profile_deletes_its_credentials_but_never_the_shared_files() {
        let f = fixture();
        fs::create_dir_all(f.common.config_dir.join("projects/slug")).unwrap();
        fs::write(
            f.common.config_dir.join("projects/slug/history.jsonl"),
            b"history",
        )
        .unwrap();
        let profile = profile(&f);
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        profile.prepare(ACCOUNT, "org").unwrap();
        profile.remove().unwrap();
        assert!(!profile.paths().config_dir.exists());
        assert_eq!(
            fs::read(f.common.config_dir.join("projects/slug/history.jsonl")).unwrap(),
            b"history"
        );
        // Removing a profile that does not exist is not an error.
        profile.remove().unwrap();
    }
}
