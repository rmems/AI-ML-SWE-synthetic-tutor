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

    /// Append `record` and return the committed entry.
    ///
    /// Fills the envelope (`log_version`, `recorded_at`, `prev_entry_sha256`).
    /// `replaces` is never inferred: a run names only what the caller set —
    /// a rerun that supersedes an earlier batch must pass that run's id
    /// explicitly (e.g. `--replaces` / `RunConfig::replaces`), otherwise the
    /// entry records that it replaces nothing. When set, `replaces` must
    /// resolve to a run earlier in this log.
    ///
    /// Appending is one write call so readers never see a partial line, but
    /// concurrent writers to the same path are not serialized — callers
    /// sharing an output directory must serialize themselves (as
    /// `tutor-generate simulate` does with `.simulate.lock`).
    pub fn append(&self, record: Record) -> Result<RunLogEntry, RunLogError> {
        let text = self.read_text()?;
        // Refuse to extend a log whose history does not verify.
        let entries = verify_lines(&text)?;
        record.validate()?;
        check_references(&entries, &record, entries.len() + 1)?;
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
        line.push('\n');
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
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

/// Parse and chain-verify every line of a run-log file's text.
fn verify_lines(text: &str) -> Result<Vec<RunLogEntry>, RunLogError> {
    let mut entries = Vec::new();
    let mut previous_line: Option<&str> = None;
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let entry: RunLogEntry =
            serde_json::from_str(line).map_err(|source| RunLogError::Parse {
                line: line_number,
                source,
            })?;
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

    #[test]
    fn tampered_history_is_detected() {
        let path = temp_path("tampered.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        log.append(run_record("run-b", Stage::Generate)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let tampered = text.replace("\"generated\":40", "\"generated\":41");
        assert_ne!(tampered, text);
        std::fs::write(&path, tampered).unwrap();
        assert!(matches!(
            log.entries(),
            Err(RunLogError::BrokenChain { line: 2 })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reordered_history_is_detected() {
        let path = temp_path("reordered.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        log.append(run_record("run-b", Stage::Generate)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        std::fs::write(&path, format!("{}\n{}\n", lines[1], lines[0])).unwrap();
        assert!(matches!(
            log.entries(),
            Err(RunLogError::FirstEntryLinks { line: 1 })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn deleted_middle_entry_is_detected() {
        let path = temp_path("deleted.jsonl");
        let log = RunLog::at(&path);
        log.append(run_record("run-a", Stage::Generate)).unwrap();
        log.append(run_record("run-b", Stage::Generate)).unwrap();
        log.append(run_record("run-c", Stage::Generate)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        std::fs::write(&path, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
        assert!(matches!(
            log.entries(),
            Err(RunLogError::BrokenChain { line: 2 })
        ));
        let _ = std::fs::remove_file(&path);
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

    #[test]
    fn replaces_must_name_an_earlier_run() {
        let path = temp_path("replaces.jsonl");
        let log = RunLog::at(&path);
        let mut record = run_record("run-b", Stage::Generate);
        let Record::Run(run) = &mut record else {
            panic!("run record");
        };
        run.replaces = Some("run-a".to_owned());
        assert!(matches!(
            log.append(record),
            Err(RunLogError::ReplacesUnknown { .. })
        ));
        let _ = std::fs::remove_file(&path);
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
            evidence: BTreeMap::new(),
        });
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
