//! The last result of each test, kept in the project's [`LAST_RESULTS_FILE`] by every run that
//! passed or failed. The run folders tell a run from the one before on a Mac; a CI job starts
//! from a fresh checkout without them, and its workflow keeps this small file between jobs.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::{fs, io};

use jevision_core::{
    LAST_RESULTS_FILE, SettingsMetadata, parse_settings_document, write_file_atomically,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Run, Verdict};

/// The layout the file is saved in.
pub const LAST_RESULTS_FORMAT: u32 = OLDEST_FORMAT + MIGRATIONS.len() as u32;
/// The oldest layout read. The first files had no `format`; their layout is format 1.
const OLDEST_FORMAT: u32 = 1;
/// `MIGRATIONS[i]` upgrades a file from format `OLDEST_FORMAT + i` to the next one. Appending a
/// step raises [`LAST_RESULTS_FORMAT`] by one.
const MIGRATIONS: &[fn(&mut Map<String, Value>)] = &[];

/// Runs of this process finish in parallel; each changes the file in turn. [`Writing`] does the
/// same for the other programs.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct LastResults {
    format: u32,
    /// By test file, relative to the project folder.
    #[serde(default)]
    tests: BTreeMap<String, LastResult>,
    #[serde(flatten)]
    metadata: SettingsMetadata,
}

