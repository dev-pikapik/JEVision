//! GitHub for a project whose server is on github.com: the user's account, pull requests and
//! what they change.
//!
//! The sign-in is GitHub CLI's: its token is asked for whenever GitHub is called, kept only in
//! memory for that call, and never written, logged or shown.

use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::{FileChange, Git, GitError, Repo, display_url, find_tool};

/// The address of GitHub's REST API.
pub const API: &str = "https://api.github.com";

const TIMEOUT: Duration = Duration::from_secs(30);
/// Lists longer than this many pages of 100 are cut short.
const MAX_PAGES: usize = 10;

/// A repository on github.com.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
}

impl GitHubRepo {
    /// The repository a remote's URL points to, when it is on github.com.
    pub fn from_url(url: &str) -> Option<Self> {
        let shown = display_url(url);
        let (host, path) = shown.split_once('/')?;
        let host = host.split_once(':').map_or(host, |(host, _)| host);
        if !["github.com", "www.github.com", "ssh.github.com"]
            .iter()
            .any(|h| host.eq_ignore_ascii_case(h))
        {
            return None;
        }
        let (owner, name) = path.split_once('/')?;
        let valid = |part: &str| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        (valid(owner) && valid(name)).then(|| Self {
            owner: owner.into(),
            name: name.into(),
        })
    }

    /// `owner/name`.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// Who GitHub CLI is signed in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAccount {
    pub login: String,
    /// The name on the profile; empty when there is none.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    /// The page of the pull request on github.com.
    pub url: String,
    /// The branch with the changes.
    pub head: String,
    pub head_commit: String,
    /// The branch the changes are proposed for.
    pub base: String,
    pub base_commit: String,
    pub draft: bool,
    /// Seconds since 1970.
    pub updated: i64,
}

/// What a new pull request from the current branch would start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestDraft {
    /// The current branch, as named on GitHub.
    pub branch: String,
    /// The repository's default branch, proposed as the target.
    pub base: String,
    /// The branches on GitHub the changes could go to.
    pub bases: Vec<String>,
    /// The branch has commits that aren't on GitHub, or isn't there at all.
    pub unpushed: bool,
    /// The pull request already open for the branch.
    pub existing: Option<PullRequest>,
}

/// The files of the project a pull request changes, and the two commits to compare them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestChanges {
    /// Where the branch started from its target: the versions before the changes.
    pub before: String,
    /// The latest commit of the branch: the versions with the changes.
    pub after: String,
    /// Relative to the project folder.
    pub files: Vec<FileChange>,
}

/// An issue of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    /// The page of the issue on github.com.
    pub url: String,
}

/// Branch of the project's repository that keeps the screenshots of bug reports, apart from
/// the project's own history.
pub const ATTACHMENTS_BRANCH: &str = "jevision-attachments";

/// GitHub's REST API, called with the user's token.
pub struct GitHub {
    agent: ureq::Agent,
    api: String,
    token: String,
}

