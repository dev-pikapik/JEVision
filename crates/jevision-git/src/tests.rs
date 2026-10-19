use std::fs;

use super::*;

/// A scratch folder, removed with everything in it at the end of the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("jevision-git-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Git that reads only a config file of this test, not the developer's own.
    fn git(&self) -> Git {
        let config = self.0.join("gitconfig");
        fs::write(
            &config,
            "[init]\n\tdefaultBranch = main\n[user]\n\tuseConfigOnly = true\n[commit]\n\tgpgsign = false\n",
        )
        .unwrap();
        Git::at(find_tool("git").expect("git is installed"))
            .env("GIT_CONFIG_GLOBAL", config.to_str().unwrap())
            .env("GIT_CONFIG_NOSYSTEM", "1")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

fn read(dir: &Path, path: &str) -> String {
    fs::read_to_string(dir.join(path)).unwrap()
}

fn me() -> GitIdentity {
    GitIdentity {
        name: "Ann Tester".into(),
        email: "ann@example.com".into(),
    }
}

/// A new repository with the user set up, holding the project folder `qa/` and a file
/// outside it.
fn repository(scratch: &Scratch) -> (Git, Repo) {
    let git = scratch.git();
    let top = scratch.0.join("repo");
    fs::create_dir_all(top.join("qa")).unwrap();
    git.init(&top).unwrap();
    git.set_identity(&top, &me()).unwrap();
    write(&top, "app.swift", "outside");
    let repo = git.open(&top.join("qa")).unwrap().unwrap();
    (git, repo)
}

fn change(path: &str, kind: ChangeKind) -> FileChange {
    FileChange {
        path: path.into(),
        from: None,
        kind,
    }
}

/// Runs git in `dir` for the test's own setup.
fn sh(git: &Git, dir: &Path, args: &[&str]) -> String {
    let out = check(git.output(dir, args).unwrap()).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_folder_outside_git_is_not_a_repository_until_tracked() {
    let scratch = Scratch::new("init");
    let git = scratch.git();
    assert!(git.open(&scratch.0).unwrap().is_none());

    let repo = git.init(&scratch.0).unwrap();
    write(&scratch.0, "login.flow.json", "{}");
    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(!status.has_commits);
    assert_eq!(status.upstream, None);
    assert_eq!(status.remote, None);
    assert!(
        status
            .changes
            .contains(&change("login.flow.json", ChangeKind::Added))
    );
    assert!(repo.history("login.flow.json").unwrap().is_empty());
}

#[test]
fn status_lists_the_project_changes_relative_to_the_project() {
    let scratch = Scratch::new("status");
    let (_, repo) = repository(&scratch);
    let qa = &repo.dir;
    write(qa, "login.flow.json", "1");
    write(qa, "old name.flow.json", "keep");
    write(qa, "gone.flow.json", "x");
    repo.commit(
        &[
            "login.flow.json".into(),
            "old name.flow.json".into(),
            "gone.flow.json".into(),
        ],
        "Add tests",
    )
    .unwrap();

    write(qa, "login.flow.json", "2");
    write(qa, "refs/new button.png", "png");
    fs::remove_file(qa.join("gone.flow.json")).unwrap();
    sh(
        &repo.git,
        qa,
        &["mv", "old name.flow.json", "new name.flow.json"],
    );
    write(&qa.join(".."), "app.swift", "changed outside the project");

    let mut changes = repo.status().unwrap().changes;
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        changes,
        [
            change("gone.flow.json", ChangeKind::Deleted),
            change("login.flow.json", ChangeKind::Modified),
            FileChange {
                path: "new name.flow.json".into(),
                from: Some("old name.flow.json".into()),
                kind: ChangeKind::Renamed,
            },
            change("refs/new button.png", ChangeKind::Added),
        ]
    );
}

#[test]
fn commit_takes_only_the_chosen_changes_and_the_users_words() {
    let scratch = Scratch::new("commit");
    let (git, repo) = repository(&scratch);
    write(&repo.dir, "a.flow.json", "a");
    write(&repo.dir, "b.flow.json", "b");

    assert_eq!(
        repo.commit(&["a.flow.json".into()], "  "),
        Err(GitError::EmptyMessage)
    );
    assert_eq!(repo.commit(&[], "Add"), Err(GitError::NothingToCommit));
    let message = "Add sign-in test\n\nCovers the wrong password case.";
    let id = repo.commit(&["a.flow.json".into()], message).unwrap();

    assert_eq!(
        sh(&git, &repo.dir, &["log", "-1", "--format=%H"]).trim(),
        id
    );
    assert_eq!(
        sh(&git, &repo.dir, &["log", "-1", "--format=%B"]),
        format!("{message}\n\n")
    );
    assert_eq!(
        sh(&git, &repo.dir, &["log", "-1", "--format=%an <%ae>"]).trim(),
        "Ann Tester <ann@example.com>"
    );
    assert_eq!(
        sh(
            &git,
            &repo.dir,
            &["show", "--name-only", "--format=", "HEAD"]
        )
        .trim(),
        "qa/a.flow.json"
    );
    assert_eq!(
        repo.status().unwrap().changes,
        [change("b.flow.json", ChangeKind::Added)]
    );

    fs::remove_file(repo.dir.join("a.flow.json")).unwrap();
    repo.commit(&["a.flow.json".into()], "Remove a").unwrap();
    assert_eq!(
        repo.status().unwrap().changes,
        [change("b.flow.json", ChangeKind::Added)]
    );
}

#[test]
fn commit_without_a_name_asks_for_one() {
    let scratch = Scratch::new("identity");
    let git = scratch.git();
    let repo = git.init(&scratch.0).unwrap();
    assert_eq!(
        repo.identity().unwrap(),
        GitIdentity {
            name: String::new(),
            email: String::new()
        }
    );
    write(&scratch.0, "a.flow.json", "a");
    assert_eq!(
        repo.commit(&["a.flow.json".into()], "Add"),
        Err(GitError::NoIdentity)
    );
    assert_eq!(
        git.set_identity(
            &scratch.0,
            &GitIdentity {
                name: "Ann".into(),
                email: " ".into()
            }
        ),
        Err(GitError::NoIdentity)
    );

    git.set_identity(&scratch.0, &me()).unwrap();
    assert_eq!(repo.identity().unwrap(), me());
    repo.commit(&["a.flow.json".into()], "Add").unwrap();
}

#[test]
fn history_follows_a_test_and_a_folder_and_restores_old_versions() {
    let scratch = Scratch::new("history");
    let (git, repo) = repository(&scratch);
    let qa = &repo.dir;
    write(qa, "login.flow.json", "v1");
    write(qa, "other.flow.json", "o1");
    repo.commit(
        &["login.flow.json".into(), "other.flow.json".into()],
        "First",
    )
    .unwrap();
    write(&qa.join(".."), "app.swift", "v2");
    sh(&git, qa, &["add", "../app.swift"]);
    sh(
        &git,
        qa,
        &["commit", "-qm", "Not about tests", "--", "../app.swift"],
    );
    write(qa, "login.flow.json", "v2");
    repo.commit(&["login.flow.json".into()], "Second").unwrap();
    fs::create_dir(qa.join("auth")).unwrap();
    sh(
        &git,
        qa,
        &["mv", "login.flow.json", "auth/sign in.flow.json"],
    );
    repo.commit(
        &["login.flow.json".into(), "auth/sign in.flow.json".into()],
        "Move",
    )
    .unwrap();

    let history = repo.history("auth/sign in.flow.json").unwrap();
    let summaries: Vec<_> = history.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["Move", "Second", "First"]);
    assert_eq!(
        history[0].files,
        [FileChange {
            path: "auth/sign in.flow.json".into(),
            from: Some("login.flow.json".into()),
            kind: ChangeKind::Renamed,
        }]
    );
    assert_eq!(
        history[1].files,
        [change("login.flow.json", ChangeKind::Modified)]
    );
    assert_eq!(
        history[2].files,
        [change("login.flow.json", ChangeKind::Added)]
    );
    assert_eq!(history[2].parent, None);
    assert!(history[1].parent.is_some());
    assert_eq!(history[0].author, "Ann Tester");
    assert!(history[0].time > 1_700_000_000);
    assert!(history[0].id.starts_with(&history[0].short_id));

    let project = repo.history("").unwrap();
    assert_eq!(
        project.len(),
        3,
        "the commit outside the project is left out"
    );
    assert_eq!(project[2].files.len(), 2);

    let first = &history[2].id;
    assert_eq!(repo.file(first, "login.flow.json").unwrap().unwrap(), b"v1");
    assert_eq!(repo.file(first, "auth/sign in.flow.json").unwrap(), None);
    assert!(matches!(
        repo.file(first, "../app.swift"),
        Err(GitError::Failed { .. })
    ));

    repo.restore(first, "login.flow.json", "auth/sign in.flow.json")
        .unwrap();
    assert_eq!(read(qa, "auth/sign in.flow.json"), "v1");
    repo.restore(&history[1].id, "login.flow.json", "login.flow.json")
        .unwrap();
    assert_eq!(read(qa, "login.flow.json"), "v2");
    assert!(matches!(
        repo.restore(first, "login.flow.json", "../app.swift"),
        Err(GitError::Failed { .. })
    ));
    assert_eq!(
        repo.restore(first, "auth/sign in.flow.json", "auth/sign in.flow.json"),
        Err(GitError::NotInCommit {
            path: "auth/sign in.flow.json".into()
        })
    );
}

