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

/// Saves the report of the run in `dir` as one HTML file with its screenshots inside.
#[uniffi::export]
pub fn export_html(dir: String, to: String) -> Result<(), ReportError> {
    let run = Run::load(Path::new(&dir))?;
    std::fs::write(to, jevision_runner::html(&run.report, &run.dir))?;
    Ok(())
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
        assert!(std::fs::read_to_string(html).unwrap().contains("login"));
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
}