impl GitHub {
    pub fn new(api: &str, token: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .user_agent(concat!("JEVision/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            agent,
            api: api.trim_end_matches('/').to_owned(),
            token,
        }
    }

    pub fn account(&self) -> Result<GitHubAccount, GitError> {
        let user = self.get("/user")?;
        Ok(GitHubAccount {
            login: text(&user["login"]),
            name: text(&user["name"]),
        })
    }

    pub fn default_branch(&self, repo: &GitHubRepo) -> Result<String, GitError> {
        Ok(text(&self.get(&repo_path(repo, ""))?["default_branch"]))
    }

    /// The repository's branches, by name.
    pub fn branches(&self, repo: &GitHubRepo) -> Result<Vec<String>, GitError> {
        let mut names: Vec<String> = self
            .pages(&repo_path(repo, "/branches"))?
            .iter()
            .map(|b| text(&b["name"]))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        Ok(names)
    }

    /// Open pull requests, the latest updated first; only those from `head` (a branch of
    /// this repository) when given.
    pub fn pull_requests(
        &self,
        repo: &GitHubRepo,
        head: Option<&str>,
    ) -> Result<Vec<PullRequest>, GitError> {
        let mut path = repo_path(repo, "/pulls?state=open&sort=updated&direction=desc");
        if let Some(head) = head {
            path.push_str(&format!(
                "&head={}",
                encode(&format!("{}:{head}", repo.owner))
            ));
        }
        Ok(self.pages(&path)?.iter().map(pull_request).collect())
    }

    /// Opens a pull request proposing `head` for `base`, with exactly the user's words.
    pub fn create_pull_request(
        &self,
        repo: &GitHubRepo,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<PullRequest, GitError> {
        let mut request = json!({ "title": title, "head": head, "base": base });
        if !body.trim().is_empty() {
            request["body"] = body.into();
        }
        let path = repo_path(repo, "/pulls");
        Ok(pull_request(&self.created("POST", &path, &request)?))
    }

    /// The open issue whose description contains `marker`, the latest updated first.
    pub fn open_issue_with(
        &self,
        repo: &GitHubRepo,
        marker: &str,
    ) -> Result<Option<GitHubIssue>, GitError> {
        let path = repo_path(repo, "/issues?state=open&sort=updated&direction=desc");
        Ok(self
            .pages(&path)?
            .iter()
            // The list has the pull requests too.
            .filter(|i| i.get("pull_request").is_none())
            .find(|i| i["body"].as_str().is_some_and(|b| b.contains(marker)))
            .map(issue))
    }

    /// Opens an issue with exactly these words.
    pub fn create_issue(
        &self,
        repo: &GitHubRepo,
        title: &str,
        body: &str,
    ) -> Result<GitHubIssue, GitError> {
        let request = json!({ "title": title, "body": body });
        let path = repo_path(repo, "/issues");
        Ok(issue(&self.created("POST", &path, &request)?))
    }

    /// Adds a comment to an issue; returns the comment's page on github.com.
    pub fn comment_on_issue(
        &self,
        repo: &GitHubRepo,
        number: u64,
        body: &str,
    ) -> Result<String, GitError> {
        let path = repo_path(repo, &format!("/issues/{number}/comments"));
        Ok(text(
            &self.created("POST", &path, &json!({ "body": body }))?["html_url"],
        ))
    }

    /// Saves `files`, by path, in [`ATTACHMENTS_BRANCH`], making the branch when it isn't
    /// there, in one commit described by `message`. Returns the address
    /// each file is shown from, in the order given: it points to the commit, so it keeps
    /// showing that very file.
    pub fn attach(
        &self,
        repo: &GitHubRepo,
        files: &[(String, Vec<u8>)],
        message: &str,
    ) -> Result<Vec<String>, GitError> {
        use base64::Engine;
        let reference = repo_path(repo, &format!("/git/ref/heads/{ATTACHMENTS_BRANCH}"));
        let parent = match self.send("GET", &reference, None)? {
            (200, body) => Some(text(&parse(&body)?["object"]["sha"])),
            (404, _) => None,
            (status, body) => return Err(error(status, &body)),
        };
        let base_tree = match &parent {
            Some(commit) => {
                let path = repo_path(repo, &format!("/git/commits/{commit}"));
                Some(text(&self.get(&path)?["tree"]["sha"]))
            }
            None => None,
        };
        let mut tree = Vec::new();
        let mut paths = Vec::new();
        for (path, bytes) in files {
            let blob = self.created(
                "POST",
                &repo_path(repo, "/git/blobs"),
                &json!({
                    "content": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "encoding": "base64",
                }),
            )?;
            tree.push(json!({ "path": path, "mode": "100644", "type": "blob", "sha": text(&blob["sha"]) }));
            paths.push(path.clone());
        }
        let mut request = json!({ "tree": tree });
        if let Some(base) = base_tree {
            request["base_tree"] = base.into();
        }
        let tree = self.created("POST", &repo_path(repo, "/git/trees"), &request)?;
        let commit = self.created(
            "POST",
            &repo_path(repo, "/git/commits"),
            &json!({ "message": message, "tree": text(&tree["sha"]), "parents": parent.iter().collect::<Vec<_>>() }),
        )?;
        let commit = text(&commit["sha"]);
        let (method, path, request) = match parent {
            Some(_) => (
                "PATCH",
                repo_path(repo, &format!("/git/refs/heads/{ATTACHMENTS_BRANCH}")),
                json!({ "sha": commit }),
            ),
            None => (
                "POST",
                repo_path(repo, "/git/refs"),
                json!({ "ref": format!("refs/heads/{ATTACHMENTS_BRANCH}"), "sha": commit }),
            ),
        };
        match self.send(method, &path, Some(&request))? {
            (200 | 201, _) => {}
            (status, body) => return Err(error(status, &body)),
        }
        Ok(paths
            .iter()
            .map(|path| {
                let path: Vec<String> = path.split('/').map(encode).collect();
                format!(
                    "https://github.com/{}/blob/{commit}/{}?raw=true",
                    repo.full_name(),
                    path.join("/")
                )
            })
            .collect())
    }

    /// Files a bug report with `pictures`, PNG files saved by [`GitHub::attach`] and shown
    /// under its text by their names: a new issue titled `title`, or a comment on issue
    /// `comment_on`. `marker`, hidden in a new issue, is how [`GitHub::open_issue_with`] finds
    /// it again. Returns the page of the issue or comment.
    pub fn report_bug(
        &self,
        repo: &GitHubRepo,
        title: &str,
        body: &str,
        marker: &str,
        pictures: &[(String, Vec<u8>)],
        comment_on: Option<u64>,
    ) -> Result<String, GitError> {
        let mut text = match comment_on {
            Some(_) => format!("### {title}\n\n{}", body.trim_end()),
            None => body.trim_end().to_owned(),
        };
        if !pictures.is_empty() {
            let urls = self.attach(repo, pictures, title)?;
            text.push_str("\n\n### Screenshots\n");
            for ((name, _), url) in pictures.iter().zip(urls) {
                let name = name.rsplit('/').next().unwrap_or(name);
                let label = name.strip_suffix(".png").unwrap_or(name);
                text.push_str(&format!("\n**{label}**\n\n![{label}]({url})\n"));
            }
        }
        match comment_on {
            Some(number) => self.comment_on_issue(repo, number, &text),
            None => {
                text.push_str(&format!("\n{marker}\n"));
                Ok(self.create_issue(repo, title, &text)?.url)
            }
        }
    }

    /// The answer to a request that makes something, which GitHub answers with 201.
    fn created(&self, method: &str, path: &str, body: &Value) -> Result<Value, GitError> {
        match self.send(method, path, Some(body))? {
            (201, answer) => parse(&answer),
            (status, answer) => Err(error(status, &answer)),
        }
    }

    fn get(&self, path: &str) -> Result<Value, GitError> {
        let (status, body) = self.send("GET", path, None)?;
        if status != 200 {
            return Err(error(status, &body));
        }
        parse(&body)
    }

    /// Every item of a list, 100 at a time.
    fn pages(&self, path: &str) -> Result<Vec<Value>, GitError> {
        let joint = if path.contains('?') { '&' } else { '?' };
        let mut items = Vec::new();
        for page in 1..=MAX_PAGES {
            let Value::Array(found) =
                self.get(&format!("{path}{joint}per_page=100&page={page}"))?
            else {
                return Err(unreadable());
            };
            let last = found.len() < 100;
            items.extend(found);
            if last {
                break;
            }
        }
        Ok(items)
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, String), GitError> {
        let url = format!("{}{path}", self.api);
        let authorization = format!("Bearer {}", self.token);
        let result = match body {
            Some(body) => {
                let request = match method {
                    "PATCH" => self.agent.patch(&url),
                    _ => self.agent.post(&url),
                };
                request
                    .header("Accept", "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .header("Authorization", &authorization)
                    .content_type("application/json")
                    .send(body.to_string())
            }
            None => self
                .agent
                .get(&url)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("Authorization", &authorization)
                .call(),
        };
        let mut response = result.map_err(|_| GitError::Unreachable)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|_| GitError::Unreachable)?;
        Ok((status, body))
    }
}

impl Git {
    /// GitHub, signed in as GitHub CLI is.
    pub fn github(&self) -> Result<GitHub, GitError> {
        let gh = self
            .gh
            .clone()
            .or_else(|| find_tool("gh"))
            .ok_or(GitError::NoGitHubCli)?;
        let out = Command::new(gh)
            .args(["auth", "token", "--hostname", "github.com"])
            .stdin(Stdio::null())
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .output()
            .map_err(|_| GitError::NoGitHubCli)?;
        let token = String::from_utf8(out.stdout).unwrap_or_default();
        let token = token.trim();
        if !out.status.success() || token.is_empty() {
            return Err(GitError::GitHubSignedOut);
        }
        Ok(GitHub::new(API, token.to_owned()))
    }
}

impl Repo {
    /// The repository on GitHub the project syncs with, and the name of its remote.
    pub fn github(&self) -> Result<(String, GitHubRepo), GitError> {
        let remote = self
            .server(self.upstream()?.as_deref())?
            .ok_or(GitError::NoRemote)?;
        let url = self
            .config(&format!("remote.{remote}.url"))?
            .ok_or(GitError::NoRemote)?;
        let repo = GitHubRepo::from_url(&url).ok_or(GitError::NotGitHub)?;
        Ok((remote, repo))
    }

    pub fn pull_requests(&self, hub: &GitHub) -> Result<Vec<PullRequest>, GitError> {
        hub.pull_requests(&self.github()?.1, None)
    }

    /// What a pull request from the current branch would start from.
    pub fn pull_request_draft(&self, hub: &GitHub) -> Result<PullRequestDraft, GitError> {
        let (remote, repo) = self.github()?;
        let (branch, unpushed) = self.branch_on(&remote)?;
        let base = hub.default_branch(&repo)?;
        let bases = hub
            .branches(&repo)?
            .into_iter()
            .filter(|b| *b != branch)
            .collect();
        let existing = hub.pull_requests(&repo, Some(&branch))?.into_iter().next();
        Ok(PullRequestDraft {
            branch,
            base,
            bases,
            unpushed,
            existing,
        })
    }

    /// Proposes the current branch, already on GitHub, for `base`.
    pub fn create_pull_request(
        &self,
        hub: &GitHub,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<PullRequest, GitError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(GitError::EmptyMessage);
        }
        let (remote, repo) = self.github()?;
        let (branch, unpushed) = self.branch_on(&remote)?;
        if unpushed {
            return Err(GitError::NotPushed { branch });
        }
        if branch == base {
            return Err(GitError::NothingToPropose { base: base.into() });
        }
        hub.create_pull_request(&repo, &branch, base, title, body)
    }

    /// The files of the project that change between where `head` left `base` and `head`,
    /// downloading the commits first when they aren't here.
    pub fn pull_request_changes(
        &self,
        base: &str,
        head: &str,
    ) -> Result<PullRequestChanges, GitError> {
        for id in [base, head] {
            if id.len() < 7 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(GitError::Failed {
                    message: format!("`{id}` is not a commit"),
                });
            }
        }
        let here = |id: &str| -> Result<bool, GitError> {
            let spec = format!("{id}^{{commit}}");
            Ok(self.output(&["cat-file", "-e", &spec])?.status.success())
        };
        if !here(base)? || !here(head)? {
            let (remote, _) = self.github()?;
            self.network(&[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                &remote,
                base,
                head,
            ])?;
        }
        let before = self.run(&["merge-base", base, head])?.trim().to_owned();
        let files = self.run(&[
            "diff",
            "--name-status",
            "-z",
            "-M",
            &before,
            head,
            "--",
            ".",
        ])?;
        Ok(PullRequestChanges {
            files: self.name_status(&files),
            before,
            after: head.to_owned(),
        })
    }

