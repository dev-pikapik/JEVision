//! Getting block packages from where people keep them: a folder, a zip file, or an address on the
//! web, such as a GitHub repository or release. The package is unpacked into a folder of its own
//! in the temporary files, where it can be read and shown before anything enters a project; the
//! rest of the work is in [`jevision_core`].
//!
//! Nothing is run. A zip can't write outside its folder or unpack to more than the limits
//! below, and an address must be `https` all the way, redirects included.

use std::fs;
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use jevision_core::{MANIFEST_FILE, PackageError, parse_manifest};
use serde_json::Value;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

use crate::import::{copy_counted, zip_invalid};

/// A downloaded package is refused above this many bytes.
const MAX_DOWNLOAD: u64 = 64 << 20;
/// An archive, which may be a whole repository around the package, unpacks to at most this.
const MAX_UNPACKED: u64 = 256 << 20;
/// An archive holds at most this many entries.
const MAX_ENTRIES: usize = 20_000;
/// How deep in an archive or a folder a package is looked for.
const MAX_SEARCH_DEPTH: usize = 3;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Staging folders older than this are removed when a new one is made.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const STAGING_PREFIX: &str = "jevision-block-";
const STAGED_FOLDER: &str = "package";

/// A package ready to be read and installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// The folder with `block.json` and `flow.json`.
    pub folder: PathBuf,
    /// Whether `folder` was made by [`stage_source`] in the temporary files, to be removed with
    /// [`discard_staged`]; a folder the user pointed at is never removed.
    pub temporary: bool,
}

/// Makes a package from `source` ready to be read: a package folder, a folder with one inside, a
/// `.zip` file, or an `https` address of a zip, a GitHub repository, a release or a branch.
pub fn stage_source(source: &str) -> Result<Staged, PackageError> {
    stage(source, &Network::Internet)
}

/// Removes what [`stage_source`] unpacked into the temporary files. Does nothing for any other
/// folder.
pub fn discard_staged(folder: &Path) {
    if let Some(root) = staging_root(folder) {
        let _ = fs::remove_dir_all(root);
    }
}

/// The folder a staged package lives in, if `folder` is one.
fn staging_root(folder: &Path) -> Option<&Path> {
    let root = folder.parent()?;
    let name = root.file_name()?.to_str()?;
    (folder.file_name()? == STAGED_FOLDER
        && name.starts_with(STAGING_PREFIX)
        && root.parent()? == std::env::temp_dir().as_path())
    .then_some(root)
}

/// Writes the package folder `folder` as a zip file `to`, holding the folder itself, so that
/// unpacking it gives `ID.jevblock` again.
pub fn zip_package(folder: &Path, to: &Path) -> Result<(), PackageError> {
    let failed =
        |e: &dyn ToString| PackageError::new("block.export.failed", &[("reason", &e.to_string())]);
    let top = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| failed(&"the package has no name"))?;
    let partial = to.with_extension("zip.partial");
    let result = (|| -> io::Result<()> {
        let mut writer = zip::ZipWriter::new(fs::File::create(&partial)?);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut files = Vec::new();
        collect_files(folder, "", &mut files)?;
        files.sort();
        for path in files {
            writer
                .start_file(format!("{top}/{path}"), options)
                .map_err(io::Error::other)?;
            writer.write_all(&fs::read(folder.join(&path))?)?;
        }
        writer.finish().map_err(io::Error::other)?;
        fs::rename(&partial, to)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&partial);
        return Err(failed(&e));
    }
    Ok(())
}

fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_files(&entry.path(), &format!("{prefix}{name}/"), out)?;
        } else if kind.is_file() {
            out.push(format!("{prefix}{name}"));
        }
    }
    Ok(())
}

/// Where addresses are fetched from; a test points them at a server of its own.
enum Network {
    Internet,
    #[cfg(test)]
    Local,
}

impl Network {
    fn https_only(&self) -> bool {
        matches!(self, Self::Internet)
    }
}

