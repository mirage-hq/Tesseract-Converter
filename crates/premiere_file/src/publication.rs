//! Publication of staged output files into a directory that this run created.
//!
//! A hard link publishes a staged file without copying its bytes. Where the
//! output filesystem cannot link, such as FAT32, exFAT and many SMB shares,
//! the file is copied instead. Neither path replaces or follows an existing
//! entry. Neither makes publication atomic: a copy is visible under its final
//! name while it is written.

use std::{
    fs::{self, File},
    io::{self, ErrorKind},
    path::Path,
};

/// Creates `target` with the bytes and permissions of the staged file
/// `staged`. `link` is `fs::hard_link` outside fault-injection tests.
///
/// Filesystems without hard links refuse them inconsistently: Linux FAT and
/// exFAT drivers report `EPERM`, and macOS reports `ENOTSUP`, which the
/// standard library leaves uncategorized. So any link failure other than an
/// existing target or a missing path falls back to an exclusive copy, and a
/// genuine failure, such as a full disk or an unreadable staged file, is
/// reported by the copy.
pub(crate) fn publish_file<'a>(
    staged: &'a Path,
    target: &'a Path,
    link: impl FnOnce(&'a Path, &'a Path) -> io::Result<()>,
) -> io::Result<()> {
    let Err(refusal) = link(staged, target) else {
        return Ok(());
    };
    if matches!(
        refusal.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotFound
    ) {
        return Err(refusal);
    }
    // Open the staged file before creating anything.
    let mut source = File::open(staged)?;
    let permissions = source.metadata()?.permissions();
    create_new_with(target, |output| {
        // A hard link shares the staged permissions; the copy takes them
        // before it holds any content.
        output.set_permissions(permissions)?;
        io::copy(&mut source, output)?;
        // Staged files are synced before they are published; so is a copy.
        output.sync_all()
    })
}

/// Creates `target`, which must not exist, and writes it with `fill`. A
/// failure after the creation removes that partial file and nothing else.
fn create_new_with(
    target: &Path,
    fill: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let mut output = File::create_new(target)?;
    let written = fill(&mut output);
    // Close the file first; Windows cannot remove an open file.
    drop(output);
    if written.is_err() {
        // Best effort: report the failure that stopped the copy.
        let _ = fs::remove_file(target);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{ErrorKind, Write},
        path::PathBuf,
    };

    /// A staged file in a fresh directory, and the absent target beside it.
    fn staged(bytes: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("staged.bin");
        fs::write(&staged, bytes).unwrap();
        let target = root.path().join("target.bin");
        (root, staged, target)
    }

    /// The link of a filesystem with hard links, or of one without them.
    fn link(supported: bool) -> impl FnOnce(&Path, &Path) -> io::Result<()> {
        move |staged: &Path, target: &Path| {
            if supported {
                fs::hard_link(staged, target)
            } else {
                Err(ErrorKind::Unsupported.into())
            }
        }
    }

    fn absent(path: &Path) -> bool {
        fs::symlink_metadata(path).is_err_and(|error| error.kind() == ErrorKind::NotFound)
    }

    #[test]
    fn a_filesystem_without_hard_links_gets_an_exact_copy() {
        let bytes: Vec<u8> = (0..=u8::MAX).cycle().take(100_000).collect();
        // Linux FAT and exFAT drivers refuse links with EPERM; errno 45 is
        // Darwin's ENOTSUP, which the standard library leaves uncategorized.
        for refusal in [
            io::Error::from(ErrorKind::Unsupported),
            io::Error::from(ErrorKind::PermissionDenied),
            io::Error::from_raw_os_error(45),
        ] {
            let (_root, staged, target) = staged(&bytes);
            let refused = refusal.to_string();
            publish_file(&staged, &target, |_, _| Err(refusal)).unwrap();
            assert_eq!(fs::read(&target).unwrap(), bytes, "{refused}");
            assert_eq!(fs::read(&staged).unwrap(), bytes, "{refused}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_keeps_the_staged_permissions() {
        use std::os::unix::fs::PermissionsExt;
        // A hard link shares the staged file's mode, such as the 0600 of a
        // staged archive. Two modes, so that no process umask can make a
        // freshly created file match both by chance.
        for mode in [0o600, 0o640] {
            let (_root, staged, target) = staged(b"staged");
            fs::set_permissions(&staged, fs::Permissions::from_mode(mode)).unwrap();
            publish_file(&staged, &target, link(false)).unwrap();
            let copied = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(copied, mode, "{mode:o}");
            assert_eq!(fs::read(&target).unwrap(), b"staged");
        }
    }

    #[test]
    fn an_existing_target_is_never_replaced() {
        for supported in [true, false] {
            let (_root, staged, target) = staged(b"staged");
            fs::write(&target, b"foreign").unwrap();
            let error = publish_file(&staged, &target, link(supported)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::AlreadyExists, "links {supported}");
            assert_eq!(fs::read(&target).unwrap(), b"foreign");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_target_is_neither_replaced_nor_followed() {
        for supported in [true, false] {
            let (root, staged, target) = staged(b"staged");
            let foreign = root.path().join("foreign.bin");
            fs::write(&foreign, b"foreign").unwrap();
            let dangling = root.path().join("absent.bin");
            for pointee in [&foreign, &dangling] {
                std::os::unix::fs::symlink(pointee, &target).unwrap();
                let error = publish_file(&staged, &target, link(supported)).unwrap_err();
                assert_eq!(error.kind(), ErrorKind::AlreadyExists, "links {supported}");
                assert_eq!(&fs::read_link(&target).unwrap(), pointee);
                fs::remove_file(&target).unwrap();
            }
            assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
            assert!(absent(&dangling), "links {supported}");
        }
    }

    #[test]
    fn a_staged_file_that_cannot_be_opened_creates_nothing() {
        for supported in [true, false] {
            let (_root, staged, target) = staged(b"staged");
            fs::remove_file(&staged).unwrap();
            let error = publish_file(&staged, &target, link(supported)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::NotFound, "links {supported}");
            assert!(absent(&target), "links {supported}");
        }
    }

    #[test]
    fn a_failed_copy_or_sync_removes_only_its_partial_file() {
        let root = tempfile::tempdir().unwrap();
        let foreign = root.path().join("foreign.bin");
        fs::write(&foreign, b"foreign").unwrap();
        let target = root.path().join("target.bin");
        // A full disk during the copy, and a failed sync after it.
        let copy: fn(&mut File) -> io::Result<()> = |output| {
            output.write_all(b"partial")?;
            Err(ErrorKind::StorageFull.into())
        };
        let sync: fn(&mut File) -> io::Result<()> = |output| {
            output.write_all(b"complete")?;
            Err(io::Error::other("sync failed"))
        };
        for (fill, kind) in [(copy, ErrorKind::StorageFull), (sync, ErrorKind::Other)] {
            let error = create_new_with(&target, fill).unwrap_err();
            assert_eq!(error.kind(), kind);
            assert!(absent(&target), "{kind}");
        }
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
