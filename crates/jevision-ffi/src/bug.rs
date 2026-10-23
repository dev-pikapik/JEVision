//! Bug reports about failed runs, filed as GitHub issues of the project's repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use jevision_core::{Flow, ProjectSettings, required_secrets};
use jevision_git::{GitError, GitHub, GitHubIssue, GitHubRepo};
use jevision_runner::{BugContext, MaskedImage, Run, mask_secrets, picture_caption_in, run_flows};

use crate::git::{on_github, repo};
use crate::{ReportError, SecretSource};

/// What a bug report about a failed run says, for the user to edit before it is sent.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BugReport {
    pub title: String,
    /// Markdown, without the screenshots: they are uploaded when the report is sent.
    pub body: String,
    /// The same for every failure of the same test at the same block for the same reason.
    pub fingerprint: String,
    pub screenshots: Vec<BugScreenshot>,
}

/// A picture of the failed step.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BugScreenshot {
    /// `Expected`, `Actual` or `Difference`, the same in every language; the app shows its
    /// caption from the catalog key of the same name.
    pub label: String,
    /// The picture's file.
    pub path: String,
}

/// The bug report about the failed run in `dir`, secrets masked; `None` when the run didn't
/// fail. Reads only this Mac: nothing is sent.
#[uniffi::export]
pub fn draft_bug_report(
    project: String,
    dir: String,
    secrets: Arc<dyn SecretSource>,
) -> Result<Option<BugReport>, ReportError> {
    let run = Run::load(Path::new(&dir))?;
    let project = Path::new(&project);
    let report = &run.report;
    let flows = run_flows(project, report);
    let context = BugContext {
        macos: macos(),
        jevision: crate::jevision_version(),
        app_version: app_version(&report.app),
        test_url: repo(&project.to_string_lossy())
            .and_then(|repo| repo.web_url(&report.file))
            .ok(),
    };
    let Some(draft) = jevision_runner::bug_report(report, &flows, &context) else {
        return Ok(None);
    };
    let values = secret_values(&flows, report.environment.as_deref(), &*secrets);
    Ok(Some(BugReport {
        title: mask_secrets(&draft.title, &values),
        body: mask_secrets(&draft.body, &values),
        fingerprint: draft.fingerprint,
        screenshots: draft
            .pictures
            .into_iter()
            .map(|p| BugScreenshot {
                label: p.label,
                path: run.dir.join(p.path).to_string_lossy().into_owned(),
            })
            .collect(),
    }))
}

/// The open issue of the project's repository already filed for this failure, by its
/// fingerprint: a repeat is better told there.
#[uniffi::export]
pub fn github_open_bug_report(
    project: String,
    fingerprint: String,
) -> Result<Option<GitHubIssue>, GitError> {
    let (repo, hub) = on_github(&project)?;
    hub.open_issue_with(&repo.github()?.1, &marker(&fingerprint))
}

/// Files the bug report about the run in `dir` in the project's repository: a new issue, or a
/// comment on issue `comment_on`. The title and text are the user's, with secrets masked
/// again in case they typed one; the screenshots, private areas painted over, are uploaded to
/// the repository's `jevision-attachments` branch. Returns the page of the issue or comment.
#[uniffi::export]
pub fn github_send_bug_report(
    project: String,
    dir: String,
    report: BugReport,
    comment_on: Option<u64>,
    secrets: Arc<dyn SecretSource>,
) -> Result<String, GitError> {
    let (repo, hub) = on_github(&project)?;
    send(
        &hub,
        &repo.github()?.1,
        Path::new(&project),
        Path::new(&dir),
        &report,
        comment_on,
        &*secrets,
    )
}

/// Hidden in an issue's text, so the same failure is found again.
pub(crate) fn marker(fingerprint: &str) -> String {
    format!("<!-- jevision-bug: {fingerprint} -->")
}

pub(crate) fn send(
    hub: &GitHub,
    repo: &GitHubRepo,
    project: &Path,
    dir: &Path,
    report: &BugReport,
    comment_on: Option<u64>,
    secrets: &dyn SecretSource,
) -> Result<String, GitError> {
    let failed = |reason: String| GitError::Failed { message: reason };
    if report.title.trim().is_empty() {
        return Err(GitError::EmptyMessage);
    }
    let run = Run::load(dir).map_err(|e| failed(e.to_string()))?;
    let settings = ProjectSettings::load(project).map_err(failed)?;
    let values = secret_values(
        &run_flows(project, &run.report),
        run.report.environment.as_deref(),
        secrets,
    );
    let folder = run
        .dir
        .file_name()
        .map_or_else(|| "run".to_owned(), |n| n.to_string_lossy().into_owned());
    let inside = run.dir.canonicalize().map_err(|e| failed(e.to_string()))?;
    let mut pictures = Vec::new();
    for shot in &report.screenshots {
        let file = PathBuf::from(&shot.path)
            .canonicalize()
            .ok()
            .filter(|file| file.starts_with(&inside))
            .ok_or_else(|| failed(jevision_l10n::tr!("bug.picture.notInRun", &shot.path)))?;
        let png = std::fs::read(&file).map_err(|e| failed(e.to_string()))?;
        let masked = MaskedImage::from_png(&png, &settings.hide).map_err(failed)?;
        // The issue shows each picture under its file name, so the name is its caption.
        let caption = picture_caption_in(&jevision_l10n::language(), &shot.label);
        pictures.push((format!("{folder}/{caption}.png"), masked.png().to_vec()));
    }
    hub.report_bug(
        repo,
        &mask_secrets(&report.title, &values),
        &mask_secrets(&report.body, &values),
        &marker(&report.fingerprint),
        &pictures,
        comment_on,
    )
}

/// Values of every secret the tests of a run use, in the run's environment.
fn secret_values(
    flows: &HashMap<String, Flow>,
    environment: Option<&str>,
    secrets: &dyn SecretSource,
) -> Vec<String> {
    let mut names: Vec<String> = flows.values().flat_map(required_secrets).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| secrets.secret(name, environment.map(str::to_owned)))
        .collect()
}

/// Such as `26.1 (25B78)`.
fn macos() -> String {
    let read = |what: &str| {
        Command::new("/usr/bin/sw_vers")
            .arg(what)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    match (read("-productVersion"), read("-buildVersion")) {
        (Some(version), Some(build)) => format!("{version} ({build})"),
        (version, _) => version.unwrap_or_default(),
    }
}

/// Version of the installed app with bundle id `app`, such as `1.21 (402)`.
fn app_version(app: &str) -> Option<String> {
    let query = format!("kMDItemCFBundleIdentifier == '{}'", app.replace('\'', ""));
    let found = Command::new("/usr/bin/mdfind").arg(query).output().ok()?;
    let bundle = String::from_utf8_lossy(&found.stdout)
        .lines()
        .find(|line| line.ends_with(".app"))?
        .to_owned();
    let plist = Path::new(&bundle).join("Contents/Info.plist");
    let read = |key: &str| {
        Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(&plist)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    match (read("CFBundleShortVersionString"), read("CFBundleVersion")) {
        (Some(short), Some(build)) if short != build => Some(format!("{short} ({build})")),
        (Some(version), _) | (None, Some(version)) => Some(version),
        (None, None) => None,
    }
}