#[test]
fn branches_are_listed_created_and_switched() {
    let scratch = Scratch::new("branches");
    let (_, repo) = repository(&scratch);
    write(&repo.dir, "a.flow.json", "main");
    repo.commit(&["a.flow.json".into()], "Add").unwrap();

    repo.create_branch(" new-checkout ").unwrap();
    assert_eq!(
        repo.status().unwrap().branch.as_deref(),
        Some("new-checkout")
    );
    assert_eq!(
        repo.create_branch("main"),
        Err(GitError::BranchExists {
            name: "main".into()
        })
    );
    for bad in ["two words", "-x", "a..b", ""] {
        assert_eq!(
            repo.create_branch(bad),
            Err(GitError::BadBranchName { name: bad.into() })
        );
    }
    write(&repo.dir, "a.flow.json", "branch");
    repo.commit(&["a.flow.json".into()], "FileChange").unwrap();

    let names: Vec<_> = repo
        .branches()
        .unwrap()
        .into_iter()
        .map(|b| (b.name, b.is_current, b.is_remote))
        .collect();
    assert_eq!(
        names,
        [
            ("main".into(), false, false),
            ("new-checkout".into(), true, false)
        ]
    );

    write(&repo.dir, "a.flow.json", "not committed");
    assert_eq!(
        repo.switch("main"),
        Err(GitError::LocalChanges {
            paths: vec!["a.flow.json".into()]
        })
    );
    assert_eq!(read(&repo.dir, "a.flow.json"), "not committed");
    repo.restore("HEAD", "a.flow.json", "a.flow.json").unwrap();
    repo.switch("main").unwrap();
    assert_eq!(read(&repo.dir, "a.flow.json"), "main");
}

