//! Bounded, offline usage estimates from Codex's per-response rollout records.
//!
//! The caller supplies a qualified sessions directory and activation intervals.
//! No home discovery, credentials, provider calls or conversation text belong here.
//! One runtime owner must serialize ledger saves with account deletion.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const FORMAT: u32 = 1;
const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
const BUCKET_DAYS: i64 = 35;
const HASH_DAYS: i64 = 62;
const MAX_HASHES: usize = 100_000;
const MAX_FILES: usize = 100_000;
const MAX_LINE: usize = 256 * 1024;
const READ_CHUNK: usize = 32 * 1024;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DURATION: Duration = Duration::from_millis(300);
const MAX_LEDGER_BYTES: u64 = 32 * 1024 * 1024;
const NEEDLE: &[u8] = b"\"type\":\"token_usage_record\"";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationSpan {
    pub start: i64,
    pub end: i64,
}

pub type KeyActivationSpans = BTreeMap<String, Vec<ActivationSpan>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageCounters {
    pub input_uncached: u64,
    pub cached: u64,
    pub output: u64,
    pub total: u64,
    pub records: u64,
}

impl UsageCounters {
    pub fn saturating_add_assign(&mut self, other: Self) {
        self.input_uncached = self.input_uncached.saturating_add(other.input_uncached);
        self.cached = self.cached.saturating_add(other.cached);
        self.output = self.output.saturating_add(other.output);
        self.total = self.total.saturating_add(other.total);
        self.records = self.records.saturating_add(other.records);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyUsageLedger {
    pub tracking_since: i64,
    pub all_time: UsageCounters,
    /// UTC hour indices (`unix_seconds.div_euclid(3600)`); kept for 35 days.
    pub buckets: BTreeMap<i64, UsageCounters>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageWindows {
    pub last_24h: UsageCounters,
    pub last_7d: UsageCounters,
    pub last_30d: UsageCounters,
    pub all_time: UsageCounters,
    pub tracking_since: i64,
}

impl KeyUsageLedger {
    /// Windows include the hour overlapping the left boundary, so rounding can
    /// include up to one additional hour. All-time totals have no rounding.
    pub fn windows(&self, now: i64) -> UsageWindows {
        let mut windows = UsageWindows {
            all_time: self.all_time,
            tracking_since: self.tracking_since,
            ..Default::default()
        };
        for (&hour, &counters) in &self.buckets {
            let hour = hour.saturating_mul(HOUR);
            if hour > now {
                continue;
            }
            for (days, target) in [
                (1, &mut windows.last_24h),
                (7, &mut windows.last_7d),
                (30, &mut windows.last_30d),
            ] {
                if hour.saturating_add(HOUR) > now.saturating_sub(days * DAY) {
                    target.saturating_add_assign(counters);
                }
            }
        }
        windows
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct FileProgress {
    /// Last complete newline; a normal half line never advances this offset.
    offset: u64,
    len: u64,
    /// Oversized lines are discarded across budgets without retaining bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    discard_offset: Option<u64>,
}

/// Public counters plus private, content-free incremental scan state. Fields
/// unknown to this format are ignored and never copied back to disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageLedger {
    format: u32,
    #[serde(default)]
    pub keys: BTreeMap<String, KeyUsageLedger>,
    #[serde(default)]
    files: BTreeMap<String, FileProgress>,
    #[serde(default)]
    seen: BTreeMap<u64, i64>,
    #[serde(default)]
    scan_after: Option<String>,
    /// Binds relative offsets to a qualified home without persisting its path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_hash: Option<u64>,
    /// Live discovery resumes mid-directory; cold starts safely rediscover
    /// using persisted per-file offsets. Paths and handles never enter JSON.
    #[serde(skip)]
    walker: WalkContinuation,
}

impl Default for UsageLedger {
    fn default() -> Self {
        Self {
            format: FORMAT,
            keys: BTreeMap::new(),
            files: BTreeMap::new(),
            seen: BTreeMap::new(),
            scan_after: None,
            source_hash: None,
            walker: WalkContinuation::default(),
        }
    }
}

impl UsageLedger {
    pub fn remove_key(&mut self, id: &str) {
        self.keys.remove(id);
    }

    pub fn retain_keys(&mut self, ids: &BTreeSet<String>) {
        self.keys.retain(|id, _| ids.contains(id));
    }

    fn clear_file_progress(&mut self) {
        self.files.clear();
        self.scan_after = None;
        self.walker = WalkContinuation::default();
    }

    fn prune(&mut self, now: i64) {
        let oldest_hour = now.saturating_sub(BUCKET_DAYS * DAY).div_euclid(HOUR);
        for key in self.keys.values_mut() {
            key.buckets
                .retain(|hour, _| *hour >= oldest_hour && *hour <= now.div_euclid(HOUR));
        }
        self.seen
            .retain(|_, timestamp| *timestamp >= now.saturating_sub(HASH_DAYS * DAY));
        trim_hashes(&mut self.seen);
        self.files.retain(|path, progress| {
            valid_rollout_path(path)
                && progress.offset <= progress.len
                && progress
                    .discard_offset
                    .is_none_or(|offset| offset >= progress.offset && offset <= progress.len)
        });
        while self.files.len() > MAX_FILES {
            self.files.pop_first();
        }
        if self
            .scan_after
            .as_ref()
            .is_some_and(|p| !valid_rollout_path(p))
        {
            self.scan_after = None;
        }
    }
}

fn trim_hashes(hashes: &mut BTreeMap<u64, i64>) {
    if hashes.len() <= MAX_HASHES {
        return;
    }
    let mut by_age: Vec<_> = hashes.iter().map(|(&hash, &at)| (at, hash)).collect();
    by_age.sort_unstable();
    for (_, hash) in by_age.into_iter().take(hashes.len() - MAX_HASHES) {
        hashes.remove(&hash);
    }
}

/// Stable codes only: never includes a source path, native error, or JSON text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum UsageError {
    #[error("usageUnsafePath")]
    UnsafePath,
    #[error("usageReadFailed")]
    ReadFailed,
    #[error("usageSaveFailed")]
    SaveFailed,
}

#[derive(Clone)]
pub struct UsageStore {
    path: PathBuf,
}

impl std::fmt::Debug for UsageStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UsageStore")
    }
}

impl UsageStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Usage is an optional estimate: missing, unreadable, damaged and unknown
    /// formats all start empty and can never prevent credential switching.
    pub fn load(&self) -> UsageLedger {
        if fs::symlink_metadata(&self.path).map_or(true, |metadata| {
            !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_LEDGER_BYTES
        }) {
            return UsageLedger::default();
        }
        let Some(bytes) = crate::files::read_optional(&self.path).ok().flatten() else {
            return UsageLedger::default();
        };
        match serde_json::from_slice::<UsageLedger>(&bytes) {
            Ok(ledger) if ledger.format == FORMAT => ledger,
            _ => UsageLedger::default(),
        }
    }