    /// The page of `path`, relative to the project, on GitHub: on the current branch when it
    /// is there, otherwise on the repository's default branch.
    pub fn web_url(&self, path: &str) -> Result<String, GitError> {
        let (remote, repo) = self.github()?;
        let branch = match self.upstream()? {
            Some(upstream) => upstream.strip_prefix(&format!("{remote}/")).map_or_else(
                || "HEAD".to_owned(),
                |b| b.split('/').map(encode).collect::<Vec<_>>().join("/"),
            ),
            None => "HEAD".to_owned(),
        };
        let path: Vec<String> = format!("{}{path}", self.prefix)
            .split('/')
            .map(encode)
            .collect();
        Ok(format!(
            "https://github.com/{}/blob/{branch}/{}",
            repo.full_name(),
            path.join("/")
        ))
    }

    /// The current branch as named on `remote`, and whether it has commits that aren't there.
    fn branch_on(&self, remote: &str) -> Result<(String, bool), GitError> {
        let branch = self.branch()?;
        if !self.has_commits()? {
            return Err(GitError::NoCommits);
        }
        let Some(upstream) = self.upstream()? else {
            return Ok((branch, true));
        };
        let Some(name) = upstream.strip_prefix(&format!("{remote}/")) else {
            return Ok((branch, true));
        };
        let ahead = self.run(&["rev-list", "--count", "@{u}..HEAD"])?;
        Ok((name.to_owned(), ahead.trim() != "0"))
    }
}

