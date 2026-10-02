use super::{CodexResult, CodexStoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};
use zeroize::Zeroizing;

/// Equality includes native identity/change metadata, not just the payload digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreGeneration {
    pub(crate) fingerprint: String,
    pub(crate) native_identity: String,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn metadata(file: &File, identity_only: bool) -> CodexResult<String> {
    let meta = file.metadata()?;
    #[cfg(windows)]
    {
        use std::{mem::size_of, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle,
            GetFileInformationByHandleEx,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        let handle = file.as_raw_handle();
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0
            || unsafe {
                GetFileInformationByHandleEx(
                    handle,
                    FileBasicInfo,
                    (&mut basic as *mut FILE_BASIC_INFO).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
                )
            } == 0
        {
            return Err(crate::PlatformError::Io.into());
        }
        if identity_only {
            return Ok(format!(
                "{}:{}:{}",
                info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
            ));
        }
        Ok(format!(
            "{}:{}:{}:{}:{}:{}",
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
            meta.len(),
            basic.LastWriteTime,
            basic.ChangeTime
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if identity_only {
            return Ok(format!("{}:{}", meta.dev(), meta.ino()));
        }
        Ok(format!(
            "{}:{}:{}:{}:{}:{}:{}",
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec()
        ))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (meta, identity_only);
        Err(CodexStoreError::UnsupportedContext)
    }
}

fn open(path: &Path) -> CodexResult<File> {
    crate::files::check_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    Ok(options.open(path)?)
}

/// A handle-bound read followed by a path identity check rejects replacement races.
pub(crate) fn read(
    path: &Path,
    context: &str,
    max: u64,
    private_auth: bool,
) -> CodexResult<(Option<Zeroizing<Vec<u8>>>, StoreGeneration)> {
    crate::files::check_path(path)?;
    let mut file = match open(path) {
        Ok(file) => file,
        Err(CodexStoreError::Storage(crate::PlatformError::Io)) if !path.exists() => {
            // Parent identity/change metadata catches create/remove/create while absent.
            let mut parent = path.parent().ok_or(crate::PlatformError::UnsafePath)?;
            while !parent.try_exists()? {
                parent = parent.parent().ok_or(crate::PlatformError::UnsafePath)?;
            }
            let native = metadata(&open(parent)?, !private_auth)?;
            crate::files::check_path(path)?;
            if path.try_exists()? {
                return Err(CodexStoreError::ExternalChange);
            }
            return Ok((
                None,
                StoreGeneration {
                    fingerprint: digest(format!("{context}:absent:{native}").as_bytes()),
                    native_identity: digest(metadata(&open(parent)?, true)?.as_bytes()),
                },
            ));
        }
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > max {
        return Err(crate::PlatformError::UnsafePath.into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if private_auth && (meta.uid() != unsafe { libc::geteuid() } || meta.nlink() != 1) {
            return Err(crate::PlatformError::UnsafePath.into());
        }
    }
    let before = metadata(&file, false)?;
    let mut bytes = Zeroizing::new(Vec::new());
    (&mut file).take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(crate::PlatformError::Io.into());
    }
    let after = metadata(&file, false)?;
    let current = metadata(&open(path)?, false)?;
    if before != after || after != current {
        return Err(CodexStoreError::ExternalChange);
    }
    let fingerprint = digest(format!("{context}:present:{after}:{}", digest(&bytes)).as_bytes());
    Ok((
        Some(bytes),
        StoreGeneration {
            fingerprint,
            native_identity: digest(metadata(&file, true)?.as_bytes()),
        },
    ))
}

/// Native identity excludes timestamps so an owned rename can be recognized even
/// when the filesystem changes its change time during that rename.
pub(crate) fn native_identity(path: &Path) -> CodexResult<String> {
    Ok(digest(metadata(&open(path)?, true)?.as_bytes()))
}

pub(crate) fn path_digest(prefix: &[u8], path: &Path) -> String {
    let mut hash = Sha256::new();
    hash.update(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hash.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            hash.update(unit.to_le_bytes());
        }
    }
    #[cfg(not(any(windows, unix)))]
    hash.update(path.to_string_lossy().as_bytes());
    format!("{:x}", hash.finalize())
}
pub(crate) fn directory_generation(path: &Path) -> CodexResult<StoreGeneration> {
    let file = open(path)?;
    if !file.metadata()?.is_dir() {
        return Err(CodexStoreError::ExternalChange);
    }
    let native_identity = digest(metadata(&file, true)?.as_bytes());
    let fingerprint = digest(metadata(&file, false)?.as_bytes());
    if native_identity != self::native_identity(path)? {
        return Err(CodexStoreError::ExternalChange);
    }
    Ok(StoreGeneration {
        fingerprint,
        native_identity,
    })
}
