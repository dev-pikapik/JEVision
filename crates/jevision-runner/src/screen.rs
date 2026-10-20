//! One run at a time moves the mouse and types on this Mac. The app, the command line and the
//! assistant's server share one lock file; a run that acts on the screen holds it from its first
//! block to its last, and the file says who has it, so whoever waits can tell the person.
//!
//! The lock is a `flock` on the file: the system lets go of it when the process holding it
//! ends, however it ends, so a crash never leaves the screen taken.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jevision_core::{Batch, RunMode};
use serde::{Deserialize, Serialize};

/// Who has the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenHolder {
    /// The program, such as `JEVision` or `jevision`.
    pub process: String,
    pub pid: u32,
    /// Name of the test that runs.
    pub test: String,
    /// When the run started, in seconds since 1970.
    pub since: u64,
}

impl ScreenHolder {
    /// This process, running `test` from now on.
    pub fn this_process(test: &str) -> Self {
        let process = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.file_stem().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "JEVision".to_owned());
        Self {
            process,
            pid: std::process::id(),
            test: test.to_owned(),
            since: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// For people: `Screen busy: JEVision, test Sign in, since 12:03.`
pub fn describe_screen_holder(holder: Option<&ScreenHolder>) -> String {
    let Some(holder) = holder else {
        return "Screen busy: another run is using it.".to_owned();
    };
    let time = i64::try_from(holder.since)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string());
    match time {
        Some(time) => format!(
            "Screen busy: {}, test {}, since {time}.",
            holder.process, holder.test
        ),
        None => format!("Screen busy: {}, test {}.", holder.process, holder.test),
    }
}

/// The screen is yours while this exists. Dropping it, or the process ending, gives it back.
#[derive(Debug)]
pub struct ScreenLock {
    _file: File,
}

impl ScreenLock {
    /// Takes the screen for `me`, or tells who has it: `Err(None)` when the file does not say.
    pub fn try_take(
        path: &Path,
        me: &ScreenHolder,
    ) -> io::Result<Result<Self, Option<ScreenHolder>>> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if !lock(&file)? {
            let mut text = String::new();
            let _ = file.read_to_string(&mut text);
            return Ok(Err(serde_json::from_str(&text).ok()));
        }
        file.set_len(0)?;
        file.write_all(
            serde_json::to_string(me)
                .map_err(io::Error::other)?
                .as_bytes(),
        )?;
        Ok(Ok(Self { _file: file }))
    }
}

/// Locks `file` for this open file, which also keeps out the other runs of this process.
/// Returns whether it got the lock.
#[cfg(unix)]
fn lock(file: &File) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    // SAFETY: `flock` only reads the descriptor, which `file` keeps open.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error)
    }
}

/// Where this platform's runner will keep its own lock; until then every run gets the screen.
#[cfg(not(unix))]
fn lock(_: &File) -> io::Result<bool> {
    Ok(true)
}

/// The lock file every JEVision program on this Mac shares:
/// `~/Library/Application Support/JEVision/screen.lock`.
pub fn default_screen_lock() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support/JEVision")
            .join("screen.lock"),
    )
}

/// Whether the test `file` of `project` acts on the screen or looks at it, itself or through a
/// test it runs. Tests of commands, requests and files alone leave the screen to others.
pub(crate) fn uses_screen(project: &Path, file: &str) -> bool {
    let batch = Batch::new(project, &[file.to_owned()], RunMode::Sequential);
    batch
        .position(file)
        .is_none_or(|i| batch.tests()[i].uses_screen)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    use super::*;

    fn lock_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-screen-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("nested/screen.lock")
    }

    fn holder(test: &str) -> ScreenHolder {
        ScreenHolder {
            process: "JEVision".to_owned(),
            pid: 7,
            test: test.to_owned(),
            since: 1_700_000_000,
        }
    }

    #[test]
    fn the_second_taker_learns_who_has_the_screen_and_gets_it_after() {
        let path = lock_file("threads");
        let first = holder("Sign in");
        let taken = ScreenLock::try_take(&path, &first).unwrap().unwrap();
        let second = std::thread::spawn({
            let path = path.clone();
            move || {
                ScreenLock::try_take(&path, &holder("Checkout"))
                    .unwrap()
                    .err()
            }
        })
        .join()
        .unwrap();
        assert_eq!(second, Some(Some(first)));
        drop(taken);
        assert!(
            ScreenLock::try_take(&path, &holder("Checkout"))
                .unwrap()
                .is_ok()
        );
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_holder_that_dies_gives_the_screen_back() {
        let path = lock_file("process");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Another process, which holds the lock the way any other JEVision program would.
        let mut other = Command::new("perl")
            .args([
                "-e",
                r#"use Fcntl qw(:flock); $| = 1; open(F, ">>", $ARGV[0]) or die; flock(F, LOCK_EX) or die; print "held\n"; sleep 30"#,
            ])
            .arg(&path)
            .stdout(Stdio::piped())
            .spawn()
            .expect("perl starts");
        let mut line = String::new();
        BufReader::new(other.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "held\n");

        let busy = ScreenLock::try_take(&path, &holder("Checkout")).unwrap();
        assert_eq!(busy.err(), Some(None), "taken, and the file says nobody");

        other.kill().unwrap();
        other.wait().unwrap();
        assert!(
            ScreenLock::try_take(&path, &holder("Checkout"))
                .unwrap()
                .is_ok()
        );
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn the_holder_is_described_for_people() {
        let text = describe_screen_holder(Some(&holder("Sign in")));
        assert!(
            text.starts_with("Screen busy: JEVision, test Sign in, since "),
            "{text}"
        );
        assert!(text.ends_with('.'));
        assert_eq!(
            describe_screen_holder(None),
            "Screen busy: another run is using it."
        );
    }
}
