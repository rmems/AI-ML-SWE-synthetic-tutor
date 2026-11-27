//! The append-only JSONL log file and its hash-chain verification.
//!
//! Appending never rewrites earlier lines: each new [`RunLogEntry`] records
//! the SHA-256 of the previous serialized line in `prev_entry_sha256`, so any
//! later edit, reorder, or removal of history is detected by
//! [`RunLog::entries`]. Appending itself refuses to extend a log whose
//! existing chain does not verify — a corrupted log cannot be silently
//! built on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::entry::{RUN_LOG_SCHEMA_VERSION, Record, RunLogEntry};

/// Why a log read, verify, or append failed.
#[derive(Debug, Error)]
pub enum RunLogError {
    /// Filesystem failure reading or appending the log.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A log line did not serialize/deserialize as a [`RunLogEntry`].
    #[error("line {line}: not a run-log entry: {source}")]
    Parse {
        /// 1-based line number.
        line: usize,
        /// The serde failure.
        source: serde_json::Error,
    },
    /// An entry carries a `log_version` this build does not know.
    #[error("line {line}: unsupported log_version {found} (expected {RUN_LOG_SCHEMA_VERSION})")]
    UnsupportedVersion {
        /// 1-based line number.
        line: usize,
        /// The version found.
        found: u32,
    },
    /// The first entry must have `prev_entry_sha256: null`.
    #[error("line {line}: the first entry must not link to a previous entry")]
    FirstEntryLinks {
        /// 1-based line number.
        line: usize,
    },
    /// An entry after the first is missing its chain link.
    #[error("line {line}: entry has no prev_entry_sha256")]
    MissingLink {
        /// 1-based line number.
        line: usize,
    },
    /// `prev_entry_sha256` does not match the previous line's bytes — the
    /// history was edited, reordered, or a middle entry was deleted.
    #[error(
        "line {line}: prev_entry_sha256 does not match the previous entry; the log was edited, reordered, or truncated mid-history"
    )]
    BrokenChain {
        /// 1-based line number.
        line: usize,
    },
    /// A `replaces` reference names no earlier run in this log.
    #[error(
        "line {line}: `replaces` names run {run_id:?}, which is not an earlier run in this log"
    )]
    ReplacesUnknown {
        /// 1-based line number.
        line: usize,
        /// The unresolved run id.
        run_id: String,
    },
    /// A gate-pause `run_id` names no earlier run in this log.
    #[error(
        "line {line}: gate-pause `run_id` names run {run_id:?}, which is not an earlier run in this log"
    )]
    PauseRunUnknown {
        /// 1-based line number.
        line: usize,
        /// The unresolved run id.
        run_id: String,
    },
    /// A run id was appended twice; run ids are unique.
    #[error("line {line}: run id {run_id:?} already exists earlier in this log")]
    DuplicateRunId {
        /// 1-based line number.
        line: usize,
        /// The duplicated run id.
        run_id: String,
    },
    /// A record violates a format invariant (see its `field`).
    #[error("invalid `{field}`: {reason}")]
    Invalid {
        /// JSON path-ish name of the offending field.
        field: &'static str,
        /// What is wrong with it.
        reason: String,
    },
    /// Another append holds the log's lock file. The lock survives a killed
    /// append, so a writer that stopped mid-append leaves it for manual
    /// removal — a stuck lock is recoverable where a forked chain is not.
    #[error("lock {0} is held by another writer; remove it if that writer stopped mid-write")]
    LockBusy(PathBuf),
}

/// An append-only, hash-chained generation run log: one [`RunLogEntry`] per
/// JSONL line.
pub struct RunLog {
    path: PathBuf,
}

impl RunLog {
    /// The log at `path`; the file is created on the first [`RunLog::append`].
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this log writes to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read and verify every entry. A missing file is an empty log; a file
    /// whose chain, references, or format invariants do not hold is an error —
    /// never a partial read.
    pub fn entries(&self) -> Result<Vec<RunLogEntry>, RunLogError> {
        let text = self.read_text()?;
        verify_lines(&text)
    }

    /// Read and verify an existing log: unlike [`entries`](Self::entries), a
    /// missing file is an error. Verification commands use this so a mistyped
    /// path fails instead of reporting `ok (0 entries)`; appends keep the
    /// missing-file-means-new-log fallback.
    pub fn entries_strict(&self) -> Result<Vec<RunLogEntry>, RunLogError> {
        verify_lines(&std::fs::read_to_string(&self.path)?)
    }

