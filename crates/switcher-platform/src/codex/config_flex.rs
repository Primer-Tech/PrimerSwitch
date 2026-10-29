use super::{CodexFileContext, CodexResult, CodexStoreError, StoreGeneration};
use crate::Vault;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use zeroize::Zeroizing;

/// The only key this type manages: the top-level `service_tier` of `config.toml`.
const KEY: &str = "service_tier";
const FLEX: &str = "\"flex\"";
const MAX_CONFIG: u64 = 32 * 1024 * 1024;
/// A value longer than this is never read back from a saved mark.
const MAX_PREVIOUS: usize = 256;

/// What `config.toml` held before PrimerSwitch set Flex, kept so it can be put back.
/// `previous` is the TOML text of the old value, for example `"default"` with its
/// quotes (`None`: the key was absent and PrimerSwitch added it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlexMark {
    pub previous: Option<String>,
    pub config_existed: bool,
}

/// Sets and clears `service_tier = "flex"` in `$CODEX_HOME/config.toml`.
///
/// The file is edited as bytes: the parser only locates the value, so comments, line
/// endings, tables and every other key stay byte for byte as they were. A
/// `service_tier` inside a table or profile is never touched. Writes are atomic
/// (staged file, then replace) and are refused when the file changed since it was
/// read; the new file keeps the permissions of the old one on Unix and inherits the
/// directory's access rules on Windows, as a file created by Codex itself does.
/// No other file is read or written.
pub struct CodexConfigFlex {
    context: CodexFileContext,
}
impl CodexConfigFlex {
    pub fn new(context: CodexFileContext) -> Self {
        Self { context }
    }
    fn path(&self) -> std::path::PathBuf {
        self.context.home().join("config.toml")
    }
    fn read(&self) -> CodexResult<(Option<Zeroizing<Vec<u8>>>, StoreGeneration)> {
        self.context.revalidate()?;
        super::generation::read(&self.path(), "codex-config-flex-v1", MAX_CONFIG, false)
    }
    /// The decoded top-level `service_tier`, or `None` when it is not set.
    pub fn current(&self) -> CodexResult<Option<String>> {
        let (bytes, _) = self.read()?;
        let text = text_of(slice(&bytes))?;
        let found = locate(text)?;
        if found.other {
            return Err(CodexStoreError::UnsupportedContext);
        }
        Ok(found.value.map(|value| value.decoded))
    }
    /// Make the top-level `service_tier` "flex". Returns what to restore later. Nothing
    /// is written when the value is already "flex" (the mark then keeps "flex" as the
    /// value to restore, so an owner's own setting is never removed).
    pub fn apply(&self, vault: &Vault) -> CodexResult<FlexMark> {
        self.edit(vault, |text, existed| {
            let found = locate(text)?;
            if found.other {
                return Err(CodexStoreError::UnsupportedContext);
            }
            let mark = FlexMark {
                previous: found
                    .value
                    .as_ref()
                    .map(|value| text[value.span.clone()].to_owned()),
                config_existed: existed,
            };
            let new = match &found.value {
                Some(value) if value.decoded == "flex" => return Ok((mark, None)),
                Some(value) => splice(text, value.span.clone(), FLEX),
                None => {
                    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
                    let at = found.insert_at;
                    splice(text, at..at, &format!("{KEY} = {FLEX}{eol}"))
                }
            };
            verify(&new, Some("flex"))?;
            Ok((mark, Some(new)))
        })
    }
    /// Put the saved value back, but only while the file still says "flex"; a value the
    /// owner changed since is left alone. A file this type created is removed again when
    /// it is empty. Returns whether the file no longer sets Flex.
    pub fn restore(&self, mark: &FlexMark, vault: &Vault) -> CodexResult<bool> {
        let previous = match &mark.previous {
            Some(raw) if raw.len() > MAX_PREVIOUS => {
                return Err(CodexStoreError::UnsupportedContext);
            }
            Some(raw) => Some((raw.as_str(), string_token(raw)?)),
            None => None,
        };
        let (flex_off, key_removed) = self.edit(vault, |text, _| {
            let found = locate(text)?;
            let Some(value) = found.value.filter(|value| value.decoded == "flex") else {
                // The owner changed it since: not ours to put back.
                return Ok(((true, false), None));
            };
            let (outcome, new) = match &previous {
                // The owner had Flex set already; it stays.
                Some((_, decoded)) if decoded == "flex" => return Ok(((false, false), None)),
                Some((raw, decoded)) => {
                    let new = splice(text, value.span, raw);
                    verify(&new, Some(decoded))?;
                    ((true, false), new)
                }
                None => {
                    let new = splice(text, value.line, "");
                    verify(&new, None)?;
                    ((true, true), new)
                }
            };
            Ok((outcome, Some(new)))
        })?;
        if key_removed && !mark.config_existed {
            self.remove_if_empty(vault)?;
        }
        Ok(flex_off)
    }
    /// Read, compute, write: retried when Codex rewrites the file in between.
    fn edit<T>(
        &self,
        vault: &Vault,
        compute: impl Fn(&str, bool) -> CodexResult<(T, Option<String>)>,
    ) -> CodexResult<T> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let (bytes, generation) = self.read()?;
            let text = text_of(slice(&bytes))?;
            let (result, new) = compute(text, bytes.is_some())?;
            let Some(new) = new else {
                return Ok(result);
            };
            match self.write(slice(&bytes), &generation, &new) {
                Ok(()) => return Ok(result),
                Err(CodexStoreError::ExternalChange) if attempts < 4 => continue,
                Err(error) => return Err(error),
            }
        }
    }
    fn remove_if_empty(&self, vault: &Vault) -> CodexResult<()> {
        let _lock = vault.context_lock(self.context.context_id())?;
        let (bytes, generation) = self.read()?;
        let Some(bytes) = bytes else {
            return Ok(());
        };
        if !text_of(Some(bytes.as_slice()))?.trim().is_empty() {
            return Ok(());
        }
        if self.read()?.1 != generation {
            return Err(CodexStoreError::ExternalChange);
        }
        crate::files::remove(&self.path())?;
        Ok(())
    }
    fn write(
        &self,
        original: Option<&[u8]>,
        expected: &StoreGeneration,
        text: &str,
    ) -> CodexResult<()> {
        use std::io::Write;
        let path = self.path();
        crate::files::check_path(&path)?;
        let mut temp = tempfile::Builder::new()
            .prefix(".primerswitch-codex-config-")
            .tempfile_in(self.context.home())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = match original {
                Some(_) => std::fs::metadata(&path)?.permissions().mode() & 0o777,
                None => 0o600,
            };
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode))?;
        }
        temp.write_all(text.as_bytes())?;
        temp.as_file_mut().sync_all()?;
        let staged = temp.into_temp_path();
        // Compare immediately before replacement: Codex rewrites this file itself.
        if &self.read()?.1 != expected {
            return Err(CodexStoreError::ExternalChange);
        }
        crate::files::check_path(&path)?;
        let existed = original.is_some();
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let source: Vec<u16> = staged.as_os_str().encode_wide().chain(Some(0)).collect();
            let dest: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut attempts = 0;
            loop {
                let flags = if existed {
                    MOVEFILE_REPLACE_EXISTING
                } else {
                    0
                };
                let moved = unsafe {
                    MoveFileExW(
                        source.as_ptr(),
                        dest.as_ptr(),
                        flags | MOVEFILE_WRITE_THROUGH,
                    )
                } != 0;
                if moved {
                    break;
                }
                attempts += 1;
                if attempts >= 20 {
                    return Err(crate::PlatformError::Io.into());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
                if &self.read()?.1 != expected {
                    return Err(CodexStoreError::ExternalChange);
                }
            }
        }
        #[cfg(not(windows))]
        if existed {
            std::fs::rename(&staged, &path)?;
        } else {
            // Atomic no-replace creation: never overwrite a config created meanwhile.
            std::fs::hard_link(&staged, &path)?;
            std::fs::remove_file(&staged)?;
        }
        #[cfg(unix)]
        std::fs::File::open(self.context.home())?.sync_all()?;
        if slice(&self.read()?.0) != Some(text.as_bytes()) {
            return Err(CodexStoreError::ExternalChange);
        }
        Ok(())
    }
}

