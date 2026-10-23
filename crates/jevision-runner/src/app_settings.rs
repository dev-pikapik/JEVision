//! The settings of apps a test starts as on their first launch. With the app closed, a run
//! copies its settings (its `defaults` domain, and its settings for this Mac alone) into an
//! entry of a journal, then clears them; once the run has ended and the app has quit, it puts
//! them back and removes the entry. Entries that a run which crashed or was killed leaves go
//! back at the next run or the next launch of JEVision, as soon as their app is closed.
//!
//! Only `defaults` reads and writes the settings: `cfprefsd`, which keeps every app's settings
//! in memory, then never holds an old copy. The one file removed directly is the empty one an
//! app that had no settings is left with.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::home::{alive, private_dir};

/// An app's settings saved before a run cleared them: the file `<bundle id>.json` in the
/// journal folder, replaced whole, never written in place. It holds the copy itself, so no
/// entry ever points to a copy that is missing.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    app: String,
    /// The process whose run cleared the settings. `None` once that run has ended with the app
    /// still open: the settings go back at the next run or launch that finds it closed.
    pid: Option<u32>,
    /// The app's settings as `defaults export` writes them; `None` if it had none.
    settings: Option<String>,
    /// Its settings for this Mac alone, `defaults -currentHost`.
    host_settings: Option<String>,
}

/// The two places `defaults` keeps an app's settings in.
#[derive(Clone, Copy)]
enum Scope {
    Everywhere,
    ThisMac,
}

const SCOPES: [Scope; 2] = [Scope::Everywhere, Scope::ThisMac];

/// Where the JEVision app, the command line and the assistant's server keep the settings of
/// the apps their runs cleared: `~/Library/Application Support/JEVision/Saved app settings`.
pub fn default_saved_settings() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join("Library/Application Support/JEVision/Saved app settings"))
}

/// Saves the settings of `app`, which must be closed, in the journal `folder` and clears them.
/// When an earlier run that is over left them saved there, that copy is the person's and is
/// kept: what the app has now came from a test.
pub(crate) fn save_and_clear(folder: &Path, app: &str) -> io::Result<()> {
    check_bundle_id(app)?;
    let me = std::process::id();
    match read(folder, app)? {
        Some(entry) if entry.pid.is_some_and(|pid| pid != me && alive(pid)) => {
            return Err(io::Error::other(format!(
                "a run in process {} is resetting this app now",
                entry.pid.unwrap_or_default()
            )));
        }
        Some(entry) if entry.pid == Some(me) => {}
        Some(entry) => write(
            folder,
            &Entry {
                pid: Some(me),
                ..entry
            },
        )?,
        None => write(
            folder,
            &Entry {
                app: app.to_owned(),
                pid: Some(me),
                settings: export(app, Scope::Everywhere)?,
                host_settings: export(app, Scope::ThisMac)?,
            },
        )?,
    }
    SCOPES.into_iter().try_for_each(|scope| clear(app, scope))
}

/// Puts back the settings of `app` this process saved and removes their entry; `false` if the
/// app is still open, as when it asks whether to save changes, and they wait for the next run
/// or launch instead.
pub(crate) fn put_back(folder: &Path, app: &str) -> io::Result<bool> {
    let Some(entry) = read(folder, app)? else {
        return Ok(true);
    };
    if entry.pid != Some(std::process::id()) {
        return Ok(true);
    }
    let restored = !still_open(app) && restore(folder, &entry).is_ok();
    if !restored {
        write(folder, &Entry { pid: None, ..entry })?;
    }
    Ok(restored)
}

/// Puts back the settings that runs which are over left in the journal `folder`, of every app
/// that is closed; those of open apps wait for the next time.
pub fn put_back_left_settings(folder: &Path) {
    let Ok(files) = fs::read_dir(folder) else {
        return;
    };
    for file in files.flatten() {
        let path = file.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Some(entry) = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
        else {
            continue;
        };
        if !entry.pid.is_some_and(alive) && !running(&entry.app) {
            let _ = restore(folder, &entry);
        }
    }
}

