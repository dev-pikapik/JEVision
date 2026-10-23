//! Empty home folders for apps a test starts as on their first launch. They live in
//! `jevision-homes` in this user's temporary folder, never in the person's own home folder, each
//! named after the process that made it. A run removes its own when it ends; one that ends
//! without doing so, because it crashed or was killed, leaves them to the next run, which
//! removes every folder whose process is gone.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Name of the folder, in the temporary folder, that holds the home folders.
const HOMES: &str = "jevision-homes";

/// An empty home folder made for `app`; removed with everything the app wrote into it when
/// dropped.
#[derive(Debug)]
pub(crate) struct CleanHome {
    pub app: String,
    pub path: PathBuf,
}

impl Drop for CleanHome {
    fn drop(&mut self) {
        // Links the app made are removed, not followed.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Where the home folders of every run on this computer are made.
pub(crate) fn homes_folder() -> PathBuf {
    std::env::temp_dir().join(HOMES)
}

/// Makes a new empty home folder for `app`, readable only by this user.
pub(crate) fn make(app: &str) -> io::Result<CleanHome> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = homes_folder();
    private_dir(&root, true)?;
    let pid = std::process::id();
    loop {
        let path = root.join(format!("{pid}-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        match private_dir(&path, false) {
            Ok(()) => {
                return Ok(CleanHome {
                    app: app.to_owned(),
                    path,
                });
            }
            // Left by an earlier process with the same number that is gone.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Removes the home folders of processes that are gone; those of runs still going stay.
pub(crate) fn sweep() {
    sweep_in(&homes_folder());
}

fn sweep_in(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let owner = name
            .to_str()
            .and_then(|n| n.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<u32>().ok());
        if owner.is_some_and(|pid| !alive(pid)) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(unix)]
pub(crate) fn private_dir(path: &Path, recursive: bool) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(recursive)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
pub(crate) fn private_dir(path: &Path, recursive: bool) -> io::Result<()> {
    fs::DirBuilder::new().recursive(recursive).create(path)
}

/// Whether the process `pid` is running. A process of another user counts as running.
#[cfg(unix)]
pub(crate) fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process exists; nothing is sent.
    let exists = unsafe { libc::kill(pid, 0) } == 0;
    exists || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Where this platform can't tell, every folder is taken to be in use.
#[cfg(not(unix))]
pub(crate) fn alive(_: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_is_empty_private_and_goes_when_dropped() {
        let home = make("com.example.App").unwrap();
        assert!(home.path.starts_with(homes_folder()));
        assert_eq!(fs::read_dir(&home.path).unwrap().count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&home.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let other = make("com.example.App").unwrap();
        assert_ne!(home.path, other.path);
        fs::create_dir_all(home.path.join("Library/Preferences")).unwrap();
        fs::write(home.path.join("Library/Preferences/x.plist"), "x").unwrap();
        let path = home.path.clone();
        drop(home);
        assert!(!path.exists());
        assert!(other.path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_next_run_removes_what_a_gone_process_left() {
        let root = std::env::temp_dir().join(format!("jevision-home-sweep-{}", std::process::id()));
        let gone = std::process::Command::new("true").spawn().unwrap();
        let gone_pid = gone.id();
        let mut gone = gone;
        gone.wait().unwrap();
        let left = root.join(format!("{gone_pid}-0"));
        let ours = root.join(format!("{}-7", std::process::id()));
        let other = root.join("notes");
        for dir in [&left, &ours, &other] {
            fs::create_dir_all(dir.join("Library")).unwrap();
        }
        sweep_in(&root);
        assert!(!left.exists());
        assert!(ours.exists());
        assert!(other.exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