fn slice(bytes: &Option<Zeroizing<Vec<u8>>>) -> Option<&[u8]> {
    bytes.as_deref().map(Vec::as_slice)
}
fn text_of(bytes: Option<&[u8]>) -> CodexResult<&str> {
    match bytes {
        None => Ok(""),
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| CodexStoreError::UnsupportedContext),
    }
}
fn splice(text: &str, range: Range<usize>, with: &str) -> String {
    let mut result = String::with_capacity(text.len() + with.len());
    result.push_str(&text[..range.start]);
    result.push_str(with);
    result.push_str(&text[range.end..]);
    result
}
/// The raw text is exactly one single-line TOML string value; returns what it says.
fn string_token(raw: &str) -> CodexResult<String> {
    let line = format!("{KEY} = {raw}\n");
    match locate(&line)?.value {
        Some(value) if !raw.contains(['\n', '\r']) && line[value.span.clone()] == *raw => {
            Ok(value.decoded)
        }
        _ => Err(CodexStoreError::UnsupportedContext),
    }
}
/// The edited document still parses and its top-level `service_tier` says `want`.
fn verify(text: &str, want: Option<&str>) -> CodexResult<()> {
    let found = locate(text)?;
    if !found.other && found.value.as_ref().map(|value| value.decoded.as_str()) == want {
        Ok(())
    } else {
        Err(CodexStoreError::UnsupportedContext)
    }
}