    /// Ledger and offsets are replaced together using the platform's private,
    /// same-directory, atomic JSON write. Runtime serializes this with deletion.
    pub fn save(&self, ledger: &UsageLedger) -> Result<(), UsageError> {
        let bytes = encode_ledger(ledger, MAX_LEDGER_BYTES as usize)?;
        crate::files::atomic_write(&self.path, &bytes).map_err(|_| UsageError::SaveFailed)
    }
}

fn encode_ledger(ledger: &UsageLedger, max_bytes: usize) -> Result<Vec<u8>, UsageError> {
    struct BoundedWriter {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for BoundedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|len| len > self.limit)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "usageSaveFailed",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit: max_bytes,
    };
    serde_json::to_writer(&mut writer, ledger).map_err(|_| UsageError::SaveFailed)?;
    Ok(writer.bytes)
}

#[derive(Debug)]
pub struct UsageScanReport {
    pub ledger: UsageLedger,
    pub bytes_read: u64,
    pub records_added: u64,
    pub budget_exhausted: bool,
    /// Unique generic codes; per-file errors do not abandon other files.
    pub errors: Vec<UsageError>,
}

struct Budget {
    started: Instant,
    duration: Duration,
    bytes: u64,
    max_bytes: u64,
}

#[derive(Clone, Default)]
struct WalkContinuation(Arc<Mutex<Option<DiscoveryWalker>>>);

impl std::fmt::Debug for WalkContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WalkContinuation")
    }
}

struct WalkFrame {
    relative: PathBuf,
    depth: usize,
    entries: fs::ReadDir,
}

struct DiscoveryWalker {
    root: PathBuf,
    stack: Vec<WalkFrame>,
}

impl DiscoveryWalker {
    fn new(root: &Path) -> Result<Self, UsageError> {
        Ok(Self {
            root: root.to_owned(),
            stack: vec![WalkFrame {
                relative: PathBuf::new(),
                depth: 0,
                entries: fs::read_dir(root).map_err(|_| UsageError::ReadFailed)?,
            }],
        })
    }

    fn next_file(&mut self, budget: &Budget) -> Result<Option<String>, UsageError> {
        while !budget.exhausted() {
            let Some(frame) = self.stack.last_mut() else {
                return Ok(None);
            };
            let Some(entry) = frame.entries.next() else {
                self.stack.pop();
                continue;
            };
            let entry = entry.map_err(|_| UsageError::ReadFailed)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let next = frame.relative.join(name);
            let depth = frame.depth;
            if depth < 3 {
                if !valid_date_component(name, depth) {
                    continue;
                }
                let path = self.root.join(&next);
                crate::files::check_path(&path).map_err(|_| UsageError::UnsafePath)?;
                let metadata = fs::symlink_metadata(&path).map_err(|_| UsageError::ReadFailed)?;
                if is_link(&metadata) {
                    return Err(UsageError::UnsafePath);
                }
                if !metadata.is_dir() {
                    continue;
                }
                let entries = fs::read_dir(&path).map_err(|_| UsageError::ReadFailed)?;
                crate::files::check_path(&path).map_err(|_| UsageError::UnsafePath)?;
                self.stack.push(WalkFrame {
                    relative: next,
                    depth: depth + 1,
                    entries,
                });
            } else {
                let Some(relative) = next.to_str() else {
                    continue;
                };
                let relative = relative.replace('\\', "/");
                if valid_rollout_path(&relative) {
                    return Ok(Some(relative));
                }
            }
        }
        Ok(None)
    }
}

impl Budget {
    fn exhausted(&self) -> bool {
        self.bytes >= self.max_bytes || self.started.elapsed() >= self.duration
    }
}

/// Synchronous bounded work, intended for `spawn_blocking`. Time includes tree
/// traversal. As with any synchronous filesystem API, one blocked OS operation
/// can outlast the soft deadline; no additional operations start after it.
pub fn scan_usage(
    sessions: &Path,
    spans: &KeyActivationSpans,
    ledger: UsageLedger,
    now: i64,
) -> Result<UsageScanReport, UsageError> {
    scan_with_budget(sessions, spans, ledger, now, MAX_BYTES, MAX_DURATION)
}

/// Explicit fixture injection, including for tests in the runtime crate.
/// Ordinary fixtures use `Duration::MAX`; deadline tests can force `ZERO`.
/// The production entry point and byte cap are unchanged.
#[cfg(any(test, feature = "fixture-scanner"))]
pub fn scan_usage_for_fixture(
    sessions: &Path,
    spans: &KeyActivationSpans,
    ledger: UsageLedger,
    now: i64,
    duration: Duration,
) -> Result<UsageScanReport, UsageError> {
    scan_with_budget(sessions, spans, ledger, now, MAX_BYTES, duration)
}

