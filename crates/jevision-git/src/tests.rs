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
