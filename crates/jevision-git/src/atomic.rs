//! Writing a file in one step: readers see the old contents or the new ones, never a half-written
//! file, and a write that fails midway leaves the old file as it was.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes `bytes` to `path` through a temporary file next to it, then renames it over the old one.
/// The file keeps its permissions, and a symbolic link keeps pointing where it did.
pub(crate) fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let path = match fs::canonicalize(path) {
        Ok(real) => real,
        Err(e) if e.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(e) => return Err(e),
    };
    let temporary = temporary(&path);
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Ok(old) = fs::metadata(&path) {
            file.set_permissions(old.permissions())?;
        }
        file.write_all(bytes.as_ref())?;
        file.sync_all()?;
        fs::rename(&temporary, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// A name no other write uses, hidden, in the same folder so the rename stays on one disk.
fn temporary(path: &Path) -> PathBuf {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let unique = format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    );
    path.with_file_name(unique)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-git-atomic-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_failed_write_keeps_the_old_file() {
        let dir = folder("fail");
        let file = dir.join("login.flow.json");
        fs::write(&file, "old").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        let result = write(&file, "new");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "old");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_keeps_pointing_at_the_written_file() {
        let dir = folder("link");
        let file = dir.join("real.flow.json");
        fs::write(&file, "old").unwrap();
        let link = dir.join("link.flow.json");
        symlink(&file, &link).unwrap();
        write(&link, "new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        write(&dir.join("new.flow.json"), "made").unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("new.flow.json")).unwrap(),
            "made"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
        let _ = fs::remove_dir_all(&dir);
    }
}