fn scan_with_budget(
    sessions: &Path,
    spans: &KeyActivationSpans,
    mut ledger: UsageLedger,
    now: i64,
    max_bytes: u64,
    duration: Duration,
) -> Result<UsageScanReport, UsageError> {
    let mut budget = Budget {
        started: Instant::now(),
        duration,
        bytes: 0,
        max_bytes,
    };
    if ledger.format != FORMAT {
        ledger = UsageLedger::default();
    }
    ledger.prune(now);
    let earliest = spans
        .values()
        .flatten()
        .filter(|span| span.start <= span.end)
        .map(|span| span.start)
        .min();
    let mut report = UsageScanReport {
        ledger,
        bytes_read: 0,
        records_added: 0,
        budget_exhausted: false,
        errors: Vec::new(),
    };
    let Some(earliest) = earliest else {
        return Ok(report);
    };
    for (id, intervals) in spans {
        if let Some(start) = intervals
            .iter()
            .filter(|span| span.start <= span.end)
            .map(|span| span.start)
            .min()
        {
            let key = report
                .ledger
                .keys
                .entry(id.clone())
                .or_insert_with(|| KeyUsageLedger {
                    tracking_since: start,
                    ..Default::default()
                });
            key.tracking_since = key.tracking_since.min(start);
        }
    }
    if crate::files::check_path(sessions).is_err() {
        return Err(UsageError::UnsafePath);
    }
    let source_hash = stable_hash(sessions.as_os_str().as_encoded_bytes());
    if report.ledger.source_hash != Some(source_hash) {
        report.ledger.clear_file_progress();
        report.ledger.source_hash = Some(source_hash);
    }
    if !sessions.exists() {
        report.ledger.clear_file_progress();
        return Ok(report);
    }

    // Existing files go first, in a rotating order. A file larger than a byte
    // budget cannot starve another file and a late append is always revisited.
    let after = report.ledger.scan_after.clone();
    let known: Vec<_> = report.ledger.files.keys().cloned().collect();
    let pivot = after
        .as_ref()
        .map_or(0, |after| known.partition_point(|path| path <= after));
    // Reserve half the work for discovery so a growing known rollout cannot
    // prevent another session from ever entering the incremental file set.
    let reserved_bytes = budget.max_bytes;
    let reserved_duration = budget.duration;
    budget.max_bytes = (reserved_bytes / 2).max(1);
    budget.duration = reserved_duration / 2;
    for relative in known[pivot..].iter().chain(&known[..pivot]) {
        if budget.exhausted() {
            break;
        }
        visit_file(
            sessions,
            relative,
            spans,
            earliest,
            now,
            &mut report,
            &mut budget,
        );
    }
    budget.max_bytes = reserved_bytes;
    budget.duration = reserved_duration;
    // Discover only YYYY/MM/DD rollout files. No recursive walking of project
    // paths and no following symlinks, junctions or reparse points.
    if !budget.exhausted() {
        let continuation = report.ledger.walker.clone();
        let mut walker = continuation.0.lock().map_err(|_| UsageError::ReadFailed)?;
        if walker.as_ref().is_none_or(|walker| walker.root != sessions) {
            *walker = Some(DiscoveryWalker::new(sessions)?);
        }
        while !budget.exhausted() {
            match walker
                .as_mut()
                .expect("walker initialized")
                .next_file(&budget)
            {
                Ok(Some(relative)) => {
                    if !report.ledger.files.contains_key(&relative) {
                        visit_file(
                            sessions,
                            &relative,
                            spans,
                            earliest,
                            now,
                            &mut report,
                            &mut budget,
                        );
                    }
                }
                Ok(None) => {
                    if !budget.exhausted() {
                        *walker = None;
                    }
                    break;
                }
                Err(error) => add_error(&mut report.errors, error),
            }
        }
    }
    report.bytes_read = budget.bytes;
    report.budget_exhausted = budget.exhausted();
    report.ledger.prune(now);
    Ok(report)
}

fn add_error(errors: &mut Vec<UsageError>, code: UsageError) {
    if !errors.contains(&code) {
        errors.push(code);
    }
}

fn valid_date_component(name: &str, depth: usize) -> bool {
    if name.len() != if depth == 0 { 4 } else { 2 }
        || !name.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    let value = name.parse::<u16>().unwrap_or(0);
    match depth {
        0 => (1970..=9999).contains(&value),
        1 => (1..=12).contains(&value),
        2 => (1..=31).contains(&value),
        _ => false,
    }
}

fn valid_rollout_path(path: &str) -> bool {
    if path.contains('\\') || path.len() > 240 {
        return false;
    }
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 4
        && (0..3).all(|depth| valid_date_component(parts[depth], depth))
        && parts[3].starts_with("rollout-")
        && parts[3].ends_with(".jsonl")
        && parts[3]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

fn open_rollout(path: &Path) -> Result<File, UsageError> {
    crate::files::check_path(path).map_err(|_| UsageError::UnsafePath)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| UsageError::ReadFailed)?;
    let metadata = file.metadata().map_err(|_| UsageError::ReadFailed)?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(UsageError::UnsafePath);
    }
    crate::files::check_path(path).map_err(|_| UsageError::UnsafePath)?;
    Ok(file)
}

#[allow(clippy::too_many_arguments)] // Explicit borrowed scan inputs; no hidden global state.
fn visit_file(
    root: &Path,
    relative: &str,
    spans: &KeyActivationSpans,
    earliest: i64,
    now: i64,
    report: &mut UsageScanReport,
    budget: &mut Budget,
) {
    report.ledger.scan_after = Some(relative.to_owned());
    let result = read_file(root, relative, spans, earliest, now, report, budget);
    if let Err(code) = result {
        add_error(&mut report.errors, code);
    }
}

#[allow(clippy::too_many_arguments)]
fn read_file(
    root: &Path,
    relative: &str,
    spans: &KeyActivationSpans,
    earliest: i64,
    now: i64,
    report: &mut UsageScanReport,
    budget: &mut Budget,
) -> Result<(), UsageError> {
    if !valid_rollout_path(relative) {
        return Err(UsageError::UnsafePath);
    }
    let path = root.join(relative);
    crate::files::check_path(&path).map_err(|_| UsageError::UnsafePath)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.ledger.files.remove(relative);
            return Ok(());
        }
        Err(_) => return Err(UsageError::ReadFailed),
        Ok(_) => (),
    }
    if !report.ledger.files.contains_key(relative) && report.ledger.files.len() >= MAX_FILES {
        return Err(UsageError::ReadFailed);
    }
    let mut file = open_rollout(&path)?;
    let metadata = file.metadata().map_err(|_| UsageError::ReadFailed)?;
    let modified = metadata.modified().map_err(|_| UsageError::ReadFailed)?;
    let mtime = modified
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().min(i64::MAX as u64) as i64);
    if mtime < earliest.saturating_sub(120) {
        return Ok(());
    }
    let len = metadata.len();
    let mut progress = report
        .ledger
        .files
        .get(relative)
        .cloned()
        .unwrap_or_default();
    if len < progress.len || progress.offset > len {
        progress = FileProgress::default();
    }
    progress.len = len;
    let mut position = progress.discard_offset.unwrap_or(progress.offset);
    file.seek(SeekFrom::Start(position))
        .map_err(|_| UsageError::ReadFailed)?;
    // Allocate once: reallocating a growing vector could leave an old text
    // prefix in an unzeroized allocation.
    let mut line = Zeroizing::new(Vec::with_capacity(MAX_LINE));
    let mut chunk = Zeroizing::new(vec![0u8; READ_CHUNK]);
    let mut oversized = progress.discard_offset.is_some();
    while position < len && !budget.exhausted() {
        let limit =
            (len - position).min((budget.max_bytes - budget.bytes).min(READ_CHUNK as u64)) as usize;
        if limit == 0 {
            break;
        }
        let read = file
            .read(&mut chunk[..limit])
            .map_err(|_| UsageError::ReadFailed)?;
        if read == 0 {
            break;
        }
        budget.bytes += read as u64;
        for &byte in &chunk[..read] {
            position += 1;
            if byte == b'\n' {
                if !oversized {
                    consume_record(&line, spans, now, report);
                }
                use zeroize::Zeroize;
                line.as_mut_slice().zeroize();
                line.clear();
                oversized = false;
                progress.offset = position;
                progress.discard_offset = None;
                if budget.started.elapsed() >= budget.duration {
                    break;
                }
            } else if !oversized {
                if line.len() < MAX_LINE {
                    line.push(byte);
                } else {
                    // Erase the captured prefix immediately, even though it is
                    // not retained by the final, zeroizing allocation either.
                    use zeroize::Zeroize;
                    line.zeroize();
                    oversized = true;
                }
            }
        }
        if oversized {
            progress.discard_offset = Some(position);
        }
    }
    if report.ledger.files.len() < MAX_FILES || report.ledger.files.contains_key(relative) {
        report.ledger.files.insert(relative.to_owned(), progress);
    }
    Ok(())
}