    /// Check that `record` could be appended right now — the existing chain
    /// verifies, the record validates, and its references resolve — without
    /// writing anything. Callers run this before replacing output artifacts
    /// so a doomed append (e.g. a `replaces` naming no earlier run) fails
    /// while the previous batch is still intact. [`append`](Self::append)
    /// re-checks under its lock before writing.
    pub fn check(&self, record: &Record) -> Result<(), RunLogError> {
        let text = self.read_text()?;
        let entries = verify_lines(&text)?;
        record.validate()?;
        check_references(&entries, record, entries.len() + 1)
    }

    /// Take this log's append lock and hold it until the returned guard
    /// drops. Callers that publish a batch hold the guard from the preflight
    /// [`LockedRunLog::check`] through [`LockedRunLog::append`], so no other
    /// writer can take the lock or change the log between the check and the
    /// entry: once the batch is published, the append can only fail on an
    /// I/O error. Fails with [`RunLogError::LockBusy`] when another writer
    /// holds the lock.
    pub fn lock(&self) -> Result<LockedRunLog<'_>, RunLogError> {
        Ok(LockedRunLog {
            log: self,
            _lock: LockFile::acquire(&lock_path(&self.path))?,
        })
    }

    /// Append `record` and return the committed entry.
    ///
    /// Fills the envelope (`log_version`, `recorded_at`, `prev_entry_sha256`).
    /// `replaces` is never inferred: a run names only what the caller set —
    /// a rerun that supersedes an earlier batch must pass that run's id
    /// explicitly (e.g. `--replaces` / `RunConfig::replaces`), otherwise the
    /// entry records that it replaces nothing. When set, `replaces` must
    /// resolve to a run earlier in this log.
    ///
    /// The read–verify–link–write sequence runs under a `<log>.lock` sibling
    /// file so two processes sharing a log cannot interleave an append — the
    /// second fails with [`RunLogError::LockBusy`] instead of linking to the
    /// wrong predecessor. A writer killed mid-append leaves the lock for
    /// manual removal, the same trade `.simulate.lock` makes. The entry
    /// itself is still one write call, so readers never see a partial line.
    pub fn append(&self, record: Record) -> Result<RunLogEntry, RunLogError> {
        let mut locked = self.lock()?;
        locked.append(record)
    }

    fn append_locked(&self, record: Record) -> Result<RunLogEntry, RunLogError> {
        let mut text = self.read_text()?;
        // Refuse to extend a log whose history does not verify.
        let entries = verify_lines(&text)?;
        record.validate()?;
        check_references(&entries, &record, entries.len() + 1)?;
        // A valid log may end without a newline (the last line still parses).
        // Separating the new entry with a newline here keeps the two JSON
        // values on distinct lines; pushing it into `text` first means
        // `prev_entry_sha256` hashes the previous line exactly as the reader
        // will see it (a trailing `\r` joins the CRLF terminator, not the
        // line's bytes).
        let unterminated = !text.is_empty() && !text.ends_with('\n');
        if unterminated {
            text.push('\n');
        }
        let entry = RunLogEntry {
            log_version: RUN_LOG_SCHEMA_VERSION,
            recorded_at: now_rfc3339_utc(),
            prev_entry_sha256: last_line(&text).map(line_sha256),
            record,
        };
        let mut line = serde_json::to_string(&entry).map_err(|err| RunLogError::Parse {
            line: entries.len() + 1,
            source: err,
        })?;
        if unterminated {
            line.insert(0, '\n');
        }
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.sync_data()?;
        Ok(entry)
    }

    fn read_text(&self) -> Result<String, RunLogError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }
}

/// A [`RunLog`] whose append lock is held; see [`RunLog::lock`].
pub struct LockedRunLog<'a> {
    log: &'a RunLog,
    _lock: LockFile,
}

impl LockedRunLog<'_> {
    /// [`RunLog::check`] under the held lock.
    pub fn check(&self, record: &Record) -> Result<(), RunLogError> {
        self.log.check(record)
    }

    /// Append `record` under the held lock; see [`RunLog::append`]. Takes
    /// `&mut self` so one guard cannot drive two appends at once (two
    /// threads sharing a guard would otherwise both hash the same
    /// predecessor).
    pub fn append(&mut self, record: Record) -> Result<RunLogEntry, RunLogError> {
        self.log.append_locked(record)
    }
}