/// Whether `app`, just asked to quit, is still open. Launch Services lists an app for a moment
/// after its process has gone, so this gives it a few seconds before saying yes.
pub(crate) fn still_open(app: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(app) {
        if Instant::now() >= deadline {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Whether `app` is open. When that can't be told it counts as open, so its settings wait.
fn running(app: &str) -> bool {
    Command::new("/usr/bin/lsappinfo")
        .args(["find", &format!("bundleid={app}")])
        .output()
        .map_or(true, |out| {
            !out.status.success() || !out.stdout.trim_ascii().is_empty()
        })
}

/// Puts the saved settings in place of the app's and removes the entry.
fn restore(folder: &Path, entry: &Entry) -> io::Result<()> {
    for (scope, saved) in [
        (Scope::Everywhere, &entry.settings),
        (Scope::ThisMac, &entry.host_settings),
    ] {
        clear(&entry.app, scope)?;
        match saved {
            Some(saved) => import(&entry.app, scope, saved)?,
            // An app that had no settings had no file for them either, and the empty one
            // `defaults delete` leaves would show it as having some. There is nothing in it
            // that `cfprefsd` could hold an old copy of.
            None => {
                for file in files(&entry.app, scope) {
                    let _ = fs::remove_file(file);
                }
            }
        }
    }
    fs::remove_file(file(folder, &entry.app))
}

/// The files `defaults` keeps the app's settings in `scope` in.
fn files(app: &str, scope: Scope) -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) else {
        return Vec::new();
    };
    let prefs = PathBuf::from(home).join("Library/Preferences");
    match scope {
        Scope::Everywhere => vec![prefs.join(format!("{app}.plist"))],
        // `<bundle id>.<this Mac's hardware UUID>.plist`.
        Scope::ThisMac => fs::read_dir(prefs.join("ByHost"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_prefix(app)?.strip_prefix('.'))
                    .and_then(|rest| rest.strip_suffix(".plist"))
                    .is_some_and(|uuid| {
                        uuid.len() == 36 && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                    })
            })
            .collect(),
    }
}

/// The app's settings in `scope`, or `None` if it has none.
fn export(app: &str, scope: Scope) -> io::Result<Option<String>> {
    let out = defaults(scope, &["export", app, "-"], None)?;
    let plist = String::from_utf8(out).map_err(io::Error::other)?;
    // Every setting has a `<key>`; settings that were all removed leave an empty `<dict/>`.
    Ok(plist.contains("<key>").then_some(plist))
}

fn import(app: &str, scope: Scope, plist: &str) -> io::Result<()> {
    defaults(scope, &["import", app, "-"], Some(plist)).map(drop)
}

/// Removes all of the app's settings in `scope`. `defaults import` adds to the settings an app
/// has, so the settings are cleared before every import too.
fn clear(app: &str, scope: Scope) -> io::Result<()> {
    if export(app, scope)?.is_some() {
        defaults(scope, &["delete", app], None)?;
    }
    Ok(())
}

