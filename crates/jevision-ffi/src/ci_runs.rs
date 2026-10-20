//! Brings the run reports that CI uploaded into the history of a project.

use std::fs;
use std::io::Seek;
use std::path::Path;

use jevision_core::CI_ARTIFACT;
use jevision_git::{Artifact, GitError, GitHub, GitHubRepo};
use jevision_runner::{CiOrigin, import_ci_artifact, imported_ci_artifacts};

use crate::git::{git, repo};

/// How many of the newest CI artifacts one refresh looks at.
const LATEST: usize = 20;

/// The biggest artifact worth downloading, in bytes.
const LARGEST: u64 = 512 << 20;

/// What a refresh of the CI runs found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiRefresh {
    /// Runs added to the history now.
    pub imported: u32,
    /// Artifacts with run reports that GitHub still has, including those fetched before. Zero
    /// means the workflow hasn't uploaded any: it isn't set up, or hasn't run yet.
    pub artifacts: u32,
    /// Artifacts that couldn't be brought in because they are too large, broken or gone.
    pub failed: u32,
}

/// Fetches what the project's CI runs on GitHub uploaded and adds the runs to the history, each
/// once. Needs the project's repository to be on GitHub and the user signed in to it, and says
/// which is missing.
#[uniffi::export]
pub fn refresh_ci_runs(project: String) -> Result<CiRefresh, GitError> {
    let (_, github) = repo(&project)?.github()?;
    refresh(&git()?.github()?, &github, Path::new(&project))
}

fn refresh(hub: &GitHub, github: &GitHubRepo, project: &Path) -> Result<CiRefresh, GitError> {
    let artifacts = hub.artifacts(github, CI_ARTIFACT, LATEST)?;
    let done = imported_ci_artifacts(project).map_err(failed)?;
    let mut refresh = CiRefresh {
        imported: 0,
        artifacts: artifacts.len() as u32,
        failed: 0,
    };
    for artifact in artifacts.iter().filter(|a| !done.contains(&a.id)) {
        match bring_in(hub, github, project, artifact) {
            Ok(runs) => refresh.imported += runs as u32,
            Err(GitError::RemoteNotFound | GitError::Failed { .. }) => refresh.failed += 1,
            Err(e) => return Err(e),
        }
    }
    Ok(refresh)
}

/// Downloads one artifact into a file of its own, which is gone again afterwards, and imports
/// its runs. The token only goes to GitHub, never into a file.
fn bring_in(
    hub: &GitHub,
    github: &GitHubRepo,
    project: &Path,
    artifact: &Artifact,
) -> Result<usize, GitError> {
    if artifact.size > LARGEST {
        return Err(GitError::Failed {
            message: "The files from the CI run are too large to download".into(),
        });
    }
    let folder = project.join(".jevision");
    fs::create_dir_all(&folder).map_err(failed)?;
    let path = folder.join(format!("ci-{}.zip", artifact.id));
    let result = (|| {
        let mut zip = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(failed)?;
        hub.download_artifact(github, artifact.id, &mut zip, LARGEST)?;
        zip.rewind().map_err(failed)?;
        match import_ci_artifact(project, artifact.id, zip, &origin(github, artifact)) {
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Err(GitError::Failed {
                message: e.to_string(),
            }),
            other => other.map_err(failed),
        }
    })();
    let _ = fs::remove_file(path);
    result
}

/// Where a run came from when its report doesn't say, as far as GitHub does.
fn origin(github: &GitHubRepo, artifact: &Artifact) -> CiOrigin {
    let repository = format!("{}/{}", github.owner, github.name);
    let some = |text: &str| Some(text.to_owned()).filter(|text| !text.is_empty());
    CiOrigin {
        system: "GitHub Actions".into(),
        run_url: Some(format!(
            "https://github.com/{repository}/actions/runs/{}",
            artifact.run
        )),
        repository: Some(repository),
        branch: some(&artifact.branch),
        commit: some(&artifact.commit),
        pull_request: None,
    }
}

