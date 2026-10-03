#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
use super::{CodexResult, CodexStoreError};
use std::path::Path;

/// Informational inventory for the switch result. It never blocks a switch: since
/// Codex 0.160.0 terminals are clients of the shared app-server daemon, which the
/// runtime restarts so they reconnect on the new account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodexProcessSummary {
    /// Codex processes that hold their own login and keep the previous account until
    /// they restart: the desktop app, IDE extensions and other stdio app-servers,
    /// `codex exec`, and terminals started with `--no-daemon`.
    pub other_clients: u32,
    /// The lampese "Codex Switcher" app, which also rewrites auth.json and force-kills
    /// `codex.exe` processes when it switches.
    pub switcher_running: bool,
    /// The shared app-server daemon (`--managed-daemon`) of this CODEX_HOME runs.
    pub daemon_running: bool,
    /// Its process id, used to restart it with the environment it was started with.
    pub daemon_pid: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
enum Role {
    /// The shared app-server daemon itself.
    DaemonServer,
    /// The daemon's updater or another helper from its package.
    Daemon,
    /// A terminal attached to the daemon: it reconnects after a restart.
    DaemonClient,
    /// Holds its own login.
    OtherClient,
    Ignored,
}

/// Classify one process from its image path and command line. `daemon_root` is
/// `<CODEX_HOME>/packages/app-server-daemon`.
fn classify(image: &Path, command_line: Option<&str>, daemon_root: &Path) -> Role {
    let name = image
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    if name != "codex" {
        return Role::Ignored;
    }
    let in_package = starts_with_folded(image, daemon_root);
    let Some(command_line) = command_line else {
        if in_package {
            return Role::Daemon;
        }
        // Unknown arguments: count it conservatively as a separate client.
        return Role::OtherClient;
    };
    let args = split_arguments(command_line);
    if in_package {
        return if args.iter().any(|arg| arg == "--managed-daemon") {
            Role::DaemonServer
        } else {
            Role::Daemon
        };
    }
    // Flag values and an initial prompt are positional too, so only a known
    // subcommand name counts; anything else is an interactive terminal.
    const SUBCOMMANDS: &[&str] = &[
        "exec",
        "e",
        "app-server",
        "mcp-server",
        "review",
        "resume",
        "fork",
        "login",
        "logout",
        "apply",
        "a",
        "completion",
        "mcp",
        "features",
        "sandbox",
        "debug",
        "cloud",
        "help",
    ];
    let subcommand = args
        .iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| arg.to_ascii_lowercase())
        .find(|arg| SUBCOMMANDS.contains(&arg.as_str()));
    let image_text = image.to_string_lossy().to_ascii_lowercase();
    if image_text.contains("\\windowsapps\\openai.codex")
        || image_text.contains("/applications/codex.app/")
    {
        return Role::OtherClient;
    }
    if args.iter().any(|arg| arg == "--managed-daemon") {
        return Role::Daemon;
    }
    match subcommand.as_deref() {
        // Interactive terminal: daemon client unless explicitly embedded.
        None | Some("resume") | Some("fork") => {
            if args.iter().any(|arg| arg == "--no-daemon") {
                Role::OtherClient
            } else {
                Role::DaemonClient
            }
        }
        Some("app-server") if args.iter().any(|arg| arg == "daemon") => Role::Daemon,
        Some("exec") | Some("e") | Some("app-server") | Some("mcp-server") | Some("review") => {
            Role::OtherClient
        }
        // login/logout/apply/completion and other short-lived commands.
        Some(_) => Role::Ignored,
    }
}

fn starts_with_folded(path: &Path, prefix: &Path) -> bool {
    let normalize = |p: &Path| {
        let text = p.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        text.strip_prefix("\\\\?\\")
            .map(str::to_owned)
            .unwrap_or(text)
    };
    let (path, prefix) = (normalize(path), normalize(prefix));
    !prefix.is_empty() && path.starts_with(&prefix)
}

/// Minimal Windows command-line splitting (quotes and backslash-quote escapes).
fn split_arguments(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
                started = true;
            }
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
        if args.len() >= 64 {
            break;
        }
    }
    if started {
        args.push(current);
    }
    args
}

/// Best-effort inventory of the current user's Codex processes for `codex_home`.
pub fn scan_codex_processes(codex_home: &Path) -> CodexProcessSummary {
    native_scan(&codex_home.join("packages").join("app-server-daemon"))
}

