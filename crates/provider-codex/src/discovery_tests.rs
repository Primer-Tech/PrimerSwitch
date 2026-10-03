use super::*;
fn native_bytes() -> Vec<u8> {
    let mut bytes = if cfg!(windows) {
        b"MZ00".to_vec()
    } else if cfg!(target_os = "macos") {
        vec![0xcf, 0xfa, 0xed, 0xfe]
    } else {
        b"\x7fELF".to_vec()
    };
    bytes.extend_from_slice(b"fixture-code");
    bytes
}
fn native_name() -> &'static str {
    if cfg!(windows) { "codex.exe" } else { "codex" }
}
#[test]
fn discovery_is_native_only_deduplicates_and_never_opens_auth() {
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join(native_name());
    std::fs::write(&native, native_bytes()).unwrap();
    std::fs::write(root.path().join("auth.json"), "SECRET-AUTH").unwrap();
    let options = DiscoveryOptions {
        explicit_native: Some(native.clone()),
        path_directories: vec![root.path().into(), root.path().into()],
        npm_package_roots: vec![],
    };
    let candidates = discover_candidates(&options).unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(!format!("{:?}", candidates[0]).contains("SECRET"));
    std::fs::write(&native, b"#!/bin/sh\nnot-a-native-binary").unwrap();
    assert!(discover_candidates(&options).unwrap().is_empty());
    assert_eq!(
        std::fs::read(root.path().join("auth.json")).unwrap(),
        b"SECRET-AUTH"
    );
}
#[test]
fn executable_fingerprint_rejects_replacement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(native_name());
    std::fs::write(&path, native_bytes()).unwrap();
    let path = path.canonicalize().unwrap();
    let executable = VerifiedCodexExecutable {
        identity: identity(&path).unwrap(),
        path: path.clone(),
        version: "0.160.0".into(),
    };
    assert_eq!(executable.recheck().unwrap(), path);
    let mut changed = native_bytes();
    changed.push(1);
    std::fs::write(path, changed).unwrap();
    assert_eq!(
        executable.recheck().unwrap_err(),
        CodexError::UnsupportedInstallation
    );
}
#[test]
fn npm_platform_resolution_reads_only_recognized_package_metadata() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("node_modules/@openai/codex");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("package.json"),
        format!(r#"{{"name":"@openai/codex","version":"{}"}}"#, "0.161.2"),
    )
    .unwrap();
    let (platform, target) = if cfg!(all(windows, target_arch = "x86_64")) {
        ("win32-x64", "x86_64-pc-windows-msvc")
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        ("win32-arm64", "aarch64-pc-windows-msvc")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        ("darwin-x64", "x86_64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        ("darwin-arm64", "aarch64-apple-darwin")
    } else if cfg!(target_arch = "aarch64") {
        ("linux-arm64", "aarch64-unknown-linux-musl")
    } else {
        ("linux-x64", "x86_64-unknown-linux-musl")
    };
    let native_package = package.join(format!("node_modules/@openai/codex-{platform}"));
    let bin = native_package.join("vendor").join(target).join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        native_package.join("package.json"),
        format!(r#"{{"name":"@openai/codex","version":"0.161.2-{platform}"}}"#),
    )
    .unwrap();
    std::fs::write(bin.join(native_name()), native_bytes()).unwrap();
    let options = DiscoveryOptions {
        npm_package_roots: vec![package.clone()],
        ..Default::default()
    };
    assert_eq!(discover_candidates(&options).unwrap().len(), 1);
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"@openai/codex","version":"0.159.0"}"#,
    )
    .unwrap();
    assert!(discover_candidates(&options).unwrap().is_empty());
}