struct Found {
    /// The top-level `service_tier` when it is a plain string.
    value: Option<Value>,
    /// The key exists but holds something else (a number, a table): not editable.
    other: bool,
    /// Where a new top-level key goes: the top of the file, before any table header.
    insert_at: usize,
}
struct Value {
    decoded: String,
    /// The value token in the file, quotes included.
    span: Range<usize>,
    /// The whole line of the key, line ending included.
    line: Range<usize>,
}
/// Parse `text` and find the top-level `service_tier`. A document that does not parse,
/// or whose `service_tier` is not a plain string, is refused: it cannot be edited safely.
fn locate(text: &str) -> CodexResult<Found> {
    let document =
        toml_edit::Document::parse(text).map_err(|_| CodexStoreError::UnsupportedContext)?;
    let insert_at = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let Some((key, item)) = document.as_table().get_key_value(KEY) else {
        return Ok(Found {
            value: None,
            other: false,
            insert_at,
        });
    };
    let Some(decoded) = item.as_value().and_then(|value| value.as_str()) else {
        return Ok(Found {
            value: None,
            other: true,
            insert_at,
        });
    };
    let decoded = decoded.to_owned();
    let value = item.as_value().ok_or(CodexStoreError::UnsupportedContext)?;
    let span = value.span().ok_or(CodexStoreError::UnsupportedContext)?;
    let key_span = key.span().ok_or(CodexStoreError::UnsupportedContext)?;
    // A byte order mark belongs to the first line but is not part of the key line.
    let start = text[..key_span.start]
        .rfind('\n')
        .map_or(insert_at, |i| i + 1);
    let end = text[span.end..]
        .find('\n')
        .map_or(text.len(), |i| span.end + i + 1);
    Ok(Found {
        value: Some(Value {
            decoded,
            span,
            line: start..end,
        }),
        other: false,
        insert_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::store::tests::evidence;
    use std::fs;

    fn fixture(config: Option<&[u8]>) -> (tempfile::TempDir, CodexConfigFlex, Vault) {
        let root = crate::files::test_root();
        let home = root.path().join("codex-home");
        fs::create_dir(&home).unwrap();
        if let Some(bytes) = config {
            fs::write(home.join("config.toml"), bytes).unwrap();
        }
        // The shared evidence helper writes a default config when there is none.
        let evidence = evidence(root.path(), &home);
        if config.is_none() {
            fs::remove_file(home.join("config.toml")).unwrap();
        }
        let context = CodexFileContext::qualify(home, evidence).unwrap();
        let vault = Vault::with_key(root.path().join("app-vault"), [47; 32]).unwrap();
        (root, CodexConfigFlex::new(context), vault)
    }

    #[test]
    fn spans_locate_the_value_the_line_and_the_insertion_point() {
        let text = "# top\nmodel = \"x\"\nservice_tier = \"default\" # keep\n[profiles.p]\nservice_tier = \"fast\"\n";
        let found = locate(text).unwrap();
        let value = found.value.unwrap();
        assert_eq!(&text[value.span], "\"default\"");
        assert_eq!(&text[value.line], "service_tier = \"default\" # keep\n");
        assert_eq!(found.insert_at, 0);
    }

    fn config(flex: &CodexConfigFlex) -> Vec<u8> {
        fs::read(flex.path()).unwrap()
    }
    fn leftovers(flex: &CodexConfigFlex) -> Vec<String> {
        fs::read_dir(flex.context.home())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".primerswitch"))
            .collect()
    }

    #[test]
    fn apply_and_restore_change_only_the_value_and_leave_every_other_byte_alone() {
        // (original, the file while Flex is on)
        let cases: &[(&[u8], &[u8])] = &[
            // The owner's own file: a top-level "default" and an opt-out flag.
            (
                b"model = \"x\"\nservice_tier = \"default\"\nfast_default_opt_out = true\n",
                b"model = \"x\"\nservice_tier = \"flex\"\nfast_default_opt_out = true\n",
            ),
            // CRLF, a trailing comment, spacing and tables after the key.
            (
                b"# c\r\nmodel = \"x\"\r\nservice_tier   =   \"default\"   # trailing\r\n[tui]\r\ntheme = \"y\"\r\n",
                b"# c\r\nmodel = \"x\"\r\nservice_tier   =   \"flex\"   # trailing\r\n[tui]\r\ntheme = \"y\"\r\n",
            ),
            // A literal string comes back as a literal string; no final newline.
            (b"service_tier = 'default'", b"service_tier = \"flex\""),
            // A table key is never the managed one; the new key goes above every table.
            (
                b"# only comments, then tables\n[projects.'C:\\\\x']\ntrust_level = \"trusted\"\n[profiles.p]\nservice_tier = \"fast\"\n",
                b"service_tier = \"flex\"\n# only comments, then tables\n[projects.'C:\\\\x']\ntrust_level = \"trusted\"\n[profiles.p]\nservice_tier = \"fast\"\n",
            ),
            // Both a top-level and a profile value: only the top-level one changes.
            (
                b"service_tier = \"default\"\n[profiles.p]\nservice_tier = \"fast\"\n",
                b"service_tier = \"flex\"\n[profiles.p]\nservice_tier = \"fast\"\n",
            ),
            // Absent key in a CRLF file and in a file without a final newline.
            (b"model = \"x\"\r\n", b"service_tier = \"flex\"\r\nmodel = \"x\"\r\n"),
            (b"model = \"x\"", b"service_tier = \"flex\"\nmodel = \"x\""),
            // An empty file that exists.
            (b"", b"service_tier = \"flex\"\n"),
            // A multi-line array before the key does not confuse the location.
            (
                b"notify = [\n  \"a\",\n  [\"b\"],\n]\nservice_tier = \"default\"\n",
                b"notify = [\n  \"a\",\n  [\"b\"],\n]\nservice_tier = \"flex\"\n",
            ),
        ];
        for (original, applied) in cases {
            let (_root, flex, vault) = fixture(Some(original));
            let mark = flex.apply(&vault).unwrap();
            assert!(mark.config_existed);
            assert_eq!(
                String::from_utf8_lossy(&config(&flex)),
                String::from_utf8_lossy(applied)
            );
            assert_eq!(flex.current().unwrap().as_deref(), Some("flex"));
            // The saved mark survives the app state file.
            let mark: FlexMark =
                serde_json::from_str(&serde_json::to_string(&mark).unwrap()).unwrap();
            assert!(flex.restore(&mark, &vault).unwrap());
            assert_eq!(
                String::from_utf8_lossy(&config(&flex)),
                String::from_utf8_lossy(original)
            );
            assert!(leftovers(&flex).is_empty());
            flex.context.revalidate().unwrap();
        }
    }

    #[test]
    fn a_missing_config_is_created_with_one_line_and_removed_again() {
        let (_root, flex, vault) = fixture(None);
        assert!(!flex.path().exists());
        assert_eq!(flex.current().unwrap(), None);
        let mark = flex.apply(&vault).unwrap();
        assert_eq!(
            mark,
            FlexMark {
                previous: None,
                config_existed: false
            }
        );
        assert_eq!(config(&flex), b"service_tier = \"flex\"\n");
        assert!(flex.restore(&mark, &vault).unwrap());
        assert!(!flex.path().exists());
        // Codex added its own settings meanwhile: only our line goes, the file stays.
        flex.apply(&vault).unwrap();
        fs::write(flex.path(), b"service_tier = \"flex\"\nmodel = \"y\"\n").unwrap();
        assert!(flex.restore(&mark, &vault).unwrap());
        assert_eq!(config(&flex), b"model = \"y\"\n");
        assert!(leftovers(&flex).is_empty());
    }

    #[test]
    fn a_value_the_owner_changed_meanwhile_is_never_overwritten() {
        let (_root, flex, vault) = fixture(Some(b"service_tier = \"default\"\nmodel = \"x\"\n"));
        let mark = flex.apply(&vault).unwrap();
        for changed in [
            &b"service_tier = \"priority\"\nmodel = \"x\"\n"[..],
            b"model = \"x\"\n",
            b"service_tier = 5\nmodel = \"x\"\n",
            b"model = \"x\"\n[profiles.p]\nservice_tier = \"flex\"\n",
        ] {
            fs::write(flex.path(), changed).unwrap();
            // Nothing to put back: the state can be cleared and the file is as the owner left it.
            assert!(flex.restore(&mark, &vault).unwrap());
            assert_eq!(config(&flex), changed);
        }
        // An unreadable file is an error, not a guess.
        fs::write(flex.path(), b"service_tier = \n").unwrap();
        assert!(flex.restore(&mark, &vault).is_err());
        assert_eq!(config(&flex), b"service_tier = \n");
    }

    #[test]
    fn an_owner_set_flex_is_kept_and_never_written_again() {
        let original = b"# mine\nservice_tier = \"flex\"\n";
        let (_root, flex, vault) = fixture(Some(original));
        let before = flex.read().unwrap().1;
        let mark = flex.apply(&vault).unwrap();
        // Nothing was written, not even a new timestamp.
        assert_eq!(flex.read().unwrap().1, before);
        assert_eq!(mark.previous.as_deref(), Some("\"flex\""));
        // Restoring leaves the owner's setting: the file still has Flex on.
        assert!(!flex.restore(&mark, &vault).unwrap());
        assert_eq!(config(&flex), original);
    }

    #[test]
    fn a_config_that_cannot_be_edited_safely_is_refused_untouched() {
        for refused in [
            &b"service_tier = 5\n"[..],
            b"service_tier.x = 1\n",
            b"service_tier = \n",
            b"[a\n",
            b"\xff\xfe",
        ] {
            let (_root, flex, vault) = fixture(Some(refused));
            assert!(flex.apply(&vault).is_err(), "{refused:?}");
            assert_eq!(config(&flex), refused);
            assert!(leftovers(&flex).is_empty());
        }
    }

    #[test]
    fn a_damaged_mark_never_writes_anything() {
        let original = b"service_tier = \"flex\"\nmodel = \"x\"\n";
        let (_root, flex, vault) = fixture(Some(original));
        let long = format!("\"{}\"", "a".repeat(300));
        for previous in [
            "nope",
            "5",
            "\"a\"\n[x]",
            "\"a\" # c",
            "\"a\"\r\nmodel = \"z\"",
            long.as_str(),
        ] {
            let mark = FlexMark {
                previous: Some(previous.into()),
                config_existed: true,
            };
            assert!(flex.restore(&mark, &vault).is_err(), "{previous}");
            assert_eq!(config(&flex), original);
        }
    }

    #[test]
    fn a_file_changed_after_it_was_read_is_never_overwritten() {
        let (_root, flex, vault) = fixture(Some(b"service_tier = \"default\"\n"));
        let _lock = vault.context_lock(flex.context.context_id()).unwrap();
        let (bytes, generation) = flex.read().unwrap();
        // Codex rewrites its config between the read and the replacement.
        fs::write(
            flex.path(),
            b"service_tier = \"default\"\nmodel = \"codex\"\n",
        )
        .unwrap();
        let error = flex
            .write(slice(&bytes), &generation, "service_tier = \"flex\"\n")
            .unwrap_err();
        assert!(matches!(error, CodexStoreError::ExternalChange));
        assert_eq!(
            config(&flex),
            b"service_tier = \"default\"\nmodel = \"codex\"\n"
        );
        assert!(leftovers(&flex).is_empty());
        // Created meanwhile while we expected no file: also refused.
        fs::remove_file(flex.path()).unwrap();
        let (bytes, generation) = flex.read().unwrap();
        assert!(bytes.is_none());
        fs::write(flex.path(), b"model = \"codex\"\n").unwrap();
        assert!(
            flex.write(None, &generation, "service_tier = \"flex\"\n")
                .is_err()
        );
        assert_eq!(config(&flex), b"model = \"codex\"\n");
        assert!(leftovers(&flex).is_empty());
    }

    #[test]
    fn nothing_but_config_toml_is_written_and_the_context_stays_qualified() {
        let (root, flex, vault) = fixture(Some(b"service_tier = \"default\"\n"));
        let home = flex.context.home().to_owned();
        for name in ["auth.json", "history.jsonl", "mcp.json"] {
            fs::write(home.join(name), b"unrelated-sentinel").unwrap();
        }
        let mark = flex.apply(&vault).unwrap();
        for name in ["auth.json", "history.jsonl", "mcp.json"] {
            assert_eq!(fs::read(home.join(name)).unwrap(), b"unrelated-sentinel");
        }
        // The home is the same directory and a new qualification accepts the new file.
        flex.context.revalidate().unwrap();
        CodexFileContext::qualify(home.clone(), evidence(root.path(), &home)).unwrap();
        assert!(flex.restore(&mark, &vault).unwrap());
        let mut names: Vec<_> = fs::read_dir(&home)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["auth.json", "config.toml", "history.jsonl", "mcp.json"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, flex, vault) = fixture(Some(b"service_tier = \"default\"\n"));
        fs::set_permissions(flex.path(), fs::Permissions::from_mode(0o640)).unwrap();
        let mark = flex.apply(&vault).unwrap();
        let mode = |flex: &CodexConfigFlex| {
            fs::metadata(flex.path()).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode(&flex), 0o640);
        flex.restore(&mark, &vault).unwrap();
        assert_eq!(mode(&flex), 0o640);
    }

    #[test]
    fn a_leading_byte_order_mark_stays_first() {
        let original = "\u{feff}model = \"x\"\n";
        let (_root, flex, vault) = fixture(Some(original.as_bytes()));
        match flex.apply(&vault) {
            Ok(mark) => {
                assert_eq!(
                    String::from_utf8(config(&flex)).unwrap(),
                    "\u{feff}service_tier = \"flex\"\nmodel = \"x\"\n"
                );
                assert!(flex.restore(&mark, &vault).unwrap());
                assert_eq!(config(&flex), original.as_bytes());
            }
            // A parser that does not accept a BOM refuses the file, untouched.
            Err(_) => assert_eq!(config(&flex), original.as_bytes()),
        }
    }
}