fn stage(source: &str, network: &Network) -> Result<Staged, PackageError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(PackageError::new("block.source.missing", &[("source", "")]));
    }
    if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("github.com/")
    {
        let url = if source.starts_with("github.com/") {
            format!("https://{source}")
        } else {
            source.to_owned()
        };
        let zip = download_package(&url, network)?;
        return stage_zip(Cursor::new(zip));
    }
    let path = Path::new(source);
    if path.is_dir() {
        return find_package(path, false).map(|folder| Staged {
            folder,
            temporary: false,
        });
    }
    if path.is_file() && source.to_ascii_lowercase().ends_with(".zip") {
        let file = fs::File::open(path).map_err(|e| {
            PackageError::new(
                "block.file.unreadable",
                &[("file", source), ("reason", &e.to_string())],
            )
        })?;
        return stage_zip(file);
    }
    if path.exists() {
        return Err(PackageError::new(
            "block.source.unsupported",
            &[("source", source)],
        ));
    }
    if source.contains("://") {
        return Err(PackageError::new(
            "block.source.unsupported",
            &[("source", source)],
        ));
    }
    Err(PackageError::new(
        "block.source.missing",
        &[("source", source)],
    ))
}

/// Unpacks `zip` into a new staging folder and finds the package in it.
fn stage_zip(zip: impl Read + Seek) -> Result<Staged, PackageError> {
    let root = new_staging_root()
        .map_err(|e| PackageError::new("block.archive.invalid", &[("reason", &e.to_string())]))?;
    let unpacked = root.join("unpacked");
    let found = unpack_archive(zip, &unpacked).and_then(|()| find_package(&unpacked, true));
    let result = found.and_then(|found| {
        let folder = root.join(STAGED_FOLDER);
        fs::rename(&found, &folder).map_err(|e| {
            PackageError::new("block.archive.invalid", &[("reason", &e.to_string())])
        })?;
        Ok(Staged {
            folder,
            temporary: true,
        })
    });
    let _ = fs::remove_dir_all(&unpacked);
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
}

fn new_staging_root() -> io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let temp = std::env::temp_dir();
    sweep(&temp);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let root = temp.join(format!(
        "{STAGING_PREFIX}{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Removes staging folders that were forgotten, such as after the app quit in the middle.
fn sweep(temp: &Path) {
    let Ok(entries) = fs::read_dir(temp) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_AFTER);
        if old
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(STAGING_PREFIX)
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Writes the files of `zip` into `to`. An entry that would end up outside `to`, an archive with
/// too many entries and one that unpacks to too much are refused; links are left out.
fn unpack_archive(zip: impl Read + Seek, to: &Path) -> Result<(), PackageError> {
    let invalid =
        |e: io::Error| PackageError::new("block.archive.invalid", &[("reason", &e.to_string())]);
    let mut archive = ZipArchive::new(zip).map_err(|e| invalid(zip_invalid(e)))?;
    if archive.len() > MAX_ENTRIES {
        return Err(PackageError::new(
            "block.package.tooManyFiles",
            &[("max", &MAX_ENTRIES.to_string())],
        ));
    }
    fs::create_dir_all(to).map_err(invalid)?;
    let mut left = MAX_UNPACKED;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| invalid(zip_invalid(e)))?;
        if entry.is_symlink() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            return Err(PackageError::new(
                "block.path.unsafe",
                &[("path", entry.name())],
            ));
        };
        if name
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == "__MACOSX")
        {
            continue;
        }
        let target = to.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(invalid)?;
            continue;
        }
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder).map_err(invalid)?;
        }
        let mut file = fs::File::create(&target).map_err(invalid)?;
        if !copy_counted(&mut entry, &mut file, &mut left).map_err(invalid)? {
            return Err(PackageError::new(
                "block.package.tooLarge",
                &[("max", &MAX_UNPACKED.to_string())],
            ));
        }
    }
    Ok(())
}

