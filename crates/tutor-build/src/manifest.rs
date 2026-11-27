//! The per-release split manifest, written atomically and last.
//!
//! The manifest is the release record: what went in (input file hashes),
//! what came out (per-split counts, counts per topic and per taxonomy id,
//! per-file SHA-256 + byte length, rejected-row totals by reason), the
//! frozen `row_id`/`problem_id` → split assignments a later
//! `--previous-manifest` run reads back, and the tool identity. Its own
//! SHA-256 cannot be part of the document, so it is printed by the CLI and
//! stored beside the manifest in `manifest.json.sha256` — the generation
//! run log (#7) can record that digest.

use std::collections::BTreeMap;
use std::io::{Read, Write as _};
use std::path::Path;

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{BuildConfig, RejectedRow, dedup::NEAR_DUPLICATE_THRESHOLD, splits};

/// Hex-encoded SHA-256 plus byte length of a blob.
#[derive(Debug, Clone, Serialize)]
pub struct FileDigest {
    /// Hex SHA-256.
    pub sha256: String,
    /// Byte length.
    pub bytes: u64,
}

/// SHA-256 + byte length of an input file.
#[derive(Debug, Clone, Serialize)]
pub struct FileMeta {
    /// Path as given on the command line.
    pub path: String,
    /// Hex SHA-256 of the file contents.
    pub sha256: String,
    /// Byte length of the file.
    pub bytes: u64,
}

impl FileMeta {
    /// Stream-hash a file.
    ///
    /// # Errors
    /// The underlying `std::io::Error` when the file cannot be read.
    pub fn of(path: &Path) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 8192];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            bytes += read as u64;
        }
        Ok(Self {
            path: path.display().to_string(),
            sha256: format!("{:x}", hasher.finalize()),
            bytes,
        })
    }
}

/// Input file digests recorded under `inputs` in the manifest.
#[derive(Debug, Serialize)]
pub struct Inputs {
    /// Every `--rows` file.
    pub rows: Vec<FileMeta>,
    /// The `--problems` file, when given.
    pub problems: Vec<FileMeta>,
    /// Every held-out file used for decontamination.
    pub heldout: Vec<FileMeta>,
}

/// One written artifact's digest and row count.
#[derive(Debug, Serialize)]
pub struct Artifact {
    /// Hex SHA-256 of the file.
    pub sha256: String,
    /// Byte length.
    pub bytes: u64,
    /// Rows in the file.
    pub rows: usize,
}

/// Per-release accepted/rejected tallies.
#[derive(Debug, Serialize)]
pub struct Counts {
    /// Rows exported.
    pub accepted: usize,
    /// Rows rejected.
    pub rejected: usize,
    /// Accepted rows per split.
    pub splits: BTreeMap<String, usize>,
    /// Accepted rows per topic, broken down per split.
    pub topics: BTreeMap<String, BTreeMap<String, usize>>,
    /// Accepted rows per taxonomy id (`none` for confirmation rows),
    /// broken down per split.
    pub taxonomy_ids: BTreeMap<String, BTreeMap<String, usize>>,
    /// Rejected rows per stable reason code.
    pub rejected_by_reason: BTreeMap<String, usize>,
}

impl Counts {
    /// Tally accepted rows (already bucketed per split/topic/taxonomy) and
    /// rejected rows (per reason code) for the manifest.
    pub fn of(
        splits: BTreeMap<String, usize>,
        topics: BTreeMap<String, BTreeMap<String, usize>>,
        taxonomy_ids: BTreeMap<String, BTreeMap<String, usize>>,
        rejected: &[RejectedRow],
    ) -> Self {
        let mut rejected_by_reason: BTreeMap<String, usize> = BTreeMap::new();
        for row in rejected {
            *rejected_by_reason.entry(row.code.clone()).or_insert(0) += 1;
        }
        Self {
            accepted: splits.values().sum(),
            rejected: rejected.len(),
            splits,
            topics,
            taxonomy_ids,
            rejected_by_reason,
        }
    }
}

