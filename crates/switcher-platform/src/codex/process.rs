use super::{CodexFileContext, CodexResult, CodexStoreError};
use std::path::Path;

/// Injectable for hermetic fixtures. Production uses the native fail-closed guard.
/// Guard snapshots cannot prevent a new process launch atomically; runtime must
/// retain reconciliation state on any post-write failure and reopen client sessions.
pub trait CodexWriteGuard: Send + Sync {
    fn check(&self, context: &CodexFileContext) -> CodexResult<()>;
}
#[derive(Debug, Default)]
pub struct NativeCodexWriteGuard;
impl CodexWriteGuard for NativeCodexWriteGuard {
    fn check(&self, context: &CodexFileContext) -> CodexResult<()> {
        context.revalidate()?;
        macos_policy_is_unrestricted()?;
        for name in [
            "CODEX_API_KEY",
            "CODEX_ACCESS_TOKEN",
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "CODEX_AUTHAPI_BASE_URL",
            "CODEX_INTERNAL_ORIGINATOR_OVERRIDE",
            "OPENAI_FEDERATION_RULE_ID",
            "OPENAI_IDENTITY_TOKEN_FILE",
            "OPENAI_WORKLOAD_IDENTITY_CONTEXT",
            "CODEX_APP_SERVER_LOGIN_CLIENT_ID",
            "CODEX_REFRESH_TOKEN_URL_OVERRIDE",
            "CODEX_REVOKE_TOKEN_URL_OVERRIDE",
        ] {
            if std::env::var_os(name).is_some() {
                return Err(CodexStoreError::UnsupportedContext);
            }
        }
        let home = match std::env::var_os("CODEX_HOME") {
            Some(home) if !home.is_empty() => std::path::PathBuf::from(home),
            Some(_) => return Err(CodexStoreError::UnsupportedContext),
            None => std::path::PathBuf::from(
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .ok_or(CodexStoreError::UnsupportedContext)?,
            )
            .join(".codex"),
        };
        crate::files::check_path(&home)?;
        if home.canonicalize()? != context.home() {
            return Err(CodexStoreError::UnsupportedContext);
        }
        native_process_guard(context.executable()?)
    }
}

fn stable_generation<T: PartialEq>(before: T, after: T) -> CodexResult<()> {
    if before != after {
        Err(CodexStoreError::ProcessVisibility)
    } else {
        Ok(())
    }
}

fn candidate(path: &Path, selected: &Path) -> bool {
    if path == selected {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = name.strip_suffix(" (deleted)").unwrap_or(&name);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    name == "codex" || name.starts_with("codex-") || path.file_name() == selected.file_name()
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

#[cfg(windows)]
fn native_process_guard(selected: &Path) -> CodexResult<()> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_NO_MORE_FILES, FILETIME, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
        },
        Security::{EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
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
    fn user(process: HANDLE) -> CodexResult<Vec<usize>> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(CodexStoreError::ProcessVisibility);
        }
        let token = Handle(token);
        let mut needed = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 || needed > 65536 {
            return Err(CodexStoreError::ProcessVisibility);
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(CodexStoreError::ProcessVisibility);
        }
        Ok(bytes)
    }
    fn creation(process: HANDLE) -> CodexResult<u64> {
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exit = created;
        let mut kernel = created;
        let mut user = created;
        if unsafe { GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user) } == 0
        {
            return Err(CodexStoreError::ProcessVisibility);
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    let own = user(unsafe { GetCurrentProcess() })?;
    let own_sid = unsafe { (*(own.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(CodexStoreError::ProcessVisibility);
    }
    let snapshot = Handle(raw);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(CodexStoreError::ProcessVisibility);
    }
    let mut count = 0;
    loop {
        count += 1;
        if count > 65536 {
            return Err(CodexStoreError::ProcessVisibility);
        }
        let len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let named = std::path::PathBuf::from(OsString::from_wide(&entry.szExeFile[..len]));
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
        if raw.is_null() {
            if candidate(&named, selected) {
                return Err(CodexStoreError::ProcessVisibility);
            }
        } else {
            let process = Handle(raw);
            let mut path = vec![0u16; 32768];
            let mut length = path.len() as u32;
            let path_ok =
                unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length) }
                    != 0;
            let image = std::path::PathBuf::from(OsString::from_wide(&path[..length as usize]));
            let relevant = candidate(&named, selected) || (path_ok && candidate(&image, selected));
            if relevant {
                if !path_ok {
                    return Err(CodexStoreError::ProcessVisibility);
                }
                let before = creation(process.0)?;
                let owner = user(process.0)?;
                let sid = unsafe { (*(owner.as_ptr().cast::<TOKEN_USER>())).User.Sid };
                stable_generation(before, creation(process.0)?)?;
                if unsafe { EqualSid(own_sid, sid) } != 0 {
                    return Err(CodexStoreError::RunningClients);
                }
            }
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                return Err(CodexStoreError::ProcessVisibility);
            }
            break;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn native_process_guard(selected: &Path) -> CodexResult<()> {
    use std::{fs, os::unix::fs::MetadataExt};
    let uid = unsafe { libc::geteuid() };
    let mounts =
        fs::read_to_string("/proc/mounts").map_err(|_| CodexStoreError::ProcessVisibility)?;
    if mounts.len() > 1024 * 1024 {
        return Err(CodexStoreError::ProcessVisibility);
    }
    let options = mounts
        .lines()
        .find_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            (fields.len() >= 4 && fields[1] == "/proc" && fields[2] == "proc").then(|| fields[3])
        })
        .ok_or(CodexStoreError::ProcessVisibility)?;
    if options
        .split(',')
        .any(|option| option.starts_with("hidepid=") && option != "hidepid=0")
    {
        return Err(CodexStoreError::ProcessVisibility);
    }
    let entries = fs::read_dir("/proc").map_err(|_| CodexStoreError::ProcessVisibility)?;
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|_| CodexStoreError::ProcessVisibility)?;
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        count += 1;
        if count > 65536 {
            return Err(CodexStoreError::ProcessVisibility);
        }
        let path = entry.path();
        let metadata = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(CodexStoreError::ProcessVisibility),
        };
        if metadata.uid() != uid {
            continue;
        }
        let before = match fs::read_to_string(path.join("stat")) {
            Ok(stat) => stat,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(CodexStoreError::ProcessVisibility),
        };
        let before = proc_start(&before)?;
        let image = match fs::read_link(path.join("exe")) {
            Ok(image) => image,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !path.exists() => continue,
            Err(_) => return Err(CodexStoreError::ProcessVisibility),
        };
        let after = match fs::read_to_string(path.join("stat")) {
            Ok(stat) => stat,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(CodexStoreError::ProcessVisibility),
        };
        stable_generation(before, proc_start(&after)?)?;
        if candidate(&image, selected) {
            return Err(CodexStoreError::RunningClients);
        }
    }
    Ok(())
}
#[cfg(any(target_os = "linux", test))]
fn proc_start(stat: &str) -> CodexResult<u64> {
    stat.rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .and_then(|s| s.parse().ok())
        .ok_or(CodexStoreError::ProcessVisibility)
}