#[test]
fn push_and_pull_sync_with_the_server_and_keep_files_whole_on_conflict() {
    let scratch = Scratch::new("sync");
    let git = scratch.git();
    let server = scratch.0.join("server.git");
    fs::create_dir_all(&server).unwrap();
    sh(&git, &server, &["init", "--quiet", "--bare"]);

    let (_, mine) = repository(&scratch);
    let top = mine.dir.join("..");
    assert_eq!(mine.push(), Err(GitError::NoCommits));
    write(&mine.dir, "a.flow.json", "1");
    mine.commit(&["a.flow.json".into()], "Add").unwrap();
    assert_eq!(mine.push(), Err(GitError::NoRemote));
    assert_eq!(mine.pull(), Err(GitError::NoRemote));
    sh(
        &git,
        &top,
        &["remote", "add", "origin", server.to_str().unwrap()],
    );
    assert_eq!(
        mine.pull(),
        Err(GitError::NoUpstream {
            branch: "main".into()
        })
    );
    mine.push().unwrap();
    let status = mine.status().unwrap();
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((status.ahead, status.behind), (0, 0));
    assert_eq!(status.remote, Some(display_url(server.to_str().unwrap())));

    // A teammate's copy.
    let theirs_top = scratch.0.join("theirs");
    sh(
        &git,
        &scratch.0,
        &["clone", "--quiet", server.to_str().unwrap(), "theirs"],
    );
    git.set_identity(&theirs_top, &me()).unwrap();
    let theirs = git.open(&theirs_top.join("qa")).unwrap().unwrap();
    write(&theirs.dir, "b.flow.json", "theirs");
    theirs.commit(&["b.flow.json".into()], "Add b").unwrap();
    theirs.push().unwrap();

    write(&mine.dir, "a.flow.json", "2");
    mine.commit(&["a.flow.json".into()], "FileChange a")
        .unwrap();
    assert_eq!(mine.push(), Err(GitError::Rejected));
    mine.pull().unwrap();
    assert_eq!(read(&mine.dir, "b.flow.json"), "theirs");
    mine.push().unwrap();
    let status = mine.status().unwrap();
    assert_eq!((status.ahead, status.behind), (0, 0));

    // Both change the same line.
    theirs.pull().unwrap();
    write(&theirs.dir, "a.flow.json", "theirs");
    theirs.commit(&["a.flow.json".into()], "Theirs").unwrap();
    theirs.push().unwrap();
    write(&mine.dir, "a.flow.json", "mine");
    mine.commit(&["a.flow.json".into()], "Mine").unwrap();
    assert_eq!(
        mine.pull(),
        Err(GitError::Conflict {
            paths: vec!["a.flow.json".into()]
        })
    );
    assert_eq!(read(&mine.dir, "a.flow.json"), "mine");
    assert!(mine.status().unwrap().changes.is_empty());
    assert!(!top.join(".git/MERGE_HEAD").exists());

    // Uncommitted work in the way of incoming changes stays untouched.
    write(&theirs.dir, "b.flow.json", "theirs 2");
    theirs.commit(&["b.flow.json".into()], "Theirs 2").unwrap();
    theirs.push().unwrap();
    sh(&git, &top, &["reset", "--quiet", "--hard", "origin/main"]);
    write(&mine.dir, "b.flow.json", "draft");
    assert_eq!(
        mine.pull(),
        Err(GitError::LocalChanges {
            paths: vec!["b.flow.json".into()]
        })
    );
    assert_eq!(read(&mine.dir, "b.flow.json"), "draft");

    // A branch only on the server can be switched to.
    theirs.create_branch("feature").unwrap();
    theirs.push().unwrap();
    sh(&git, &top, &["fetch", "--quiet"]);
    assert!(
        mine.branches()
            .unwrap()
            .iter()
            .any(|b| b.name == "feature" && b.is_remote && !b.is_current)
    );
    repo_switch_keeps_draft(&mine);
}