/// The package in `root`: `root` itself if it is one, otherwise the shallowest folder below it
/// that is, as long as there is only one at that depth.
fn find_package(root: &Path, archive: bool) -> Result<PathBuf, PackageError> {
    let mut level = vec![root.to_path_buf()];
    for depth in 0..=MAX_SEARCH_DEPTH {
        let found: Vec<&PathBuf> = level
            .iter()
            .filter(|dir| dir.join(MANIFEST_FILE).is_file())
            .collect();
        match found.as_slice() {
            [] => {}
            [one] => return Ok((*one).clone()),
            many => {
                let ids: Vec<String> = many.iter().map(|dir| package_name(dir)).collect();
                return Err(PackageError::new(
                    "block.archive.several",
                    &[("ids", &ids.join(", "))],
                ));
            }
        }
        if depth == MAX_SEARCH_DEPTH {
            break;
        }
        let mut next = Vec::new();
        for dir in &level {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            let mut children: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|e| {
                    e.file_type().is_ok_and(|t| t.is_dir())
                        && !e.file_name().to_string_lossy().starts_with('.')
                })
                .map(|e| e.path())
                .collect();
            children.sort();
            next.extend(children);
        }
        level = next;
    }
    Err(if archive {
        PackageError::new("block.archive.empty", &[])
    } else {
        PackageError::new(
            "block.folder.notPackage",
            &[("path", &root.display().to_string())],
        )
    })
}

/// What a package is called when it can't be told by its id: the id if `block.json` says one,
/// else the name of its folder.
fn package_name(dir: &Path) -> String {
    fs::read_to_string(dir.join(MANIFEST_FILE))
        .ok()
        .and_then(|text| parse_manifest(&text).ok())
        .map(|m| m.id)
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

// MARK: Addresses

/// Downloads the zip a package comes in from `url`.
fn download_package(url: &str, network: &Network) -> Result<Vec<u8>, PackageError> {
    let address = match network {
        Network::Internet => parse_address(url)?,
        #[cfg(test)]
        Network::Local => Address::Zip(url.to_owned()),
    };
    let zip_url = match address {
        Address::Zip(url) => url,
        Address::GitHub { owner, repo, what } => github_zip(&owner, &repo, &what, network)?,
    };
    let (status, body) = get(&zip_url, MAX_DOWNLOAD, "application/zip", network)?;
    if status != 200 {
        return Err(failed_download(
            &zip_url,
            &format!("the server answered {status}"),
        ));
    }
    Ok(body)
}

#[derive(Debug, PartialEq, Eq)]
enum Address {
    Zip(String),
    GitHub {
        owner: String,
        repo: String,
        what: Pick,
    },
}

/// Which version of a repository to take.
#[derive(Debug, PartialEq, Eq)]
enum Pick {
    LatestRelease,
    Release(String),
    /// A branch, tag or commit, as the code of the repository was at that point.
    Code(String),
}

fn unsupported(url: &str) -> PackageError {
    PackageError::new("block.source.unsupported", &[("source", url)])
}

fn failed_download(url: &str, reason: &str) -> PackageError {
    PackageError::new("block.download.failed", &[("url", url), ("reason", reason)])
}

/// What an `https` address points at. Anything but a GitHub repository, release, branch or a
/// link to a `.zip` is refused rather than guessed at.
fn parse_address(url: &str) -> Result<Address, PackageError> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(PackageError::new(
            "block.download.notHttps",
            &[("url", url)],
        ));
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if host != "github.com" && host != "www.github.com" {
        return if path.to_ascii_lowercase().ends_with(".zip") {
            Ok(Address::Zip(format!("https://{host}/{path}")))
        } else {
            Err(unsupported(url))
        };
    }
    let name_ok = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    let (Some(owner), Some(repo)) = (parts.first(), parts.get(1)) else {
        return Err(unsupported(url));
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if !name_ok(owner) || !name_ok(repo) {
        return Err(unsupported(url));
    }
    let (owner, repo) = ((*owner).to_owned(), repo.to_owned());
    let reference = |tail: &[&str]| {
        let joined = tail.join("/");
        (!tail.is_empty() && tail.iter().all(|p| name_ok(p))).then_some(joined)
    };
    let what = match &parts[2..] {
        [] | ["releases"] | ["releases", "latest"] => Pick::LatestRelease,
        ["releases", "tag", tag] if name_ok(tag) => Pick::Release((*tag).to_owned()),
        ["releases", "download", ..] | ["archive", ..] => {
            return if path.to_ascii_lowercase().ends_with(".zip") {
                Ok(Address::Zip(format!("https://{host}/{path}")))
            } else {
                Err(unsupported(url))
            };
        }
        ["tree", tail @ ..] => match reference(tail) {
            Some(reference) => Pick::Code(reference),
            None => return Err(unsupported(url)),
        },
        _ => return Err(unsupported(url)),
    };
    Ok(Address::GitHub { owner, repo, what })
}

/// The zip to download for a GitHub repository: the package file a release carries
/// (`NAME.jevblock.zip`) if it has one, otherwise the code of the release, or of the repository
/// if it has no release.
fn github_zip(
    owner: &str,
    repo: &str,
    what: &Pick,
    network: &Network,
) -> Result<String, PackageError> {
    let code =
        |reference: &str| format!("https://github.com/{owner}/{repo}/archive/{reference}.zip");
    let api = match what {
        Pick::LatestRelease => {
            format!("https://api.github.com/repos/{owner}/{repo}/releases/latest")
        }
        Pick::Release(tag) => {
            format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}")
        }
        Pick::Code(reference) => return Ok(code(reference)),
    };
    let (status, body) = get(&api, 1 << 20, "application/vnd.github+json", network)?;
    match status {
        200 => {
            let release: Value = serde_json::from_slice(&body)
                .map_err(|_| failed_download(&api, "GitHub sent an answer that can't be read"))?;
            release_zip(&release)
                .ok_or_else(|| failed_download(&api, "the release has no files to download"))
        }
        404 => Ok(match what {
            Pick::Release(tag) => code(&format!("refs/tags/{tag}")),
            _ => code("HEAD"),
        }),
        403 | 429 => Err(failed_download(
            &api,
            "GitHub is limiting requests from this computer; try again in a few minutes",
        )),
        other => Err(failed_download(&api, &format!("GitHub answered {other}"))),
    }
}

