//! Native key protection; never invokes a credential-bearing child process.
#[cfg(windows)]
use crate::PlatformError;
use crate::Result;
use std::path::Path;

#[cfg(windows)]
pub(crate) fn protect_file(path: &Path, directory: bool) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
            SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(PlatformError::Io);
        }
        let mut needed = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
        // usize storage guarantees TOKEN_USER alignment.
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(PlatformError::Io);
        }
        let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
        let mut sid_string = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid_string) == 0 {
            return Err(PlatformError::Io);
        }
        let mut len = 0;
        while *sid_string.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(sid_string, len));
        LocalFree(sid_string.cast());
        let inherit = if directory { "OICI" } else { "" };
        let descriptor = format!("D:P(A;{inherit};FA;;;{sid})(A;{inherit};FA;;;SY)");
        let wide: Vec<u16> = descriptor.encode_utf16().chain(Some(0)).collect();
        let mut security = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut security,
            ptr::null_mut(),
        ) == 0
        {
            return Err(PlatformError::Io);
        }
        let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let ok = SetFileSecurityW(
            filename.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            security,
        );
        LocalFree(security.cast());
        if ok == 0 {
            return Err(PlatformError::Io);
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn protect_file(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn protect_key(key: &[u8]) -> Result<Vec<u8>> {
    dpapi(key, true)
}
#[cfg(windows)]
pub(crate) fn unprotect_key(bytes: &[u8]) -> Result<Vec<u8>> {
    dpapi(bytes, false)
}

#[cfg(windows)]
fn dpapi(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| PlatformError::KeyUnavailable)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // Additional application entropy separates this key from unrelated DPAPI blobs.
    let entropy_bytes = b"com.primertech.primerswitch/vault-key/v1";
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy_bytes.len() as u32,
        pbData: entropy_bytes.as_ptr().cast_mut(),
    };
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                ptr::null(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(PlatformError::KeyUnavailable);
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        // Wipe plaintext OS allocation before releasing it.
        if !encrypt {
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        }
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
        GetFileSecurityW, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        WinLocalSystemSid,
    };

    pub(crate) fn assert_private_acl(path: &Path, directory: bool) {
        let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut needed = 0;
            let info = DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION;
            GetFileSecurityW(filename.as_ptr(), info, ptr::null_mut(), 0, &mut needed);
            let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
            let security = buffer.as_mut_ptr().cast();
            assert_ne!(
                GetFileSecurityW(filename.as_ptr(), info, security, needed, &mut needed),
                0
            );
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                GetSecurityDescriptorControl(security, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(security, &mut present, &mut dacl, &mut defaulted),
                0
            );
            assert_ne!(present, 0);
            assert!(!dacl.is_null());
            assert_eq!((*dacl).AceCount, 2);
            let mut owner = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorOwner(security, &mut owner, &mut defaulted),
                0
            );
            let mut system_sid = [0usize; 16];
            let mut system_len = size_of_val(&system_sid) as u32;
            assert_ne!(
                CreateWellKnownSid(
                    WinLocalSystemSid,
                    ptr::null_mut(),
                    system_sid.as_mut_ptr().cast(),
                    &mut system_len
                ),
                0
            );
            for index in 0..2 {
                let mut ace = ptr::null_mut();
                assert_ne!(GetAce(dacl, index, &mut ace), 0);
                let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                assert_eq!(ace.Header.AceType, 0); // ACCESS_ALLOWED_ACE_TYPE
                assert_eq!(ace.Header.AceFlags, if directory { 3 } else { 0 }); // inheritance, never inherited
                let sid = ptr::addr_of!(ace.SidStart).cast_mut().cast();
                let expected = if index == 0 {
                    owner
                } else {
                    system_sid.as_mut_ptr().cast()
                };
                assert_ne!(EqualSid(sid, expected), 0);
            }
        }
    }
}