/// The split manifest document, `manifest.json` in the output directory.
#[derive(Debug, Serialize)]
pub struct SplitManifest {
    /// Tool identity (`tutor-build`, crate version).
    pub tool: Tool,
    /// Dataset family.
    pub family: String,
    /// Release identifier this manifest freezes.
    pub release: String,
    /// Assignment scheme (`splits::SPLIT_SCHEME`).
    pub split_scheme: &'static str,
    /// Target split shares, in percent.
    pub split_ratios: BTreeMap<String, f64>,
    /// Jaccard threshold used for both dedup and decontamination.
    pub near_duplicate_threshold: f64,
    /// Accepted/rejected tallies.
    pub counts: Counts,
    /// Input file digests.
    pub inputs: Inputs,
    /// Output file digests (the manifest and its sidecar excluded).
    pub artifacts: BTreeMap<String, Artifact>,
    /// Frozen split assignments, read back by `--previous-manifest`.
    pub assignments: AssignmentsDoc,
}

/// Tool identity block.
#[derive(Debug, Serialize)]
pub struct Tool {
    /// Crate name.
    pub name: &'static str,
    /// Crate version.
    pub version: &'static str,
}

/// Serializable form of `splits::Assignments`.
#[derive(Debug, Serialize)]
pub struct AssignmentsDoc {
    /// `row_id` → split.
    pub rows: BTreeMap<String, String>,
    /// `problem_id` → split.
    pub problems: BTreeMap<String, String>,
}

impl SplitManifest {
    /// Assemble the manifest for a finished build.
    pub fn new(
        config: &BuildConfig,
        inputs: Inputs,
        artifacts: BTreeMap<String, Artifact>,
        counts: Counts,
        assignments: &splits::Assignments,
    ) -> Self {
        Self {
            tool: Tool {
                name: "tutor-build",
                version: env!("CARGO_PKG_VERSION"),
            },
            family: config.family.clone(),
            release: config.release.clone(),
            split_scheme: splits::SPLIT_SCHEME,
            split_ratios: BTreeMap::from([
                ("train".to_owned(), 90.0),
                ("validation".to_owned(), 5.0),
                ("test".to_owned(), 5.0),
            ]),
            near_duplicate_threshold: NEAR_DUPLICATE_THRESHOLD,
            counts,
            inputs,
            artifacts,
            assignments: AssignmentsDoc {
                rows: assignments
                    .rows
                    .iter()
                    .map(|(id, split)| (id.clone(), split.as_str().to_owned()))
                    .collect(),
                problems: assignments
                    .problems
                    .iter()
                    .map(|(id, split)| (id.clone(), split.as_str().to_owned()))
                    .collect(),
            },
        }
    }
}

/// Write `contents` to `path` atomically: a temporary sibling is written
/// and renamed into place, so a failed write never leaves a truncated
/// artifact (the same `atomicwrites` pattern as `tutor-generate`).
///
/// # Errors
/// The underlying `std::io::Error` when the write or rename fails.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::other(format!("{} has no parent directory", path.display()))
    })?;
    AtomicFile::new_with_tmpdir(path, AllowOverwrite, parent)
        .write(|file| file.write_all(contents))
        .map_err(|error| {
            std::io::Error::other(format!(
                "atomic write to {} failed: {error:?}",
                path.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_meta_hashes_contents() {
        let dir = std::env::temp_dir().join(format!("tutor-build-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.json");
        std::fs::write(&file, "hello").unwrap();
        let meta = FileMeta::of(&file).unwrap();
        assert_eq!(meta.bytes, 5);
        assert_eq!(
            meta.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_atomic_creates_file() {
        let dir = std::env::temp_dir().join(format!("tutor-build-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.json");
        write_atomic(&file, b"data").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"data");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
