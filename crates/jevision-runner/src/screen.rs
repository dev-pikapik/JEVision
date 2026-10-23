//! One run at a time moves the mouse and types on this Mac. The app, the command line and the
//! assistant's server share one lock file; a run that acts on the screen holds it from its first
//! block to its last, and the file says who has it, so whoever waits can tell the person.
//!
//! The lock is a `flock` on the file: the system lets go of it when the process holding it
//! ends, however it ends, so a crash never leaves the screen taken.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jevision_core::{Batch, RunMode};
use jevision_l10n::tr_in;
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

/// For people: `Screen busy: JEVision, test Sign in, since 12:03`, in the language set with
/// `jevision_l10n::set_language`.
pub fn describe_screen_holder(holder: Option<&ScreenHolder>) -> String {
    describe_screen_holder_in(&jevision_l10n::language(), holder)
}

/// [`describe_screen_holder`] in `lang`.
pub fn describe_screen_holder_in(lang: &str, holder: Option<&ScreenHolder>) -> String {
    let Some(holder) = holder else {
        return tr_in!(lang, "screen.busy.unknown");
    };
    let time = i64::try_from(holder.since)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string());
    match time {
        Some(time) => tr_in!(lang, "screen.busy", &holder.process, &holder.test, time),
        None => tr_in!(lang, "screen.busy.process", &holder.process, &holder.test),
    }
}

/// The screen is yours while this exists. Dropping it, or the process ending, gives it back.
#[derive(Debug)]
pub struct ScreenLock {
    file: File,
}

impl Drop for ScreenLock {
    /// Lets go of the lock before the file closes: a process this program starts at that moment
    /// shares the open file until it begins, and the lock would last as long as that.
    fn drop(&mut self) {
        unlock(&self.file);
    }
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
        if !lock(&file, false)? {
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
        Ok(Ok(Self { file }))
    }
}

/// Locks `file` for this open file, which also keeps out the other runs of this process; with
/// `wait`, as soon as whoever has it lets go. Returns whether it got the lock.
#[cfg(unix)]
pub(crate) fn lock(file: &File, wait: bool) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    let how = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    loop {
        // SAFETY: `flock` only reads the descriptor, which `file` keeps open.
        if unsafe { libc::flock(file.as_raw_fd(), how) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        match error.kind() {
            io::ErrorKind::Interrupted => {}
            io::ErrorKind::WouldBlock => return Ok(false),
            _ => return Err(error),
        }
    }
}

/// Where this platform's runner will keep its own lock; until then every run gets the screen,
/// and only the runs of one process take turns with the last results.
#[cfg(not(unix))]
pub(crate) fn lock(_: &File, _: bool) -> io::Result<bool> {
    Ok(true)
}

#[cfg(unix)]
pub(crate) fn unlock(file: &File) {
    use std::os::fd::AsRawFd;
    // SAFETY: `flock` only reads the descriptor, which `file` keeps open.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[cfg(not(unix))]
pub(crate) fn unlock(_: &File) {}

/// The lock file every JEVision program on this Mac shares:
/// `~/Library/Application Support/JEVision/screen.lock`. A program that tests JEVision without
/// driving the screen names a lock of its own in `JEVISION_SCREEN_LOCK`, and so does not wait for
/// the runs of other programs, nor they for it.
pub fn default_screen_lock() -> Option<PathBuf> {
    screen_lock_in(
        std::env::var_os("JEVISION_SCREEN_LOCK"),
        std::env::var_os("HOME"),
    )
}

fn screen_lock_in(named: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(named) = named.filter(|n| !n.is_empty()) {
        return Some(PathBuf::from(named));
    }
    let home = home.filter(|h| !h.is_empty())?;
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
    fn the_lock_is_the_one_in_the_library_unless_a_program_names_its_own() {
        let home = || Some(OsString::from("/Users/me"));
        assert_eq!(
            screen_lock_in(None, home()),
            Some(PathBuf::from(
                "/Users/me/Library/Application Support/JEVision/screen.lock"
            ))
        );
        assert_eq!(
            screen_lock_in(Some(OsString::from("/tmp/mine.lock")), home()),
            Some(PathBuf::from("/tmp/mine.lock"))
        );
        assert_eq!(
            screen_lock_in(Some(OsString::from("/tmp/mine.lock")), None),
            Some(PathBuf::from("/tmp/mine.lock"))
        );
        // An empty name is no name, as an empty home is no home.
        assert_eq!(
            screen_lock_in(Some(OsString::new()), home()),
            screen_lock_in(None, home())
        );
        assert_eq!(screen_lock_in(None, Some(OsString::new())), None);
        assert_eq!(screen_lock_in(None, None), None);
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

    /// A process another thread starts shares every open file until it begins, so it shares the
    /// lock's open file too: the screen has to come back at once all the same, and not when
    /// that process has begun.
    #[test]
    fn the_screen_comes_back_while_a_starting_process_still_shares_the_file() {
        let path = lock_file("shared");
        let taken = ScreenLock::try_take(&path, &holder("Sign in"))
            .unwrap()
            .unwrap();
        let shared = taken.file.try_clone().unwrap();
        drop(taken);
        assert!(
            ScreenLock::try_take(&path, &holder("Checkout"))
                .unwrap()
                .is_ok(),
            "the screen is still taken after its holder let go"
        );
        drop(shared);
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
    fn the_holder_is_described_for_people_in_each_language() {
        let who = holder("Sign in");
        let en = describe_screen_holder_in("en", Some(&who));
        assert!(
            en.starts_with("Screen busy: JEVision, test Sign in, since "),
            "{en}"
        );
        let ru = describe_screen_holder_in("ru", Some(&who));
        assert!(
            ru.starts_with("Экран занят: JEVision, тест Sign in, с "),
            "{ru}"
        );
        assert_eq!(
            describe_screen_holder_in("en", None),
            "Screen busy: another run is using it."
        );
        assert_eq!(
            describe_screen_holder_in("ru", None),
            "Экран занят: его использует другой прогон."
        );
    }
}