/// Runs `/usr/bin/defaults` with `args`, `input` on its standard input; what it printed.
fn defaults(scope: Scope, args: &[&str], input: Option<&str>) -> io::Result<Vec<u8>> {
    let mut command = Command::new("/usr/bin/defaults");
    if matches!(scope, Scope::ThisMac) {
        command.arg("-currentHost");
    }
    let mut child = command
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(text.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(io::Error::other(format!(
            "defaults {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// A bundle identifier names the entry's file and goes to `defaults` as an argument: one that
/// could name another folder or read as an option is refused.
fn check_bundle_id(app: &str) -> io::Result<()> {
    let valid = !app.is_empty()
        && !app.starts_with(['.', '-'])
        && app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("“{app}” is not a bundle identifier"),
        ))
    }
}

fn file(folder: &Path, app: &str) -> PathBuf {
    folder.join(format!("{app}.json"))
}

fn read(folder: &Path, app: &str) -> io::Result<Option<Entry>> {
    match fs::read(file(folder, app)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Replaces the entry in one step: a crash leaves the old one or the new one, never half.
fn write(folder: &Path, entry: &Entry) -> io::Result<()> {
    // Settings can hold sign-in names and tokens: only this user reads the journal.
    private_dir(folder, true)?;
    let path = file(folder, &entry.app);
    let part = path.with_extension("json.part");
    let mut out = fs::File::create(&part)?;
    out.write_all(&serde_json::to_vec_pretty(entry).map_err(io::Error::other)?)?;
    out.sync_all()?;
    fs::rename(&part, &path)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// A settings domain of its own for each test, removed when the test ends.
    struct Domain(String);

    /// A name no other test, in this process or another, uses.
    fn unique() -> String {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{}-{now}-{n}", std::process::id())
    }

    impl Domain {
        fn new() -> Self {
            Self(format!("com.jevision.tests.{}", unique()))
        }

        fn set(&self, scope: Scope, key: &str, value: &str) {
            defaults(scope, &["write", &self.0, key, value], None).unwrap();
        }

        fn get(&self, scope: Scope, key: &str) -> Option<String> {
            defaults(scope, &["read", &self.0, key], None)
                .ok()
                .map(|out| String::from_utf8(out).unwrap().trim().to_owned())
        }

        fn has_settings(&self, scope: Scope) -> bool {
            export(&self.0, scope).unwrap().is_some()
        }
    }

    impl Drop for Domain {
        fn drop(&mut self) {
            for scope in SCOPES {
                let _ = clear(&self.0, scope);
                for file in files(&self.0, scope) {
                    let _ = fs::remove_file(file);
                }
            }
        }
    }

    fn journal() -> PathBuf {
        std::env::temp_dir().join(format!("jevision-saved-settings-{}", unique()))
    }

    #[test]
    fn the_settings_come_back_when_the_run_ends() {
        let (folder, app) = (journal(), Domain::new());
        app.set(Scope::Everywhere, "name", "Anna");
        app.set(Scope::ThisMac, "volume", "7");
        save_and_clear(&folder, &app.0).unwrap();
        assert!(!app.has_settings(Scope::Everywhere));
        assert!(!app.has_settings(Scope::ThisMac));
        // What the app writes during the test.
        app.set(Scope::Everywhere, "name", "Test");
        app.set(Scope::Everywhere, "onboarded", "1");

        assert!(put_back(&folder, &app.0).unwrap());
        assert_eq!(app.get(Scope::Everywhere, "name").as_deref(), Some("Anna"));
        assert_eq!(app.get(Scope::Everywhere, "onboarded"), None);
        assert_eq!(app.get(Scope::ThisMac, "volume").as_deref(), Some("7"));
        assert!(!file(&folder, &app.0).exists());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn an_app_without_settings_has_none_after_the_run() {
        let (folder, app) = (journal(), Domain::new());
        save_and_clear(&folder, &app.0).unwrap();
        app.set(Scope::Everywhere, "onboarded", "1");
        app.set(Scope::ThisMac, "volume", "7");

        assert!(put_back(&folder, &app.0).unwrap());
        assert!(!app.has_settings(Scope::Everywhere));
        assert!(!app.has_settings(Scope::ThisMac));
        assert_eq!(app.get(Scope::Everywhere, "onboarded"), None);
        // As before the run, `defaults` finds no settings of the app at all.
        for scope in SCOPES {
            assert!(defaults(scope, &["read", &app.0], None).is_err());
        }
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn settings_a_crashed_run_left_come_back_at_the_next_start() {
        let (folder, app) = (journal(), Domain::new());
        app.set(Scope::Everywhere, "name", "Anna");
        save_and_clear(&folder, &app.0).unwrap();
        app.set(Scope::Everywhere, "name", "Test");
        // The run's process is gone without putting them back.
        let mut gone = Command::new("true").spawn().unwrap();
        gone.wait().unwrap();
        let entry = read(&folder, &app.0).unwrap().unwrap();
        write(
            &folder,
            &Entry {
                pid: Some(gone.id()),
                ..entry
            },
        )
        .unwrap();

        put_back_left_settings(&folder);
        assert_eq!(app.get(Scope::Everywhere, "name").as_deref(), Some("Anna"));
        assert!(!file(&folder, &app.0).exists());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_later_reset_keeps_the_copy_an_earlier_run_saved() {
        let (folder, app) = (journal(), Domain::new());
        app.set(Scope::Everywhere, "name", "Anna");
        save_and_clear(&folder, &app.0).unwrap();
        app.set(Scope::Everywhere, "name", "Test");
        let entry = read(&folder, &app.0).unwrap().unwrap();
        write(&folder, &Entry { pid: None, ..entry }).unwrap();

        save_and_clear(&folder, &app.0).unwrap();
        assert!(put_back(&folder, &app.0).unwrap());
        assert_eq!(app.get(Scope::Everywhere, "name").as_deref(), Some("Anna"));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_name_that_is_not_a_bundle_identifier_is_refused() {
        let folder = journal();
        for app in ["", "../x", "-currentHost", ".hidden", "a b"] {
            assert!(save_and_clear(&folder, app).is_err(), "{app}");
        }
        assert!(!folder.exists());
    }
}
