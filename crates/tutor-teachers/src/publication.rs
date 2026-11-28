//! Publish a complete replay directory without replacing an existing destination.
use std::path::Path;

use crate::Error;

pub(crate) fn publish_replay(
    out: &Path,
    write_artifacts: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    // Reject roots and dot paths before the atomic-move helper requires a leaf.
    out.file_name()
        .ok_or("replay output must name a new directory")?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staged = tempfile::Builder::new()
        .prefix(".tutor-replay-")
        .tempdir_in(parent)?;
    write_artifacts(staged.path())?;
    // Same filesystem, one no-replace rename. In particular, an empty existing
    // directory (even one created during staging) must not be overwritten.
    atomicwrites::move_atomic(staged.path(), out)?;
    // The old staging path no longer exists; do not attempt to remove it on drop.
    let _ = staged.keep();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_io_error_leaves_no_output_and_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("replay");
        let result = publish_replay(&out, |staged| {
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
        publish_replay(&out, |staged| {
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
            let result = publish_replay(&out, |staged| {
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
}