fn repo_path(repo: &GitHubRepo, rest: &str) -> String {
    format!("/repos/{}/{}{rest}", repo.owner, repo.name)
}

fn pull_request(v: &Value) -> PullRequest {
    PullRequest {
        number: v["number"].as_u64().unwrap_or(0),
        title: text(&v["title"]),
        author: text(&v["user"]["login"]),
        url: text(&v["html_url"]),
        head: text(&v["head"]["ref"]),
        head_commit: text(&v["head"]["sha"]),
        base: text(&v["base"]["ref"]),
        base_commit: text(&v["base"]["sha"]),
        draft: v["draft"].as_bool().unwrap_or(false),
        updated: v["updated_at"].as_str().map_or(0, seconds),
    }
}

fn issue(v: &Value) -> GitHubIssue {
    GitHubIssue {
        number: v["number"].as_u64().unwrap_or(0),
        title: text(&v["title"]),
        url: text(&v["html_url"]),
    }
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn parse(body: &str) -> Result<Value, GitError> {
    serde_json::from_str(body).map_err(|_| unreadable())
}

fn unreadable() -> GitError {
    GitError::Failed {
        message: "GitHub sent an answer that can't be read".into(),
    }
}

/// What a failed answer from GitHub means.
pub(crate) fn error(status: u16, body: &str) -> GitError {
    let answer: Value = serde_json::from_str(body).unwrap_or_default();
    let mut message = text(&answer["message"]);
    if let Some(details) = answer["errors"].as_array() {
        let details: Vec<String> = details
            .iter()
            .map(|e| text(&e["message"]))
            .filter(|m| !m.is_empty())
            .collect();
        if !details.is_empty() {
            message = details.join("\n");
        }
    }
    match status {
        401 => GitError::GitHubSignedOut,
        403 | 429 if message.to_lowercase().contains("rate limit") => GitError::Failed { message },
        403 => GitError::NoPermission,
        404 => GitError::RemoteNotFound,
        422 if message.contains("already exists") => GitError::PullRequestExists,
        422 if message.starts_with("No commits between") => GitError::NothingToPropose {
            base: message
                .strip_prefix("No commits between ")
                .and_then(|m| m.split(' ').next())
                .unwrap_or_default()
                .to_owned(),
        },
        _ if message.is_empty() => GitError::Failed {
            message: format!("GitHub answered with error {status}"),
        },
        _ => GitError::Failed { message },
    }
}

/// Text for a URL: everything but letters, digits and `-_.~` escaped.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `2026-10-18T09:30:00Z` → seconds since 1970; 0 when unreadable.
pub(crate) fn seconds(iso: &str) -> i64 {
    let n = |from: usize, to: usize| iso.get(from..to).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d), Some(h), Some(min), Some(s)) =
        (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19))
    else {
        return 0;
    };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days * 86_400 + h * 3_600 + min * 60 + s
}
