//! Publish a complete artifact directory without replacing an existing destination.
use std::path::Path;

use crate::Error;

pub(crate) fn publish_artifacts(
    out: &Path,
    write_artifacts: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    publish_with_move(out, write_artifacts, move_directory_new)
}

fn publish_with_move(
    out: &Path,
    write_artifacts: impl FnOnce(&Path) -> Result<(), Error>,
    move_directory: fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), Error> {
    // Require an output leaf rather than a root or dot path.
    out.file_name()
        .ok_or("artifact output must name a new directory")?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staged = tempfile::Builder::new()
        .prefix(".tutor-artifacts-")
        .tempdir_in(parent)?;
    write_artifacts(staged.path())?;
    // Same filesystem, one no-replace rename. In particular, an empty existing
    // directory (even one created during staging) must not be overwritten.
    move_directory(staged.path(), out)?;
    // The old staging path no longer exists; do not attempt to remove it on drop.
    let _ = staged.keep();
    Ok(())
}

#[cfg(target_os = "linux")]
fn move_directory_new(staged: &Path, out: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    // Path-based, no listing permission or process-global capability cache.
    // Unsupported filesystems fail this call without affecting later attempts.
    renameat_with(CWD, staged, CWD, out, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(windows)]
fn move_directory_new(staged: &Path, out: &Path) -> std::io::Result<()> {
    // The Windows implementation calls MoveFileExW with WRITE_THROUGH only:
    // no REPLACE_EXISTING, COPY_ALLOWED, parent read-open or capability cache.
    atomicwrites::move_atomic(staged, out)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn move_directory_new(staged: &Path, out: &Path) -> std::io::Result<()> {
    unsupported_move(staged, out)
}

#[cfg(any(test, not(any(target_os = "linux", windows))))]
fn unsupported_move(_staged: &Path, _out: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "artifact publication requires Linux or Windows no-replace directory rename",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn staging_io_error_leaves_no_output_and_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("replay");
        let result = publish_artifacts(&out, |staged| {
            std::fs::write(staged.join("candidate.json"), b"candidate")?;
            assert!(
                !out.exists(),
                "candidate must not be visible at the destination"
            );
            // An ordinary filesystem error on the second artifact, after the
            // first succeeds. No process-global permissions or disk filling.
            std::fs::create_dir(staged.join("report.json"))?;
            std::fs::write(staged.join("report.json"), b"report")?;
            Ok(())
        });
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<std::io::Error>()
                .is_some()
        );
        assert!(!out.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        publish_artifacts(&out, |staged| {
            std::fs::write(staged.join("candidate.json"), b"candidate")?;
            std::fs::write(staged.join("report.json"), b"report")?;
            assert!(!out.exists());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            std::fs::read(out.join("candidate.json")).unwrap(),
            b"candidate"
        );
        assert_eq!(std::fs::read(out.join("report.json")).unwrap(), b"report");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn destination_created_during_staging_is_never_replaced() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let out = root.path().join("occupied");
            let result = publish_artifacts(&out, |staged| {
                std::fs::write(staged.join("candidate.json"), b"candidate")?;
                std::fs::write(staged.join("report.json"), b"report")?;
                if directory {
                    std::fs::create_dir(&out)?;
                } else {
                    std::fs::write(&out, b"original")?;
                }
                Ok(())
            });
            assert!(result.is_err());
            if directory {
                assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
            } else {
                assert_eq!(std::fs::read(&out).unwrap(), b"original");
            }
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn invalid_rename_does_not_disable_later_publication() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("staged");
        std::fs::create_dir(&staged).unwrap();
        // EINVAL also occurs on filesystems without NOREPLACE support. Trigger
        // the same errno locally, without requiring a second mount or privileges.
        let error = move_directory_new(&staged, &staged.join("child")).unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(rustix::io::Errno::INVAL.raw_os_error())
        );
        let out = root.path().join("published");
        move_directory_new(&staged, &out).unwrap();
        assert!(out.is_dir());
        assert!(!staged.exists());
    }

    #[test]
    fn unsupported_platform_does_not_publish_staged_files() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        let error = publish_with_move(
            &out,
            |staged| {
                std::fs::write(staged.join("candidate.json"), b"candidate")?;
                std::fs::write(staged.join("report.json"), b"report")?;
                Ok(())
            },
            unsupported_move,
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
