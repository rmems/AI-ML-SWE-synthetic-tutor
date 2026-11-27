//! Stage 4 — deterministic, frozen train / validation / test assignment.
//!
//! Assignment is keyed on the **problem**, not the row: two dialogues
//! grounded in the same problem always land in the same split, so a problem
//! cannot leak between train and eval. New assignments come from
//! [`hash_split`], a SHA-256 draw over the `problem_id` — same input, same
//! split, on any machine, with no RNG state.
//!
//! With `--previous-manifest` the previous release's `assignments` win: a
//! row that was already assigned never moves, and a problem seen before
//! keeps its split. Only rows and problems unseen by the previous release
//! draw fresh assignments. The frozen rule is therefore "a row never moves
//! between splits after a release; new data goes into the next release".

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::BuildError;

/// Identifies the hash-based assignment scheme in the manifest, so a
/// future scheme change does not silently reshuffle frozen rows.
pub const SPLIT_SCHEME: &str = "sha256-problem-v1";

/// Target shares of the hash space: 90% train, 5% validation, 5% test, in
/// basis points out of [`BASIS_POINTS`].
const BASIS_POINTS: u64 = 10_000;
/// Upper bound of the train bucket in basis points.
const TRAIN_BPS: u64 = 9_000;
/// Upper bound of the validation bucket in basis points.
const VALIDATION_BPS: u64 = 9_500;

/// One of the three frozen splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    /// Training rows (~90% of problems).
    Train,
    /// Validation rows (~5% of problems).
    Validation,
    /// Held-out test rows (~5% of problems).
    Test,
}

impl Split {
    /// All splits, in canonical order.
    pub const ALL: [Self; 3] = [Self::Train, Self::Validation, Self::Test];

    /// The file/manifest name for the split.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Train => "train",
            Self::Validation => "validation",
            Self::Test => "test",
        }
    }
}

/// Deterministic split draw for a `problem_id`: SHA-256 of a scheme-tagged
/// seed, first 8 bytes scaled into basis points of the hash space.
pub fn hash_split(problem_id: &str) -> Split {
    let mut hasher = Sha256::new();
    hasher.update(b"tutor-build/split/v1\x00");
    hasher.update(problem_id.as_bytes());
    let digest = hasher.finalize();
    let draw = u64::from_le_bytes(digest[..8].try_into().expect("SHA-256 has eight bytes"));
    let scaled =
        u64::try_from(u128::from(draw) * BASIS_POINTS as u128 / (u128::from(u64::MAX) + 1))
            .expect("scaled draw fits u64");
    if scaled < TRAIN_BPS {
        Split::Train
    } else if scaled < VALIDATION_BPS {
        Split::Validation
    } else {
        Split::Test
    }
}

/// The split assignments of one release, replayed into the manifest so a
/// later `--previous-manifest` run can freeze them.
#[derive(Debug, Default)]
pub struct Assignments {
    /// `row_id` → split for every exported row.
    pub rows: BTreeMap<String, Split>,
    /// `problem_id` → split for every exported problem.
    pub problems: BTreeMap<String, Split>,
}

/// Assigns splits, honoring frozen assignments from a previous manifest.
#[derive(Debug, Default)]
pub struct Assigner {
    previous_rows: HashMap<String, Split>,
    previous_problems: HashMap<String, Split>,
    current: Assignments,
    /// Frozen constraints that could not all be satisfied, in input order.
    conflicts: Vec<String>,
}

impl Assigner {
    /// An assigner with no frozen assignments.
    pub fn fresh() -> Self {
        Self::default()
    }

