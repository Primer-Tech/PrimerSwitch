use crate::{CodexContext, CodexError, SUPPORTED_CODEX_VERSION};
use sha2::{Digest, Sha256};
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
    fingerprint: [u8; 32],
}
impl fmt::Debug for CodexCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodexCandidate([REDACTED])")
    }
}
impl fmt::Debug for VerifiedCodexExecutable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedCodexExecutable(0.160.0, [REDACTED])")
    }
}
impl VerifiedCodexExecutable {
    /// Rust-only path for a platform receipt. Never expose through IPC.
    pub fn native_path(&self) -> &Path {
        &self.path
    }
    pub fn version(&self) -> &'static str {
        SUPPORTED_CODEX_VERSION
    }
    pub(crate) fn recheck(&self) -> Result<&Path, CodexError> {
        if fingerprint(&self.path)? != self.fingerprint {
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
        if !package_matches(root, "@openai/codex", SUPPORTED_CODEX_VERSION) {
            continue;
        }
        for platform in [
            root.join("node_modules/@openai").join(package),
            root.parent().unwrap_or(root).join(package),
        ] {
            if package_matches(
                &platform,
                "@openai/codex",
                &format!(
                    "{SUPPORTED_CODEX_VERSION}-{}",
                    package.trim_start_matches("codex-")
                ),
            ) {
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
fn package_matches(root: &Path, name: &str, version: &str) -> bool {
    let path = root.join("package.json");
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    if !meta.is_file() || meta.len() > 65536 {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    value["name"] == name && value["version"] == version
}
fn native_file(path: &Path) -> Result<(), CodexError> {
    let meta = std::fs::metadata(path).map_err(|_| CodexError::UnsupportedInstallation)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 256 * 1024 * 1024 {
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
fn fingerprint(path: &Path) -> Result<[u8; 32], CodexError> {
    native_file(path)?;
    use std::io::Read;
    let mut input = std::fs::File::open(path).map_err(|_| CodexError::UnsupportedInstallation)?;
    let mut digest = Sha256::new();
    let mut chunk = [0; 65536];
    let mut total = 0;
    loop {
        let n = input
            .read(&mut chunk)
            .map_err(|_| CodexError::UnsupportedInstallation)?;
        if n == 0 {
            break;
        }
        total += n;
        if total > 256 * 1024 * 1024 {
            return Err(CodexError::OutputLimit);
        }
        digest.update(&chunk[..n]);
    }
    Ok(digest.finalize().into())
}
pub async fn verify_executable(
    candidate: CodexCandidate,
    probe: &CodexContext,
) -> Result<VerifiedCodexExecutable, CodexError> {
    if !probe.is_isolated() {
        return Err(CodexError::UnsafeContext);
    }
    let before = fingerprint(&candidate.path)?;
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
        if text.trim() != format!("codex-cli {SUPPORTED_CODEX_VERSION}") {
            return Err(CodexError::UnsupportedVersion);
        }
        Ok(())
    })
    .await
    .unwrap_or(Err(CodexError::Timeout));
    if result.is_err() {
        let _ = child.start_kill();
        let _ = timeout(Duration::from_secs(5), child.wait()).await;
    }
    result?;
    if before != fingerprint(&candidate.path)? {
        return Err(CodexError::UnsupportedInstallation);
    }
    Ok(VerifiedCodexExecutable {
        path: candidate.path,
        fingerprint: before,
    })
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
