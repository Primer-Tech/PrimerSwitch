use crate::{CodexContext, CodexError, supported_version};
use std::{
    fmt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct DiscoveryOptions {
    pub explicit_native: Option<PathBuf>,
    pub path_directories: Vec<PathBuf>,
    pub npm_package_roots: Vec<PathBuf>,
}
impl DiscoveryOptions {
    /// Inspect PATH and known public npm package locations, never authentication files.
    pub fn from_environment() -> Self {
        let mut roots = Vec::new();
        if let Some(appdata) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(appdata).join("npm/node_modules/@openai/codex"));
        }
        let directories = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default();
        for dir in &directories {
            roots.push(dir.join("node_modules/@openai/codex"));
            roots.push(dir.join("../lib/node_modules/@openai/codex"));
        }
        Self {
            explicit_native: None,
            path_directories: directories,
            npm_package_roots: roots,
        }
    }
}
#[derive(Clone)]
pub struct CodexCandidate {
    path: PathBuf,
}
#[derive(Clone)]
pub struct VerifiedCodexExecutable {
    path: PathBuf,
    identity: FileIdentity,
    version: String,
}
/// Cheap replacement check. Hashing the ~300 MB native binary before every spawn
/// is wasteful; npm/installer updates change size or modification time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct FileIdentity {
    len: u64,
    modified: Option<std::time::SystemTime>,
}
impl fmt::Debug for CodexCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodexCandidate([REDACTED])")
    }
}
impl fmt::Debug for VerifiedCodexExecutable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerifiedCodexExecutable({}, [REDACTED])", self.version)
    }
}
impl VerifiedCodexExecutable {
    /// Rust-only path for a platform receipt. Never expose through IPC.
    pub fn native_path(&self) -> &Path {
        &self.path
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub(crate) fn recheck(&self) -> Result<&Path, CodexError> {
        if identity(&self.path)? != self.identity {
            return Err(CodexError::UnsupportedInstallation);
        }
        Ok(&self.path)
    }
}
pub fn discover_candidates(options: &DiscoveryOptions) -> Result<Vec<CodexCandidate>, CodexError> {
    let mut paths = Vec::new();
    if let Some(path) = &options.explicit_native {
        paths.push(path.clone());
    }
    for dir in &options.path_directories {
        paths.push(dir.join(if cfg!(windows) { "codex.exe" } else { "codex" }));
    }
    let (package, target) = if cfg!(all(windows, target_arch = "x86_64")) {
        ("codex-win32-x64", "x86_64-pc-windows-msvc")
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        ("codex-win32-arm64", "aarch64-pc-windows-msvc")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        ("codex-darwin-x64", "x86_64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        ("codex-darwin-arm64", "aarch64-apple-darwin")
    } else if cfg!(target_arch = "aarch64") {
        ("codex-linux-arm64", "aarch64-unknown-linux-musl")
    } else {
        ("codex-linux-x64", "x86_64-unknown-linux-musl")
    };
    for root in &options.npm_package_roots {
        let Some(version) = package_version(root, "@openai/codex")
            .filter(|version| supported_version(version).is_some())
        else {
            continue;
        };
        for platform in [
            root.join("node_modules/@openai").join(package),
            root.parent().unwrap_or(root).join(package),
        ] {
            if package_version(&platform, "@openai/codex").as_deref()
                == Some(&format!(
                    "{version}-{}",
                    package.trim_start_matches("codex-")
                ))
            {
                paths.push(
                    platform
                        .join("vendor")
                        .join(target)
                        .join("bin")
                        .join(if cfg!(windows) { "codex.exe" } else { "codex" }),
                );
            }
        }
    }
    let mut result = Vec::new();
    for path in paths {
        if let Ok(path) = std::fs::canonicalize(path)
            && native_file(&path).is_ok()
            && !result.iter().any(|c: &CodexCandidate| c.path == path)
        {
            result.push(CodexCandidate { path });
        }
    }
    Ok(result)
}
fn package_version(root: &Path, name: &str) -> Option<String> {
    let path = root.join("package.json");
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 65536 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let value = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
    if value["name"] != name {
        return None;
    }
    value["version"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .map(str::to_owned)
}
fn native_file(path: &Path) -> Result<(), CodexError> {
    let meta = std::fs::metadata(path).map_err(|_| CodexError::UnsupportedInstallation)?;
    // Codex 0.160.0 for Windows x64 is ~312 MB; keep a generous sanity cap.
    if !meta.is_file() || meta.len() == 0 || meta.len() > 1024 * 1024 * 1024 {
        return Err(CodexError::UnsupportedInstallation);
    }
    use std::io::Read;
    let mut magic = [0; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|_| CodexError::UnsupportedInstallation)?;
    let valid = if cfg!(windows) {
        magic[..2] == *b"MZ"
    } else if cfg!(target_os = "macos") {
        matches!(
            magic,
            [0xfe, 0xed, 0xfa, 0xce]
                | [0xce, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xcf, 0xfa, 0xed, 0xfe]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
        )
    } else {
        magic == *b"\x7fELF"
    };
    if !valid {
        return Err(CodexError::UnsupportedInstallation);
    }
    if cfg!(windows) && path.extension().is_none_or(|e| e != "exe") {
        return Err(CodexError::UnsupportedInstallation);
    }
    Ok(())
}
pub(crate) fn identity(path: &Path) -> Result<FileIdentity, CodexError> {
    native_file(path)?;
    let meta = std::fs::metadata(path).map_err(|_| CodexError::UnsupportedInstallation)?;
    Ok(FileIdentity {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}
pub async fn verify_executable(
    candidate: CodexCandidate,
    probe: &CodexContext,
) -> Result<VerifiedCodexExecutable, CodexError> {
    if !probe.is_isolated() {
        return Err(CodexError::UnsafeContext);
    }
    let before = identity(&candidate.path)?;
    let mut command = Command::new(&candidate.path);
    probe.configure(&mut command);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| CodexError::SpawnFailed)?;
    let mut stdout = child.stdout.take().ok_or(CodexError::SpawnFailed)?;
    let result = timeout(Duration::from_secs(5), async {
        let mut bytes = Zeroizing::new(Vec::new());
        let mut chunk = Zeroizing::new([0; 1024]);
        loop {
            let n = stdout
                .read(&mut *chunk)
                .await
                .map_err(|_| CodexError::ChildExited)?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > 4096 {
                return Err(CodexError::OutputLimit);
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        if !child
            .wait()
            .await
            .map_err(|_| CodexError::ChildExited)?
            .success()
        {
            return Err(CodexError::UnsupportedVersion);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| CodexError::UnsupportedVersion)?;
        if !text.trim().starts_with("codex-cli ") {
            return Err(CodexError::UnsupportedVersion);
        }
        supported_version(text).ok_or(CodexError::UnsupportedVersion)
    })
    .await
    .unwrap_or(Err(CodexError::Timeout));
    if result.is_err() {
        let _ = child.start_kill();
        let _ = timeout(Duration::from_secs(5), child.wait()).await;
    }
    let version = result?;
    if before != identity(&candidate.path)? {
        return Err(CodexError::UnsupportedInstallation);
    }
    Ok(VerifiedCodexExecutable {
        path: candidate.path,
        identity: before,
        version,
    })
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