fn failed(e: std::io::Error) -> GitError {
    GitError::Failed {
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Cursor, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use jevision_runner::Run;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    /// GitHub on this computer: answers by path and keeps the paths asked for.
    struct Api {
        address: String,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl Api {
        /// `routes` are `(path, status, body)`; a 302 has the address to go to as its body.
        fn serve(routes: Vec<(&'static str, u16, Vec<u8>)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let asked = Arc::new(Mutex::new(Vec::new()));
            let seen = asked.clone();
            let base = address.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header == "\r\n" || header.is_empty() {
                            break;
                        }
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
                    seen.lock().unwrap().push(path.clone());
                    let route = routes
                        .iter()
                        .find(|(prefix, ..)| path.starts_with(prefix))
                        .cloned();
                    let (status, body) = route.map_or((404, Vec::new()), |(_, s, b)| (s, b));
                    let (location, body) = match status {
                        302 => (
                            format!("Location: {base}{}\r\n", String::from_utf8_lossy(&body)),
                            Vec::new(),
                        ),
                        _ => (String::new(), body),
                    };
                    let mut out = reader.into_inner();
                    let _ = write!(
                        out,
                        "HTTP/1.1 {status} X\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = out.write_all(&body);
                }
            });
            Self { address, asked }
        }

        fn hub(&self) -> GitHub {
            GitHub::new(&self.address, "secret-token".into())
        }

        fn downloads(&self) -> Vec<String> {
            let asked = self.asked.lock().unwrap();
            asked
                .iter()
                .filter(|p| p.contains("/zip"))
                .cloned()
                .collect()
        }
    }

    fn project() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-ci-runs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn report(test: &str, ci: &str) -> String {
        format!(
            r#"{{"format":1,"test":"{test}","file":"{test}.flow.json","app":"a",
            "startedAt":"2026-10-18T09:30:00.000Z","durationMs":5,"verdict":"passed","flaky":false,
            {ci}"attempts":[]}}"#
        )
    }

    fn zip(runs: &[(&str, String)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (folder, text) in runs {
            writer
                .start_file(
                    format!("tests/.jevision/runs/{folder}/report.json"),
                    SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn listing(artifacts: &[(u64, u64, &str, u64)]) -> Vec<u8> {
        let items: Vec<String> = artifacts
            .iter()
            .map(|(id, run, branch, size)| {
                format!(
                    r#"{{"id":{id},"size_in_bytes":{size},"expired":false,"created_at":"2026-10-18T09:30:00Z",
                    "workflow_run":{{"id":{run},"head_branch":"{branch}","head_sha":"abc1234"}}}}"#
                )
            })
            .collect();
        format!(r#"{{"artifacts":[{}]}}"#, items.join(",")).into_bytes()
    }

    fn acme() -> GitHubRepo {
        GitHubRepo::from_url("git@github.com:acme/qa.git").unwrap()
    }

    #[test]
    fn ci_runs_are_fetched_once_and_say_where_they_came_from() {
        let pr = r#""ci":{"system":"GitHub Actions","repository":"acme/qa","pullRequest":482,
            "runUrl":"https://github.com/acme/qa/actions/runs/9002"},"#;
        let api = Api::serve(vec![
            (
                "/repos/acme/qa/actions/artifacts?",
                200,
                listing(&[(31, 9002, "feat/login", 100), (30, 9001, "main", 100)]),
            ),
            (
                "/repos/acme/qa/actions/artifacts/31/zip",
                302,
                b"/storage/31".to_vec(),
            ),
            (
                "/storage/31",
                200,
                zip(&[
                    ("a-login", report("login", pr)),
                    ("b-cart", report("cart", pr)),
                ]),
            ),
            (
                "/repos/acme/qa/actions/artifacts/30/zip",
                200,
                zip(&[("c-login", report("login", ""))]),
            ),
        ]);
        let project = project();
        let first = refresh(&api.hub(), &acme(), &project).unwrap();
        assert_eq!(
            first,
            CiRefresh {
                imported: 3,
                artifacts: 2,
                failed: 0
            }
        );
        let runs = Run::list(&project).unwrap();
        assert_eq!(runs.len(), 3);
        let origins: Vec<_> = runs.iter().filter_map(|r| r.report.ci.clone()).collect();
        assert_eq!(origins.len(), 3);
        assert_eq!(
            origins
                .iter()
                .filter(|o| o.pull_request == Some(482))
                .count(),
            2
        );
        let main = origins
            .iter()
            .find(|o| o.branch.as_deref() == Some("main"))
            .unwrap();
        assert_eq!(main.pull_request, None);
        assert_eq!(
            main.run_url.as_deref(),
            Some("https://github.com/acme/qa/actions/runs/9001")
        );
        assert!(
            !project.join(".jevision/ci-31.zip").exists(),
            "the download is not kept"
        );

        let again = refresh(&api.hub(), &acme(), &project).unwrap();
        assert_eq!(
            again,
            CiRefresh {
                imported: 0,
                artifacts: 2,
                failed: 0
            }
        );
        assert_eq!(
            api.downloads().len(),
            2,
            "31 and 30 once each, nothing the second time"
        );
        assert_eq!(Run::list(&project).unwrap().len(), 3);
    }

    #[test]
    fn what_cannot_be_brought_in_is_counted_and_the_rest_still_comes() {
        let api = Api::serve(vec![
            (
                "/repos/acme/qa/actions/artifacts?",
                200,
                listing(&[
                    (4, 14, "main", 100),
                    (3, 13, "main", LARGEST + 1),
                    (2, 12, "main", 100),
                    (1, 11, "main", 100),
                ]),
            ),
            (
                "/repos/acme/qa/actions/artifacts/4/zip",
                200,
                zip(&[("ok", report("ok", ""))]),
            ),
            (
                "/repos/acme/qa/actions/artifacts/2/zip",
                200,
                b"not a zip".to_vec(),
            ),
            // Artifact 1 isn't served at all: it expired after it was listed.
        ]);
        let project = project();
        let found = refresh(&api.hub(), &acme(), &project).unwrap();
        assert_eq!(
            found,
            CiRefresh {
                imported: 1,
                artifacts: 4,
                failed: 3
            }
        );
        assert!(
            !api.downloads().iter().any(|p| p.contains("/3/zip")),
            "a file too large isn't downloaded"
        );
        // The broken zip is remembered; the one that vanished and the large one are tried again.
        let again = refresh(&api.hub(), &acme(), &project).unwrap();
        assert_eq!(again.failed, 2);
        assert_eq!(again.imported, 0);
    }

    #[test]
    fn a_sign_in_that_github_does_not_accept_stops_the_refresh() {
        let api = Api::serve(vec![(
            "/repos/acme/qa/actions/artifacts?",
            401,
            br#"{"message":"Bad credentials"}"#.to_vec(),
        )]);
        let result = refresh(&api.hub(), &acme(), &project());
        assert_eq!(result, Err(GitError::GitHubSignedOut));
    }
}
