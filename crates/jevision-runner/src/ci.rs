//! Where a run happened when a CI system started it, read from the system's environment, so the
//! app can show a run that came from a pull request next to the ones made on this Mac.

use serde::{Deserialize, Serialize};

/// The CI job a run belongs to, saved in the run's report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiOrigin {
    /// The CI system, such as `GitHub Actions`.
    pub system: String,
    /// `owner/name` of the repository; absent where the system doesn't say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The branch the commit is on; for a pull request, the branch it comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The commit the run tested; for a pull request, the latest commit of its branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Number of the pull request (merge request) the run is for; absent for other runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<u64>,
    /// The page of the CI run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_url: Option<String>,
}

impl CiOrigin {
    /// The CI job this process runs in; `None` outside CI.
    pub fn from_env() -> Option<Self> {
        Self::detect(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
    }

    /// The CI job whose variables `var` reads; `None` when they don't say a CI system started
    /// the process.
    pub fn detect(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let is = |name: &str| var(name).is_some_and(|v| v == "true");
        if is("GITHUB_ACTIONS") {
            return Some(github(&var));
        }
        let first = |names: &[&str]| names.iter().find_map(|name| var(name));
        let number = |names: &[&str]| first(names).and_then(|v| v.rsplit('/').next()?.parse().ok());
        if is("GITLAB_CI") {
            return Some(Self {
                system: "GitLab CI".into(),
                repository: var("CI_PROJECT_PATH"),
                branch: first(&["CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "CI_COMMIT_BRANCH"]),
                commit: var("CI_COMMIT_SHA"),
                pull_request: number(&["CI_MERGE_REQUEST_IID"]),
                run_url: first(&["CI_PIPELINE_URL", "CI_JOB_URL"]),
            });
        }
        if is("BUILDKITE") {
            return Some(Self {
                system: "Buildkite".into(),
                repository: var("BUILDKITE_REPO").and_then(|url| repository_of(&url)),
                branch: var("BUILDKITE_BRANCH"),
                commit: var("BUILDKITE_COMMIT"),
                pull_request: number(&["BUILDKITE_PULL_REQUEST"]),
                run_url: var("BUILDKITE_BUILD_URL"),
            });
        }
        if is("CIRCLECI") {
            return Some(Self {
                system: "CircleCI".into(),
                repository: var("CIRCLE_REPOSITORY_URL").and_then(|url| repository_of(&url)),
                branch: var("CIRCLE_BRANCH"),
                commit: var("CIRCLE_SHA1"),
                pull_request: number(&["CIRCLE_PR_NUMBER", "CIRCLE_PULL_REQUEST"]),
                run_url: var("CIRCLE_BUILD_URL"),
            });
        }
        if var("JENKINS_URL").is_some() && var("BUILD_URL").is_some() {
            return Some(Self {
                system: "Jenkins".into(),
                repository: var("GIT_URL").and_then(|url| repository_of(&url)),
                branch: first(&["CHANGE_BRANCH", "BRANCH_NAME"])
                    .or_else(|| var("GIT_BRANCH").map(|b| b.trim_start_matches("origin/").into())),
                commit: var("GIT_COMMIT"),
                pull_request: number(&["CHANGE_ID"]),
                run_url: var("BUILD_URL"),
            });
        }
        var("CI")
            .is_some_and(|v| v == "true" || v == "1")
            .then(|| Self {
                system: "CI".into(),
                repository: None,
                branch: None,
                commit: None,
                pull_request: None,
                run_url: None,
            })
    }
}

fn github(var: &impl Fn(&str) -> Option<String>) -> CiOrigin {
    let event = var("GITHUB_EVENT_NAME").unwrap_or_default();
    let on_pull_request = event.starts_with("pull_request");
    let reference = var("GITHUB_REF").unwrap_or_default();
    // `refs/pull/482/merge`: GitHub names the pull request in the ref, whatever starts the run.
    let pull_request = reference
        .strip_prefix("refs/pull/")
        .and_then(|rest| rest.split('/').next()?.parse().ok());
    let branch = if on_pull_request {
        var("GITHUB_HEAD_REF")
    } else if var("GITHUB_REF_TYPE").is_none_or(|kind| kind == "branch") {
        var("GITHUB_REF_NAME")
    } else {
        None
    };
    // A pull request is tested on a merge commit nobody made; the commit people know is the
    // head of the branch, which only the event says.
    let head = on_pull_request
        .then(|| {
            let event = std::fs::read_to_string(var("GITHUB_EVENT_PATH")?).ok()?;
            let event: serde_json::Value = serde_json::from_str(&event).ok()?;
            event["pull_request"]["head"]["sha"]
                .as_str()
                .map(Into::into)
        })
        .flatten();
    let repository = var("GITHUB_REPOSITORY");
    let run_url = repository
        .as_ref()
        .zip(var("GITHUB_RUN_ID"))
        .map(|(repo, id)| {
            let server = var("GITHUB_SERVER_URL").unwrap_or_else(|| "https://github.com".into());
            let mut url = format!("{}/{repo}/actions/runs/{id}", server.trim_end_matches('/'));
            if let Some(attempt) = var("GITHUB_RUN_ATTEMPT").filter(|a| a != "1") {
                url.push_str(&format!("/attempts/{attempt}"));
            }
            url
        });
    CiOrigin {
        system: "GitHub Actions".into(),
        repository,
        branch,
        commit: head.or_else(|| var("GITHUB_SHA")),
        pull_request,
        run_url,
    }
}

/// `owner/name` from the address of a repository, as `https://host/owner/name.git` or
/// `git@host:owner/name.git`.
fn repository_of(url: &str) -> Option<String> {
    let path = url.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.rsplit(['/', ':']);
    let name = parts.next().filter(|p| !p.is_empty())?;
    let owner = parts.next().filter(|p| !p.is_empty() && !p.contains('@'))?;
    Some(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn detect(vars: &[(&str, &str)]) -> Option<CiOrigin> {
        let vars: HashMap<_, _> = vars.iter().copied().collect();
        CiOrigin::detect(|name| vars.get(name).map(|v| (*v).to_owned()))
    }

    #[test]
    fn nothing_says_a_run_on_a_mac_is_from_ci() {
        assert_eq!(detect(&[]), None);
        assert_eq!(
            detect(&[("GITHUB_ACTIONS", "false"), ("CI", "false")]),
            None
        );
    }

    #[test]
    fn a_pull_request_on_github_names_its_number_branch_and_run() {
        let event =
            std::env::temp_dir().join(format!("jevision-event-{}.json", std::process::id()));
        std::fs::write(&event, r#"{"pull_request": {"head": {"sha": "headsha"}}}"#).unwrap();
        let origin = detect(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REPOSITORY", "acme/qa"),
            ("GITHUB_SERVER_URL", "https://github.com"),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_EVENT_PATH", event.to_str().unwrap()),
            ("GITHUB_REF", "refs/pull/482/merge"),
            ("GITHUB_REF_NAME", "482/merge"),
            ("GITHUB_HEAD_REF", "feat/login"),
            ("GITHUB_SHA", "mergesha"),
            ("GITHUB_RUN_ID", "9001"),
            ("GITHUB_RUN_ATTEMPT", "2"),
        ])
        .unwrap();
        std::fs::remove_file(event).unwrap();
        assert_eq!(
            origin,
            CiOrigin {
                system: "GitHub Actions".into(),
                repository: Some("acme/qa".into()),
                branch: Some("feat/login".into()),
                commit: Some("headsha".into()),
                pull_request: Some(482),
                run_url: Some("https://github.com/acme/qa/actions/runs/9001/attempts/2".into()),
            }
        );
    }

    #[test]
    fn a_push_on_github_names_the_branch_and_a_tag_names_none() {
        let push = [
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REPOSITORY", "acme/qa"),
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF", "refs/heads/main"),
            ("GITHUB_REF_NAME", "main"),
            ("GITHUB_REF_TYPE", "branch"),
            ("GITHUB_SHA", "abc123"),
            ("GITHUB_RUN_ID", "7"),
            ("GITHUB_RUN_ATTEMPT", "1"),
        ];
        let origin = detect(&push).unwrap();
        assert_eq!(origin.branch.as_deref(), Some("main"));
        assert_eq!(origin.commit.as_deref(), Some("abc123"));
        assert_eq!(origin.pull_request, None);
        assert_eq!(
            origin.run_url.as_deref(),
            Some("https://github.com/acme/qa/actions/runs/7")
        );

        let tag = [&push[..], &[("GITHUB_REF_TYPE", "tag")]].concat();
        assert_eq!(detect(&tag).unwrap().branch, None);
    }

    #[test]
    fn other_ci_systems_are_read_from_their_own_variables() {
        let gitlab = detect(&[
            ("GITLAB_CI", "true"),
            ("CI_PROJECT_PATH", "acme/qa"),
            ("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "feat/login"),
            ("CI_COMMIT_SHA", "abc"),
            ("CI_MERGE_REQUEST_IID", "12"),
            (
                "CI_PIPELINE_URL",
                "https://gitlab.com/acme/qa/-/pipelines/5",
            ),
        ])
        .unwrap();
        assert_eq!(gitlab.system, "GitLab CI");
        assert_eq!(gitlab.pull_request, Some(12));
        assert_eq!(gitlab.branch.as_deref(), Some("feat/login"));

        let buildkite = detect(&[
            ("BUILDKITE", "true"),
            ("BUILDKITE_REPO", "git@github.com:acme/qa.git"),
            ("BUILDKITE_BRANCH", "main"),
            ("BUILDKITE_PULL_REQUEST", "false"),
            (
                "BUILDKITE_BUILD_URL",
                "https://buildkite.com/acme/qa/builds/3",
            ),
        ])
        .unwrap();
        assert_eq!(buildkite.repository.as_deref(), Some("acme/qa"));
        assert_eq!(buildkite.pull_request, None);

        let circle = detect(&[
            ("CIRCLECI", "true"),
            ("CIRCLE_REPOSITORY_URL", "https://github.com/acme/qa"),
            ("CIRCLE_PULL_REQUEST", "https://github.com/acme/qa/pull/31"),
        ])
        .unwrap();
        assert_eq!(
            (circle.pull_request, circle.system.as_str()),
            (Some(31), "CircleCI")
        );

        let jenkins = detect(&[
            ("JENKINS_URL", "https://ci.example.com/"),
            ("BUILD_URL", "https://ci.example.com/job/qa/8/"),
            ("GIT_BRANCH", "origin/main"),
            ("GIT_URL", "https://github.com/acme/qa.git"),
        ])
        .unwrap();
        assert_eq!(jenkins.branch.as_deref(), Some("main"));
        assert_eq!(jenkins.repository.as_deref(), Some("acme/qa"));

        let unknown = detect(&[("CI", "true")]).unwrap();
        assert_eq!((unknown.system.as_str(), unknown.branch), ("CI", None));
    }

    #[test]
    fn a_report_without_the_field_reads_and_one_with_it_round_trips() {
        let origin = CiOrigin {
            system: "GitHub Actions".into(),
            repository: None,
            branch: Some("main".into()),
            commit: None,
            pull_request: None,
            run_url: None,
        };
        let json = serde_json::to_string(&origin).unwrap();
        assert_eq!(json, r#"{"system":"GitHub Actions","branch":"main"}"#);
        assert_eq!(serde_json::from_str::<CiOrigin>(&json).unwrap(), origin);
    }
}
