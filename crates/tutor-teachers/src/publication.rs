//! Publish complete files or artifact pairs without replacing existing destinations.
use std::path::Path;

use crate::Error;

pub(crate) fn publish_file(
    out: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), Error> {
    #[cfg(windows)]
    let out = windows_extended_path(out)?;
    #[cfg(not(windows))]
    let out = out.to_path_buf();
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut builder = tempfile::Builder::new();
    builder.prefix(".tutor-handoff-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Match ordinary file creation; tempfile applies the caller's umask.
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let mut staged = builder.tempfile_in(parent)?;
    write(staged.as_file_mut())?;
    // Close before publication, including on Windows. Portable no-clobber file
    // persistence does not need the directory-rename support required for pairs.
    staged.into_temp_path().persist_noclobber(&out)?;
    Ok(())
}

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
    // MoveFileExW with WRITE_THROUGH only: no REPLACE_EXISTING or COPY_ALLOWED.
    // atomicwrites only encodes arguments as UTF-16, so normalize both first.
    let staged = windows_extended_path(staged)?;
    let out = windows_extended_path(out)?;
    atomicwrites::move_atomic(&staged, &out)
}

#[cfg(windows)]
fn windows_extended_path(path: &Path) -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Prefix};
    // GetFullPathNameW semantics normalize the entire ordinary path (including
    // trailing dots/spaces), without opening handles or querying ancestors.
    // Rust leaves explicit verbatim paths unchanged.
    let absolute = std::path::absolute(path)?;
    let Some(Component::Prefix(prefix)) = absolute.components().next() else {
        return Err(std::io::Error::other("Windows path has no absolute prefix"));
    };
    if prefix.kind().is_verbatim() {
        return Ok(absolute);
    }
    let (tag, skip) = match prefix.kind() {
        Prefix::Disk(_) => (r"\\?\", 0),
        Prefix::UNC(..) => (r"\\?\UNC\", 2),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "artifact paths must use a drive or UNC share, not a device namespace",
            ));
        }
    };
    let wide: Vec<u16> = tag
        .encode_utf16()
        .chain(absolute.as_os_str().encode_wide().skip(skip))
        .collect();
    Ok(std::ffi::OsString::from_wide(&wide).into())
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
    use std::io::Write;

    #[test]
    fn partial_file_write_is_not_published_and_retry_preserves_complete_output() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("request.json");
        let result = publish_file(&out, |file| {
            file.write_all(b"{\"version\":")?;
            assert!(!out.exists());
            Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "injected short write",
            ))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        let complete = b"{\"version\":2}";
        publish_file(&out, |file| file.write_all(complete)).unwrap();
        assert!(publish_file(&out, |file| file.write_all(b"replacement")).is_err());
        assert_eq!(std::fs::read(&out).unwrap(), complete);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

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

    #[cfg(unix)]
    #[test]
    fn published_file_preserves_ordinary_creation_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let ordinary = root.path().join("ordinary");
        let published = root.path().join("published");
        std::fs::write(&ordinary, b"ordinary").unwrap();
        publish_file(&published, |file| file.write_all(b"published")).unwrap();
        assert_eq!(
            std::fs::metadata(published).unwrap().permissions().mode(),
            std::fs::metadata(ordinary).unwrap().permissions().mode()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_conversion_needs_no_existing_paths_or_network_access() {
        for (input, expected) in [
            (r"C:\missing\out. ", r"\\?\C:\missing\out"),
            (
                r"\\server.invalid\share\missing\out.",
                r"\\?\UNC\server.invalid\share\missing\out",
            ),
            (r"\\?\C:\missing\literal.", r"\\?\C:\missing\literal."),
            (
                r"\\?\UNC\server.invalid\share\literal. ",
                r"\\?\UNC\server.invalid\share\literal. ",
            ),
        ] {
            assert_eq!(
                windows_extended_path(Path::new(input)).unwrap(),
                Path::new(expected)
            );
        }
    }

    #[cfg(windows)]
    fn ordinary_windows_test_path(path: &Path) -> std::path::PathBuf {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use std::path::{Component, Prefix};
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let ordinary = match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => wide[4..].to_vec(),
                Prefix::VerbatimUNC(..) => r"\\"
                    .encode_utf16()
                    .chain(wide[8..].iter().copied())
                    .collect(),
                _ => wide,
            },
            _ => wide,
        };
        std::ffi::OsString::from_wide(&ordinary).into()
    }

    #[cfg(windows)]
    #[test]
    fn windows_ordinary_aliases_refuse_existing_outputs() {
        let root = tempfile::tempdir().unwrap();
        let ordinary_root = ordinary_windows_test_path(root.path());
        let directory = ordinary_root.join("replay");
        let file = ordinary_root.join("request");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(&file, b"original").unwrap();
        for suffix in [".", " "] {
            let mut directory_staged = false;
            let directory_result =
                publish_artifacts(&ordinary_root.join(format!("replay{suffix}")), |staged| {
                    std::fs::write(staged.join("candidate.json"), b"candidate")?;
                    std::fs::write(staged.join("report.json"), b"report")?;
                    directory_staged = true;
                    Ok(())
                });
            assert_eq!((directory_result.is_err(), directory_staged), (true, true));
            let mut file_staged = false;
            let file_result =
                publish_file(&ordinary_root.join(format!("request{suffix}")), |file| {
                    file.write_all(b"replacement")?;
                    file_staged = true;
                    Ok(())
                });
            assert_eq!((file_result.is_err(), file_staged), (true, true));
        }
        assert_eq!(std::fs::read(file).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn windows_long_staging_and_destination_paths_publish_without_overwrite() {
        use std::os::windows::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let ordinary_root = ordinary_windows_test_path(root.path());
        let padding = 240usize.saturating_sub(ordinary_root.as_os_str().encode_wide().count() + 1);
        let short_parent = ordinary_root.join("p".repeat(padding.max(1)));
        let long_parent = short_parent.join("q".repeat(80));
        std::fs::create_dir_all(&long_parent).unwrap();
        assert!(long_parent.as_os_str().encode_wide().count() > 260);
        // Exercise both a short destination with long staging, and a long
        // ordinary destination; also cover explicitly verbatim destinations.
        for parent in [short_parent, long_parent] {
            for out in [
                parent.join("out"),
                windows_extended_path(&parent.join("verbatim")).unwrap(),
            ] {
                publish_artifacts(&out, |staged| {
                    assert!(staged.as_os_str().encode_wide().count() > 260);
                    std::fs::write(staged.join("candidate.json"), b"candidate")?;
                    std::fs::write(staged.join("report.json"), b"report")?;
                    Ok(())
                })
                .unwrap();
                assert_eq!(
                    std::fs::read(out.join("candidate.json")).unwrap(),
                    b"candidate"
                );
                assert_eq!(std::fs::read(out.join("report.json")).unwrap(), b"report");
                assert!(publish_artifacts(&out, |_| Ok(())).is_err());
            }
            let file_out = parent.join("request");
            publish_file(&file_out, |file| file.write_all(b"request")).unwrap();
            assert_eq!(std::fs::read(file_out).unwrap(), b"request");
        }
    }
}
