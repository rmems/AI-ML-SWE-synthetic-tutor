//! Input loading: dialogue row files (`.json` / `.jsonl`), the verified
//! problems file, and the held-out file listing.
//!
//! Row files are read as raw text lines so the checks stage can re-parse the
//! original JSON (serde's `Value` silently keeps the last of duplicate
//! fields — the same reason `tutor-validate` validates raw text). Problems
//! are parsed eagerly through [`Problem::from_json`]: a malformed problems
//! file is a bad input and fails the build, not a per-row rejection.

use std::collections::BTreeMap;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use tutor_schema::Problem;

use crate::{BuildError, MAX_ROW_BYTES};

/// One raw input row: its origin plus the original JSON text.
#[derive(Debug)]
pub struct InputLine {
    /// The `--rows` file this line came from.
    pub file: PathBuf,
    /// One-based line number for `.jsonl` files; `None` for `.json` files.
    pub line: Option<usize>,
    /// The raw JSON text of the candidate row.
    pub text: String,
}

/// Read every `--rows` file in CLI order. `.json` files hold one row;
/// `.jsonl` files hold one row per non-empty line.
///
/// # Errors
/// [`BuildError::Input`] when a file cannot be read, has another extension,
/// or a single row exceeds [`MAX_ROW_BYTES`].
pub fn load_rows(paths: &[PathBuf]) -> Result<Vec<InputLine>, BuildError> {
    let mut rows = Vec::new();
    for path in paths {
        if is_jsonl(path) {
            load_jsonl(path, &mut rows)?;
        } else if is_json(path) {
            load_json(path, &mut rows)?;
        } else {
            return Err(BuildError::input(
                path,
                "expected a .json or .jsonl dialogue row file",
            ));
        }
    }
    Ok(rows)
}

fn load_json(path: &Path, rows: &mut Vec<InputLine>) -> Result<(), BuildError> {
    let handle = std::fs::File::open(path).map_err(|e| BuildError::io(path, e))?;
    let mut text = String::new();
    std::io::Read::take(handle, MAX_ROW_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| BuildError::io(path, e))?;
    if text.len() as u64 > MAX_ROW_BYTES {
        return Err(BuildError::input(
            path,
            format!("file exceeds {MAX_ROW_BYTES} bytes; rows are single dialogue records"),
        ));
    }
    if text.trim().is_empty() {
        return Err(BuildError::input(path, "file is empty"));
    }
    rows.push(InputLine {
        file: path.to_owned(),
        line: None,
        text,
    });
    Ok(())
}

fn load_jsonl(path: &Path, rows: &mut Vec<InputLine>) -> Result<(), BuildError> {
    let handle = std::fs::File::open(path).map_err(|e| BuildError::io(path, e))?;
    let reader = std::io::BufReader::new(handle);
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| BuildError::io(path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        if line.len() as u64 > MAX_ROW_BYTES {
            return Err(BuildError::input(
                path,
                format!("line {} exceeds {MAX_ROW_BYTES} bytes", index + 1),
            ));
        }
        rows.push(InputLine {
            file: path.to_owned(),
            line: Some(index + 1),
            text: line,
        });
    }
    Ok(())
}

/// Parse a verified `problems.jsonl` into an index by `problem_id`.
///
/// # Errors
/// [`BuildError::Input`] when the file cannot be read;
/// [`BuildError::ProblemLine`] for a malformed line or a duplicate
/// `problem_id`.
pub fn load_problems(path: &Path) -> Result<BTreeMap<String, Problem>, BuildError> {
    let text = std::fs::read_to_string(path).map_err(|e| BuildError::io(path, e))?;
    let mut problems = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fail = |reason: String| BuildError::ProblemLine {
            path: path.to_owned(),
            line: index + 1,
            reason,
        };
        let problem = Problem::from_json(line).map_err(|e| fail(e.to_string()))?;
        if problems
            .insert(problem.problem_id.clone(), problem.clone())
            .is_some()
        {
            return Err(fail(format!(
                "duplicate problem_id {:?} in problems file",
                problem.problem_id
            )));
        }
    }
    Ok(problems)
}

/// Every `.json` / `.jsonl` file under `dir`, recursively, in sorted order.
/// Non-JSON files (READMEs, notes) are skipped. An empty or JSON-free
/// directory yields an empty list — the held-out set is legitimately empty
/// until the first frozen eval release.
///
/// # Errors
/// [`BuildError::HeldOut`] when `dir` does not exist or cannot be read.
pub fn heldout_files(dir: &Path) -> Result<Vec<PathBuf>, BuildError> {
    if !dir.is_dir() {
        return Err(BuildError::heldout(dir, "directory does not exist"));
    }
    let mut files = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(next) = pending.pop() {
        let entries = std::fs::read_dir(&next)
            .map_err(|e| BuildError::heldout(&next, format!("cannot read: {e}")))?;
        for entry in entries {
            let path = entry
                .map_err(|e| BuildError::heldout(&next, format!("cannot read entry: {e}")))?
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if is_json(&path) || is_jsonl(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_json(path: &Path) -> bool {
    has_extension(path, "json")
}

fn is_jsonl(path: &Path) -> bool {
    has_extension(path, "jsonl")
}

fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heldout_files_finds_json_recursively() {
        let dir = std::env::temp_dir().join(format!("tutor-build-input-{}", std::process::id()));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("README.md"), "not data").unwrap();
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        std::fs::write(nested.join("b.jsonl"), "{}\n").unwrap();
        let files = heldout_files(&dir).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.json"));
        assert!(files[1].ends_with("b.jsonl"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn heldout_files_rejects_missing_dir() {
        assert!(matches!(
            heldout_files(Path::new("definitely/missing/dir")),
            Err(BuildError::HeldOut { .. })
        ));
    }
}