/// The package file of a release if it has one, otherwise the code of the release.
fn release_zip(release: &Value) -> Option<String> {
    let asset = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|asset| {
            let name = asset["name"].as_str()?.to_ascii_lowercase();
            let url = asset["browser_download_url"].as_str()?;
            name.ends_with(".jevblock.zip").then(|| url.to_owned())
        });
    asset.or_else(|| release["zipball_url"].as_str().map(str::to_owned))
}

/// Fetches `url`: the status and up to `limit` bytes of the body.
fn get(
    url: &str,
    limit: u64,
    accept: &str,
    network: &Network,
) -> Result<(u16, Vec<u8>), PackageError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(network.https_only())
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .user_agent(concat!("JEVision/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Accept", accept)
        .call()
        .map_err(|e| match e {
            ureq::Error::RequireHttpsOnly(address) => {
                PackageError::new("block.download.notHttps", &[("url", &address)])
            }
            other => failed_download(url, &other.to_string()),
        })?;
    let status = response.status().as_u16();
    // One byte past the limit is enough to know that the answer is too large.
    let mut body = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit(u64::MAX)
        .reader()
        .take(limit.saturating_add(1))
        .read_to_end(&mut body)
        .map_err(|e| failed_download(url, &e.to_string()))?;
    if body.len() as u64 > limit {
        return Err(PackageError::new(
            "block.download.tooLarge",
            &[("url", url), ("max", &limit.to_string())],
        ));
    }
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-packages-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const MANIFEST: &str = r#"{"format":1,"id":"com.acme.sign-in","name":"Sign In","version":"1.0.0",
        "author":"Acme","license":"MIT"}"#;
    const STEPS: &str = r#"{"format":1,"name":"Sign In","app":"com.example.App","start":"a","blocks":{
        "a":{"type":"pressKeys","keys":"return"}}}"#;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn package_zip(prefix: &str) -> Vec<u8> {
        zip_of(&[
            (&format!("{prefix}block.json"), MANIFEST.as_bytes()),
            (&format!("{prefix}flow.json"), STEPS.as_bytes()),
        ])
    }

    fn code(e: PackageError) -> String {
        e.issues[0].code.clone()
    }

    #[test]
    fn a_zip_of_a_package_is_unpacked_wherever_the_package_sits_in_it() {
        for prefix in [
            "",
            "repo-main/",
            "repo-main/blocks/com.acme.sign-in.jevblock/",
        ] {
            let staged = stage_zip(Cursor::new(package_zip(prefix))).unwrap();
            assert!(staged.temporary);
            assert!(staged.folder.join("flow.json").is_file(), "{prefix}");
            assert!(staging_root(&staged.folder).is_some());
            discard_staged(&staged.folder);
            assert!(!staged.folder.exists());
        }
    }

    #[test]
    fn a_zip_that_leaves_its_folder_is_refused() {
        for name in ["../escape.txt", "a/../../escape.txt"] {
            let zip = zip_of(&[("block.json", MANIFEST.as_bytes()), (name, b"x")]);
            assert_eq!(
                code(stage_zip(Cursor::new(zip)).unwrap_err()),
                "block.path.unsafe",
                "{name}"
            );
        }
        assert!(!std::env::temp_dir().join("escape.txt").exists());
    }

    #[test]
    fn a_zip_that_unpacks_to_too_much_is_refused_by_what_comes_out() {
        let big = vec![0u8; 1 << 20];
        let zip = zip_of(&[("big.bin", &big)]);
        let mut left = (1u64 << 20) - 1;
        let mut out = Vec::new();
        let mut archive = ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut entry = archive.by_index(0).unwrap();
        assert!(!copy_counted(&mut entry, &mut out, &mut left).unwrap());
        assert!(out.len() < 1 << 20);
    }

    #[test]
    fn broken_and_empty_archives_say_so() {
        assert_eq!(
            code(stage_zip(Cursor::new(b"not a zip".to_vec())).unwrap_err()),
            "block.archive.invalid"
        );
        let empty = zip_of(&[("readme.md", b"hello")]);
        assert_eq!(
            code(stage_zip(Cursor::new(empty)).unwrap_err()),
            "block.archive.empty"
        );
        let two = zip_of(&[
            ("a/block.json", MANIFEST.as_bytes()),
            ("b/block.json", MANIFEST.as_bytes()),
        ]);
        let error = stage_zip(Cursor::new(two)).unwrap_err();
        assert_eq!(code(error.clone()), "block.archive.several");
        assert_eq!(
            error.issues[0].param("ids"),
            Some("com.acme.sign-in, com.acme.sign-in")
        );
        let deep = zip_of(&[("a/b/c/d/e/block.json", MANIFEST.as_bytes())]);
        assert_eq!(
            code(stage_zip(Cursor::new(deep)).unwrap_err()),
            "block.archive.empty"
        );
    }

    #[test]
    fn the_shallowest_package_wins_over_examples_in_a_repository() {
        let zip = zip_of(&[
            ("repo/block.json", MANIFEST.as_bytes()),
            ("repo/flow.json", STEPS.as_bytes()),
            ("repo/examples/x.jevblock/block.json", MANIFEST.as_bytes()),
        ]);
        let staged = stage_zip(Cursor::new(zip)).unwrap();
        assert!(staged.folder.join("flow.json").is_file());
        discard_staged(&staged.folder);
    }

    #[test]
    fn folders_and_files_are_found_by_their_paths() {
        let dir = temp("sources");
        let package = dir.join("pkg.jevblock");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("block.json"), MANIFEST).unwrap();
        fs::write(package.join("flow.json"), STEPS).unwrap();
        let direct = stage(package.to_str().unwrap(), &Network::Internet).unwrap();
        assert_eq!(
            (direct.folder.clone(), direct.temporary),
            (package.clone(), false)
        );
        let around = stage(dir.to_str().unwrap(), &Network::Internet).unwrap();
        assert_eq!(around.folder, package);
        discard_staged(&package);
        assert!(package.exists());
        let zip = dir.join("pkg.zip");
        fs::write(&zip, package_zip("x/")).unwrap();
        let staged = stage(zip.to_str().unwrap(), &Network::Internet).unwrap();
        assert!(staged.temporary);
        discard_staged(&staged.folder);
        let empty = dir.join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(
            code(stage(empty.to_str().unwrap(), &Network::Internet).unwrap_err()),
            "block.folder.notPackage"
        );
        assert_eq!(
            code(stage(dir.join("nope").to_str().unwrap(), &Network::Internet).unwrap_err()),
            "block.source.missing"
        );
        assert_eq!(
            code(stage("", &Network::Internet).unwrap_err()),
            "block.source.missing"
        );
        fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(
            code(stage(dir.join("a.txt").to_str().unwrap(), &Network::Internet).unwrap_err()),
            "block.source.unsupported"
        );
        assert_eq!(
            code(stage("ftp://example.com/a.zip", &Network::Internet).unwrap_err()),
            "block.source.unsupported"
        );
        assert_eq!(
            code(stage("http://example.com/a.zip", &Network::Internet).unwrap_err()),
            "block.download.notHttps"
        );
    }

    #[test]
    fn addresses_are_told_apart() {
        let github = |url: &str| parse_address(url).unwrap();
        let repo = |what| Address::GitHub {
            owner: "acme".into(),
            repo: "blocks".into(),
            what,
        };
        assert_eq!(
            github("https://github.com/acme/blocks"),
            repo(Pick::LatestRelease)
        );
        assert_eq!(
            github("https://github.com/acme/blocks.git"),
            repo(Pick::LatestRelease)
        );
        assert_eq!(
            github("https://github.com/acme/blocks/"),
            repo(Pick::LatestRelease)
        );
        assert_eq!(
            github("https://github.com/acme/blocks/releases"),
            repo(Pick::LatestRelease)
        );
        assert_eq!(
            github("https://github.com/acme/blocks/releases/latest?x=1"),
            repo(Pick::LatestRelease)
        );
        assert_eq!(
            github("https://github.com/acme/blocks/releases/tag/v1.2.0"),
            repo(Pick::Release("v1.2.0".into()))
        );
        assert_eq!(
            github("https://github.com/acme/blocks/tree/feature/x"),
            repo(Pick::Code("feature/x".into()))
        );
        assert_eq!(
            github("https://github.com/acme/blocks/releases/download/v1/sign-in.jevblock.zip"),
            Address::Zip(
                "https://github.com/acme/blocks/releases/download/v1/sign-in.jevblock.zip".into()
            )
        );
        assert_eq!(
            github("https://example.com/files/sign-in.ZIP#top"),
            Address::Zip("https://example.com/files/sign-in.ZIP".into())
        );
        for bad in [
            "https://example.com/blocks",
            "https://github.com/acme",
            "https://github.com/acme/blocks/issues",
            "https://github.com/acme/blocks/tree/../x",
            "https://github.com/ac me/blocks",
            "https://github.com/acme/blocks/releases/tag/a/b",
        ] {
            assert_eq!(
                code(parse_address(bad).unwrap_err()),
                "block.source.unsupported",
                "{bad}"
            );
        }
        assert_eq!(
            code(parse_address("http://github.com/acme/blocks").unwrap_err()),
            "block.download.notHttps"
        );
    }

    #[test]
    fn a_release_gives_its_package_file_or_else_its_code() {
        let release = serde_json::json!({
            "zipball_url": "https://api.github.com/repos/acme/blocks/zipball/v1",
            "assets": [
                {"name": "notes.zip", "browser_download_url": "https://x/notes.zip"},
                {"name": "Sign-In.jevblock.zip", "browser_download_url": "https://x/sign-in.jevblock.zip"}
            ]
        });
        assert_eq!(
            release_zip(&release).as_deref(),
            Some("https://x/sign-in.jevblock.zip")
        );
        let bare = serde_json::json!({"zipball_url": "https://api.github.com/zip", "assets": []});
        assert_eq!(
            release_zip(&bare).as_deref(),
            Some("https://api.github.com/zip")
        );
        assert_eq!(release_zip(&serde_json::json!({})), None);
    }

    /// A web server on this computer: answers each request with the next of `answers`, each a
    /// whole HTTP response, and keeps the paths asked for.
    fn serve(answers: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                asked.push(
                    line.split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned(),
                );
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                }
                stream.write_all(&answer).unwrap();
            }
            asked
        });
        (address, handle)
    }

    fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_package_is_downloaded_through_a_redirect() {
        let zip = package_zip("repo/");
        let (address, server) = serve(vec![
            response("302 Found", "Location: /real.zip\r\n", b""),
            response("200 OK", "", &zip),
        ]);
        let url = format!("{address}/start.zip");
        let staged = stage(&url, &Network::Local).unwrap();
        assert!(staged.folder.join("block.json").is_file());
        discard_staged(&staged.folder);
        assert_eq!(server.join().unwrap(), ["/start.zip", "/real.zip"]);
    }

    #[test]
    fn a_download_that_fails_or_is_too_large_says_so() {
        let (address, server) = serve(vec![response("404 Not Found", "", b"gone")]);
        let error = stage(&format!("{address}/a.zip"), &Network::Local).unwrap_err();
        assert_eq!(code(error.clone()), "block.download.failed");
        assert!(error.issues[0].param("reason").unwrap().contains("404"));
        server.join().unwrap();
        let (address, server) = serve(vec![response("200 OK", "", &vec![1u8; 2000])]);
        let url = format!("{address}/a.zip");
        let error = get(&url, 1000, "application/zip", &Network::Local).unwrap_err();
        assert_eq!(code(error), "block.download.tooLarge");
        server.join().unwrap();
        assert_eq!(
            code(stage("https://127.0.0.1:1/a.zip", &Network::Internet).unwrap_err()),
            "block.download.failed"
        );
    }

    #[test]
    fn only_https_addresses_are_fetched() {
        let error = get(
            "http://127.0.0.1:1/a.zip",
            1000,
            "application/zip",
            &Network::Internet,
        )
        .unwrap_err();
        assert_eq!(code(error), "block.download.notHttps");
    }

    #[test]
    fn a_package_zipped_for_sharing_unpacks_to_the_same_package() {
        let dir = temp("zip");
        let package = dir.join("com.acme.sign-in.jevblock");
        fs::create_dir_all(package.join("images")).unwrap();
        fs::write(package.join("block.json"), MANIFEST).unwrap();
        fs::write(package.join("flow.json"), STEPS).unwrap();
        fs::write(package.join("images/go.png"), b"png").unwrap();
        fs::write(package.join(".DS_Store"), b"junk").unwrap();
        let zip = dir.join("out.zip");
        zip_package(&package, &zip).unwrap();
        let staged = stage(zip.to_str().unwrap(), &Network::Internet).unwrap();
        assert_eq!(
            fs::read(staged.folder.join("images/go.png")).unwrap(),
            b"png"
        );
        assert!(!staged.folder.join(".DS_Store").exists());
        discard_staged(&staged.folder);
        let mut names: Vec<String> = ZipArchive::new(fs::File::open(&zip).unwrap())
            .unwrap()
            .file_names()
            .map(str::to_owned)
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "com.acme.sign-in.jevblock/block.json",
                "com.acme.sign-in.jevblock/flow.json",
                "com.acme.sign-in.jevblock/images/go.png"
            ]
        );
    }
}
