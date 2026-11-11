//! Sharing guarantees of the dedicated profile: every resource directory stays shared
//! even when it did not exist at the first launch, and the memory import can name any
//! path that can be written safely. Fixture homes only; no real configuration.
use super::tests::{ACCOUNT, credentials, fixture, identity, profile};
use super::*;

/// The import text for a path, computed independently of the implementation.
fn expected_import(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    let text = text
        .strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(text);
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    };
    text.replace(' ', "\\ ")
}

#[test]
fn every_shared_directory_is_created_and_linked_even_when_absent_at_first_launch() {
    let f = fixture();
    for name in SHARED_DIRECTORIES {
        assert!(!f.common.config_dir.join(name).exists(), "{name}");
    }
    let profile = profile(&f);
    profile
        .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
        .unwrap();
    let plan = profile.prepare(ACCOUNT, "org").unwrap();
    assert_eq!(plan.shared.linked.len(), SHARED_DIRECTORIES.len());
    assert!(plan.shared.separate.is_empty());
    for name in SHARED_DIRECTORIES {
        let shared = f.common.config_dir.join(name);
        let own = profile.paths().config_dir.join(name);
        assert!(shared.is_dir() && same_directory(&own, &shared), "{name}");
        // Something installed later through the profile lands in the shared store.
        fs::write(own.join("created-later.txt"), name.as_bytes()).unwrap();
        assert_eq!(
            fs::read(shared.join("created-later.txt")).unwrap(),
            name.as_bytes()
        );
    }
    // The profile's own login files never enter the shared side.
    assert!(!f.common.config_dir.join(".credentials.json").exists());
    assert!(!f.common.config_dir.join(".claude.json").exists());
    // A second launch changes nothing.
    assert_eq!(profile.prepare(ACCOUNT, "org").unwrap().shared, plan.shared);
}

#[test]
fn existing_shared_content_links_and_profile_directories_are_never_replaced() {
    let f = fixture();
    fs::create_dir_all(f.common.config_dir.join("plugins")).unwrap();
    fs::write(f.common.config_dir.join("plugins/keep.json"), b"installed").unwrap();
    let profile = profile(&f);
    profile
        .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
        .unwrap();
    let root = &profile.paths().config_dir;
    // An empty directory Claude Code created early is replaced by the link; one with
    // content stays as it is and is reported, while the shared side is still created.
    fs::create_dir_all(root.join("rules")).unwrap();
    fs::create_dir_all(root.join("commands")).unwrap();
    fs::write(root.join("commands/own.md"), b"own command").unwrap();
    let plan = profile.prepare(ACCOUNT, "org").unwrap();
    assert!(plan.shared.linked.contains(&"plugins"));
    assert!(plan.shared.linked.contains(&"rules"));
    assert_eq!(plan.shared.separate, vec!["commands"]);
    assert_eq!(
        fs::read(f.common.config_dir.join("plugins/keep.json")).unwrap(),
        b"installed"
    );
    assert_eq!(
        fs::read(root.join("commands/own.md")).unwrap(),
        b"own command"
    );
    let shared_commands = f.common.config_dir.join("commands");
    assert!(shared_commands.is_dir() && fs::read_dir(&shared_commands).unwrap().next().is_none());
    assert!(!plan.shared.splits_history());
}

#[test]
fn a_common_entry_that_is_not_a_directory_is_an_explicit_error_not_a_skip() {
    let f = fixture();
    fs::write(f.common.config_dir.join("skills"), b"not a directory").unwrap();
    let profile = profile(&f);
    profile
        .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
        .unwrap();
    assert!(matches!(
        profile.prepare(ACCOUNT, "org"),
        Err(PlatformError::UnsafePath)
    ));
    assert_eq!(
        fs::read(f.common.config_dir.join("skills")).unwrap(),
        b"not a directory"
    );
    assert!(!profile.paths().config_dir.join("skills").exists());
}

