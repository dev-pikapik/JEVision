use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use jevision_runner::{CiOrigin, Run, import_ci_artifact, imported_ci_artifacts};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn project() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jevision-import-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn report(test: &str, ci: &str) -> String {
    format!(
        r#"{{"format":1,"test":"{test}","file":"{test}.flow.json","app":"com.example.App",
        "startedAt":"2026-10-18T09:30:00.000Z","durationMs":5,"verdict":"passed","flaky":false,
        {ci}"attempts":[]}}"#
    )
}

fn zip(files: &[(&str, &str)], method: CompressionMethod) -> Cursor<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(method);
    for (name, text) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    let mut zip = writer.finish().unwrap();
    zip.set_position(0);
    zip
}

fn origin() -> CiOrigin {
    CiOrigin {
        system: "GitHub Actions".into(),
        repository: Some("acme/qa".into()),
        branch: Some("main".into()),
        commit: Some("abc1234".into()),
        pull_request: None,
        run_url: Some("https://github.com/acme/qa/actions/runs/9001".into()),
    }
}

fn runs(project: &Path) -> Vec<Run> {
    Run::list(project).unwrap()
}

#[test]
fn runs_in_an_artifact_join_the_history_once() {
    let own = r#""ci":{"system":"GitHub Actions","repository":"acme/qa","pullRequest":482},"#;
    let (login, cart) = (report("login", own), report("cart", ""));
    let files = [
        ("junit.xml", "<testsuites/>"),
        ("reports/index.html", "<html/>"),
        (
            "tests/.jevision/runs/20261018-093000-login/report.json",
            &*login,
        ),
        (
            "tests/.jevision/runs/20261018-093000-login/shots/1.png",
            "png",
        ),
        (
            "tests/.jevision/runs/20261018-093100-cart/report.json",
            &*cart,
        ),
    ];
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let project = project();
        let brought = import_ci_artifact(&project, 31, zip(&files, method), &origin()).unwrap();
        assert_eq!(brought, 2);
        assert_eq!(
            imported_ci_artifacts(&project)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            [31]
        );

        let found = runs(&project);
        let by_test = |name: &str| found.iter().find(|run| run.report.test == name).unwrap();
        let login = by_test("login");
        assert!(login.dir.ends_with("ci-31-20261018-093000-login"));
        assert!(login.dir.join("shots/1.png").is_file());
        assert_eq!(login.report.ci.as_ref().unwrap().pull_request, Some(482));
        assert_eq!(
            by_test("cart").report.ci,
            Some(origin()),
            "a report that doesn't say where it came from gets what GitHub says"
        );
        assert!(!project.join(".jevision/runs/.ci-31.partial").exists());

        // The same artifact again adds nothing.
        assert_eq!(
            import_ci_artifact(&project, 31, zip(&files, method), &origin()).unwrap(),
            0
        );
        assert_eq!(runs(&project).len(), 2);
    }
}

#[test]
fn a_run_deleted_from_the_history_is_not_brought_back() {
    let project = project();
    let login = report("login", "");
    let files = [(".jevision/runs/20261018-093000-login/report.json", &*login)];
    let first = import_ci_artifact(
        &project,
        5,
        zip(&files, CompressionMethod::Stored),
        &origin(),
    );
    assert_eq!(first.unwrap(), 1);
    fs::remove_dir_all(&runs(&project)[0].dir).unwrap();
    let again = import_ci_artifact(
        &project,
        5,
        zip(&files, CompressionMethod::Stored),
        &origin(),
    );
    assert_eq!(again.unwrap(), 0);
    assert!(runs(&project).is_empty());
}

#[test]
fn an_artifact_without_runs_is_remembered_and_adds_nothing() {
    let project = project();
    let old = [
        ("junit.xml", "<testsuites/>"),
        ("reports/index.html", "<html/>"),
    ];
    let brought = import_ci_artifact(&project, 7, zip(&old, CompressionMethod::Stored), &origin());
    assert_eq!(brought.unwrap(), 0);
    assert!(imported_ci_artifacts(&project).unwrap().contains(&7));
    assert!(runs(&project).is_empty());

    // A report this version can't read is left out, the readable one next to it comes in.
    let b = report("b", "");
    let files = [
        (".jevision/runs/a/report.json", "{ not json"),
        (".jevision/runs/b/report.json", &*b),
        (".jevision/runs/c/notes.txt", "no report here"),
    ];
    let brought = import_ci_artifact(
        &project,
        8,
        zip(&files, CompressionMethod::Stored),
        &origin(),
    );
    assert_eq!(brought.unwrap(), 1);
    assert_eq!(runs(&project).len(), 1);
}

#[test]
fn a_zip_that_cannot_be_read_is_marked_done_and_files_outside_the_runs_stay_out() {
    let project = project();
    let broken = import_ci_artifact(
        &project,
        9,
        Cursor::new(b"this is not a zip".to_vec()),
        &origin(),
    );
    assert_eq!(broken.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    assert!(imported_ci_artifacts(&project).unwrap().contains(&9));

    let (absolute, ok) = (report("abs", ""), report("ok", ""));
    let files = [
        ("../../escape.txt", "outside"),
        (".jevision/runs/../../escape.txt", "outside"),
        (".jevision/runs/loose.txt", "loose"),
        ("/abs/.jevision/runs/x/report.json", &*absolute),
        (".jevision/runs/ok/report.json", &*ok),
    ];
    let brought = import_ci_artifact(
        &project,
        10,
        zip(&files, CompressionMethod::Stored),
        &origin(),
    );
    assert_eq!(
        brought.unwrap(),
        2,
        "an absolute name is read as relative, so it stays inside"
    );
    let parent = project.parent().unwrap();
    assert!(!parent.join("escape.txt").exists());
    assert!(!project.join("escape.txt").exists());
    assert!(!project.join(".jevision/runs/loose.txt").exists());
}

#[test]
fn a_ledger_that_cannot_be_read_stops_the_import_instead_of_duplicating() {
    let project = project();
    fs::create_dir_all(project.join(".jevision")).unwrap();
    fs::write(project.join(".jevision/ci-artifacts.json"), "garbage").unwrap();
    let a = report("a", "");
    let files = [(".jevision/runs/a/report.json", &*a)];
    let result = import_ci_artifact(
        &project,
        1,
        zip(&files, CompressionMethod::Stored),
        &origin(),
    );
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    assert!(runs(&project).is_empty());
}
