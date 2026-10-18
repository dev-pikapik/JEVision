//! Runs saved in a project: listing them, reading one back and exporting its report.

use std::path::Path;

use jevision_runner::Run;

use crate::RunReport;

/// A saved run could not be read or its report could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ReportError {
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<std::io::Error> for ReportError {
    fn from(e: std::io::Error) -> Self {
        Self::Failed {
            reason: e.to_string(),
        }
    }
}

/// A finished run: its folder and the report saved there.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SavedRun {
    pub dir: String,
    pub report: RunReport,
}

impl From<Run> for SavedRun {
    fn from(run: Run) -> Self {
        Self {
            dir: run.dir.to_string_lossy().into_owned(),
            report: run.report.into(),
        }
    }
}

/// Runs saved in `<project>/.jevision/runs`, newest first. Runs still going and folders without
/// a readable report are left out.
#[uniffi::export]
pub fn list_runs(project: String) -> Result<Vec<SavedRun>, ReportError> {
    Ok(Run::list(Path::new(&project))?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Reads the run saved in the folder `dir`.
#[uniffi::export]
pub fn load_run(dir: String) -> Result<SavedRun, ReportError> {
    Ok(Run::load(Path::new(&dir))?.into())
}

/// Saves the report of the run in `dir` as one HTML file with its screenshots inside. The run's
/// video, too large to embed, is copied next to the file under the same name ending in `.mp4`.
#[uniffi::export]
pub fn export_html(dir: String, to: String) -> Result<(), ReportError> {
    let run = Run::load(Path::new(&dir))?;
    let to = Path::new(&to);
    let source = run
        .report
        .video
        .as_deref()
        .map(|video| run.dir.join(video))
        .filter(|video| video.is_file());
    let video = match source {
        Some(source) => {
            let copy = to.with_extension("mp4");
            // Copying a file onto itself would empty it.
            let same = std::fs::canonicalize(&copy)
                .is_ok_and(|c| std::fs::canonicalize(&source).is_ok_and(|s| s == c));
            if !same {
                std::fs::copy(&source, &copy)?;
            }
            copy.file_name().map(|name| link(&name.to_string_lossy()))
        }
        None => None,
    };
    std::fs::write(
        to,
        jevision_runner::html(&run.report, &run.dir, video.as_deref()),
    )?;
    Ok(())
}

/// A file name as a relative link: every byte but letters, digits and `-._~` percent-encoded.
fn link(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Saves the reports of the runs in `dirs` as one JUnit XML file, a test case per run, for CI
/// systems.
#[uniffi::export]
pub fn export_junit(dirs: Vec<String>, to: String) -> Result<(), ReportError> {
    let reports = dirs
        .iter()
        .map(|dir| Run::load(Path::new(dir)).map(|run| run.report))
        .collect::<std::io::Result<Vec<_>>>()?;
    std::fs::write(to, jevision_runner::junit_xml(&reports))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevision_runner::{StepStatus, Verdict};
    use std::path::PathBuf;

    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn report(test: &str, started: &str, verdict: &str) -> String {
        format!(
            r#"{{"format":1,"test":"{test}","file":"{test}.flow.json","app":"com.example.App",
            "startedAt":"{started}","durationMs":1200,"verdict":"{verdict}","flaky":false,
            "attempts":[{{"number":1,"verdict":"{verdict}","durationMs":1200,"steps":[
              {{"seq":1,"test":"{test}.flow.json","block":"check","type":"check","status":"{verdict}",
                "durationMs":300,"after":"shots/1-001-check-after.png","reference":"refs/home.png",
                "similarity":0.5}}]}}]}}"#
        )
    }

    #[test]
    fn lists_and_loads_saved_runs() {
        let root =
            std::env::temp_dir().join(format!("jevision-ffi-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = Folder(root);
        let runs = project.0.join(".jevision/runs");
        let save = |name: &str, json: &str| {
            std::fs::create_dir_all(runs.join(name)).unwrap();
            std::fs::write(runs.join(name).join("report.json"), json).unwrap();
        };
        assert_eq!(list_runs(project.0.to_string_lossy().into()), Ok(vec![]));

        save(
            "20261017-090000-login",
            &report("login", "2026-10-17T09:00:00.000Z", "passed"),
        );
        save(
            "20261018-101500-login",
            &report("login", "2026-10-18T10:15:00.000Z", "failed"),
        );
        save(
            "20261018-080000-home",
            &report("home", "2026-10-18T08:00:00.000Z", "cancelled"),
        );
        save("20261018-110000-broken", "{ not json");
        save(
            "20261018-120000-future",
            &report("future", "2026-10-18T12:00:00.000Z", "passed").replacen(
                r#""format":1"#,
                r#""format":99"#,
                1,
            ),
        );
        // Still running: no report yet.
        std::fs::create_dir_all(runs.join("20261018-130000-busy/shots")).unwrap();

        let listed = list_runs(project.0.to_string_lossy().into()).unwrap();
        let order: Vec<_> = listed
            .iter()
            .map(|r| (r.report.test.as_str(), r.report.verdict))
            .collect();
        assert_eq!(
            order,
            [
                ("login", Verdict::Failed),
                ("home", Verdict::Cancelled),
                ("login", Verdict::Passed)
            ]
        );
        assert!(listed[0].dir.ends_with("20261018-101500-login"));

        let loaded = load_run(listed[0].dir.clone()).unwrap();
        assert_eq!(loaded, listed[0]);
        let step = &loaded.report.attempts[0].steps[0];
        assert_eq!(step.status, StepStatus::Failed);
        assert_eq!(step.reference.as_deref(), Some("refs/home.png"));

        assert!(load_run(runs.join("20261018-110000-broken").to_string_lossy().into()).is_err());
        let Err(ReportError::Failed { reason }) =
            load_run(runs.join("20261018-120000-future").to_string_lossy().into())
        else {
            panic!("a newer format must not load");
        };
        assert!(reason.contains("99"));

        let html = project.0.join("report.html");
        let junit = project.0.join("junit.xml");
        export_html(listed[0].dir.clone(), html.to_string_lossy().into()).unwrap();
        export_junit(vec![listed[0].dir.clone()], junit.to_string_lossy().into()).unwrap();
        assert!(std::fs::read_to_string(&html).unwrap().contains("login"));
        assert!(!std::fs::read_to_string(&html).unwrap().contains("<video"));
        assert!(
            std::fs::read_to_string(&junit)
                .unwrap()
                .contains("<failure")
        );
        let all = listed.iter().map(|r| r.dir.clone()).collect();
        export_junit(all, junit.to_string_lossy().into()).unwrap();
        let xml = std::fs::read_to_string(&junit).unwrap();
        assert!(
            xml.contains(r#"tests="3" failures="1" errors="0" skipped="1""#),
            "{xml}"
        );
        assert!(
            export_junit(
                vec![runs.join("20261018-110000-broken").to_string_lossy().into()],
                junit.to_string_lossy().into()
            )
            .is_err()
        );
        assert!(
            export_html(
                listed[0].dir.clone(),
                project.0.join("missing/x.html").to_string_lossy().into()
            )
            .is_err()
        );
    }

    #[test]
    fn exported_html_brings_the_video_along() {
        let root = std::env::temp_dir().join(format!("jevision-ffi-video-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = Folder(root);
        let dir = project.0.join(".jevision/runs/20261018-101500-login");
        std::fs::create_dir_all(&dir).unwrap();
        let json = report("login", "2026-10-18T10:15:00.000Z", "failed")
            .replacen(
                r#""flaky":false,"#,
                r#""flaky":false,"video":"video.mp4","#,
                1,
            )
            .replacen(
                r#""similarity":0.5"#,
                r#""similarity":0.5,"videoAtMs":2500"#,
                1,
            );
        std::fs::write(dir.join("report.json"), json).unwrap();
        std::fs::write(dir.join("video.mp4"), b"movie").unwrap();

        let run = load_run(dir.to_string_lossy().into()).unwrap();
        assert_eq!(run.report.video.as_deref(), Some("video.mp4"));
        assert_eq!(run.report.attempts[0].steps[0].video_at_ms, Some(2500));

        let html = project.0.join("Sign in #1.html");
        export_html(dir.to_string_lossy().into(), html.to_string_lossy().into()).unwrap();
        assert_eq!(
            std::fs::read(project.0.join("Sign in #1.mp4")).unwrap(),
            b"movie"
        );
        let page = std::fs::read_to_string(&html).unwrap();
        assert!(page.contains(r#"src="Sign%20in%20%231.mp4""#), "{page}");
        assert!(page.contains(r#"data-t="2.500""#), "{page}");

        // Saving the page into the run folder as `video.html` must not empty the video.
        let inside = dir.join("video.html");
        export_html(
            dir.to_string_lossy().into(),
            inside.to_string_lossy().into(),
        )
        .unwrap();
        assert_eq!(std::fs::read(dir.join("video.mp4")).unwrap(), b"movie");

        std::fs::remove_file(dir.join("video.mp4")).unwrap();
        export_html(dir.to_string_lossy().into(), html.to_string_lossy().into()).unwrap();
        assert!(!std::fs::read_to_string(&html).unwrap().contains("<video"));
    }
}