#[test]
fn a_common_path_with_spaces_and_unicode_is_imported_with_escaped_spaces() {
    let root = files::test_root();
    let common = CliPaths::for_home(root.path().join("Casa Ștefan și Ana"));
    fs::create_dir_all(&common.config_dir).unwrap();
    let source = common.config_dir.join("CLAUDE.md");
    let original = "Reguli comune: păstrează memoria.\n";
    fs::write(&source, original).unwrap();
    let profiles = root.path().join("date").join("claude-profiles");
    let profile = ClaudeProfile::new(&profiles, ACCOUNT, common.clone()).unwrap();
    profile
        .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
        .unwrap();
    let plan = profile.prepare(ACCOUNT, "org").unwrap();
    assert!(plan.instructions);
    let memory = fs::read_to_string(&profile.paths().memory_file).unwrap();
    assert_eq!(
        memory,
        format!("{MEMORY_MARKER}\n@{}\n", expected_import(&source))
    );
    // Each space of the path is escaped with a backslash; quotes are not used.
    let import = memory.lines().nth(1).unwrap().strip_prefix('@').unwrap();
    assert!(import.contains("Casa\\ Ștefan\\ și\\ Ana"));
    assert!(!import.contains('"') && !import.contains('\''));
    assert!(!import.replace("\\ ", "").contains(' '));
    // Reading the import back names the original file, whose content is untouched.
    let resolved = PathBuf::from(import.replace("\\ ", " "));
    assert_eq!(
        fs::canonicalize(&resolved).unwrap(),
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(fs::read(&source).unwrap(), original.as_bytes());
    // The next launch keeps the same file; the original stays the only copy.
    profile.prepare(ACCOUNT, "org").unwrap();
    assert_eq!(
        fs::read_to_string(&profile.paths().memory_file).unwrap(),
        memory
    );
    assert_eq!(fs::read(&source).unwrap(), original.as_bytes());
}

#[test]
fn imports_escape_only_ascii_spaces_and_refuse_ambiguous_syntax() {
    let import = |text: &str| memory_import(Path::new(text));
    assert_eq!(
        import("/fixtures/ana maria/.claude/CLAUDE.md").unwrap(),
        "/fixtures/ana\\ maria/.claude/CLAUDE.md"
    );
    assert_eq!(
        import("/h/Ștefan  și/CLAUDE.md").unwrap(),
        "/h/Ștefan\\ \\ și/CLAUDE.md"
    );
    if cfg!(windows) {
        let sep = '\\';
        assert_eq!(
            import(&format!("C:{sep}Fixtures{sep}Ana Maria{sep}CLAUDE.md")).unwrap(),
            "C:/Fixtures/Ana\\ Maria/CLAUDE.md"
        );
        assert_eq!(
            import("//?/C:/a b/CLAUDE.md").unwrap(),
            "C:/a\\ b/CLAUDE.md"
        );
        assert_eq!(
            import("//?/UNC/srv/share/x y/CLAUDE.md").unwrap(),
            "//srv/share/x\\ y/CLAUDE.md"
        );
    }
    for bad in [
        "a\tb",
        "a\nb",
        "a\rb",
        "a\u{b}b",
        "a\u{a0}b",
        "a\u{2003}b",
        "a`b",
        "a\0b",
    ] {
        assert!(
            matches!(
                import(&format!("/x/{bad}/CLAUDE.md")),
                Err(PlatformError::UnsafePath)
            ),
            "{bad:?}"
        );
    }
    if cfg!(unix) {
        assert!(matches!(
            import("/x/a\\b/CLAUDE.md"),
            Err(PlatformError::UnsafePath)
        ));
    }
}

#[test]
fn an_unrepresentable_source_stops_the_launch_and_loses_no_memory() {
    let mut names = vec!["a\u{a0}b", "a`b"];
    if cfg!(unix) {
        names.extend(["a\nb", "a\tb", "a\\b"]);
    }
    for name in names {
        let root = files::test_root();
        let common = CliPaths::for_home(root.path().join(name));
        fs::create_dir_all(&common.config_dir).unwrap();
        fs::write(common.config_dir.join("CLAUDE.md"), b"memory").unwrap();
        let profile =
            ClaudeProfile::new(&root.path().join("profiles"), ACCOUNT, common.clone()).unwrap();
        profile
            .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
            .unwrap();
        // An import written by an earlier launch is kept, and the original is untouched.
        let managed = format!("{MEMORY_MARKER}\n@/earlier/CLAUDE.md\n");
        fs::write(&profile.paths().memory_file, &managed).unwrap();
        assert!(
            matches!(
                profile.prepare(ACCOUNT, "org"),
                Err(PlatformError::UnsafePath)
            ),
            "{name:?}"
        );
        assert_eq!(
            fs::read_to_string(&profile.paths().memory_file).unwrap(),
            managed
        );
        assert_eq!(
            fs::read(common.config_dir.join("CLAUDE.md")).unwrap(),
            b"memory"
        );
        // A CLAUDE.md the user wrote in the profile is never touched or replaced.
        fs::write(&profile.paths().memory_file, b"my own rules").unwrap();
        assert!(!profile.prepare(ACCOUNT, "org").unwrap().instructions);
        assert_eq!(
            fs::read(&profile.paths().memory_file).unwrap(),
            b"my own rules"
        );
    }
}

#[test]
fn a_missing_source_removes_only_the_managed_import() {
    let f = fixture();
    let profile = profile(&f);
    profile
        .authorize(&credentials("one"), &identity(ACCOUNT, "org"))
        .unwrap();
    fs::write(
        &profile.paths().memory_file,
        format!("{MEMORY_MARKER}\n@/earlier/CLAUDE.md\n"),
    )
    .unwrap();
    assert!(!profile.prepare(ACCOUNT, "org").unwrap().instructions);
    assert!(!profile.paths().memory_file.exists());
}