    /// Load frozen assignments from `path` (the `assignments` object of a
    /// manifest written by an earlier `tutor-build` run), or a fresh
    /// assigner when `path` is `None`.
    ///
    /// # Errors
    /// [`BuildError::PreviousManifest`] when the file is unreadable, is not
    /// a `tutor-build` manifest, belongs to another family, or carries
    /// malformed assignments.
    pub fn load_previous(path: Option<&Path>, family: &str) -> Result<Self, BuildError> {
        let Some(path) = path else {
            return Ok(Self::fresh());
        };
        let fail = |reason: String| BuildError::PreviousManifest {
            path: path.to_owned(),
            reason,
        };
        let text = std::fs::read_to_string(path).map_err(|e| fail(format!("cannot read: {e}")))?;
        let manifest: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| fail(format!("not JSON: {e}")))?;
        if manifest["tool"]["name"].as_str() != Some("tutor-build") {
            return Err(fail(
                "not a tutor-build split manifest (tool.name is not \"tutor-build\")".to_owned(),
            ));
        }
        if let Some(other) = manifest["family"].as_str()
            && other != family
        {
            return Err(fail(format!(
                "manifest is for family {other:?}, not {family:?}"
            )));
        }
        let assignments = &manifest["assignments"];
        if assignments.is_null() {
            return Err(fail("manifest has no assignments record".to_owned()));
        }
        Ok(Self {
            previous_rows: parse_assignments(&assignments["rows"])
                .map_err(|e| fail(format!("assignments.rows: {e}")))?,
            previous_problems: parse_assignments(&assignments["problems"])
                .map_err(|e| fail(format!("assignments.problems: {e}")))?,
            current: Assignments::default(),
            conflicts: Vec::new(),
        })
    }

    /// The split for a row: frozen by a previous row assignment, then by a
    /// problem assignment (current release first, then previous), else a
    /// deterministic hash draw. The row-level freeze wins over the
    /// problem-level rule because "a row already assigned never moves" is
    /// the explicit contract. When a frozen row split disagrees with the
    /// split its problem resolves to, the row still keeps its frozen split
    /// and the disagreement is recorded in [`Assigner::conflicts`]: the
    /// build fails rather than shipping one problem in two splits.
    pub fn assign(&mut self, row_id: &str, problem_id: &str) -> Split {
        let problem_split = self
            .current
            .problems
            .get(problem_id)
            .or_else(|| self.previous_problems.get(problem_id))
            .copied();
        let row_split = self.previous_rows.get(row_id).copied();
        if let (Some(row_split), Some(problem_split)) = (row_split, problem_split)
            && row_split != problem_split
        {
            self.conflicts.push(format!(
                "row {row_id:?} is frozen to {} but problem {problem_id:?} resolves to {}",
                row_split.as_str(),
                problem_split.as_str()
            ));
        }
        let split = row_split
            .or(problem_split)
            .unwrap_or_else(|| hash_split(problem_id));
        self.current
            .problems
            .entry(problem_id.to_owned())
            .or_insert(split);
        self.current.rows.insert(row_id.to_owned(), split);
        split
    }

    /// Whether a previous release froze `row_id` to a split. Callers assign
    /// frozen rows before unfrozen ones so a new row's provisional hash draw
    /// never becomes a constraint a frozen row then conflicts with.
    pub fn is_frozen_row(&self, row_id: &str) -> bool {
        self.previous_rows.contains_key(row_id)
    }

    /// Frozen constraints that could not all be satisfied. Non-empty means
    /// some row kept a frozen split that disagrees with its problem's split;
    /// publishing either silently moves a frozen row or splits one problem.
    pub fn conflicts(&self) -> &[String] {
        &self.conflicts
    }

    /// This release's assignments, for the manifest.
    pub fn assignments(&self) -> &Assignments {
        &self.current
    }
}

/// Parse a `{id: "split"}` object from a previous manifest.
fn parse_assignments(value: &serde_json::Value) -> Result<HashMap<String, Split>, String> {
    let mut assignments = HashMap::new();
    let Some(map) = value.as_object() else {
        return Err("expected an object mapping ids to splits".to_owned());
    };
    for (id, split) in map {
        let split: Split =
            serde_json::from_value(split.clone()).map_err(|e| format!("{id:?}: {e}"))?;
        assignments.insert(id.clone(), split);
    }
    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_split_is_deterministic() {
        assert_eq!(hash_split("problem-a"), hash_split("problem-a"));
        assert_eq!(hash_split(""), hash_split(""));
    }

    #[test]
    fn hash_split_covers_all_buckets() {
        let mut seen = std::collections::BTreeSet::new();
        for index in 0..500 {
            seen.insert(hash_split(&format!("problem-{index}")));
        }
        assert!(seen.contains(&Split::Train));
        assert!(seen.contains(&Split::Validation));
        assert!(seen.contains(&Split::Test));
    }

    #[test]
    fn assigner_freezes_previous_rows() {
        let mut first = Assigner::fresh();
        let problem = "problem-freeze";
        first.assign("row-a", problem);
        let previous = Assigner {
            previous_rows: HashMap::from([("row-a".to_owned(), Split::Test)]),
            previous_problems: HashMap::new(),
            current: Assignments::default(),
            conflicts: Vec::new(),
        };
        let mut assigner = previous;
        // The frozen row keeps its split even though the hash says otherwise.
        assert_eq!(assigner.assign("row-a", problem), Split::Test);
        // A new row for the same problem follows the frozen problem split.
        assert_eq!(assigner.assign("row-b", problem), Split::Test);
        assert_eq!(
            assigner.assignments().problems.get(problem),
            Some(&Split::Test)
        );
    }

    #[test]
    fn assigner_freezes_previous_problems() {
        let mut assigner = Assigner {
            previous_rows: HashMap::new(),
            previous_problems: HashMap::from([("p".to_owned(), Split::Validation)]),
            current: Assignments::default(),
            conflicts: Vec::new(),
        };
        assert_eq!(assigner.assign("row-x", "p"), Split::Validation);
        assert_eq!(assigner.assign("row-y", "p"), Split::Validation);
    }

    #[test]
    fn assigner_flags_conflicting_frozen_splits() {
        // Version 1 froze row-a to train and row-b (problem p) to test.
        // Version 2 repoints row-a at p: the freezes cannot both hold.
        let mut assigner = Assigner {
            previous_rows: HashMap::from([
                ("row-a".to_owned(), Split::Train),
                ("row-b".to_owned(), Split::Test),
            ]),
            previous_problems: HashMap::from([("p".to_owned(), Split::Test)]),
            current: Assignments::default(),
            conflicts: Vec::new(),
        };
        assert_eq!(assigner.assign("row-b", "p"), Split::Test);
        assert_eq!(assigner.assign("row-a", "p"), Split::Train);
        assert_eq!(assigner.conflicts().len(), 1);
        assert!(assigner.conflicts()[0].contains("row-a"));
    }
}