#[cfg(windows)]
fn native_scan(daemon_root: &Path) -> CodexProcessSummary {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::{
        Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation},
        Win32::{
            Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING},
            Security::{EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                    TH32CS_SNAPPROCESS,
                },
                Threading::{
                    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
                    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
                },
            },
        },
    };
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn user(process: HANDLE) -> Option<Vec<usize>> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        let token = Handle(token);
        let mut needed = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 || needed > 65536 {
            return None;
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        (unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } != 0)
            .then_some(bytes)
    }
    fn command_line(process: HANDLE) -> Option<String> {
        let mut needed = 0u32;
        unsafe {
            NtQueryInformationProcess(
                process,
                ProcessCommandLineInformation,
                ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 || needed > 1 << 20 {
            return None;
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                ProcessCommandLineInformation,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        if status < 0 {
            return None;
        }
        let text = unsafe { &*(buffer.as_ptr().cast::<UNICODE_STRING>()) };
        if text.Buffer.is_null() {
            return None;
        }
        let units = unsafe { std::slice::from_raw_parts(text.Buffer, (text.Length / 2) as usize) };
        Some(String::from_utf16_lossy(units))
    }
    let mut summary = CodexProcessSummary::default();
    let Some(own) = user(unsafe { GetCurrentProcess() }) else {
        return summary;
    };
    let own_sid = unsafe { (*(own.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let own_pid = unsafe { GetCurrentProcessId() };
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return summary;
    }
    let snapshot = Handle(raw);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return summary;
    }
    for _ in 0..65536 {
        let len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
        let pid = entry.th32ProcessID;
        // Our own quota readers are children of this process.
        if pid != own_pid
            && entry.th32ParentProcessID != own_pid
            && (name == "codex.exe" || name == "codex-switcher.exe")
        {
            let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if !raw.is_null() {
                let process = Handle(raw);
                let same_user = user(process.0).is_some_and(|owner| {
                    let sid = unsafe { (*(owner.as_ptr().cast::<TOKEN_USER>())).User.Sid };
                    let equal = unsafe { EqualSid(own_sid, sid) };
                    equal != 0
                });
                if same_user {
                    if name == "codex-switcher.exe" {
                        summary.switcher_running = true;
                    } else {
                        let mut path = vec![0u16; 32768];
                        let mut length = path.len() as u32;
                        if unsafe {
                            QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length)
                        } != 0
                        {
                            let image = std::path::PathBuf::from(OsString::from_wide(
                                &path[..length as usize],
                            ));
                            let line = command_line(process.0);
                            match classify(&image, line.as_deref(), daemon_root) {
                                Role::OtherClient => {
                                    summary.other_clients = summary.other_clients.saturating_add(1)
                                }
                                Role::DaemonServer => {
                                    summary.daemon_running = true;
                                    summary.daemon_pid = Some(pid);
                                }
                                _ => (),
                            }
                        }
                    }
                }
            }
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            break;
        }
    }
    summary
}

#[cfg(target_os = "linux")]
fn native_scan(daemon_root: &Path) -> CodexProcessSummary {
    use std::os::unix::fs::MetadataExt;
    let mut summary = CodexProcessSummary::default();
    let uid = unsafe { libc::geteuid() };
    let own = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return summary;
    };
    for entry in entries.flatten().take(65536) {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own || std::fs::metadata(entry.path()).map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse::<u32>().ok())
            == Some(own)
        {
            continue;
        }
        let Ok(image) = std::fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        if image.file_name().is_some_and(|n| n == "codex-switcher") {
            summary.switcher_running = true;
            continue;
        }
        let line = std::fs::read(entry.path().join("cmdline"))
            .ok()
            .map(|bytes| {
                bytes
                    .split(|b| *b == 0)
                    .filter(|part| !part.is_empty())
                    .map(|part| {
                        let text = String::from_utf8_lossy(part);
                        if text.contains(char::is_whitespace) {
                            format!("\"{text}\"")
                        } else {
                            text.into_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        match classify(&image, line.as_deref(), daemon_root) {
            Role::OtherClient => summary.other_clients = summary.other_clients.saturating_add(1),
            Role::DaemonServer => {
                summary.daemon_running = true;
                summary.daemon_pid = Some(pid);
            }
            _ => (),
        }
    }
    summary
}

#[cfg(not(any(windows, target_os = "linux")))]
fn native_scan(_: &Path) -> CodexProcessSummary {
    CodexProcessSummary::default()
}

/// The environment block of another process of this user: the shared daemon's, so a
/// restart can give the new daemon the same environment the terminal that started it
/// had (agent shell commands inherit it). None when it cannot be read.
pub fn process_environment(pid: u32) -> Option<Vec<(std::ffi::OsString, std::ffi::OsString)>> {
    let block = native_environment(pid)?;
    let mut pairs = Vec::new();
    for entry in block.split(|unit| *unit == 0) {
        if entry.is_empty() {
            break;
        }
        // Skip per-drive working directories such as "=C:=C:\repo".
        let Some(split) = entry
            .iter()
            .skip(1)
            .position(|unit| *unit == u16::from(b'='))
        else {
            continue;
        };
        let (name, value) = entry.split_at(split + 1);
        pairs.push((os_string(name), os_string(&value[1..])));
        if pairs.len() > 4096 {
            return None;
        }
    }
    (!pairs.is_empty()).then_some(pairs)
}
#[cfg(windows)]
fn os_string(units: &[u16]) -> std::ffi::OsString {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(units)
}
#[cfg(not(windows))]
fn os_string(units: &[u16]) -> std::ffi::OsString {
    String::from_utf16_lossy(units).into()
}
/// Read the UTF-16 environment block from the target's process parameters (x64 PEB:
/// ProcessParameters at +0x20; Environment at +0x80 and EnvironmentSize at +0x3F0).
#[cfg(all(windows, target_pointer_width = "64"))]
fn native_environment(pid: u32) -> Option<Vec<u16>> {
    use std::ffi::c_void;
    use windows_sys::{
        Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation},
        Win32::{
            Foundation::CloseHandle,
            System::{
                Diagnostics::Debug::ReadProcessMemory,
                Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
            },
        },
    };
    #[repr(C)]
    struct BasicInformation {
        exit_status: i32,
        peb: usize,
        affinity: usize,
        base_priority: i32,
        unique_process_id: usize,
        parent: usize,
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let read = |address: usize, buffer: *mut c_void, size: usize| -> bool {
        let mut done = 0usize;
        (unsafe { ReadProcessMemory(handle, address as *const c_void, buffer, size, &mut done) }
            != 0)
            && done == size
    };
    let result = (|| {
        let mut info: BasicInformation = unsafe { std::mem::zeroed() };
        let mut length = 0u32;
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                ProcessBasicInformation,
                (&mut info as *mut BasicInformation).cast(),
                size_of::<BasicInformation>() as u32,
                &mut length,
            )
        };
        if status < 0 || info.peb == 0 || info.unique_process_id != pid as usize {
            return None;
        }
        let mut parameters = 0usize;
        if !read(info.peb + 0x20, (&mut parameters as *mut usize).cast(), 8) || parameters == 0 {
            return None;
        }
        let (mut environment, mut size) = (0usize, 0usize);
        if !read(
            parameters + 0x80,
            (&mut environment as *mut usize).cast(),
            8,
        ) || !read(parameters + 0x3F0, (&mut size as *mut usize).cast(), 8)
            || environment == 0
            || !(4..=4 * 1024 * 1024).contains(&size)
        {
            return None;
        }
        let mut block = vec![0u16; size / 2];
        if !read(environment, block.as_mut_ptr().cast(), block.len() * 2) {
            return None;
        }
        // A well-formed block ends with an empty entry.
        block.windows(2).any(|w| w == [0, 0]).then_some(block)
    })();
    unsafe {
        CloseHandle(handle);
    }
    result
}
#[cfg(target_os = "linux")]
fn native_environment(pid: u32) -> Option<Vec<u16>> {
    let bytes = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let mut block: Vec<u16> = String::from_utf8_lossy(&bytes).encode_utf16().collect();
    block.extend([0, 0]);
    Some(block)
}
#[cfg(not(any(all(windows, target_pointer_width = "64"), target_os = "linux")))]
fn native_environment(_: u32) -> Option<Vec<u16>> {
    None
}

/// Whether this process runs elevated. Codex refuses to restart its daemon from an
/// elevated process on Windows, so a switch must not start in that case.
#[cfg(windows)]
pub fn process_is_elevated() -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut length = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    } != 0;
    unsafe {
        CloseHandle(token);
    }
    ok && elevation.TokenIsElevated != 0
}
#[cfg(not(windows))]
pub fn process_is_elevated() -> bool {
    false
}

/// Public for the effective-context resolver. Forced configuration/requirements
/// are never silently overridden. This reads no values and returns no policy data.
#[cfg(not(target_os = "macos"))]
pub fn macos_policy_is_unrestricted() -> CodexResult<()> {
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn macos_policy_is_unrestricted() -> CodexResult<()> {
    use std::{
        ffi::{c_char, c_void},
        ptr,
    };
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFPreferencesAppValueIsForced(key: *const c_void, app: *const c_void) -> u8;
        fn CFPreferencesAppSynchronize(app: *const c_void) -> u8;
        fn CFRelease(value: *const c_void);
    }
    struct Cf(*const c_void);
    impl Drop for Cf {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
    fn string(bytes: &'static [u8]) -> CodexResult<Cf> {
        let value =
            unsafe { CFStringCreateWithCString(ptr::null(), bytes.as_ptr().cast(), 0x08000100) };
        if value.is_null() {
            Err(CodexStoreError::UnsupportedContext)
        } else {
            Ok(Cf(value))
        }
    }
    let app = string(b"com.openai.codex\0")?;
    if unsafe { CFPreferencesAppSynchronize(app.0) } == 0 {
        return Err(CodexStoreError::UnsupportedContext);
    }
    for key in [
        b"config_toml_base64\0".as_slice(),
        b"requirements_toml_base64\0".as_slice(),
    ] {
        let key = string(key)?;
        if unsafe { CFPreferencesAppValueIsForced(key.0, app.0) } != 0 {
            return Err(CodexStoreError::UnsupportedContext);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn role(image: &str, line: Option<&str>) -> Role {
        classify(
            &PathBuf::from(image),
            line,
            &PathBuf::from(r"C:\Users\fixture\.codex\packages\app-server-daemon"),
        )
    }
    const NPM: &str = r"C:\Users\fixture\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe";
    #[test]
    fn terminals_reconnect_everything_else_keeps_its_login() {
        // Interactive terminals are daemon clients.
        assert_eq!(role(NPM, Some(&format!("\"{NPM}\""))), Role::DaemonClient);
        assert_eq!(
            role(NPM, Some(&format!("{NPM} resume --last"))),
            Role::DaemonClient
        );
        assert_eq!(
            role(NPM, Some(&format!("{NPM} -m gpt-6 \"fix the build\""))),
            Role::DaemonClient
        );
        assert_eq!(
            role(NPM, Some(&format!("{NPM} --no-daemon"))),
            Role::OtherClient
        );
        // The daemon, its updater and helpers.
        let daemon = r"\\?\C:\Users\fixture\.codex\packages\app-server-daemon\releases\0.160.0-x86_64-pc-windows-msvc\bin\codex.exe";
        assert_eq!(
            role(
                daemon,
                Some(&format!(
                    "\"{daemon}\" app-server --listen unix:// --managed-daemon"
                ))
            ),
            Role::DaemonServer
        );
        assert_eq!(role(daemon, None), Role::Daemon);
        assert_eq!(
            role(
                daemon,
                Some(&format!("\"{daemon}\" app-server daemon pid-update-loop"))
            ),
            Role::Daemon
        );
        // Separate logins.
        assert_eq!(
            role(
                NPM,
                Some(&format!("{NPM} exec -s read-only -C C:\\repo --json"))
            ),
            Role::OtherClient
        );
        assert_eq!(
            role(NPM, Some(&format!("{NPM} app-server --listen stdio://"))),
            Role::OtherClient
        );
        let desktop = r"C:\Program Files\WindowsApps\OpenAI.Codex_26.930.2377.0_x64__abc\app\resources\codex.exe";
        assert_eq!(
            role(desktop, Some(&format!("\"{desktop}\" app-server"))),
            Role::OtherClient
        );
        assert_eq!(role(NPM, None), Role::OtherClient);
        // Short-lived commands and other programs.
        assert_eq!(
            role(NPM, Some(&format!("{NPM} --version"))),
            Role::DaemonClient
        );
        assert_eq!(
            role(NPM, Some(&format!("{NPM} login status"))),
            Role::Ignored
        );
        assert_eq!(
            role(
                r"C:\Program Files\Codex Switcher\codex-switcher.exe",
                Some("x")
            ),
            Role::Ignored
        );
        assert_eq!(role(r"C:\x\codex-code-mode-host.exe", None), Role::Ignored);
    }
    #[test]
    fn command_lines_split_like_windows() {
        assert_eq!(
            split_arguments(r#""C:\a b\codex.exe" exec  -C "C:\x y" "say \"hi\"""#),
            vec![r"C:\a b\codex.exe", "exec", "-C", r"C:\x y", r#"say "hi""#]
        );
        assert!(split_arguments("").is_empty());
    }
    #[test]
    fn the_environment_of_this_process_can_be_read_back() {
        let pairs = process_environment(std::process::id());
        if cfg!(any(
            all(windows, target_pointer_width = "64"),
            target_os = "linux"
        )) {
            let pairs = pairs.expect("own environment is readable");
            let path = std::env::var_os("PATH").unwrap();
            assert!(
                pairs
                    .iter()
                    .any(|(name, value)| name.eq_ignore_ascii_case("PATH") && *value == path)
            );
            assert!(pairs.iter().all(|(name, _)| !name.is_empty()));
        }
    }
    #[test]
    fn scanning_never_fails() {
        let summary = scan_codex_processes(&std::env::temp_dir().join("no-codex-home"));
        assert!(summary.other_clients < 65536);
    }
}
