use super::{CodexResult, CodexStoreError, StoreGeneration, generation};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub const PINNED_CODEX_VERSION: &str = "0.160.0";
pub const PINNED_CODEX_SCHEMA: &str = "a956835d020762cb2b570053af06f643a11c0ecc";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexStorageMode {
    File,
    Keyring,
    Auto,
    Ephemeral,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexProviderMode {
    DefaultOpenAi,
    Custom,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexPolicyState {
    Unrestricted,
    Restricted,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CodexSourceRole {
    Executable,
    System,
    Managed,
    User,
    Project,
    Session,
}

/// Captured source receipts cannot be constructed from UI JSON or a bare allow flag.
/// The trusted resolver must enumerate authoritative layers, parse these same bytes,
/// and classify their effective policy. Missing/unknown managed-layer proof is denied.
#[derive(Clone)]
pub struct CodexWatchedSource {
    role: CodexSourceRole,
    path: Option<PathBuf>,
    generation: Option<StoreGeneration>,
    directory: bool,
}
impl std::fmt::Debug for CodexWatchedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexWatchedSource")
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}
impl CodexWatchedSource {
    pub fn capture(
        role: CodexSourceRole,
        path: PathBuf,
    ) -> CodexResult<(Self, Option<Zeroizing<Vec<u8>>>)> {
        if role == CodexSourceRole::Session {
            return Err(CodexStoreError::UnsupportedContext);
        }
        let limit = if role == CodexSourceRole::Executable {
            256 * 1024 * 1024
        } else {
            32 * 1024 * 1024
        };
        let directory = role == CodexSourceRole::Project && path.is_dir();
        let (bytes, generation) = if directory {
            (None, generation::directory_generation(&path)?)
        } else {
            generation::read(&path, "codex-source-v1", limit, false)?
        };
        if role == CodexSourceRole::Executable && bytes.is_none() {
            return Err(CodexStoreError::UnsupportedContext);
        }
        Ok((
            Self {
                role,
                path: Some(path),
                generation: Some(generation),
                directory,
            },
            bytes,
        ))
    }
    /// Fixed `app-server` invocation without config/profile/provider/session overrides.
    pub fn default_session() -> Self {
        Self {
            role: CodexSourceRole::Session,
            path: None,
            generation: None,
            directory: false,
        }
    }
    pub fn role(&self) -> CodexSourceRole {
        self.role
    }
    pub fn is_directory(&self) -> bool {
        self.directory
    }
    pub fn revalidate(&self) -> CodexResult<()> {
        if let Some(path) = &self.path {
            let limit = if self.role == CodexSourceRole::Executable {
                256 * 1024 * 1024
            } else {
                32 * 1024 * 1024
            };
            let current = if self.directory {
                generation::directory_generation(path)?
            } else {
                generation::read(path, "codex-source-v1", limit, false)?.1
            };
            if self.generation.as_ref() != Some(&current) {
                return Err(CodexStoreError::ExternalChange);
            }
        } else if self.role != CodexSourceRole::Session {
            return Err(CodexStoreError::UnsupportedContext);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CodexContextEvidence {
    pub version: String,
    pub schema: String,
    pub storage: CodexStorageMode,
    pub provider: CodexProviderMode,
    pub policy: CodexPolicyState,
    pub sources: Vec<CodexWatchedSource>,
}

#[derive(Clone)]
pub struct CodexFileContext {
    home: PathBuf,
    context_id: String,
    home_identity: String,
    evidence: CodexContextEvidence,
}
impl std::fmt::Debug for CodexFileContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexFileContext")
            .field("context_id", &self.context_id)
            .finish_non_exhaustive()
    }
}
impl CodexFileContext {
    pub fn qualify(home: PathBuf, evidence: CodexContextEvidence) -> CodexResult<Self> {
        crate::files::check_path(&home)?;
        if !home.is_dir()
            || evidence.version != PINNED_CODEX_VERSION
            || evidence.schema != PINNED_CODEX_SCHEMA
            || evidence.storage != CodexStorageMode::File
            || evidence.provider != CodexProviderMode::DefaultOpenAi
            || evidence.policy != CodexPolicyState::Unrestricted
        {
            return Err(CodexStoreError::UnsupportedContext);
        }
        let roles: BTreeSet<_> = evidence.sources.iter().map(|s| s.role).collect();
        if roles
            != BTreeSet::from([
                CodexSourceRole::Executable,
                CodexSourceRole::System,
                CodexSourceRole::Managed,
                CodexSourceRole::User,
                CodexSourceRole::Project,
                CodexSourceRole::Session,
            ])
        {
            return Err(CodexStoreError::UnsupportedContext);
        }
        let home = home.canonicalize()?;
        crate::files::check_path(&home)?;
        let user_config_bound = evidence
            .sources
            .iter()
            .filter(|s| s.role == CodexSourceRole::User)
            .any(|source| {
                source.path.as_ref().is_some_and(|path| {
                    path.file_name().is_some_and(|name| name == "config.toml")
                        && path.parent().is_some_and(|parent| {
                            parent.canonicalize().ok().as_ref() == Some(&home)
                        })
                })
            });
        if !user_config_bound {
            return Err(CodexStoreError::UnsupportedContext);
        }
        let context_id = generation::path_digest(b"codex:file:v1:", &home);
        let home_identity = generation::native_identity(&home)?;
        let context = Self {
            home,
            context_id,
            home_identity,
            evidence,
        };
        context.revalidate()?;
        Ok(context)
    }
    pub fn home(&self) -> &Path {
        &self.home
    }
    pub fn context_id(&self) -> &str {
        &self.context_id
    }
    pub fn revalidate(&self) -> CodexResult<()> {
        crate::files::check_path(&self.home)?;
        if self.home.canonicalize()? != self.home
            || generation::native_identity(&self.home)? != self.home_identity
        {
            return Err(CodexStoreError::ExternalChange);
        }
        for source in &self.evidence.sources {
            source.revalidate()?;
        }
        Ok(())
    }
    pub(crate) fn executable(&self) -> CodexResult<&Path> {
        self.evidence
            .sources
            .iter()
            .find(|s| s.role == CodexSourceRole::Executable)
            .and_then(|s| s.path.as_deref())
            .ok_or(CodexStoreError::UnsupportedContext)
    }
}

/// Create only an application-owned isolated login directory. Never call this on
/// the user's selected CODEX_HOME merely to qualify that existing context.
pub fn create_private_context(path: PathBuf) -> CodexResult<PathBuf> {
    crate::files::private_dir(&path)?;
    let canonical = path.canonicalize()?;
    crate::files::check_path(&canonical)?;
    Ok(canonical)
}
/// Read an application-owned login output without reconstructing its JSON. The
/// caller retains ownership proof for this temporary directory and child process.
pub fn read_private_auth(home: &Path) -> CodexResult<Option<Zeroizing<Vec<u8>>>> {
    crate::files::check_path(home)?;
    if !home.is_dir() {
        return Err(CodexStoreError::UnsupportedContext);
    }
    let canonical = home.canonicalize()?;
    crate::files::check_path(&canonical)?;
    let id = generation::path_digest(b"codex:owned:v1:", &canonical);
    let (bytes, _) = generation::read(&canonical.join("auth.json"), &id, 32 * 1024 * 1024, true)?;
    Ok(bytes)
}