#[derive(Deserialize)]
struct Record<'a> {
    #[serde(borrow)]
    timestamp: &'a str,
    #[serde(rename = "type", borrow)]
    kind: &'a str,
    #[serde(borrow)]
    payload: RecordPayload<'a>,
}

#[derive(Deserialize)]
struct RecordPayload<'a> {
    #[serde(borrow)]
    response_id: &'a str,
    usage: RawUsage,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

fn response_hash(response_id: &str) -> u64 {
    stable_hash(response_id.as_bytes())
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let digest = Sha256::digest(bytes);
    u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"))
}

fn consume_record(line: &[u8], spans: &KeyActivationSpans, now: i64, report: &mut UsageScanReport) {
    if !line.windows(NEEDLE.len()).any(|window| window == NEEDLE) {
        return;
    }
    let Ok(record) = serde_json::from_slice::<Record<'_>>(line) else {
        return;
    };
    if record.kind != "token_usage_record" || record.payload.response_id.is_empty() {
        return;
    }
    let Some(timestamp) = parse_timestamp(record.timestamp) else {
        return;
    };
    if timestamp > now {
        return;
    }
    let owner = spans
        .iter()
        .flat_map(|(id, spans)| {
            spans.iter().filter_map(move |span| {
                (span.start <= timestamp && timestamp <= span.end).then_some((span.start, id))
            })
        })
        .max();
    let Some((_, id)) = owner else { return };
    let hash = response_hash(record.payload.response_id);
    if report.ledger.seen.contains_key(&hash) {
        return;
    }
    // Insert only matched records. Skipped non-key periods cannot prevent later
    // appends, and no plaintext response identifier is retained in the ledger.
    report.ledger.seen.insert(hash, timestamp);
    if report.ledger.seen.len() > MAX_HASHES + 1024 {
        trim_hashes(&mut report.ledger.seen);
    }
    let usage = record.payload.usage;
    let counters = UsageCounters {
        input_uncached: usage.input_tokens.saturating_sub(usage.cached_input_tokens),
        cached: usage.cached_input_tokens,
        output: usage.output_tokens,
        total: usage.total_tokens,
        records: 1,
    };
    let key = report
        .ledger
        .keys
        .get_mut(id)
        .expect("span initialized ledger");
    key.all_time.saturating_add_assign(counters);
    let hour = timestamp.div_euclid(HOUR);
    if hour >= now.saturating_sub(BUCKET_DAYS * DAY).div_euclid(HOUR) {
        key.buckets
            .entry(hour)
            .or_default()
            .saturating_add_assign(counters);
    }
    report.records_added = report.records_added.saturating_add(1);
}

