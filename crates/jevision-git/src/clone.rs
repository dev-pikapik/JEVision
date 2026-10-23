//! Copying a repository from a server to a new project folder.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, io, thread};

use crate::{Git, GitError, GitHubRepo, Repo, classify};

/// How often a copy in progress looks whether it was asked to stop.
const CANCEL_CHECK: Duration = Duration::from_millis(50);

/// How long git's last messages may take to arrive after it exits.
const LAST_WORDS: Duration = Duration::from_millis(500);

/// Why [`Git::clone_repository`] didn't copy a repository. Nothing it made is left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneFailure {
    /// It was asked to stop.
    Cancelled,
    /// The folder to copy into is there and has something in it.
    FolderExists,
    /// git didn't copy it, for the reason in `error`. `detail` is git's own line about it:
    /// the cause it named, else the last line it wrote; empty when it wrote nothing.
    Git { error: GitError, detail: String },
}

impl From<GitError> for CloneFailure {
    fn from(error: GitError) -> Self {
        Self::Git {
            error,
            detail: String::new(),
        }
    }
}

impl Git {
    /// Copies the repository at `url` into the folder `into`, which must not be there yet or
    /// be empty, making the folders above it as needed.
    ///
    /// Nothing is asked: the user's GitHub sign-in is on offer, and where git or ssh would
    /// want an answer (a password, a passphrase) the copy fails instead. A server reached over
    /// ssh for the first time is trusted, as answering “yes” would; one whose key changed is
    /// not. While git works, `cancelled` is looked at every few hundredths of a second: once
    /// it's set, git is stopped. On any failure the half-made copy is removed.
    pub fn clone_repository(
        &self,
        url: &str,
        into: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Repo, CloneFailure> {
        let existed = match fs::read_dir(into) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    return Err(CloneFailure::FolderExists);
                }
                true
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(_) => return Err(CloneFailure::FolderExists),
        };
        let parent = into.parent().unwrap_or(into);
        fs::create_dir_all(parent).map_err(|e| GitError::Failed {
            message: e.to_string(),
        })?;
        let mut command = self.command(parent);
        self.offer_github_sign_in(&mut command, GitHubRepo::from_url(url).is_some());
        if let Some(ssh) = self.ssh_command(parent) {
            command.env("GIT_SSH_COMMAND", ssh);
        }
        command
            .args(["clone", "--"])
            .arg(url)
            .arg(into)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let result = self.run_clone(command, cancelled);
        if result.is_err() {
            if existed {
                let _ = fs::remove_dir_all(into).and_then(|()| fs::create_dir(into));
            } else {
                let _ = fs::remove_dir_all(into);
            }
        }
        result?;
        self.open(into)?
            .ok_or(CloneFailure::from(GitError::NotRepository))
    }

    /// Runs `git clone` in its own process group, so that stopping it stops ssh and the
    /// helpers it started too.
    fn run_clone(&self, mut command: Command, cancelled: &AtomicBool) -> Result<(), CloneFailure> {
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().map_err(|e| {
            if self.exe.is_file() {
                GitError::Failed {
                    message: e.to_string(),
                }
            } else {
                GitError::NotInstalled
            }
        })?;
        // Lines come through a channel rather than one read to the end: a helper git leaves
        // running, such as a shared ssh connection, may hold the pipe open after git exits.
        let stderr = child.stderr.take().expect("stderr is piped");
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let status = loop {
            if cancelled.load(Ordering::Relaxed) {
                if let Ok(pid) = i32::try_from(child.id()) {
                    // SAFETY: `kill` only sends a signal; a negative id names the process
                    // group git leads.
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(CloneFailure::Cancelled);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(CANCEL_CHECK),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(GitError::Failed {
                        message: e.to_string(),
                    }
                    .into());
                }
            }
        };
        if status.success() {
            return Ok(());
        }
        let text: Vec<String> =
            std::iter::from_fn(|| lines.recv_timeout(LAST_WORDS).ok()).collect();
        // git ends some failures with advice; the cause is its first `fatal:` line.
        let mut lines = text.iter().map(|line| line.trim());
        let detail = lines
            .clone()
            .find_map(|line| line.strip_prefix("fatal: "))
            .or_else(|| lines.rfind(|line| !line.is_empty()))
            .unwrap_or_default();
        Err(CloneFailure::Git {
            error: classify(&text.join("\n")),
            detail: detail.to_owned(),
        })
    }

    /// How git should run ssh so that it never waits for an answer: the user's own ssh
    /// command, if they set one, with questions turned off. `None` when they set `GIT_SSH`,
    /// a program git runs as it is.
    fn ssh_command(&self, dir: &Path) -> Option<String> {
        let set = |key: &str| {
            self.env
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .or_else(|| std::env::var(key).ok())
                .filter(|v| !v.trim().is_empty())
        };
        let own = set("GIT_SSH_COMMAND").or_else(|| {
            self.output(dir, &["config", "--get", "core.sshCommand"])
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
                .filter(|command| !command.is_empty())
        });
        if own.is_none() && set("GIT_SSH").is_some() {
            return None;
        }
        let ssh = own.unwrap_or_else(|| "ssh".to_owned());
        Some(format!(
            "{ssh} -o BatchMode=yes -o StrictHostKeyChecking=accept-new"
        ))
    }
}

/// The name of the folder `git clone` copies the repository at `url` into:
/// `https://github.com/acme/shop-tests.git` and `git@github.com:acme/shop-tests` are both
/// `shop-tests`. Empty when the address has no name in it.
pub fn clone_folder_name(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let url = url
        .strip_suffix(".git")
        .unwrap_or(url)
        .trim_end_matches('/');
    url.rsplit(['/', ':']).next().unwrap_or_default().to_owned()
}

/// The repository address `text` is, such as what the user copied, to offer for a copy: one
/// word that is an address `git clone` reaches over the network or on disk, with a path after
/// the server and a folder name at its end. `https://`, `http://`, `ssh://`, `git://` and
/// `file://` addresses and the `git@github.com:acme/shop.git` kind count; a sentence, a bare
/// web site or a folder path doesn't. `None` when it isn't one.
pub fn repository_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let schemes = ["https://", "http://", "ssh://", "git://", "file://"];
    let (server, path) = match schemes.iter().find_map(|scheme| {
        let start = text.get(..scheme.len())?;
        start
            .eq_ignore_ascii_case(scheme)
            .then(|| (&text[scheme.len()..], *scheme == "file://"))
    }) {
        Some((rest, on_disk)) => {
            let (server, path) = rest.split_once('/')?;
            (if on_disk { "disk" } else { server }, path)
        }
        None => {
            let (user_server, path) = text.split_once(':')?;
            let (user, server) = user_server.split_once('@')?;
            if user.is_empty() || user_server.contains('/') {
                return None;
            }
            (server, path)
        }
    };
    let named = !clone_folder_name(text).is_empty();
    (!server.is_empty() && !path.trim_matches('/').is_empty() && named).then(|| text.to_owned())
}