/// An exclusive lock file created with `create_new`, so it serializes
/// writers across processes. Removed on drop; a killed writer leaves it
/// behind for manual removal (a stuck lock is recoverable, a forked chain or
/// a mixed batch is not). Run logs use a `<log>.lock` sibling; publishers use
/// one per output directory.
pub struct LockFile {
    path: PathBuf,
}

impl LockFile {
    /// Create `path` exclusively, failing with [`RunLogError::LockBusy`]
    /// when it already exists.
    pub fn acquire(path: &Path) -> Result<Self, RunLogError> {
        let path = path.to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(RunLogError::LockBusy(path));
            }
            Err(error) => return Err(error.into()),
        };
        let lock = Self { path };
        let result = writeln!(file, "pid {}", std::process::id());
        drop(file);
        result?;
        Ok(lock)
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `<log>` with `.lock` appended to its file name.
fn lock_path(log: &Path) -> PathBuf {
    let mut name = log
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "run-log".into());
    name.push(".lock");
    log.with_file_name(name)
}

/// Refuse `log` as a run-log destination when it aliases one of `reserved` —
/// the batch's artifacts or publication-control files — sits underneath a
/// reserved *file*, or is an existing directory, so a misplaced `--run-log`
/// cannot overwrite the output it should only describe, nor make the log's
/// lock create a directory where a batch file must go. A reserved path that
/// is an ancestor of another reserved path is the output directory itself
/// and may hold the log.
pub fn ensure_log_path_is_clear(log: &Path, reserved: &[PathBuf]) -> Result<(), RunLogError> {
    if log.is_dir() {
        return Err(RunLogError::Invalid {
            field: "run_log.path",
            reason: format!("{} is a directory", log.display()),
        });
    }
    let refuse = |relation: &str, artifact: &Path| RunLogError::Invalid {
        field: "run_log.path",
        reason: format!(
            "{} {relation} the batch artifact {}",
            log.display(),
            artifact.display()
        ),
    };
    let log_path = normalized_path(log);
    let normalized: Vec<PathBuf> = reserved.iter().map(|path| normalized_path(path)).collect();
    for (artifact, path) in reserved.iter().zip(&normalized) {
        if same_path(&log_path, path) {
            return Err(refuse("aliases", artifact));
        }
        if !is_directory_entry(path, &normalized) && nested_under(&log_path, path) {
            return Err(refuse("is nested under", artifact));
        }
    }
    Ok(())
}

/// Whether `candidate` is the parent directory of another reserved path.
fn is_directory_entry(candidate: &Path, reserved: &[PathBuf]) -> bool {
    reserved
        .iter()
        .any(|other| other != candidate && nested_under(other, candidate))
}

/// Whether normalized `path` lies strictly inside normalized `ancestor`.
fn nested_under(path: &Path, ancestor: &Path) -> bool {
    if cfg!(windows) {
        let (path, ancestor) = (
            path.to_string_lossy().to_ascii_lowercase(),
            ancestor.to_string_lossy().to_ascii_lowercase(),
        );
        return Path::new(&path).starts_with(&ancestor) && path != ancestor;
    }
    path.starts_with(ancestor) && path != ancestor
}

/// Normalized-path equality (case-insensitive on Windows).
fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || (cfg!(windows)
            && a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy()))
}

