use crate::{PlatformError, Result, protection};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_FILE: u64 = 32 * 1024 * 1024;

/// Reject links/junctions rather than following them while writing credentials.
pub(crate) fn check_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(PlatformError::UnsafePath);
    }
    #[cfg(windows)]
    {
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                use std::os::windows::ffi::OsStrExt;
                let units: Vec<u16> = name.encode_wide().collect();
                // Alternate data streams and Win32 trailing-character aliases
                // cannot safely represent distinct auth contexts.
                if units.contains(&58) || units.last().is_some_and(|u| matches!(u, 32 | 46)) {
                    return Err(PlatformError::UnsafePath);
                }
            }
        }
    }
    for component in path.ancestors() {
        match fs::symlink_metadata(component) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(PlatformError::UnsafePath);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(PlatformError::UnsafePath);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(PlatformError::Io),
        }
    }
    Ok(())
}

pub(crate) fn private_dir(path: &Path) -> Result<()> {
    check_path(path)?;
    fs::create_dir_all(path)?;
    check_path(path)?;
    protection::protect_file(path, true)
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    check_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PlatformError::Io),
    };
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1 {
            return Err(PlatformError::UnsafePath);
        }
    }
    if !file.metadata()?.is_file() {
        return Err(PlatformError::UnsafePath);
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(PlatformError::Io);
    }
    Ok(Some(bytes))
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    check_path(path)?;
    let parent = path.parent().ok_or(PlatformError::UnsafePath)?;
    // Caller chooses which directory is owned. Never tighten the home directory ACL.
    fs::create_dir_all(parent)?;
    check_path(parent)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".primerswitch-")
        .tempfile_in(parent)?;
    protection::protect_file(temp.path(), false)?;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;
    let temp_path = temp.into_temp_path(); // closes the handle before Windows replacement
    check_path(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let source: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let dest: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // No delete-destination fallback: held handles fail without destroying the original.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                dest.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(PlatformError::Io);
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temp_path, path)?;
    protection::protect_file(path, false)?;
    sync_parent(parent)?;
    Ok(())
}

pub(crate) fn remove(path: &Path) -> Result<()> {
    check_path(path)?;
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path.parent().ok_or(PlatformError::UnsafePath)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(PlatformError::Io),
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) struct Lock(File);
impl Lock {
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        check_path(path)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
            #[cfg(target_os = "linux")]
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        protection::protect_file(path, false)?;
        file.try_lock_exclusive().map_err(|_| PlatformError::Busy)?;
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[cfg(test)]
pub(crate) fn test_root() -> tempfile::TempDir {
    // macOS /var is a normal OS alias for /private/var. Resolve the trusted
    // temporary directory before creating fixture roots; credential path checks
    // continue to reject unexpected symlinks in the tested tree.
    let parent = std::env::temp_dir().canonicalize().unwrap();
    tempfile::Builder::new()
        .prefix("primerswitch-test-")
        .tempdir_in(parent)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_replace_are_private_and_leave_no_temporary_files() {
        let root = crate::files::test_root();
        let private = root.path().join("private");
        private_dir(&private).unwrap();
        let path = private.join("auth.json");
        atomic_write(&path, b"first-secret").unwrap();
        atomic_write(&path, b"second-secret").unwrap();
        assert_eq!(read_optional(&path).unwrap().unwrap(), b"second-secret");
        assert_eq!(fs::read_dir(&private).unwrap().count(), 1);
        #[cfg(windows)]
        {
            crate::protection::tests::assert_private_acl(&private, true);
            crate::protection::tests::assert_private_acl(&path, false);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&private).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_handle_denying_delete_does_not_destroy_the_destination() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = crate::files::test_root();
        let path = root.path().join("held.json");
        atomic_write(&path, b"original").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(atomic_write(&path, b"replacement").is_err());
        assert_eq!(read_optional(&path).unwrap().unwrap(), b"original");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        drop(held);
        atomic_write(&path, b"replacement").unwrap();
        assert_eq!(read_optional(&path).unwrap().unwrap(), b"replacement");
    }

    #[cfg(windows)]
    #[test]
    fn alternate_streams_and_trailing_aliases_are_unsafe() {
        let root = crate::files::test_root();
        for name in ["auth.json:alternate", "auth.json.", "auth.json "] {
            assert!(matches!(
                atomic_write(&root.path().join(name), b"secret"),
                Err(PlatformError::UnsafePath)
            ));
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hardlinked_credential_files_and_fifo_locks_are_rejected() {
        use std::os::unix::ffi::OsStrExt;
        let root = crate::files::test_root();
        let target = root.path().join("credential.json");
        atomic_write(&target, b"fixture-secret").unwrap();
        let alias = root.path().join("hardlink.json");
        fs::hard_link(&target, &alias).unwrap();
        assert!(matches!(
            read_optional(&alias),
            Err(PlatformError::UnsafePath)
        ));
        assert!(matches!(
            protection::protect_file(&alias, false),
            Err(PlatformError::UnsafePath)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"fixture-secret");
        let fifo = root.path().join("fifo.lock");
        let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            Lock::acquire(&fifo),
            Err(PlatformError::UnsafePath)
        ));
        assert!(matches!(
            read_optional(&fifo),
            Err(PlatformError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_paths_are_rejected_without_touching_target() {
        use std::os::unix::fs::symlink;
        let root = crate::files::test_root();
        let target = root.path().join("target");
        fs::write(&target, b"original").unwrap();
        let link = root.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            atomic_write(&link, b"secret"),
            Err(PlatformError::UnsafePath)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"original");
    }
}