/// Switching branches carries uncommitted changes the other branch doesn't touch.
#[test]
fn a_conflict_is_settled_with_the_files_the_user_chose_and_a_merge_commit() {
    let scratch = Scratch::new("settle");
    let git = scratch.git();
    let server = scratch.0.join("server.git");
    fs::create_dir_all(&server).unwrap();
    sh(&git, &server, &["init", "--quiet", "--bare"]);
    let (_, mine) = repository(&scratch);
    let top = mine.dir.join("..");
    sh(
        &git,
        &top,
        &["remote", "add", "origin", server.to_str().unwrap()],
    );
    write(&mine.dir, "a.flow.json", "base a");
    write(&mine.dir, "b.flow.json", "base b");
    write(&mine.dir, "gone.png", "picture");
    let all = [
        "a.flow.json".into(),
        "b.flow.json".into(),
        "gone.png".into(),
    ];
    mine.commit(&all, "Start").unwrap();
    mine.push().unwrap();

    sh(
        &git,
        &scratch.0,
        &["clone", "--quiet", server.to_str().unwrap(), "theirs"],
    );
    git.set_identity(&scratch.0.join("theirs"), &me()).unwrap();
    let theirs = git.open(&scratch.0.join("theirs/qa")).unwrap().unwrap();
    write(&theirs.dir, "a.flow.json", "their a");
    write(&theirs.dir, "b.flow.json", "their b");
    write(&theirs.dir, "gone.png", "their picture");
    theirs.commit(&all, "Theirs").unwrap();
    theirs.push().unwrap();

    write(&mine.dir, "a.flow.json", "my a");
    write(&mine.dir, "b.flow.json", "my b");
    fs::remove_file(mine.dir.join("gone.png")).unwrap();
    mine.commit(&all, "Mine").unwrap();
    let conflicted = vec![
        "a.flow.json".to_owned(),
        "b.flow.json".into(),
        "gone.png".into(),
    ];
    assert_eq!(
        mine.pull(),
        Err(GitError::Conflict {
            paths: conflicted.clone()
        })
    );

    let versions = |base: &str, mine: Option<&str>, theirs: &str| ConflictVersions {
        base: Some(base.into()),
        mine: mine.map(Into::into),
        theirs: Some(theirs.into()),
    };
    assert_eq!(
        mine.conflict_versions("a.flow.json").unwrap(),
        versions("base a", Some("my a"), "their a")
    );
    assert_eq!(
        mine.conflict_versions("gone.png").unwrap(),
        versions("picture", None, "their picture")
    );

    // A file left unsettled undoes the merge.
    let head = sh(&git, &top, &["rev-parse", "HEAD"]);
    assert_eq!(
        mine.finish_pull(&[("a.flow.json".into(), Some(b"both a".to_vec()))]),
        Err(GitError::Conflict {
            paths: vec!["b.flow.json".into(), "gone.png".into()]
        })
    );
    assert_eq!(sh(&git, &top, &["rev-parse", "HEAD"]), head);
    assert_eq!(read(&mine.dir, "a.flow.json"), "my a");
    assert!(!top.join(".git/MERGE_HEAD").exists());
    assert!(mine.status().unwrap().changes.is_empty());

    let id = mine
        .finish_pull(&[
            ("a.flow.json".into(), Some(b"both a".to_vec())),
            ("b.flow.json".into(), Some(b"their b".to_vec())),
            ("gone.png".into(), None),
        ])
        .unwrap();
    assert_eq!(sh(&git, &top, &["rev-parse", "HEAD"]).trim(), id);
    assert_eq!(
        sh(&git, &top, &["rev-list", "--parents", "-n", "1", "HEAD"])
            .split(' ')
            .count(),
        3
    );
    assert_eq!(read(&mine.dir, "a.flow.json"), "both a");
    assert_eq!(read(&mine.dir, "b.flow.json"), "their b");
    assert!(!mine.dir.join("gone.png").exists());
    let status = mine.status().unwrap();
    assert!(status.changes.is_empty());
    assert_eq!((status.ahead, status.behind), (2, 0));
    mine.push().unwrap();

    // Nothing left to bring in.
    assert!(matches!(
        mine.finish_pull(&[]),
        Err(GitError::Failed { .. })
    ));
}