impl Default for LastResults {
    fn default() -> Self {
        Self {
            format: LAST_RESULTS_FORMAT,
            tests: BTreeMap::new(),
            metadata: SettingsMetadata::default(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LastResult {
    /// The run's folder, like `20261019-093000-login`.
    run: String,
    verdict: Verdict,
    /// The verdict kept for the test before this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<Verdict>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// A missing cache starts empty. Unreadable JSON, unknown required values and unsupported
/// requirements stay untouched; a cached baseline is then unavailable, without stopping tests.
fn load(path: &Path) -> io::Result<Option<LastResults>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(LastResults::default())),
        Err(e) => return Err(e),
    };
    let Ok(Value::Object(mut file)) =
        parse_settings_document(&text, LAST_RESULTS_FORMAT, Some(OLDEST_FORMAT))
    else {
        return Ok(None);
    };
    let found = file["format"].as_u64().expect("a checked format");
    for step in &MIGRATIONS[(found - u64::from(OLDEST_FORMAT)) as usize..] {
        step(&mut file);
    }
    file.insert("format".into(), LAST_RESULTS_FORMAT.into());
    let Ok(results) = serde_json::from_value::<LastResults>(Value::Object(file)) else {
        return Ok(None);
    };
    let finished = |verdict| matches!(verdict, Verdict::Passed | Verdict::Failed);
    if results
        .tests
        .values()
        .any(|last| !finished(last.verdict) || last.before.is_some_and(|v| !finished(v)))
    {
        return Ok(None);
    }
    Ok(Some(results))
}

/// The lock file beside the file, held while this exists: the app, the command line and the
/// assistant's server may each finish a run of the same project at once, and each changes the
/// file after reading it.
struct Writing(File);

impl Writing {
    fn wait(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        crate::screen::lock(&file, true)?;
        Ok(Self(file))
    }
}

impl Drop for Writing {
    /// Lets go of the lock before the file closes, as the screen lock does.
    fn drop(&mut self) {
        crate::screen::unlock(&self.0);
    }
}

fn folder(run: &Run) -> Option<String> {
    Some(run.dir.file_name()?.to_string_lossy().into_owned())
}

/// Keeps `run` of a test in `project` as the test's last result. A stopped or blocked run
/// doesn't say whether the test works, so it leaves the one before in place.
pub(crate) fn record(project: &Path, run: &Run) -> io::Result<()> {
    let verdict = run.report.verdict;
    if !matches!(verdict, Verdict::Passed | Verdict::Failed) {
        return Ok(());
    }
    let Some(name) = folder(run) else {
        return Ok(());
    };
    let path = project.join(LAST_RESULTS_FILE);
    let _this_process = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    let _other_programs = Writing::wait(&path.with_extension("json.lock"))?;
    let Some(mut results) = load(&path)? else {
        return Ok(());
    };
    let previous = results.tests.remove(&run.report.file);
    let before = previous.as_ref().map(|last| last.verdict);
    let extra = previous.map_or_else(BTreeMap::new, |last| last.extra);
    results.tests.insert(
        run.report.file.clone(),
        LastResult {
            run: name,
            verdict,
            before,
            extra,
        },
    );
    let mut text = serde_json::to_string_pretty(&results).map_err(io::Error::other)?;
    text.push('\n');
    write_file_atomically(&path, text)
}

/// The verdict of the run of `run`'s test before it that passed or failed, as `project` keeps
/// it in [`LAST_RESULTS_FILE`]; `None` when the file has none, or only a later run, or is from a
/// newer JEVision.
pub fn verdict_before(project: &Path, run: &Run) -> Option<Verdict> {
    let name = folder(run)?;
    let last = load(&project.join(LAST_RESULTS_FILE))
        .ok()??
        .tests
        .remove(&run.report.file)?;
    match last.run.cmp(&name) {
        Ordering::Equal => last.before,
        Ordering::Less => Some(last.verdict),
        Ordering::Greater => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn run(project: &Path, name: &str, file: &str, verdict: &str) -> Run {
        Run {
            dir: project.join(".jevision/runs").join(name),
            report: serde_json::from_value(json!({
                "format": 1, "test": "T", "file": file, "app": "com.example.App",
                "startedAt": "2026-10-19T09:30:00Z", "durationMs": 1, "verdict": verdict,
                "flaky": false, "attempts": []
            }))
            .unwrap(),
        }
    }

    #[test]
    fn only_finished_results_are_kept_and_broken_files_wait_for_explicit_repair() {
        let project = std::env::temp_dir().join(format!("jevision-last-{}", std::process::id()));
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join(".jevision")).unwrap();
        fs::write(project.join(LAST_RESULTS_FILE), "{ not json").unwrap();
        let first = run(&project, "20261019-080000-t", "t.flow.json", "failed");
        assert_eq!(verdict_before(&project, &first), None);
        record(&project, &first).unwrap();
        assert_eq!(verdict_before(&project, &first), None);
        assert_eq!(
            fs::read_to_string(project.join(LAST_RESULTS_FILE)).unwrap(),
            "{ not json"
        );
        // Removing the damaged local cache is an explicit repair, never an automatic reset.
        fs::remove_file(project.join(LAST_RESULTS_FILE)).unwrap();
        record(&project, &first).unwrap();

        let stopped = run(&project, "20261019-083000-t", "t.flow.json", "cancelled");
        record(&project, &stopped).unwrap();
        record(
            &project,
            &run(&project, "20261019-083100-t", "t.flow.json", "blocked"),
        )
        .unwrap();
        record(
            &project,
            &run(&project, "20261019-083200-u", "u.flow.json", "passed"),
        )
        .unwrap();
        assert_eq!(verdict_before(&project, &stopped), Some(Verdict::Failed));

        let second = run(&project, "20261019-090000-t", "t.flow.json", "passed");
        record(&project, &second).unwrap();
        assert_eq!(verdict_before(&project, &second), Some(Verdict::Failed));
        assert_eq!(
            verdict_before(&project, &first),
            None,
            "a later run is kept"
        );
        assert_eq!(
            verdict_before(
                &project,
                &run(&project, "20261019-100000-t", "t.flow.json", "passed")
            ),
            Some(Verdict::Passed)
        );
        assert_eq!(
            listed(&project),
            ["last-results.json", "last-results.json.lock"],
            "no partial file is left"
        );
        fs::remove_dir_all(project).unwrap();
    }

    fn project(name: &str) -> PathBuf {
        let project =
            std::env::temp_dir().join(format!("jevision-last-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join(".jevision")).unwrap();
        project
    }

    fn listed(project: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(project.join(".jevision"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_file_from_a_newer_jevision_is_neither_read_nor_replaced() {
        let project = project("newer");
        let newer = r#"{"format": 99, "tests": {"t.flow.json": {"run": "20261019-080000-t", "verdict": "failed"}}, "kept": true}"#;
        fs::write(project.join(LAST_RESULTS_FILE), newer).unwrap();
        let later = run(&project, "20261019-090000-t", "t.flow.json", "passed");
        assert_eq!(verdict_before(&project, &later), None);
        record(&project, &later).unwrap();
        assert_eq!(
            fs::read_to_string(project.join(LAST_RESULTS_FILE)).unwrap(),
            newer
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn optional_root_and_test_fields_survive_recording_the_next_verdict() {
        let project = project("fields");
        let file = project.join(LAST_RESULTS_FILE);
        let original = json!({"format":1,"$schema":"https://example.test/results.json","minVersion":"0.1.0+state",
            "cursor":[null,42],"tests":{"t.flow.json":{"run":"20261019-080000-t","verdict":"failed","extra":{"keep":true}},
            "u.flow.json":{"run":"20261019-080000-u","verdict":"passed","extra":[1,2]}}});
        fs::write(&file, original.to_string()).unwrap();
        let next = run(&project, "20261019-090000-t", "t.flow.json", "passed");
        record(&project, &next).unwrap();
        let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        for key in ["$schema", "minVersion", "cursor"] {
            assert_eq!(saved[key], original[key]);
        }
        assert_eq!(
            saved["tests"]["u.flow.json"],
            original["tests"]["u.flow.json"]
        );
        assert_eq!(
            saved["tests"]["t.flow.json"]["extra"],
            original["tests"]["t.flow.json"]["extra"]
        );
        assert_eq!(verdict_before(&project, &next), Some(Verdict::Failed));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn unreadable_or_unsupported_result_state_is_never_reset_by_a_finished_run() {
        let project = project("refuse-state");
        let file = project.join(LAST_RESULTS_FILE);
        let next = run(&project, "20261019-090000-t", "t.flow.json", "passed");
        for text in [
            r#"{"format":99,"tests":false}"#,
            r#"{"minVersion":"9999.0.0","tests":false}"#,
            r#"{"format":1,"minVersion":null}"#,
            r#"{"format":1,"$schema":false}"#,
            r#"{"format":1,"tests":{},"tests":{}}"#,
            r#"{"format":1,"extra":{"x":1,"x":2}}"#,
            r#"{"format":1,"tests":{"t.flow.json":{"run":"old","verdict":"deferred"}}}"#,
            r#"{"format":1,"tests":{"t.flow.json":{"run":"old","verdict":"blocked"}}}"#,
            "{",
        ] {
            fs::write(&file, text).unwrap();
            assert_eq!(verdict_before(&project, &next), None);
            record(&project, &next).unwrap();
            assert_eq!(fs::read_to_string(&file).unwrap(), text);
        }
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn cached_folder_names_can_repeat_between_fresh_ci_jobs() {
        let project = project("same-name");
        let failed = run(&project, "20261019-080000-t", "t.flow.json", "failed");
        let passed = run(&project, "20261019-080000-t", "t.flow.json", "passed");
        record(&project, &failed).unwrap();
        record(&project, &passed).unwrap();
        assert_eq!(verdict_before(&project, &passed), Some(Verdict::Failed));
        record(&project, &passed).unwrap();
        assert_eq!(verdict_before(&project, &passed), Some(Verdict::Passed));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn a_file_without_a_format_is_read_as_format_1() {
        let project = project("unversioned");
        fs::write(
            project.join(LAST_RESULTS_FILE),
            r#"{"tests": {"t.flow.json": {"run": "20261019-080000-t", "verdict": "failed"}}}"#,
        )
        .unwrap();
        let later = run(&project, "20261019-090000-t", "t.flow.json", "passed");
        assert_eq!(verdict_before(&project, &later), Some(Verdict::Failed));
        record(&project, &later).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(project.join(LAST_RESULTS_FILE)).unwrap())
                .unwrap();
        assert_eq!(saved["format"], LAST_RESULTS_FORMAT);
        assert_eq!(saved["tests"]["t.flow.json"]["before"], "failed");
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn a_run_waits_for_another_program_changing_the_file() {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Command, Stdio};

        let project = project("lock");
        let file = project.join(LAST_RESULTS_FILE);
        let theirs = r#"{"format": 1, "tests": {"u.flow.json": {"run": "20261019-080000-u", "verdict": "failed"}}}"#;
        // Another program, which takes the lock the way any other JEVision program would, and
        // writes what it read before this run finished, once told to.
        let mut other = Command::new("perl")
            .args([
                "-e",
                r#"use Fcntl qw(:flock); $| = 1; open(L, ">>", $ARGV[0]) or die; flock(L, LOCK_EX) or die; print "held\n"; <STDIN>; open(F, ">", $ARGV[1]) or die; print F $ARGV[2]; close(F)"#,
            ])
            .arg(file.with_extension("json.lock"))
            .arg(&file)
            .arg(theirs)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("perl starts");
        let mut line = String::new();
        BufReader::new(other.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "held\n");

        let ours = std::thread::spawn({
            let project = project.clone();
            move || {
                let run = run(&project, "20261019-090000-t", "t.flow.json", "passed");
                record(&project, &run)
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        let waited = !ours.is_finished();
        writeln!(other.stdin.take().unwrap()).unwrap();
        assert!(other.wait().unwrap().success());
        ours.join().unwrap().unwrap();
        assert!(waited, "the run waits while the other program has the file");
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            saved["tests"]["u.flow.json"]["verdict"], "failed",
            "{saved}"
        );
        assert_eq!(
            saved["tests"]["t.flow.json"]["verdict"], "passed",
            "{saved}"
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_no_partial_file() {
        let project = project("unwritable");
        // A folder in the file's place: the new file can't take it.
        fs::create_dir_all(project.join(LAST_RESULTS_FILE).join("in the way")).unwrap();
        let run = run(&project, "20261019-090000-t", "t.flow.json", "passed");
        assert!(record(&project, &run).is_err());
        assert!(
            !listed(&project)
                .iter()
                .any(|name| name.ends_with(".partial")),
            "{:?}",
            listed(&project)
        );
        fs::remove_dir_all(project).unwrap();
    }
}