fn normalized_path(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    // Canonicalize the longest existing prefix of the path *as written* and
    // re-attach the missing suffix. Resolving symlinks before collapsing
    // `..` matters: `link/..` is the parent of the link's target, not the
    // directory holding the link. Only the suffix that does not exist yet is
    // normalized lexically, so `alias/new/file` and `real/new/file` compare
    // equal when `alias` is a symlink to `real`.
    let components: Vec<_> = absolute.components().collect();
    for split in (1..=components.len()).rev() {
        let prefix: PathBuf = components[..split].iter().collect();
        if let Ok(canonical) = std::fs::canonicalize(&prefix) {
            let suffix: PathBuf = components[split..].iter().collect();
            return normalize_lexically(&canonical.join(suffix));
        }
    }
    normalize_lexically(&absolute)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Parse and chain-verify every line of a run-log file's text.
fn verify_lines(text: &str) -> Result<Vec<RunLogEntry>, RunLogError> {
    let mut entries = Vec::new();
    let mut previous_line: Option<&str> = None;
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let parse = |source| RunLogError::Parse {
            line: line_number,
            source,
        };
        // Decode the entry from the text itself, not from a `Value`: a
        // `Value` keeps only the last of duplicate keys, while the struct
        // decoder rejects a duplicated field.
        let raw: serde_json::Value = serde_json::from_str(line).map_err(parse)?;
        let entry: RunLogEntry = serde_json::from_str(line).map_err(parse)?;
        // Serde reads an omitted `Option` field as `None`; the format (and
        // the schema) require nullable fields to be written as explicit
        // `null`. Re-encoding the entry and comparing catches any omission.
        if serde_json::to_value(&entry).map_err(parse)? != raw {
            return Err(RunLogError::Invalid {
                field: "entry",
                reason: format!(
                    "line {line_number} is not in canonical form (a field is missing; nullable fields must be explicit null)"
                ),
            });
        }
        if entry.log_version != RUN_LOG_SCHEMA_VERSION {
            return Err(RunLogError::UnsupportedVersion {
                line: line_number,
                found: entry.log_version,
            });
        }
        match (previous_line, &entry.prev_entry_sha256) {
            (None, None) => {}
            (None, Some(_)) => return Err(RunLogError::FirstEntryLinks { line: line_number }),
            (Some(previous), Some(hash)) if *hash == line_sha256(previous) => {}
            (Some(_), Some(_)) => return Err(RunLogError::BrokenChain { line: line_number }),
            (Some(_), None) => return Err(RunLogError::MissingLink { line: line_number }),
        }
        entry.record.validate()?;
        check_references(&entries, &entry.record, line_number)?;
        entries.push(entry);
        previous_line = Some(line);
    }
    Ok(entries)
}