fn repo_switch_keeps_draft(repo: &Repo) {
    write(&repo.dir, "c.flow.json", "draft");
    repo.restore("HEAD", "b.flow.json", "b.flow.json").unwrap();
    repo.switch("feature").unwrap();
    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("feature"));
    assert_eq!(status.upstream.as_deref(), Some("origin/feature"));
    assert_eq!(read(&repo.dir, "c.flow.json"), "draft");
}

#[test]
fn server_addresses_read_as_host_and_path() {
    for (url, shown) in [
        ("git@github.com:acme/qa.git", "github.com/acme/qa"),
        ("https://github.com/acme/qa.git", "github.com/acme/qa"),
        ("https://token@github.com/acme/qa/", "github.com/acme/qa"),
        (
            "ssh://git@gitlab.example.com/team/qa.git",
            "gitlab.example.com/team/qa",
        ),
        ("/srv/git/qa.git", "/srv/git/qa"),
    ] {
        assert_eq!(display_url(url), shown, "{url}");
    }
}

#[test]
fn git_messages_become_errors_the_app_can_explain() {
    let cases = [
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            GitError::SignIn,
        ),
        (
            "git@github.com: Permission denied (publickey).",
            GitError::SignIn,
        ),
        (
            "ERROR: Repository not found.\nfatal: Could not read from remote repository.",
            GitError::RemoteNotFound,
        ),
        (
            "ssh: Could not resolve hostname github.com: nodename nor servname provided",
            GitError::Unreachable,
        ),
        (
            " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs",
            GitError::Rejected,
        ),
        (
            "error: Your local changes to the following files would be overwritten by merge:\n\tqa/a.flow.json\n\tqa/b.flow.json\nPlease commit your changes or stash them before you merge.",
            GitError::LocalChanges {
                paths: vec!["qa/a.flow.json".into(), "qa/b.flow.json".into()],
            },
        ),
        (
            "fatal: something else\nhint: try this",
            GitError::Failed {
                message: "something else".into(),
            },
        ),
    ];
    for (text, error) in cases {
        assert_eq!(classify(text), error, "{text}");
    }
}

#[test]
fn github_repositories_are_read_from_https_and_ssh_addresses() {
    for url in [
        "https://github.com/acme/qa.git",
        "https://github.com/acme/qa",
        "https://token@github.com/acme/qa/",
        "http://www.github.com/acme/qa.git",
        "git@github.com:acme/qa.git",
        "ssh://git@github.com/acme/qa.git",
        "ssh://git@ssh.github.com:443/acme/qa.git",
        "git://GitHub.com/acme/qa",
    ] {
        let repo = GitHubRepo::from_url(url);
        assert_eq!(
            repo.map(|r| r.full_name()).as_deref(),
            Some("acme/qa"),
            "{url}"
        );
    }
    for url in [
        "git@gitlab.com:acme/qa.git",
        "https://github.example.com/acme/qa.git",
        "https://github.com/acme",
        "https://github.com/acme/qa/tree/main",
        "https://github.com/acme/qa?x=1",
        "https://github.com/../qa",
        "/srv/git/qa.git",
    ] {
        assert_eq!(GitHubRepo::from_url(url), None, "{url}");
    }
}