#[cfg(target_os = "macos")]
fn native_process_guard(selected: &Path) -> CodexResult<()> {
    use std::{
        ffi::{CStr, OsStr},
        mem::size_of,
        os::unix::ffi::OsStrExt,
    };
    // A bounded oversized list avoids silently accepting a truncated snapshot.
    let mut pids = vec![0i32; 65536];
    let count = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * size_of::<i32>()) as i32,
        )
    };
    if count <= 0 || count as usize >= pids.len() {
        return Err(CodexStoreError::ProcessVisibility);
    }
    pids.truncate(count as usize);
    let uid = unsafe { libc::geteuid() };
    for pid in pids {
        if pid <= 0 {
            continue;
        }
        let mut before: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = size_of::<libc::proc_bsdinfo>() as i32;
        if unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut before as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        } != size
        {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(CodexStoreError::ProcessVisibility);
        }
        if before.pbi_uid != uid {
            continue;
        }
        let mut path = vec![0i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        if unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) } <= 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(CodexStoreError::ProcessVisibility);
        }
        let image = std::path::PathBuf::from(OsStr::from_bytes(
            unsafe { CStr::from_ptr(path.as_ptr()) }.to_bytes(),
        ));
        let mut after: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut after as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        } != size
        {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(CodexStoreError::ProcessVisibility);
        }
        stable_generation(
            (
                before.pbi_start_tvsec,
                before.pbi_start_tvusec,
                before.pbi_uid,
            ),
            (after.pbi_start_tvsec, after.pbi_start_tvusec, after.pbi_uid),
        )?;
        if candidate(&image, selected) {
            return Err(CodexStoreError::RunningClients);
        }
    }
    Ok(())
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn native_process_guard(_: &Path) -> CodexResult<()> {
    Err(CodexStoreError::ProcessVisibility)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_names_and_native_executable_alias_are_conservative() {
        let selected = Path::new("/fixtures/renamed-native");
        for name in [
            "/fixtures/renamed-native",
            "/other/Codex.exe",
            "/other/codex-aarch64",
            "/other/codex (deleted)",
        ] {
            assert!(candidate(Path::new(name), selected));
        }
        assert!(!candidate(Path::new("/other/node"), selected));
        assert!(candidate(
            Path::new("/other/codex-helper-unrelated"),
            selected
        ));
    }
    #[test]
    fn pid_reuse_or_changed_owner_is_visibility_failure() {
        assert!(stable_generation((27u32, 948u64, 1000u32), (27, 948, 1000)).is_ok());
        assert!(matches!(
            stable_generation((27u32, 948u64, 1000u32), (27, 950, 1000)),
            Err(CodexStoreError::ProcessVisibility)
        ));
        assert!(matches!(
            stable_generation((27u32, 948u64, 1000u32), (27, 948, 0)),
            Err(CodexStoreError::ProcessVisibility)
        ));
    }
    #[test]
    fn proc_stat_handles_spaces_and_rejects_missing_generation() {
        let fields = (0..20)
            .map(|i| if i == 19 { "948" } else { "0" })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            proc_start(&format!("27 (name with ) spaces) {fields}")).unwrap(),
            948
        );
        assert!(matches!(
            proc_start("27 (codex) S 1"),
            Err(CodexStoreError::ProcessVisibility)
        ));
    }
}