/// Check that run references (`replaces`, gate-pause `run_id`) resolve to
/// earlier run entries and that run ids stay unique.
fn check_references(
    prior: &[RunLogEntry],
    record: &Record,
    line: usize,
) -> Result<(), RunLogError> {
    let run_exists = |run_id: &str| {
        prior
            .iter()
            .any(|entry| matches!(&entry.record, Record::Run(run) if run.run_id == run_id))
    };
    match record {
        Record::Run(run) => {
            if run_exists(&run.run_id) {
                return Err(RunLogError::DuplicateRunId {
                    line,
                    run_id: run.run_id.clone(),
                });
            }
            if let Some(replaces) = &run.replaces
                && !run_exists(replaces)
            {
                return Err(RunLogError::ReplacesUnknown {
                    line,
                    run_id: replaces.clone(),
                });
            }
        }
        Record::GatePause(pause) => {
            if let Some(run_id) = &pause.run_id
                && !run_exists(run_id)
            {
                return Err(RunLogError::PauseRunUnknown {
                    line,
                    run_id: run_id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn last_line(text: &str) -> Option<&str> {
    text.lines().rev().find(|line| !line.trim().is_empty())
}

/// SHA-256 (lowercase hex) of one serialized log line, excluding its newline.
fn line_sha256(line: &str) -> String {
    sha256_hex(line.as_bytes())
}

/// SHA-256 (lowercase hex) of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

/// A fresh run id: `run-<yyyymmddThhmmssZ>-<8 hex chars>`; the suffix mixes
/// the nanosecond clock, pid, and a per-process counter.
pub fn new_run_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(now.as_nanos().to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let digest = hasher.finalize();
    let mut suffix = String::with_capacity(8);
    for byte in &digest[..4] {
        use std::fmt::Write as _;
        write!(suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("run-{}-{suffix}", compact_utc(now.as_secs()))
}

/// Current UTC time as RFC 3339 (`YYYY-MM-DDTHH:MM:SSZ`).
fn now_rfc3339_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (date, time) = civil_utc(secs);
    format!("{date}T{time}Z")
}

/// `yyyymmddThhmmss` for run ids.
fn compact_utc(secs: u64) -> String {
    let (date, time) = civil_utc(secs);
    format!("{}T{}", date.replace('-', ""), time.replace(':', ""))
}

/// `YYYY-MM-DD` and `HH:MM:SS` for a unix timestamp (UTC). Howard Hinnant's
/// civil-from-days algorithm; valid for the whole `u64` seconds range.
fn civil_utc(secs: u64) -> (String, String) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (
        format!("{year:04}-{m:02}-{d:02}"),
        format!("{:02}:{:02}:{:02}", rem / 3_600, (rem / 60) % 60, rem % 60),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::*;
    use std::collections::BTreeMap;
    use tutor_schema::GeneratorLanguage;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "tutor-runlog-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn run_record(run_id: &str, stage: Stage) -> Record {
        Record::Run(Box::new(RunRecord {
            run_id: run_id.to_owned(),
            family: Some("derivative_misconceptions".to_owned()),
            spec: None,
            stage,
            replaces: None,
            environment: Environment {
                git_commit: None,
                rustc_version: "rustc test".to_owned(),
                cargo_version: "cargo test".to_owned(),
                pipeline_version: "0.1.0".to_owned(),
                julia_version: None,
                julia_worker_version: None,
            },
            generators: vec![GeneratorUse {
                name: "binomial_derivative_at_point".to_owned(),
                version: 1,
                language: GeneratorLanguage::Rust,
                seeds: SeedUse {
                    start: 0,
                    end: Some(40),
                    derivation: None,
                },
                parameter_ranges: BTreeMap::new(),
            }],
            teacher: None,
            counts: Counts {
                generated: 40,
                verified: 40,
                rejected: 0,
                rejected_by_reason: BTreeMap::new(),
                deduplicated: None,
                decontaminated: None,
                exported: None,
            },
            output: OutputRef {
                directory: "out".to_owned(),
                manifest_sha256: None,
            },
            split_manifest_sha256: None,
            qa: None,
        }))
    }

    #[test]
    fn entries_round_trip() {
        let path = temp_path("roundtrip.jsonl");
        let log = RunLog::at(&path);
        let first = log
            .append(run_record("run-a", Stage::Generate))
            .expect("first append");
        assert_eq!(first.log_version, RUN_LOG_SCHEMA_VERSION);
        assert_eq!(first.prev_entry_sha256, None);
        let second = log
            .append(run_record("run-b", Stage::Generate))
            .expect("second append");
        let text = std::fs::read_to_string(&path).expect("log written");
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(second.prev_entry_sha256, Some(line_sha256(lines[0])));
        // A rerun without an explicit `replaces` replaces nothing.
        let Record::Run(second_run) = &second.record else {
            panic!("run record");
        };
        assert_eq!(second_run.replaces, None);

        let entries = log.entries().expect("verify");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], first);
        assert_eq!(entries[1], second);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn replaces_is_explicit_never_inferred() {
        let path = temp_path("stages.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-gen", Stage::Generate)).unwrap();
        // A rerun of the same stage with no `replaces` passed does not name
        // the earlier run — a new batch is not a replacement.
        let sim = log.append(run_record("run-sim", Stage::Generate)).unwrap();
        let Record::Run(sim_run) = &sim.record else {
            panic!("run record");
        };
        assert_eq!(sim_run.replaces, None);
        // An explicit `replaces` is recorded and resolves to the earlier run.
        let mut rerun = run_record("run-fix", Stage::Generate);
        let Record::Run(rerun_run) = &mut rerun else {
            panic!("run record");
        };
        rerun_run.replaces = Some("run-gen".to_owned());
        let committed = log.append(rerun).unwrap();
        let Record::Run(committed_run) = &committed.record else {
            panic!("run record");
        };
        assert_eq!(committed_run.replaces.as_deref(), Some("run-gen"));
        let _ = std::fs::remove_file(&path);
    }

    /// Append `runs` entries to a fresh log, rewrite it with `corrupt`, and
    /// assert that verification fails with `expected`.
    fn corrupted_log_fails_verify(
        name: &str,
        runs: usize,
        corrupt: impl FnOnce(&str) -> String,
        expected: impl FnOnce(&RunLogError) -> bool,
    ) {
        let path = temp_path(name);
        let log = RunLog::at(&path);
        for index in 0..runs {
            log.append(run_record(&format!("run-{index}"), Stage::Generate))
                .unwrap();
        }
        let original = std::fs::read_to_string(&path).unwrap();
        let corrupted = corrupt(&original);
        assert_ne!(corrupted, original, "corruption must change the log");
        std::fs::write(&path, corrupted).unwrap();
        let result = log.entries();
        assert!(
            matches!(result, Err(ref error) if expected(error)),
            "unexpected verification result: {result:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupted_history_is_detected() {
        type Corrupt = fn(&str) -> String;
        type Expect = fn(&RunLogError) -> bool;
        let cases: [(&str, usize, Corrupt, Expect); 5] = [
            (
                "tampered.jsonl",
                2,
                |text| text.replace("\"generated\":40", "\"generated\":41"),
                |error| matches!(error, RunLogError::BrokenChain { line: 2 }),
            ),
            (
                "reordered.jsonl",
                2,
                |text| {
                    let lines: Vec<_> = text.lines().collect();
                    format!("{}\n{}\n", lines[1], lines[0])
                },
                |error| matches!(error, RunLogError::FirstEntryLinks { line: 1 }),
            ),
            (
                "deleted.jsonl",
                3,
                |text| {
                    let lines: Vec<_> = text.lines().collect();
                    format!("{}\n{}\n", lines[0], lines[2])
                },
                |error| matches!(error, RunLogError::BrokenChain { line: 2 }),
            ),
            (
                // A duplicated field must not let the last copy win.
                "duplicate-key.jsonl",
                1,
                |text| {
                    text.replacen(
                        "{\"log_version\":1,",
                        "{\"log_version\":2,\"log_version\":1,",
                        1,
                    )
                },
                |error| matches!(error, RunLogError::Parse { line: 1, .. }),
            ),
            (
                // Serde would read the omitted nullable field as `None`; the
                // format requires an explicit `null`.
                "omitted-null.jsonl",
                1,
                |text| text.replacen("\"prev_entry_sha256\":null,", "", 1),
                |error| matches!(error, RunLogError::Invalid { field: "entry", .. }),
            ),
        ];
        for (name, runs, corrupt, expected) in cases {
            corrupted_log_fails_verify(name, runs, corrupt, expected);
        }
    }

    #[test]
    fn append_to_an_unterminated_log_keeps_entries_on_separate_lines() {
        for (name, tail) in [("no-newline.jsonl", ""), ("trailing-cr.jsonl", "\r")] {
            let path = temp_path(name);
            let log = RunLog::at(&path);
            log.append(run_record("run-a", Stage::Generate)).unwrap();
            // Drop the trailing newline (and leave a bare CR in the second
            // case): the file still parses as a one-entry log.
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.pop();
            text.push_str(tail);
            std::fs::write(&path, text).unwrap();
            assert_eq!(log.entries().unwrap().len(), 1);

            log.append(run_record("run-b", Stage::Generate)).unwrap();
            let entries = log.entries().expect("log still verifies");
            assert_eq!(entries.len(), 2);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap().lines().count(),
                2,
                "{name}: the appended entry must not merge into the previous line"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn entries_strict_refuses_a_missing_log() {
        let path = temp_path("missing.jsonl");
        let log = RunLog::at(&path);
        assert_eq!(log.entries().unwrap().len(), 0);
        assert!(matches!(
            log.entries_strict(),
            Err(RunLogError::Io(ref error)) if error.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn a_stale_append_lock_blocks_writers_until_removed() {
        let path = temp_path("locked.jsonl");
        let log = RunLog::at(&path);
        std::fs::write(lock_path(&path), "pid 0").unwrap();
        assert!(matches!(
            log.append(run_record("run-a", Stage::Generate)),
            Err(RunLogError::LockBusy(_))
        ));
        std::fs::remove_file(lock_path(&path)).unwrap();
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        assert!(
            !lock_path(&path).exists(),
            "the lock is released when append returns"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_log_paths_must_not_alias_artifacts() {
        let out = temp_path("artifact-out");
        std::fs::create_dir_all(&out).unwrap();
        let artifact = out.join("rejections.jsonl");
        // Direct hit, `..` components, and an existing directory all refuse.
        assert!(ensure_log_path_is_clear(&artifact, std::slice::from_ref(&artifact)).is_err());
        assert!(
            ensure_log_path_is_clear(
                &out.join("sub").join("..").join("rejections.jsonl"),
                std::slice::from_ref(&artifact)
            )
            .is_err()
        );
        assert!(ensure_log_path_is_clear(&out, std::slice::from_ref(&artifact)).is_err());
        // An unrelated log name next to the artifacts is fine.
        assert!(
            ensure_log_path_is_clear(&out.join("run-log.jsonl"), std::slice::from_ref(&artifact))
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn append_refuses_a_broken_log() {
        let path = temp_path("broken.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        log.append(run_record("run-b", Stage::Generate)).unwrap();
        std::fs::write(
            &path,
            std::fs::read_to_string(&path)
                .unwrap()
                .replace("run-a", "run-x"),
        )
        .unwrap();
        assert!(matches!(
            log.append(run_record("run-c", Stage::Generate)),
            Err(RunLogError::BrokenChain { .. })
        ));
        let _ = std::fs::remove_file(&path);
    }

    /// Mutate a valid run record with `mutate`, and assert the append is
    /// rejected with an error matching `expected` and leaves no log behind.
    fn append_is_rejected(
        name: &str,
        mutate: impl FnOnce(&mut RunRecord),
        expected: impl FnOnce(&RunLogError) -> bool,
    ) {
        let path = temp_path(name);
        let log = RunLog::at(&path);
        let mut record = run_record("run-b", Stage::Generate);
        let Record::Run(run) = &mut record else {
            panic!("run record");
        };
        mutate(run);
        let result = log.append(record);
        assert!(
            matches!(result, Err(ref error) if expected(error)),
            "{name}: unexpected append result: {result:?}"
        );
        assert!(!path.exists(), "{name}: a rejected append must not write");
    }

    type Mutate = fn(&mut RunRecord);
    type Expect = fn(&RunLogError) -> bool;

    #[test]
    fn manifest_digests_must_be_lowercase_sha256() {
        let cases: [(&str, Mutate); 2] = [
            ("bad-sha.jsonl", |run| {
                run.output.manifest_sha256 = Some("not-a-digest".to_owned());
            }),
            ("upper-sha.jsonl", |run| {
                run.output.manifest_sha256 = Some("AB".repeat(32));
            }),
        ];
        for (name, mutate) in cases {
            append_is_rejected(name, mutate, |error| {
                matches!(
                    error,
                    RunLogError::Invalid {
                        field: "output.manifest_sha256",
                        ..
                    }
                )
            });
        }
    }

    #[test]
    fn invalid_run_records_are_rejected() {
        let cases: [(&str, Mutate, Expect); 4] = [
            (
                "unsafe-end.jsonl",
                |run| run.generators[0].seeds.end = Some(tutor_schema::MAX_SEED + 2),
                |error| {
                    matches!(
                        error,
                        RunLogError::Invalid {
                            field: "generators[].seeds.end",
                            ..
                        }
                    )
                },
            ),
            (
                "blank-derivation.jsonl",
                |run| {
                    run.generators[0].seeds.end = None;
                    run.generators[0].seeds.derivation = Some("  ".to_owned());
                },
                |error| {
                    matches!(
                        error,
                        RunLogError::Invalid {
                            field: "generators[].seeds.derivation",
                            ..
                        }
                    )
                },
            ),
            (
                "overflow.jsonl",
                |run| {
                    run.counts.rejected = 1;
                    run.counts.rejected_by_reason =
                        BTreeMap::from([("a".to_owned(), u64::MAX), ("b".to_owned(), 2)]);
                },
                |error| {
                    matches!(
                        error,
                        RunLogError::Invalid {
                            field: "counts.rejected_by_reason",
                            ..
                        }
                    )
                },
            ),
            (
                "replaces.jsonl",
                |run| run.replaces = Some("run-a".to_owned()),
                |error| matches!(error, RunLogError::ReplacesUnknown { .. }),
            ),
        ];
        for (name, mutate, expected) in cases {
            append_is_rejected(name, mutate, expected);
        }
    }

    #[test]
    fn lock_holds_the_log_between_check_and_append() {
        let path = temp_path("held.jsonl");
        let log = RunLog::at(&path);
        let mut locked = log.lock().expect("lock is free");
        let record = run_record("run-a", Stage::Generate);
        locked.check(&record).unwrap();
        // Another writer cannot slip an entry in between check and append.
        assert!(matches!(
            log.append(run_record("run-x", Stage::Generate)),
            Err(RunLogError::LockBusy(_))
        ));
        locked.append(record).unwrap();
        drop(locked);
        assert!(!lock_path(&path).exists(), "the lock is released on drop");
        assert_eq!(log.entries().unwrap().len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    fn paths_alias_for_test(a: &Path, b: &Path) -> bool {
        same_path(&normalized_path(a), &normalized_path(b))
    }

    #[cfg(unix)]
    #[test]
    fn missing_paths_alias_through_a_symlinked_ancestor() {
        let root = temp_path("alias-root");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // Neither `new/` nor the file exists yet; only an ancestor does.
        assert!(paths_alias_for_test(
            &link.join("new/run-log.jsonl"),
            &real.join("new/run-log.jsonl")
        ));
        assert!(!paths_alias_for_test(
            &link.join("new/run-log.jsonl"),
            &real.join("other/run-log.jsonl")
        ));
        // `link/..` is the parent of the link's *target*: resolve the link
        // before collapsing `..`.
        let nested = real.join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        let deep_link = root.join("deep");
        std::os::unix::fs::symlink(&nested, &deep_link).unwrap();
        assert!(paths_alias_for_test(
            &deep_link.join("../manifest.json"),
            &real.join("manifest.json")
        ));
        assert!(!paths_alias_for_test(
            &deep_link.join("../manifest.json"),
            &root.join("manifest.json")
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_log_nested_under_a_reserved_file_is_refused() {
        let root = temp_path("nested-reserved");
        let out = root.join("out");
        let reserved = [
            out.clone(),
            out.join("manifest.json"),
            out.join("manifest.json.tmp"),
        ];
        // The output directory may hold the log...
        assert!(ensure_log_path_is_clear(&out.join("run-log.jsonl"), &reserved).is_ok());
        // ...but a reserved file may not become its parent directory.
        assert!(matches!(
            ensure_log_path_is_clear(&out.join("manifest.json.tmp/run-log.jsonl"), &reserved),
            Err(RunLogError::Invalid { field: "run_log.path", ref reason }) if reason.contains("nested under")
        ));
    }

    #[test]
    fn duplicate_run_id_is_rejected() {
        let path = temp_path("duplicate.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        assert!(matches!(
            log.append(run_record("run-a", Stage::Simulate)),
            Err(RunLogError::DuplicateRunId { .. })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn gate_pause_appends_and_checks_run_references() {
        let path = temp_path("pause.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        let mut evidence = BTreeMap::new();
        evidence.insert("reason".to_owned(), serde_json::json!("qa_fail"));
        let pause = log
            .append(Record::GatePause(GatePauseRecord {
                family: "derivative_misconceptions".to_owned(),
                gate: Gate::QaSample,
                run_id: Some("run-a".to_owned()),
                detail: "QA sample failed".to_owned(),
                evidence,
            }))
            .expect("pause appends");
        let Record::GatePause(record) = &pause.record else {
            panic!("pause record");
        };
        assert_eq!(record.gate, Gate::QaSample);
        assert_eq!(log.entries().unwrap().len(), 2);

        let bad = Record::GatePause(GatePauseRecord {
            family: "derivative_misconceptions".to_owned(),
            gate: Gate::Export,
            run_id: Some("run-nope".to_owned()),
            detail: "export failed".to_owned(),
            evidence: BTreeMap::from([("reason".to_owned(), serde_json::json!("x"))]),
        });
        assert!(matches!(
            log.append(Record::GatePause(GatePauseRecord {
                family: "derivative_misconceptions".to_owned(),
                gate: Gate::Export,
                run_id: None,
                detail: "export failed".to_owned(),
                evidence: BTreeMap::new(),
            })),
            Err(RunLogError::Invalid {
                field: "evidence",
                ..
            })
        ));
        assert!(matches!(
            log.append(bad),
            Err(RunLogError::PauseRunUnknown { .. })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_ids_are_unique() {
        assert_ne!(new_run_id(), new_run_id());
        assert!(new_run_id().starts_with("run-"));
    }

    #[test]
    fn civil_utc_matches_known_dates() {
        assert_eq!(now_rfc3339_utc().len(), 20);
        assert_eq!(compact_utc(1_700_000_000), "20231114T221320");
        // 1970-01-01T00:00:00Z
        assert_eq!(compact_utc(0), "19700101T000000");
        // 2000-02-29T12:34:56Z — leap day.
        assert_eq!(compact_utc(951827696), "20000229T123456");
    }
}