/// A stand-in for GitHub's API on this computer: answers the requests in order with `replies`
/// and keeps each request, head and body, as it arrived.
struct Api {
    address: String,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Api {
    fn serve(replies: Vec<(u16, &str)>) -> Self {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = requests.clone();
        let replies: Vec<(u16, String)> = replies.into_iter().map(|(s, b)| (s, b.into())).collect();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    request.push_str(&line);
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                let mut content = vec![0; length];
                reader.read_exact(&mut content).unwrap();
                request.push_str(&String::from_utf8_lossy(&content));
                seen.lock().unwrap().push(request);
                let _ = write!(
                    reader.into_inner(),
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self { address, requests }
    }

    fn hub(&self) -> GitHub {
        GitHub::new(&self.address, "secret-token".into())
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

const PULL: &str = r#"{"number": 7, "title": "Check the login screen", "user": {"login": "ann"},
    "html_url": "https://github.com/acme/qa/pull/7", "draft": true,
    "head": {"ref": "feat/login", "sha": "1111111"}, "base": {"ref": "main", "sha": "2222222"},
    "updated_at": "2026-10-18T09:30:00Z"}"#;

fn acme() -> GitHubRepo {
    GitHubRepo::from_url("git@github.com:acme/qa.git").unwrap()
}

#[test]
fn github_requests_carry_the_token_only_in_their_header() {
    let api = Api::serve(vec![(200, r#"{"login": "ann", "name": "Ann Tester"}"#)]);
    let account = api.hub().account().unwrap();
    assert_eq!(
        account,
        GitHubAccount {
            login: "ann".into(),
            name: "Ann Tester".into()
        }
    );
    let request = &api.requests()[0];
    assert!(request.starts_with("GET /user HTTP/1.1\r\n"), "{request}");
    let lower = request.to_lowercase();
    assert!(lower.contains("authorization: bearer secret-token\r\n"));
    assert!(lower.contains("accept: application/vnd.github+json\r\n"));
    assert!(lower.contains("x-github-api-version: 2022-11-28\r\n"));
    assert_eq!(request.matches("secret-token").count(), 1);
}

#[test]
fn open_pull_requests_are_listed_page_by_page() {
    let full = format!("[{}]", vec![PULL; 100].join(","));
    let api = Api::serve(vec![(200, &full), (200, &format!("[{PULL}]"))]);
    let pulls = api.hub().pull_requests(&acme(), None).unwrap();
    assert_eq!(pulls.len(), 101);
    assert_eq!(
        pulls[0],
        PullRequest {
            number: 7,
            title: "Check the login screen".into(),
            author: "ann".into(),
            url: "https://github.com/acme/qa/pull/7".into(),
            head: "feat/login".into(),
            head_commit: "1111111".into(),
            base: "main".into(),
            base_commit: "2222222".into(),
            draft: true,
            updated: 1_792_315_800,
        }
    );
    let requests = api.requests();
    let line = |i: usize| requests[i].lines().next().unwrap().to_owned();
    assert_eq!(
        line(0),
        "GET /repos/acme/qa/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1 HTTP/1.1"
    );
    assert!(line(1).contains("&page=2 "));

    let api = Api::serve(vec![(200, "[]")]);
    api.hub().pull_requests(&acme(), Some("feat/a b")).unwrap();
    assert!(api.requests()[0].contains("&head=acme%3Afeat%2Fa%20b&"));
}

#[test]
fn a_new_pull_request_holds_exactly_the_users_words() {
    let api = Api::serve(vec![(201, PULL), (201, PULL)]);
    let hub = api.hub();
    let made = hub
        .create_pull_request(
            &acme(),
            "feat/login",
            "main",
            "Check the login screen",
            "Covers\n\"wrong password\".",
        )
        .unwrap();
    assert_eq!(made.number, 7);
    hub.create_pull_request(&acme(), "feat/login", "main", "Title", "  \n")
        .unwrap();
    let requests = api.requests();
    assert!(requests[0].starts_with("POST /repos/acme/qa/pulls HTTP/1.1\r\n"));
    let body = |i: usize| -> serde_json::Value {
        serde_json::from_str(requests[i].split("\r\n\r\n").nth(1).unwrap()).unwrap()
    };
    assert_eq!(
        body(0),
        serde_json::json!({"title": "Check the login screen", "head": "feat/login", "base": "main",
            "body": "Covers\n\"wrong password\"."})
    );
    assert_eq!(
        body(1),
        serde_json::json!({"title": "Title", "head": "feat/login", "base": "main"})
    );
}

#[test]
fn github_failures_become_errors_the_app_can_explain() {
    let api = Api::serve(vec![
        (401, r#"{"message": "Bad credentials"}"#),
        (
            403,
            r#"{"message": "Resource not accessible by integration"}"#,
        ),
        (404, r#"{"message": "Not Found"}"#),
        (
            422,
            r#"{"message": "Validation Failed", "errors": [{"resource": "PullRequest", "code": "custom", "message": "A pull request already exists for acme:feat/login."}]}"#,
        ),
        (
            422,
            r#"{"message": "Validation Failed", "errors": [{"resource": "PullRequest", "code": "custom", "message": "No commits between main and feat/login"}]}"#,
        ),
        (
            403,
            r#"{"message": "API rate limit exceeded for user ID 1."}"#,
        ),
        (500, ""),
    ]);
    let hub = api.hub();
    let create = || hub.create_pull_request(&acme(), "feat/login", "main", "T", "");
    assert_eq!(hub.account(), Err(GitError::GitHubSignedOut));
    assert_eq!(create(), Err(GitError::NoPermission));
    assert_eq!(hub.account(), Err(GitError::RemoteNotFound));
    assert_eq!(create(), Err(GitError::PullRequestExists));
    assert_eq!(
        create(),
        Err(GitError::NothingToPropose {
            base: "main".into()
        })
    );
    assert_eq!(
        hub.account(),
        Err(GitError::Failed {
            message: "API rate limit exceeded for user ID 1.".into()
        })
    );
    assert_eq!(
        hub.account(),
        Err(GitError::Failed {
            message: "GitHub answered with error 500".into()
        })
    );

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let offline = GitHub::new(&address, "secret-token".into());
    assert_eq!(offline.account(), Err(GitError::Unreachable));
}

#[test]
fn github_times_read_as_seconds() {
    assert_eq!(github::seconds("1970-01-01T00:00:00Z"), 0);
    assert_eq!(github::seconds("2000-02-29T12:00:00Z"), 951_825_600);
    assert_eq!(github::seconds("2026-10-18T09:30:00Z"), 1_792_315_800);
    assert_eq!(github::seconds("soon"), 0);
}

/// A project pushed to a server that git reaches by its GitHub address.
fn on_github(scratch: &Scratch) -> (Git, Repo, PathBuf) {
    let git = scratch.git();
    let server = scratch.0.join("server.git");
    fs::create_dir_all(&server).unwrap();
    sh(&git, &server, &["init", "--quiet", "--bare"]);
    let (_, repo) = repository(scratch);
    let top = repo.dir.join("..");
    sh(
        &git,
        &top,
        &["remote", "add", "origin", "https://github.com/acme/qa.git"],
    );
    let rewrite = format!("url.{}.insteadOf", server.display());
    sh(
        &git,
        &top,
        &["config", &rewrite, "https://github.com/acme/qa.git"],
    );
    write(&repo.dir, "login.flow.json", "1");
    write(&repo.dir, "refs/home.png", "png");
    repo.commit(&["login.flow.json".into(), "refs/home.png".into()], "Add")
        .unwrap();
    repo.push().unwrap();
    (git, repo, server)
}

#[test]
fn pull_requests_are_proposed_only_once_the_branch_is_on_github() {
    let scratch = Scratch::new("propose");
    let (_, repo, _) = on_github(&scratch);
    let status = repo.status().unwrap();
    assert_eq!(status.github.as_deref(), Some("acme/qa"));
    assert_eq!(repo.github().unwrap(), ("origin".into(), acme()));

    // Nothing reaches GitHub while the branch is behind.
    let nowhere = GitHub::new("http://127.0.0.1:9", "secret-token".into());
    repo.create_branch("feat/login").unwrap();
    assert_eq!(
        repo.create_pull_request(&nowhere, "main", "Login", ""),
        Err(GitError::NotPushed {
            branch: "feat/login".into()
        })
    );
    assert_eq!(
        repo.create_pull_request(&nowhere, "main", "  ", ""),
        Err(GitError::EmptyMessage)
    );
    repo.push().unwrap();
    write(&repo.dir, "login.flow.json", "2");
    repo.commit(&["login.flow.json".into()], "Change").unwrap();

    let api = Api::serve(vec![
        (200, r#"{"default_branch": "main"}"#),
        (
            200,
            r#"[{"name": "main"}, {"name": "feat/login"}, {"name": "Beta"}]"#,
        ),
        (200, &format!("[{PULL}]")),
        (201, PULL),
    ]);
    let draft = repo.pull_request_draft(&api.hub()).unwrap();
    assert_eq!(draft.branch, "feat/login");
    assert_eq!(draft.base, "main");
    assert_eq!(draft.bases, ["Beta", "main"]);
    assert!(draft.unpushed);
    assert_eq!(draft.existing.map(|p| p.number), Some(7));
    assert_eq!(draft.title, "Change", "one new commit gives its title");
    repo.push().unwrap();
    repo.create_pull_request(&api.hub(), "main", " Login ", "Why")
        .unwrap();
    let requests = api.requests();
    assert!(requests[0].starts_with("GET /repos/acme/qa HTTP/1.1"));
    assert!(requests[1].starts_with("GET /repos/acme/qa/branches?per_page=100&page=1 "));
    assert!(requests[2].contains("&head=acme%3Afeat%2Flogin&"));
    assert!(
        requests[3]
            .ends_with(r#"{"base":"main","body":"Why","head":"feat/login","title":"Login"}"#)
    );
}

#[test]
fn a_pull_request_shows_the_project_files_it_changes_after_downloading_it() {
    let scratch = Scratch::new("changes");
    let (git, repo, server) = on_github(&scratch);
    let base = sh(&git, &repo.dir, &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    repo.create_branch("feat/login").unwrap();
    write(&repo.dir, "login.flow.json", "2");
    write(&repo.dir, "new.flow.json", "3");
    write(
        &repo.dir.join(".."),
        "app.swift",
        "changed outside the project",
    );
    fs::remove_file(repo.dir.join("refs/home.png")).unwrap();
    repo.commit(
        &[
            "login.flow.json".into(),
            "new.flow.json".into(),
            "refs/home.png".into(),
        ],
        "Change",
    )
    .unwrap();
    sh(&git, &repo.dir.join(".."), &["add", "app.swift"]);
    sh(
        &git,
        &repo.dir.join(".."),
        &["commit", "--quiet", "-m", "Outside"],
    );
    repo.push().unwrap();
    let head = sh(&git, &repo.dir, &["rev-parse", "HEAD"])
        .trim()
        .to_owned();

    // A teammate who has never seen the branch.
    let server_path = server.to_str().unwrap();
    sh(
        &git,
        &scratch.0,
        &[
            "clone",
            "--quiet",
            "--no-local",
            "--single-branch",
            "--branch",
            "main",
            server_path,
            "theirs",
        ],
    );
    let theirs_top = scratch.0.join("theirs");
    sh(
        &git,
        &theirs_top,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/qa.git",
        ],
    );
    let rewrite = format!("url.{}.insteadOf", server.display());
    sh(
        &git,
        &theirs_top,
        &["config", &rewrite, "https://github.com/acme/qa.git"],
    );
    let theirs = git.open(&theirs_top.join("qa")).unwrap().unwrap();
    let commit = format!("{head}^{{commit}}");
    let has_head = |repo: &Repo| {
        repo.output(&["cat-file", "-e", &commit])
            .unwrap()
            .status
            .success()
    };
    assert!(!has_head(&theirs));

    let changes = theirs.pull_request_changes(&base, &head).unwrap();
    assert_eq!(changes.before, base);
    assert_eq!(changes.after, head);
    assert_eq!(
        changes.files,
        [
            change("login.flow.json", ChangeKind::Modified),
            change("new.flow.json", ChangeKind::Added),
            change("refs/home.png", ChangeKind::Deleted),
        ]
    );
    assert_eq!(
        theirs.file(&head, "login.flow.json").unwrap(),
        Some(b"2".to_vec())
    );
    assert!(has_head(&theirs));
    assert!(matches!(
        theirs.pull_request_changes("--upload-pack=x", &head),
        Err(GitError::Failed { .. })
    ));
}