/// Strict RFC3339 to Unix seconds, without adding a date-time dependency.
fn parse_timestamp(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |start: usize, end: usize| -> Option<i64> {
        bytes
            .get(start..end)?
            .iter()
            .try_fold(0i64, |number, byte| {
                byte.is_ascii_digit()
                    .then_some(number * 10 + (byte - b'0') as i64)
            })
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    if !(1970..=9999).contains(&year)
        || !(1..=days_in_month).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut zone = 19;
    if bytes.get(zone) == Some(&b'.') {
        zone += 1;
        let start = zone;
        while bytes.get(zone).is_some_and(u8::is_ascii_digit) {
            zone += 1;
        }
        if zone == start {
            return None;
        }
    }
    let offset = if bytes.get(zone..) == Some(b"Z".as_slice()) {
        0
    } else {
        if bytes.len() != zone + 6 || bytes[zone + 3] != b':' {
            return None;
        }
        let sign = match bytes[zone] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let hours = number(zone + 1, zone + 3)?;
        let minutes = number(zone + 4, zone + 6)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        sign * (hours * HOUR + minutes * 60)
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146097 + day_of_era - 719468;
    Some(days * DAY + hour * HOUR + minute * 60 + second - offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NOW_TEXT: &str = "2026-11-04T12:01:00Z";
    const SECRET: &str = "SYNTHETIC_SECRET_DO_NOT_PERSIST";

    struct Fixture {
        _root: tempfile::TempDir,
        sessions: PathBuf,
        store: UsageStore,
    }

    impl Fixture {
        fn new() -> Self {
            let root = crate::files::test_root();
            let sessions = root.path().join("home/sessions");
            fs::create_dir_all(&sessions).unwrap();
            let store = UsageStore::new(root.path().join("data/codex-key-usage.json"));
            Self {
                _root: root,
                sessions,
                store,
            }
        }

        fn write(&self, name: &str, data: impl AsRef<[u8]>) -> PathBuf {
            let path = self
                .sessions
                .join("2026/11/04")
                .join(format!("rollout-{name}.jsonl"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, data).unwrap();
            OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(UNIX_EPOCH + Duration::from_secs(now() as u64)),
                )
                .unwrap();
            path
        }

        fn scan(&self, ledger: UsageLedger) -> UsageScanReport {
            scan_with_budget(
                &self.sessions,
                &spans(),
                ledger,
                now(),
                MAX_BYTES,
                Duration::MAX,
            )
            .unwrap()
        }
    }

    fn now() -> i64 {
        parse_timestamp(NOW_TEXT).unwrap()
    }

    fn spans() -> KeyActivationSpans {
        BTreeMap::from([(
            "key-a".to_owned(),
            vec![ActivationSpan {
                start: now() - 100 * DAY,
                end: now(),
            }],
        )])
    }

    fn record(id: &str, timestamp: &str) -> String {
        // Reasoning is already included in output; cumulative values and text
        // deliberately differ by orders of magnitude from the response usage.
        serde_json::json!({
            "timestamp": timestamp,
            "type": "token_usage_record",
            "payload": {
                "response_id": id,
                "usage": {"input_tokens":10,"cached_input_tokens":3,"output_tokens":4,
                    "reasoning_output_tokens":999,"total_tokens":14},
                "turn_token_usage": {"input_tokens":900000,"output_tokens":900000,"total_tokens":1800000},
                "thread_token_usage": {"input_tokens":9000000,"total_tokens":9000000},
                "text": SECRET, "cwd": format!("C:/projects/{SECRET}")
            }
        }).to_string() + "\n"
    }

    fn counters(records: u64) -> UsageCounters {
        UsageCounters {
            input_uncached: 7 * records,
            cached: 3 * records,
            output: 4 * records,
            total: 14 * records,
            records,
        }
    }

    fn append(path: &Path, bytes: impl AsRef<[u8]>) {
        OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(bytes.as_ref())
            .unwrap();
        OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(now() as u64)),
            )
            .unwrap();
    }

    #[test]
    fn exact_response_totals_windows_and_all_time_ignore_cumulative_and_reasoning() {
        let fixture = Fixture::new();
        let dates = [
            "2026-11-04T12:00:00Z",
            "2026-11-03T11:00:00Z",
            "2026-10-31T12:00:00Z",
            "2026-10-27T11:00:00Z",
            "2026-10-10T12:00:00Z",
            "2026-10-04T11:00:00Z",
            "2026-09-01T12:00:00Z",
        ];
        let data: String = dates
            .iter()
            .enumerate()
            .map(|(i, at)| record(&format!("resp-{i}"), at))
            .collect();
        fixture.write("one", data);
        let report = fixture.scan(UsageLedger::default());
        assert_eq!(report.records_added, 7);
        let key = &report.ledger.keys["key-a"];
        assert_eq!(key.all_time, counters(7));
        let windows = key.windows(now());
        assert_eq!(windows.last_24h, counters(1));
        assert_eq!(windows.last_7d, counters(3));
        assert_eq!(windows.last_30d, counters(5));
        assert_eq!(windows.all_time, counters(7));
        assert_eq!(key.buckets.len(), 6);
        assert_eq!(windows.tracking_since, now() - 100 * DAY);
    }

    #[test]
    fn attribution_is_inclusive_and_latest_start_wins_overlapping_keys() {
        let fixture = Fixture::new();
        let start = parse_timestamp("2026-11-04T12:00:00Z").unwrap();
        let spans = BTreeMap::from([
            (
                "a".into(),
                vec![ActivationSpan {
                    start,
                    end: start + 20,
                }],
            ),
            (
                "b".into(),
                vec![ActivationSpan {
                    start: start + 10,
                    end: start + 30,
                }],
            ),
        ]);
        let data: String = [0, 10, 20, 30, 31]
            .iter()
            .map(|second| {
                record(
                    &format!("resp-{second}"),
                    &format!("2026-11-04T12:00:{second:02}Z"),
                )
            })
            .collect();
        fixture.write("one", data);
        let report = scan_with_budget(
            &fixture.sessions,
            &spans,
            UsageLedger::default(),
            now(),
            MAX_BYTES,
            Duration::MAX,
        )
        .unwrap();
        assert_eq!(report.ledger.keys["a"].all_time, counters(1));
        assert_eq!(report.ledger.keys["b"].all_time, counters(3));
        assert_eq!(report.records_added, 4);
    }

    #[test]
    fn dedupe_survives_copies_persisted_restarts_and_late_duplicate_appends() {
        let fixture = Fixture::new();
        let data = record("resp-shared", NOW_TEXT);
        let first = fixture.write("one", &data);
        fixture.write("two", &data);
        let report = fixture.scan(UsageLedger::default());
        assert_eq!(report.records_added, 1);
        fixture.store.save(&report.ledger).unwrap();
        append(&first, &data);
        append(&first, record("resp-new", NOW_TEXT));
        let report = fixture.scan(fixture.store.load());
        assert_eq!(report.records_added, 1);
        assert_eq!(report.ledger.keys["key-a"].all_time, counters(2));
        let second = fixture.scan(report.ledger);
        assert_eq!(second.records_added, 0);
        assert_eq!(second.bytes_read, 0);
    }

    #[test]
    fn append_half_line_and_truncation_preserve_complete_line_offsets() {
        let fixture = Fixture::new();
        let path = fixture.write("one", record("resp-first", NOW_TEXT));
        let first = fixture.scan(UsageLedger::default());
        let second_line = record("resp-second", NOW_TEXT);
        let split = second_line.len() / 2;
        append(&path, &second_line[..split]);
        let second = fixture.scan(first.ledger);
        assert_eq!(second.records_added, 0);
        assert_eq!(
            second.ledger.files.values().next().unwrap().offset,
            record("resp-first", NOW_TEXT).len() as u64
        );
        append(&path, &second_line[split..]);
        let third = fixture.scan(second.ledger);
        assert_eq!(third.records_added, 1);
        fixture.write("one", record("resp-third", NOW_TEXT));
        let fourth = fixture.scan(third.ledger);
        assert_eq!(fourth.records_added, 1);
        assert_eq!(fourth.ledger.keys["key-a"].all_time, counters(3));
    }

    #[test]
    fn records_outside_spans_advance_offsets_and_late_appends_are_counted() {
        let fixture = Fixture::new();
        let interval = BTreeMap::from([(
            "a".into(),
            vec![ActivationSpan {
                start: now() - 10,
                end: now(),
            }],
        )]);
        let path = fixture.write("one", record("resp-earlier", "2026-11-04T12:00:00Z"));
        let first = scan_with_budget(
            &fixture.sessions,
            &interval,
            UsageLedger::default(),
            now(),
            MAX_BYTES,
            Duration::MAX,
        )
        .unwrap();
        assert_eq!(first.records_added, 0);
        assert_eq!(
            first.ledger.files.values().next().unwrap().offset,
            fs::metadata(&path).unwrap().len()
        );
        append(&path, record("resp-late", NOW_TEXT));
        let second = scan_with_budget(
            &fixture.sessions,
            &interval,
            first.ledger,
            now(),
            MAX_BYTES,
            Duration::MAX,
        )
        .unwrap();
        assert_eq!(second.ledger.keys["a"].all_time, counters(1));
    }

    #[test]
    fn no_spans_does_not_even_validate_or_walk_the_source() {
        let fixture = Fixture::new();
        let path = fixture.sessions.join("not/a/qualified/../source");
        let result = scan_usage(
            &path,
            &KeyActivationSpans::new(),
            UsageLedger::default(),
            now(),
        )
        .unwrap();
        assert_eq!(result.bytes_read, 0);
        assert!(result.errors.is_empty());
        assert!(result.ledger.files.is_empty());
    }

    #[test]
    fn missing_corrupt_unreadable_and_unsupported_stores_start_empty() {
        let fixture = Fixture::new();
        assert!(fixture.store.load().keys.is_empty());
        fs::create_dir_all(fixture.store.path.parent().unwrap()).unwrap();
        fs::write(&fixture.store.path, format!("damaged {SECRET}")).unwrap();
        assert!(fixture.store.load().keys.is_empty());
        fs::write(&fixture.store.path, r#"{"format":99,"keys":{"a":{"tracking_since":1,"all_time":{"total":999},"buckets":{}}}}"#).unwrap();
        assert!(fixture.store.load().keys.is_empty());
        let directory = UsageStore::new(fixture.sessions.clone());
        assert!(directory.load().keys.is_empty());
        assert_eq!(
            directory.save(&UsageLedger::default()).unwrap_err(),
            UsageError::SaveFailed
        );
        assert!(
            !directory
                .save(&UsageLedger::default())
                .unwrap_err()
                .to_string()
                .contains(SECRET)
        );
    }

    #[test]
    fn old_bucket_pruning_preserves_all_time_and_old_format_defaults_scan_state() {
        let fixture = Fixture::new();
        let mut ledger = UsageLedger::default();
        let mut key = KeyUsageLedger {
            tracking_since: 123,
            all_time: counters(99),
            ..Default::default()
        };
        key.buckets
            .insert((now() - 36 * DAY).div_euclid(HOUR), counters(40));
        key.buckets.insert(now().div_euclid(HOUR), counters(2));
        key.buckets
            .insert((now() + DAY).div_euclid(HOUR), counters(9));
        ledger.keys.insert("key-a".into(), key);
        ledger.seen.insert(1, now() - 63 * DAY);
        ledger.seen.insert(2, now() - 61 * DAY);
        fixture.store.save(&ledger).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.store.path).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("files");
        value.as_object_mut().unwrap().remove("scan_after");
        fs::write(&fixture.store.path, value.to_string()).unwrap();
        let report = fixture.scan(fixture.store.load());
        assert_eq!(report.ledger.keys["key-a"].all_time, counters(99));
        assert_eq!(report.ledger.keys["key-a"].buckets.len(), 1);
        assert_eq!(report.ledger.keys["key-a"].tracking_since, 123);
        assert_eq!(report.ledger.seen, BTreeMap::from([(2, now() - 61 * DAY)]));
    }

    #[test]
    fn retained_hashes_are_bounded_by_age_then_stable_hash() {
        let mut hashes: BTreeMap<_, _> = (0..MAX_HASHES as u64 + 20)
            .map(|id| (id, id as i64))
            .collect();
        trim_hashes(&mut hashes);
        assert_eq!(hashes.len(), MAX_HASHES);
        assert!(!hashes.contains_key(&19));
        assert!(hashes.contains_key(&20));
        let mut ledger = UsageLedger {
            seen: hashes,
            ..Default::default()
        };
        ledger.prune(100 * DAY);
        assert!(ledger.seen.is_empty());
    }

    #[test]
    fn budgets_continue_and_large_known_rollouts_cannot_starve_discovery() {
        let fixture = Fixture::new();
        for file in 0..4 {
            let data: String = (0..4)
                .map(|line| record(&format!("resp-{file}-{line}"), NOW_TEXT))
                .collect();
            fixture.write(&format!("{file}"), data);
        }
        let mut ledger = UsageLedger::default();
        let budget = 2048;
        let mut exhausted = false;
        let mut reports = 0;
        for _ in 0..80 {
            let report = scan_with_budget(
                &fixture.sessions,
                &spans(),
                ledger,
                now(),
                budget,
                Duration::MAX,
            )
            .unwrap();
            assert!(report.bytes_read <= budget);
            exhausted |= report.budget_exhausted;
            reports += 1;
            let done = report.ledger.keys["key-a"].all_time.records == 16;
            ledger = report.ledger;
            if done {
                break;
            }
        }
        assert!(reports > 1);
        assert!(exhausted);
        assert_eq!(ledger.keys["key-a"].all_time, counters(16));
        assert_eq!(ledger.files.len(), 4);
    }

    #[test]
    fn zero_time_budget_starts_no_rollout_read() {
        let fixture = Fixture::new();
        fixture.write("one", record("resp-a", NOW_TEXT));
        let report = scan_with_budget(
            &fixture.sessions,
            &spans(),
            UsageLedger::default(),
            now(),
            MAX_BYTES,
            Duration::ZERO,
        )
        .unwrap();
        assert!(report.budget_exhausted);
        assert_eq!(report.bytes_read, 0);
        assert_eq!(report.records_added, 0);
    }

    #[test]
    fn production_budget_caps_many_large_rollouts_and_eventually_resumes_each() {
        let fixture = Fixture::new();
        const FILES: u64 = 6;
        const SPARSE_BYTES: u64 = 12 * 1024 * 1024;
        for index in 0..FILES {
            let path = fixture.write(&format!("large-{index}"), []);
            let mut file = OpenOptions::new().write(true).open(&path).unwrap();
            // A large zero-filled hole exercises streaming without allocating
            // an equally large fixture buffer or retaining conversation text.
            file.set_len(SPARSE_BYTES).unwrap();
            file.seek(SeekFrom::End(0)).unwrap();
            file.write_all(b"\n").unwrap();
            file.write_all(record(&format!("resp-large-{index}"), NOW_TEXT).as_bytes())
                .unwrap();
        }
        let first = scan_usage(&fixture.sessions, &spans(), UsageLedger::default(), now()).unwrap();
        assert!(first.budget_exhausted);
        assert!(first.bytes_read <= MAX_BYTES);
        assert!(first.records_added < FILES);
        let mut ledger = first.ledger;
        let mut bytes_read = first.bytes_read;
        for _ in 0..200 {
            let report = scan_usage(&fixture.sessions, &spans(), ledger, now()).unwrap();
            assert!(report.bytes_read <= MAX_BYTES);
            bytes_read += report.bytes_read;
            let done = report.ledger.keys["key-a"].all_time.records == FILES;
            ledger = report.ledger;
            if done {
                break;
            }
        }
        assert!(bytes_read >= FILES * SPARSE_BYTES);
        assert_eq!(ledger.files.len(), FILES as usize);
        assert_eq!(ledger.keys["key-a"].all_time, counters(FILES));
        let second = fixture.scan(ledger);
        assert_eq!(second.bytes_read, 0);
        assert_eq!(second.records_added, 0);
    }

    #[test]
    fn oversized_lines_are_streamed_discarded_and_resume_without_retaining_prefixes() {
        let fixture = Fixture::new();
        let path = fixture.write("one", []);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        for _ in 0..96 {
            file.write_all(&vec![b'x'; READ_CHUNK]).unwrap()
        }
        file.write_all(b"\n").unwrap();
        file.write_all(record("resp-after-huge", NOW_TEXT).as_bytes())
            .unwrap();
        drop(file);
        let mut ledger = UsageLedger::default();
        for _ in 0..30 {
            let report = scan_with_budget(
                &fixture.sessions,
                &spans(),
                ledger,
                now(),
                (MAX_LINE * 2) as u64,
                Duration::MAX,
            )
            .unwrap();
            let done = report.records_added == 1;
            ledger = report.ledger;
            if done {
                break;
            }
        }
        assert_eq!(ledger.keys["key-a"].all_time, counters(1));
        assert!(
            ledger
                .files
                .values()
                .all(|progress| progress.discard_offset.is_none())
        );
    }

    #[test]
    fn ignored_content_response_ids_paths_and_parser_errors_never_escape() {
        let fixture = Fixture::new();
        let mut data = record(SECRET, NOW_TEXT);
        data.push_str(&format!(
            "{{\"type\":\"token_usage_record\",\"password\":\"{SECRET}\"\n"
        ));
        data.push_str(&format!(
            "{{\"type\":\"response_item\",\"payload\":\"{SECRET}\"}}\n"
        ));
        fixture.write("one", data);
        let report = fixture.scan(UsageLedger::default());
        fixture.store.save(&report.ledger).unwrap();
        let bytes = fs::read(&fixture.store.path).unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains(SECRET));
        assert!(!format!("{report:?}").contains(SECRET));
        assert!(!format!("{:?}", fixture.store).contains(SECRET));
        assert_eq!(report.records_added, 1);
        assert!(report.errors.is_empty());
        assert!(
            !serde_json::to_string(&report.ledger)
                .unwrap()
                .contains("C:/projects")
        );
    }

    #[test]
    fn mtime_filter_and_date_hierarchy_exclude_unrelated_files() {
        let fixture = Fixture::new();
        let path = fixture.write("old", record("resp-old", NOW_TEXT));
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(
                    UNIX_EPOCH + Duration::from_secs((now() - 100 * DAY - 121) as u64),
                ),
            )
            .unwrap();
        let other = fixture.sessions.join("project");
        fs::create_dir_all(&other).unwrap();
        fs::write(
            other.join("rollout-unrelated.jsonl"),
            record("resp-other", NOW_TEXT),
        )
        .unwrap();
        fs::write(
            path.parent().unwrap().join("other.jsonl"),
            record("resp-other-2", NOW_TEXT),
        )
        .unwrap();
        let report = fixture.scan(UsageLedger::default());
        assert_eq!(report.bytes_read, 0);
        assert!(report.ledger.files.is_empty());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn unsafe_persisted_paths_are_discarded_and_key_deletion_keeps_other_totals() {
        let fixture = Fixture::new();
        let mut ledger = UsageLedger::default();
        ledger
            .files
            .insert("../project/secret.jsonl".into(), FileProgress::default());
        ledger.files.insert(
            "2026/11/04/rollout-a.jsonl".into(),
            FileProgress {
                offset: 2,
                len: 1,
                discard_offset: None,
            },
        );
        ledger.scan_after = Some("../secret".into());
        ledger.keys.insert(
            "delete".into(),
            KeyUsageLedger {
                all_time: counters(6),
                ..Default::default()
            },
        );
        ledger.keys.insert(
            "retain".into(),
            KeyUsageLedger {
                all_time: counters(7),
                ..Default::default()
            },
        );
        ledger.remove_key("delete");
        ledger.retain_keys(&BTreeSet::from(["retain".into()]));
        let report =
            scan_usage(&fixture.sessions, &KeyActivationSpans::new(), ledger, now()).unwrap();
        assert_eq!(report.ledger.keys.len(), 1);
        assert_eq!(report.ledger.keys["retain"].all_time, counters(7));
        assert!(report.ledger.files.is_empty());
        assert!(report.ledger.scan_after.is_none());
    }

    #[test]
    fn malformed_negative_missing_cumulative_and_future_records_do_not_count() {
        let fixture = Fixture::new();
        let valid: serde_json::Value = serde_json::from_str(&record("resp-a", NOW_TEXT)).unwrap();
        let mut cases = Vec::new();
        let mut negative = valid.clone();
        negative["payload"]["usage"]["input_tokens"] = (-1).into();
        cases.push(negative);
        let mut missing = valid.clone();
        missing["payload"].as_object_mut().unwrap().remove("usage");
        cases.push(missing);
        let mut wrong = valid.clone();
        wrong["type"] = "event_msg".into();
        cases.push(wrong);
        let mut future = valid.clone();
        future["timestamp"] = "2026-11-05T12:01:00Z".into();
        cases.push(future);
        let mut bad_date = valid.clone();
        bad_date["timestamp"] = "2026-02-30T12:01:00Z".into();
        cases.push(bad_date);
        let mut blank = valid;
        blank["payload"]["response_id"] = "".into();
        cases.push(blank);
        let data: String = cases
            .into_iter()
            .map(|value| value.to_string() + "\n")
            .collect();
        fixture.write("one", data);
        let report = fixture.scan(UsageLedger::default());
        assert_eq!(report.records_added, 0);
        assert!(report.errors.is_empty());
        assert_eq!(
            report.ledger.files.values().next().unwrap().offset,
            report.bytes_read
        );
    }

    #[test]
    fn cached_input_and_all_counters_use_saturating_arithmetic() {
        let fixture = Fixture::new();
        let mut value: serde_json::Value =
            serde_json::from_str(&record("resp-max", NOW_TEXT)).unwrap();
        value["payload"]["usage"]["cached_input_tokens"] = 99.into();
        fixture.write("one", value.to_string() + "\n");
        let report = fixture.scan(UsageLedger::default());
        assert_eq!(report.ledger.keys["key-a"].all_time.input_uncached, 0);
        assert_eq!(report.ledger.keys["key-a"].all_time.cached, 99);
        let mut max = UsageCounters {
            input_uncached: u64::MAX,
            cached: u64::MAX,
            output: u64::MAX,
            total: u64::MAX,
            records: u64::MAX,
        };
        let original = max;
        max.saturating_add_assign(counters(1));
        assert_eq!(max, original);
    }

    #[test]
    fn timestamps_support_fractional_z_and_explicit_offsets_and_reject_bad_dates() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2000-02-29T00:00:00Z"), Some(951782400));
        assert_eq!(parse_timestamp("2026-11-04T12:01:00.855Z"), Some(now()));
        assert_eq!(parse_timestamp("2026-11-04T14:01:00+02:00"), Some(now()));
        assert_eq!(parse_timestamp("2026-11-04T10:01:00-02:00"), Some(now()));
        for invalid in [
            "2026-02-29T00:00:00Z",
            "2026-11-04T12:01:00.Z",
            "2026-11-04T12:01:00+25:00",
            "2026-11-04T12:01:60Z",
            "2026-11-04T12:01:00Zjunk",
        ] {
            assert_eq!(parse_timestamp(invalid), None);
        }
    }

    #[test]
    fn vanished_files_and_missing_sessions_drop_only_scan_progress() {
        let fixture = Fixture::new();
        let path = fixture.write("gone", record("resp-gone", NOW_TEXT));
        let first = fixture.scan(UsageLedger::default());
        fs::remove_file(path).unwrap();
        let second = fixture.scan(first.ledger);
        assert!(second.ledger.files.is_empty());
        assert!(second.errors.is_empty());
        assert_eq!(second.ledger.keys["key-a"].all_time, counters(1));
        fixture.write("second", record("resp-second", NOW_TEXT));
        let third = fixture.scan(second.ledger);
        assert_eq!(third.ledger.files.len(), 1);
        let fourth = scan_usage(
            &fixture.sessions.join("missing"),
            &spans(),
            third.ledger,
            now(),
        )
        .unwrap();
        assert!(fourth.ledger.files.is_empty());
        assert!(fourth.errors.is_empty());
        assert_eq!(fourth.ledger.keys["key-a"].all_time, counters(2));
    }

    #[test]
    fn changing_qualified_home_resets_offsets_and_preserves_totals_and_dedupe() {
        let first_home = Fixture::new();
        let second_home = Fixture::new();
        first_home.write("same", record("resp-one", NOW_TEXT));
        second_home.write("same", record("resp-two", NOW_TEXT));
        let first = first_home.scan(UsageLedger::default());
        assert_eq!(first.ledger.keys["key-a"].all_time, counters(1));
        let first_hash = first.ledger.source_hash;
        let second = second_home.scan(first.ledger);
        assert_eq!(second.records_added, 1);
        assert_eq!(second.ledger.keys["key-a"].all_time, counters(2));
        assert_eq!(second.ledger.seen.len(), 2);
        assert_ne!(second.ledger.source_hash, first_hash);
        let bytes = serde_json::to_string(&second.ledger).unwrap();
        assert!(!bytes.contains(first_home.sessions.to_str().unwrap()));
        assert!(!bytes.contains(second_home.sessions.to_str().unwrap()));
        let path = second_home.write("copy", record("resp-one", NOW_TEXT));
        assert!(path.exists());
        let third = second_home.scan(second.ledger);
        assert_eq!(third.records_added, 0);
        assert_eq!(third.ledger.keys["key-a"].all_time, counters(2));
    }

    #[test]
    fn oversized_optional_store_is_rejected_before_json_allocation() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.store.path.parent().unwrap()).unwrap();
        File::create(&fixture.store.path)
            .unwrap()
            .set_len(MAX_LEDGER_BYTES + 1)
            .unwrap();
        assert!(fixture.store.load().keys.is_empty());
    }

    #[test]
    fn save_serialization_stops_at_bound_and_never_emits_an_unloadable_file() {
        let ledger = UsageLedger::default();
        let bytes = serde_json::to_vec(&ledger).unwrap();
        assert_eq!(encode_ledger(&ledger, bytes.len()).unwrap(), bytes);
        assert_eq!(
            encode_ledger(&ledger, bytes.len() - 1),
            Err(UsageError::SaveFailed)
        );
        assert_eq!(encode_ledger(&ledger, 0), Err(UsageError::SaveFailed));
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_rollout_returns_only_generic_code_and_other_files_continue() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let path = fixture.write("held", record(SECRET, NOW_TEXT));
        fixture.write("other", record("resp-other", NOW_TEXT));
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let report = fixture.scan(UsageLedger::default());
        assert_eq!(report.records_added, 1);
        assert_eq!(report.errors, vec![UsageError::ReadFailed]);
        assert!(!format!("{report:?}").contains(SECRET));
        drop(held);
        let second = fixture.scan(report.ledger);
        assert_eq!(second.records_added, 1);
        assert_eq!(second.ledger.keys["key-a"].all_time, counters(2));
    }

    #[cfg(windows)]
    #[test]
    fn failed_atomic_save_preserves_both_prior_counters_and_offsets() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let path = fixture.write("one", record("resp-first", NOW_TEXT));
        let first = fixture.scan(UsageLedger::default());
        fixture.store.save(&first.ledger).unwrap();
        let before = fs::read(&fixture.store.path).unwrap();
        append(&path, record("resp-second", NOW_TEXT));
        let second = fixture.scan(first.ledger);
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&fixture.store.path)
            .unwrap();
        assert_eq!(
            fixture.store.save(&second.ledger),
            Err(UsageError::SaveFailed)
        );
        assert_eq!(fs::read(&fixture.store.path).unwrap(), before);
        drop(held);
        let replay = fixture.scan(fixture.store.load());
        assert_eq!(replay.records_added, 1);
        assert_eq!(replay.ledger.keys["key-a"].all_time, counters(2));
    }

    #[test]
    fn symlinks_and_windows_junctions_cannot_read_outside_sessions() {
        let fixture = Fixture::new();
        let outside = fixture._root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            outside.join("rollout-secret.jsonl"),
            record(SECRET, NOW_TEXT),
        )
        .unwrap();
        let link = fixture.sessions.join("2026/11/04");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            // Junctions require no developer-mode/symlink privilege. All paths
            // are synthetic fixture names, and command output stays private.
            let output = std::process::Command::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(output.status.success(), "fixture junction creation failed");
        }
        #[cfg(any(unix, windows))]
        {
            let report = fixture.scan(UsageLedger::default());
            assert_eq!(report.records_added, 0);
            assert_eq!(report.bytes_read, 0);
            assert!(report.errors.contains(&UsageError::UnsafePath));
            assert!(!format!("{report:?}").contains(SECRET));
            assert!(
                fs::read(outside.join("rollout-secret.jsonl"))
                    .unwrap()
                    .starts_with(b"{")
            );
        }
    }
}
