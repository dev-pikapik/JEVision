//! Getting block packages from where people keep them: a folder, a zip file, or an address on the
//! web, such as a GitHub repository, a folder in one, or a release. A source may hold several
//! packages; they are unpacked into folders of their own in the temporary files, where they can
//! be read and shown before anything enters a project; the rest of the work is in
//! [`jevision_core`].
//!
//! Nothing is run. A zip can't write outside its folder, all the zips of a source together can't
//! unpack to more than the limits below, and an address must be `https` all the way, redirects
//! included.

use std::fs;
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use jevision_core::{MANIFEST_FILE, PackageError, parse_manifest};
use jevision_l10n::tr;
use serde_json::Value;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

use crate::import::{copy_counted, zip_invalid};

/// A downloaded zip is refused above this many bytes.
const MAX_DOWNLOAD: u64 = 64 << 20;
/// All the zips of one address together are refused above this many bytes.
const MAX_SET_DOWNLOAD: u64 = 128 << 20;
/// A release is refused when it carries more package files than this.
const MAX_RELEASE_FILES: usize = 20;
/// The archives of a source, which may be a whole repository around the packages, unpack to at
/// most this, all of them together.
const MAX_UNPACKED: u64 = 256 << 20;
/// The archives of a source hold at most this many entries, all of them together.
const MAX_ENTRIES: usize = 20_000;
/// A source holds at most this many packages.
const MAX_SET_PACKAGES: usize = 100;
/// How deep in an archive or a folder packages are looked for.
const MAX_SEARCH_DEPTH: usize = 3;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Staging folders older than this are removed when a new one is made.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const STAGING_PREFIX: &str = "jevision-block-";
const STAGED_FOLDER: &str = "package";

/// The packages of a source, ready to be read and installed, by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedSet {
    /// The folders with `block.json` and `flow.json`.
    pub packages: Vec<PathBuf>,
    /// Whether the folders were made by [`stage_source_set`] in the temporary files, each to be
    /// removed with [`discard_staged`]; folders the user pointed at are never removed.
    pub temporary: bool,
}

/// Makes the packages of `source` ready to be read: a package folder, a folder with packages
/// inside, a `.zip` file, or an `https` address of a zip, a GitHub repository, a folder in one
/// (`https://github.com/OWNER/REPO/tree/BRANCH/FOLDER`), a release or a branch. The source itself
/// when it is a package, otherwise every package in it down to a few folders deep, such as the
/// `*.jevblock` folders of a `blocks` folder; never a package inside another one. A release
/// gives all the package files it carries.
pub fn stage_source_set(source: &str) -> Result<StagedSet, PackageError> {
    stage(source, &Network::Internet)
}

/// Removes a package that [`stage_source_set`] unpacked into the temporary files, and the
/// staging folder around it with the last package of the set. Does nothing for any other folder.
pub fn discard_staged(folder: &Path) {
    if let Some(root) = staging_root(folder) {
        let _ = fs::remove_dir_all(folder);
        // Fails while another package of the set is still there.
        let _ = fs::remove_dir(root);
    }
}

/// The staging folder a staged package lives in, if `folder` is one: `package`, `package-2` and
/// on.
fn staging_root(folder: &Path) -> Option<&Path> {
    let root = folder.parent()?;
    let name = root.file_name()?.to_str()?;
    let number = folder.file_name()?.to_str()?.strip_prefix(STAGED_FOLDER)?;
    let staged = number.is_empty()
        || number
            .strip_prefix('-')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    (staged && name.starts_with(STAGING_PREFIX) && root.parent()? == std::env::temp_dir().as_path())
        .then_some(root)
}

/// The folder of the package at `index` of a staged set.
fn staged_name(index: usize) -> String {
    match index {
        0 => STAGED_FOLDER.to_owned(),
        i => format!("{STAGED_FOLDER}-{}", i + 1),
    }
}

/// Writes the package folder `folder` as a zip file `to`, holding the folder itself, so that
/// unpacking it gives `ID.jevblock` again.
pub fn zip_package(folder: &Path, to: &Path) -> Result<(), PackageError> {
    let failed =
        |e: &dyn ToString| PackageError::new("block.export.failed", &[("reason", &e.to_string())]);
    let top = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| failed(&tr!("package.export.noName")))?;
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
    /// A server on this computer, at this address, stands in for GitHub and its API.
    #[cfg(test)]
    Local(String),
}

impl Network {
    fn https_only(&self) -> bool {
        matches!(self, Self::Internet)
    }

    /// Where GitHub's pages are, or with `api` its API.
    fn github(&self, api: bool) -> &str {
        match self {
            Self::Internet if api => "https://api.github.com",
            Self::Internet => "https://github.com",
            #[cfg(test)]
            Self::Local(address) => address,
        }
    }
}

fn stage(source: &str, network: &Network) -> Result<StagedSet, PackageError> {
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
        return stage_archives(|into| download(&url, network, into));
    }
    let path = Path::new(source);
    if path.is_dir() {
        let found = find_packages(path);
        if found.is_empty() {
            return Err(PackageError::new(
                "block.folder.notPackage",
                &[("path", &path.display().to_string())],
            ));
        }
        return Ok(StagedSet {
            packages: by_id(found)?,
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
        return stage_archives(|into| {
            let folder = into.join("1");
            unpack_archive(file, &folder, &mut Allowance::new())?;
            Ok(vec![folder])
        });
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

/// Makes a new staging folder, has `unpack` unpack the archives of a source into the folder it
/// is given and say the folders to look for packages in, and moves the packages found there to
/// `package`, `package-2` and on, by id. The archives aren't kept; nothing is when one fails.
fn stage_archives(
    unpack: impl FnOnce(&Path) -> Result<Vec<PathBuf>, PackageError>,
) -> Result<StagedSet, PackageError> {
    let invalid =
        |e: io::Error| PackageError::new("block.archive.invalid", &[("reason", &e.to_string())]);
    let root = new_staging_root().map_err(invalid)?;
    let unpacked = root.join("unpacked");
    let result = unpack(&unpacked).and_then(|folders| {
        let found: Vec<PathBuf> = folders.iter().flat_map(|f| find_packages(f)).collect();
        if found.is_empty() {
            return Err(PackageError::new("block.archive.empty", &[]));
        }
        (by_id(found)?.into_iter().enumerate())
            .map(|(i, dir)| {
                let to = root.join(staged_name(i));
                fs::rename(&dir, &to).map_err(invalid)?;
                Ok(to)
            })
            .collect()
    });
    let _ = fs::remove_dir_all(&unpacked);
    match result {
        Ok(packages) => Ok(StagedSet {
            packages,
            temporary: true,
        }),
        Err(e) => {
            let _ = fs::remove_dir_all(&root);
            Err(e)
        }
    }
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

/// What the archives of one source may still unpack to, all of them together.
struct Allowance {
    bytes: u64,
    entries: usize,
}

impl Allowance {
    fn new() -> Self {
        Self {
            bytes: MAX_UNPACKED,
            entries: MAX_ENTRIES,
        }
    }
}

/// Writes the files of `zip` into `to`, taking what they unpack to from `left`. An entry that
/// would end up outside `to` or names an absolute path, and archives with more entries or
/// unpacking to more than `left` allows are refused; links are left out.
fn unpack_archive(
    zip: impl Read + Seek,
    to: &Path,
    left: &mut Allowance,
) -> Result<(), PackageError> {
    let invalid =
        |e: io::Error| PackageError::new("block.archive.invalid", &[("reason", &e.to_string())]);
    let mut archive = ZipArchive::new(zip).map_err(|e| invalid(zip_invalid(e)))?;
    if archive.len() > left.entries {
        return Err(PackageError::new(
            "block.package.tooManyFiles",
            &[("max", &MAX_ENTRIES.to_string())],
        ));
    }
    left.entries -= archive.len();
    fs::create_dir_all(to).map_err(invalid)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| invalid(zip_invalid(e)))?;
        if entry.is_symlink() {
            continue;
        }
        let absolute = entry.name().starts_with(['/', '\\']) || entry.name().get(1..2) == Some(":");
        let Some(name) = entry.enclosed_name().filter(|_| !absolute) else {
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
        if !copy_counted(&mut entry, &mut file, &mut left.bytes).map_err(invalid)? {
            return Err(PackageError::new(
                "block.package.tooLarge",
                &[("max", &MAX_UNPACKED.to_string())],
            ));
        }
    }
    Ok(())
}

/// The packages in `root`: `root` itself if it is one, otherwise every folder below it that is,
/// down to [`MAX_SEARCH_DEPTH`]. What is inside a package is never looked at.
fn find_packages(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut level = vec![root.to_path_buf()];
    for depth in 0..=MAX_SEARCH_DEPTH {
        let mut next = Vec::new();
        for dir in level {
            if dir.join(MANIFEST_FILE).is_file() {
                found.push(dir);
            } else if depth < MAX_SEARCH_DEPTH {
                next.extend(subfolders(&dir));
            }
        }
        level = next;
    }
    found
}

/// The folders in `dir` that aren't hidden, sorted; links aren't followed.
fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_dir())
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// The packages `found`, by id, or why they can't be one set: more than [`MAX_SET_PACKAGES`], or
/// two with one id. One whose `block.json` can't be read comes first, to say what is wrong with
/// it when it is read.
fn by_id(found: Vec<PathBuf>) -> Result<Vec<PathBuf>, PackageError> {
    if found.len() > MAX_SET_PACKAGES {
        return Err(PackageError::new(
            "block.set.tooMany",
            &[("max", &MAX_SET_PACKAGES.to_string())],
        ));
    }
    let mut found: Vec<(Option<String>, PathBuf)> = found
        .into_iter()
        .map(|dir| (package_id(&dir), dir))
        .collect();
    found.sort();
    if let Some(twice) = found
        .windows(2)
        .find_map(|w| w[0].0.as_deref().filter(|id| w[1].0.as_deref() == Some(id)))
    {
        return Err(PackageError::new("block.set.duplicate", &[("id", twice)]));
    }
    Ok(found.into_iter().map(|(_, dir)| dir).collect())
}

/// The id `block.json` in `dir` gives, if it can be read.
fn package_id(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(MANIFEST_FILE))
        .ok()
        .and_then(|text| parse_manifest(&text).ok())
        .map(|m| m.id)
}

// MARK: Addresses

/// Downloads the zips the packages at `url` come in and unpacks each into a folder of `into` as
/// soon as it is here, so that one at a time is held; the folders to look for packages in.
fn download(url: &str, network: &Network, into: &Path) -> Result<Vec<PathBuf>, PackageError> {
    let address = match network {
        #[cfg(test)]
        Network::Local(_) if url.starts_with("http://") => Address::Zip(url.to_owned()),
        _ => parse_address(url)?,
    };
    let mut folders = Vec::new();
    let mut fetched = 0;
    let mut left = Allowance::new();
    // `within`: the folder of a repository to look in, for a zip of its code.
    let mut add = |url: &str, zip: Vec<u8>, within: &[String]| {
        fetched += zip.len() as u64;
        if fetched > MAX_SET_DOWNLOAD {
            return Err(PackageError::new(
                "block.download.tooLarge",
                &[("url", url), ("max", &MAX_SET_DOWNLOAD.to_string())],
            ));
        }
        let folder = into.join((folders.len() + 1).to_string());
        unpack_archive(Cursor::new(zip), &folder, &mut left)?;
        folders.push(inside(&folder, within));
        Ok(())
    };
    match address {
        Address::Zip(url) => add(&url, fetch_zip(&url, network)?, &[])?,
        Address::GitHub {
            owner,
            repo,
            what: Pick::Code(path),
        } => {
            let (url, zip, within) = code_zip(&owner, &repo, &path, network)?;
            add(&url, zip, within)?;
        }
        Address::GitHub { owner, repo, what } => {
            let tag = match &what {
                Pick::Release(tag) => Some(tag.as_str()),
                _ => None,
            };
            for url in release_zips(&owner, &repo, tag, network)? {
                let zip = fetch_zip(&url, network)?;
                add(&url, zip, &[])?;
            }
        }
    }
    Ok(folders)
}

/// The folder `within` of the code of a repository unpacked into `folder`, where GitHub puts it
/// in one folder named after the repository and the branch.
fn inside(folder: &Path, within: &[String]) -> PathBuf {
    if within.is_empty() {
        return folder.to_path_buf();
    }
    let top = match subfolders(folder).as_slice() {
        [top] => top.clone(),
        _ => folder.to_path_buf(),
    };
    within.iter().fold(top, |path, part| path.join(part))
}

/// The zip at `url`, refused unless the server answers that it has it.
fn fetch_zip(url: &str, network: &Network) -> Result<Vec<u8>, PackageError> {
    let (status, body) = get(url, MAX_DOWNLOAD, "application/zip", network)?;
    if status != 200 {
        return Err(failed_download(
            url,
            &tr!("package.download.status", status),
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
    /// A branch, tag or commit, as the code of the repository was at that point, and maybe a
    /// folder in it, in the parts of the address after `tree/`.
    Code(Vec<String>),
}

fn unsupported(url: &str) -> PackageError {
    PackageError::new("block.source.unsupported", &[("source", url)])
}

fn failed_download(url: &str, reason: &str) -> PackageError {
    PackageError::new("block.download.failed", &[("url", url), ("reason", reason)])
}

/// What an `https` address points at. Anything but a GitHub repository, release, branch, a
/// folder in a branch or a link to a `.zip` is refused rather than guessed at.
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
        // No part can be `..`, so the folder can't lead out of the repository.
        ["tree", tail @ ..] if !tail.is_empty() && tail.iter().all(|p| name_ok(p)) => {
            Pick::Code(tail.iter().map(|p| (*p).to_owned()).collect())
        }
        _ => return Err(unsupported(url)),
    };
    Ok(Address::GitHub { owner, repo, what })
}

/// The zip of the code of a GitHub repository at a point.
fn code_url(owner: &str, repo: &str, reference: &str, network: &Network) -> String {
    format!(
        "{}/{owner}/{repo}/archive/{reference}.zip",
        network.github(false)
    )
}

/// The zip of the code of a GitHub repository at `path`, a branch, tag or commit and maybe a
/// folder after it: the address of the zip, the zip and the folder. A branch can have `/` in its
/// name, so the branch is the shortest start of `path` that GitHub has code for.
fn code_zip<'a>(
    owner: &str,
    repo: &str,
    path: &'a [String],
    network: &Network,
) -> Result<(String, Vec<u8>, &'a [String]), PackageError> {
    for split in 1..=path.len() {
        let url = code_url(owner, repo, &path[..split].join("/"), network);
        let (status, body) = get(&url, MAX_DOWNLOAD, "application/zip", network)?;
        if status == 200 {
            return Ok((url, body, &path[split..]));
        }
        if status != 404 || split == path.len() {
            return Err(failed_download(
                &url,
                &tr!("package.download.status", status),
            ));
        }
    }
    Err(unsupported(&format!(
        "https://github.com/{owner}/{repo}/tree"
    )))
}

/// The zips to download for a release of a GitHub repository, the latest one without `tag`: the
/// package files it carries (`NAME.jevblock.zip`) if it has any, otherwise the code of the
/// release, or of the repository if it has no release.
fn release_zips(
    owner: &str,
    repo: &str,
    tag: Option<&str>,
    network: &Network,
) -> Result<Vec<String>, PackageError> {
    let releases = format!("{}/repos/{owner}/{repo}/releases", network.github(true));
    let api = match tag {
        Some(tag) => format!("{releases}/tags/{tag}"),
        None => format!("{releases}/latest"),
    };
    let (status, body) = get(&api, 1 << 20, "application/vnd.github+json", network)?;
    match status {
        200 => {
            let release: Value = serde_json::from_slice(&body)
                .map_err(|_| failed_download(&api, &tr!("git.error.githubUnreadable")))?;
            let zips = release_files(&release)
                .ok_or_else(|| failed_download(&api, &tr!("package.download.noFiles")))?;
            if zips.len() > MAX_RELEASE_FILES {
                return Err(failed_download(
                    &api,
                    &tr!("package.download.tooManyFiles", MAX_RELEASE_FILES),
                ));
            }
            Ok(zips)
        }
        404 => Ok(vec![match tag {
            Some(tag) => code_url(owner, repo, &format!("refs/tags/{tag}"), network),
            None => code_url(owner, repo, "HEAD", network),
        }]),
        403 | 429 => Err(failed_download(&api, &tr!("package.download.rateLimited"))),
        other => Err(failed_download(&api, &tr!("git.error.githubStatus", other))),
    }
}

/// The package files of a release if it has any, otherwise the code of the release.
fn release_files(release: &Value) -> Option<Vec<String>> {
    let assets: Vec<String> = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            let name = asset["name"].as_str()?.to_ascii_lowercase();
            let url = asset["browser_download_url"].as_str()?;
            name.ends_with(".jevblock.zip").then(|| url.to_owned())
        })
        .collect();
    if assets.is_empty() {
        release["zipball_url"]
            .as_str()
            .map(|url| vec![url.to_owned()])
    } else {
        Some(assets)
    }
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

    /// `block.json` of the package `id`.
    fn manifest(id: &str) -> String {
        MANIFEST.replace("com.acme.sign-in", id)
    }

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

    /// A zip of the packages `ids`, each in `PREFIXID.jevblock`.
    fn set_zip(prefix: &str, ids: &[&str]) -> Vec<u8> {
        let files: Vec<(String, String)> = ids
            .iter()
            .flat_map(|id| {
                [
                    (format!("{prefix}{id}.jevblock/block.json"), manifest(id)),
                    (format!("{prefix}{id}.jevblock/flow.json"), STEPS.to_owned()),
                ]
            })
            .collect();
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_bytes()))
            .collect();
        zip_of(&entries)
    }

    /// Stages `zip` the way a zip file is.
    fn stage_zip(zip: Vec<u8>) -> Result<StagedSet, PackageError> {
        stage_archives(|into| {
            let folder = into.join("1");
            unpack_archive(Cursor::new(zip), &folder, &mut Allowance::new())?;
            Ok(vec![folder])
        })
    }

    fn ids(staged: &StagedSet) -> Vec<String> {
        staged
            .packages
            .iter()
            .map(|p| package_id(p).unwrap())
            .collect()
    }

    fn discard(staged: &StagedSet) {
        for package in &staged.packages {
            discard_staged(package);
        }
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
            let staged = stage_zip(package_zip(prefix)).unwrap();
            assert!(staged.temporary);
            let [folder] = staged.packages.as_slice() else {
                panic!("{prefix}: {:?}", staged.packages);
            };
            assert!(folder.join("flow.json").is_file(), "{prefix}");
            let root = staging_root(folder).unwrap().to_path_buf();
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1, "{prefix}");
            discard_staged(folder);
            assert!(!root.exists());
        }
    }

    #[test]
    fn a_zip_that_leaves_its_folder_is_refused() {
        for name in [
            "../escape.txt",
            "a/../../escape.txt",
            "/escape.txt",
            "\\escape.txt",
            "C:/escape.txt",
        ] {
            let zip = zip_of(&[("block.json", MANIFEST.as_bytes()), (name, b"x")]);
            assert_eq!(
                code(stage_zip(zip).unwrap_err()),
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
    fn the_limits_are_for_all_the_archives_of_a_source_together() {
        let dir = temp("allowance");
        // A megabyte of zeros, packed into a few kilobytes.
        let bomb = zip_of(&[("zeros.bin", &vec![0u8; 1 << 20])]);
        assert!(bomb.len() < 1 << 16);
        let mut left = Allowance {
            bytes: 3 << 19,
            entries: MAX_ENTRIES,
        };
        unpack_archive(Cursor::new(bomb.clone()), &dir.join("1"), &mut left).unwrap();
        let error = unpack_archive(Cursor::new(bomb), &dir.join("2"), &mut left).unwrap_err();
        assert_eq!(code(error), "block.package.tooLarge");
        let mut left = Allowance {
            bytes: MAX_UNPACKED,
            entries: 3,
        };
        unpack_archive(Cursor::new(package_zip("")), &dir.join("3"), &mut left).unwrap();
        let error =
            unpack_archive(Cursor::new(package_zip("")), &dir.join("4"), &mut left).unwrap_err();
        assert_eq!(code(error), "block.package.tooManyFiles");
        assert!(!dir.join("4/block.json").exists());
    }

    #[test]
    fn broken_and_empty_archives_say_so() {
        assert_eq!(
            code(stage_zip(b"not a zip".to_vec()).unwrap_err()),
            "block.archive.invalid"
        );
        let empty = zip_of(&[("readme.md", b"hello")]);
        assert_eq!(code(stage_zip(empty).unwrap_err()), "block.archive.empty");
        let two = zip_of(&[
            ("a/block.json", MANIFEST.as_bytes()),
            ("b/block.json", MANIFEST.as_bytes()),
        ]);
        let error = stage_zip(two).unwrap_err();
        assert_eq!(code(error.clone()), "block.set.duplicate");
        assert_eq!(error.issues[0].param("id"), Some("com.acme.sign-in"));
        let deep = zip_of(&[("a/b/c/d/e/block.json", MANIFEST.as_bytes())]);
        assert_eq!(code(stage_zip(deep).unwrap_err()), "block.archive.empty");
    }

    #[test]
    fn a_source_holds_every_package_down_to_the_search_depth_but_none_inside_another() {
        let zip = zip_of(&[
            (
                "repo/blocks/com.acme.b.jevblock/block.json",
                manifest("com.acme.b").as_bytes(),
            ),
            (
                "repo/blocks/com.acme.b.jevblock/flow.json",
                STEPS.as_bytes(),
            ),
            (
                "repo/blocks/com.acme.b.jevblock/examples/com.acme.c.jevblock/block.json",
                manifest("com.acme.c").as_bytes(),
            ),
            (
                "repo/more/com.acme.a.jevblock/block.json",
                manifest("com.acme.a").as_bytes(),
            ),
            ("repo/more/com.acme.a.jevblock/flow.json", STEPS.as_bytes()),
            (
                "repo/deep/x/y/com.acme.d.jevblock/block.json",
                manifest("com.acme.d").as_bytes(),
            ),
        ]);
        let staged = stage_zip(zip).unwrap();
        assert_eq!(ids(&staged), ["com.acme.a", "com.acme.b"]);
        let names: Vec<_> = (staged.packages.iter())
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["package", "package-2"]);
        assert!(
            staged.packages[1]
                .join("examples/com.acme.c.jevblock/block.json")
                .is_file()
        );
        let root = staging_root(&staged.packages[0]).unwrap().to_path_buf();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        discard_staged(&staged.packages[0]);
        assert!(root.exists() && staged.packages[1].exists());
        discard_staged(&staged.packages[1]);
        assert!(!root.exists());
    }

    #[test]
    fn a_source_holds_at_most_a_hundred_packages() {
        let many: Vec<String> = (0..=MAX_SET_PACKAGES)
            .map(|i| format!("com.acme.p{i:03}"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            code(stage_zip(set_zip("", &many)).unwrap_err()),
            "block.set.tooMany"
        );
        let staged = stage_zip(set_zip("blocks/", &many[1..])).unwrap();
        assert_eq!(staged.packages.len(), MAX_SET_PACKAGES);
        assert_eq!(ids(&staged)[0], "com.acme.p001");
        assert!(staged.packages[99].ends_with("package-100"));
        discard(&staged);
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
            (direct.packages, direct.temporary),
            (vec![package.clone()], false)
        );
        let around = stage(dir.to_str().unwrap(), &Network::Internet).unwrap();
        assert_eq!(around.packages, std::slice::from_ref(&package));
        discard_staged(&package);
        assert!(package.exists());
        let zip = dir.join("pkg.zip");
        fs::write(&zip, package_zip("x/")).unwrap();
        let staged = stage(zip.to_str().unwrap(), &Network::Internet).unwrap();
        assert!(staged.temporary);
        discard(&staged);
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
    fn a_folder_of_packages_is_a_set_where_it_is() {
        let dir = temp("folder-set");
        for id in ["com.acme.b", "com.acme.a"] {
            let package = dir.join(format!("blocks/{id}.jevblock"));
            fs::create_dir_all(&package).unwrap();
            fs::write(package.join("block.json"), manifest(id)).unwrap();
            fs::write(package.join("flow.json"), STEPS).unwrap();
        }
        let staged = stage(dir.to_str().unwrap(), &Network::Internet).unwrap();
        assert!(!staged.temporary);
        assert_eq!(
            staged.packages,
            [
                dir.join("blocks/com.acme.a.jevblock"),
                dir.join("blocks/com.acme.b.jevblock")
            ]
        );
        fs::write(
            dir.join("blocks/com.acme.b.jevblock/block.json"),
            manifest("com.acme.a"),
        )
        .unwrap();
        assert_eq!(
            code(stage(dir.to_str().unwrap(), &Network::Internet).unwrap_err()),
            "block.set.duplicate"
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
        let code_at = |parts: &[&str]| Pick::Code(parts.iter().map(|p| (*p).to_owned()).collect());
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
            repo(code_at(&["feature", "x"]))
        );
        assert_eq!(
            github("https://github.com/acme/blocks/tree/main/blocks/"),
            repo(code_at(&["main", "blocks"]))
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
            "https://github.com/acme/blocks/tree",
            "https://github.com/acme/blocks/tree/../x",
            "https://github.com/acme/blocks/tree/main/../../x",
            "https://github.com/acme/blocks/tree/main/blocks/..",
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
    fn a_release_gives_its_package_files_or_else_its_code() {
        let release = serde_json::json!({
            "zipball_url": "https://api.github.com/repos/acme/blocks/zipball/v1",
            "assets": [
                {"name": "Sign-In.jevblock.zip", "browser_download_url": "https://x/sign-in.jevblock.zip"},
                {"name": "notes.zip", "browser_download_url": "https://x/notes.zip"},
                {"name": "log-out.jevblock.zip", "browser_download_url": "https://x/log-out.jevblock.zip"}
            ]
        });
        assert_eq!(
            release_files(&release).unwrap(),
            [
                "https://x/sign-in.jevblock.zip",
                "https://x/log-out.jevblock.zip"
            ]
        );
        let bare = serde_json::json!({"zipball_url": "https://api.github.com/zip", "assets": []});
        assert_eq!(
            release_files(&bare).unwrap(),
            ["https://api.github.com/zip"]
        );
        assert_eq!(release_files(&serde_json::json!({})), None);
    }

    /// A web server on this computer: answers each request with the next of `answers`, each a
    /// whole HTTP response, and keeps the paths asked for.
    fn serve(answers: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
        serve_with(|_| answers)
    }

    /// [`serve`] with the answers that `answers` makes for the address of the server.
    fn serve_with(
        answers: impl FnOnce(&str) -> Vec<Vec<u8>>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let answers = answers(&address);
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

    fn ok(body: &[u8]) -> Vec<u8> {
        response("200 OK", "", body)
    }

    fn not_found() -> Vec<u8> {
        response("404 Not Found", "", b"Not Found")
    }

    #[test]
    fn a_package_is_downloaded_through_a_redirect() {
        let zip = package_zip("repo/");
        let (address, server) = serve(vec![
            response("302 Found", "Location: /real.zip\r\n", b""),
            ok(&zip),
        ]);
        let url = format!("{address}/start.zip");
        let staged = stage(&url, &Network::Local(address)).unwrap();
        assert!(staged.packages[0].join("block.json").is_file());
        discard(&staged);
        assert_eq!(server.join().unwrap(), ["/start.zip", "/real.zip"]);
    }

    #[test]
    fn a_download_that_fails_or_is_too_large_says_so() {
        let (address, server) = serve(vec![response("404 Not Found", "", b"gone")]);
        let url = format!("{address}/a.zip");
        let error = stage(&url, &Network::Local(address)).unwrap_err();
        assert_eq!(code(error.clone()), "block.download.failed");
        assert!(error.issues[0].param("reason").unwrap().contains("404"));
        server.join().unwrap();
        let (address, server) = serve(vec![ok(&vec![1u8; 2000])]);
        let url = format!("{address}/a.zip");
        let error = get(&url, 1000, "application/zip", &Network::Local(address)).unwrap_err();
        assert_eq!(code(error), "block.download.tooLarge");
        server.join().unwrap();
        assert_eq!(
            code(stage("https://127.0.0.1:1/a.zip", &Network::Internet).unwrap_err()),
            "block.download.failed"
        );
    }

    /// The code of the repository `blocks` at a branch, as GitHub zips it: packages `a` in
    /// `blocks` and `b` in `other`.
    fn repository_zip(top: &str) -> Vec<u8> {
        zip_of(&[
            (
                &format!("{top}/blocks/com.acme.a.jevblock/block.json"),
                manifest("com.acme.a").as_bytes(),
            ),
            (
                &format!("{top}/blocks/com.acme.a.jevblock/flow.json"),
                STEPS.as_bytes(),
            ),
            (
                &format!("{top}/other/com.acme.b.jevblock/block.json"),
                manifest("com.acme.b").as_bytes(),
            ),
            (
                &format!("{top}/other/com.acme.b.jevblock/flow.json"),
                STEPS.as_bytes(),
            ),
        ])
    }

    #[test]
    fn a_folder_of_a_branch_is_all_that_is_looked_in() {
        let (address, server) = serve(vec![ok(&repository_zip("blocks-main"))]);
        let staged = stage(
            "https://github.com/acme/blocks/tree/main/blocks",
            &Network::Local(address),
        )
        .unwrap();
        assert_eq!(ids(&staged), ["com.acme.a"]);
        discard(&staged);
        assert_eq!(server.join().unwrap(), ["/acme/blocks/archive/main.zip"]);

        let (address, server) = serve(vec![ok(&repository_zip("blocks-main"))]);
        let whole = stage(
            "https://github.com/acme/blocks/tree/main",
            &Network::Local(address),
        )
        .unwrap();
        assert_eq!(ids(&whole), ["com.acme.a", "com.acme.b"]);
        discard(&whole);
        server.join().unwrap();

        let (address, server) = serve(vec![ok(&repository_zip("blocks-main"))]);
        let error = stage(
            "https://github.com/acme/blocks/tree/main/nothing",
            &Network::Local(address),
        )
        .unwrap_err();
        assert_eq!(code(error), "block.archive.empty");
        server.join().unwrap();
    }

    #[test]
    fn a_branch_with_a_slash_in_its_name_is_found_part_by_part() {
        let (address, server) = serve(vec![not_found(), ok(&repository_zip("blocks-feat-x"))]);
        let staged = stage(
            "https://github.com/acme/blocks/tree/feat/x/other",
            &Network::Local(address),
        )
        .unwrap();
        assert_eq!(ids(&staged), ["com.acme.b"]);
        discard(&staged);
        assert_eq!(
            server.join().unwrap(),
            [
                "/acme/blocks/archive/feat.zip",
                "/acme/blocks/archive/feat/x.zip"
            ]
        );

        let (address, server) = serve(vec![not_found(), not_found()]);
        let error = stage(
            "https://github.com/acme/blocks/tree/gone/x",
            &Network::Local(address),
        )
        .unwrap_err();
        assert_eq!(code(error.clone()), "block.download.failed");
        assert!(error.issues[0].param("reason").unwrap().contains("404"));
        assert_eq!(server.join().unwrap().len(), 2);
    }

    /// A release whose package files are `files`, as `(name, zip)`.
    fn release_answers(address: &str, files: &[(&str, Vec<u8>)]) -> Vec<Vec<u8>> {
        let assets: Vec<Value> = files
            .iter()
            .map(|(name, _)| {
                serde_json::json!({"name": name, "browser_download_url": format!("{address}/{name}")})
            })
            .collect();
        let release =
            serde_json::json!({"assets": assets, "zipball_url": format!("{address}/code.zip")});
        let mut answers = vec![ok(release.to_string().as_bytes())];
        answers.extend(files.iter().map(|(_, zip)| ok(zip)));
        answers
    }

    #[test]
    fn a_release_with_several_package_files_is_one_set() {
        let (address, server) = serve_with(|address| {
            release_answers(
                address,
                &[
                    ("b.jevblock.zip", set_zip("", &["com.acme.b"])),
                    (
                        "a.jevblock.zip",
                        set_zip("x/", &["com.acme.a", "com.acme.c"]),
                    ),
                ],
            )
        });
        let staged = stage("https://github.com/acme/blocks", &Network::Local(address)).unwrap();
        assert_eq!(ids(&staged), ["com.acme.a", "com.acme.b", "com.acme.c"]);
        let root = staging_root(&staged.packages[0]).unwrap().to_path_buf();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        discard(&staged);
        assert!(!root.exists());
        assert_eq!(
            server.join().unwrap(),
            [
                "/repos/acme/blocks/releases/latest",
                "/b.jevblock.zip",
                "/a.jevblock.zip"
            ]
        );

        let (address, server) = serve_with(|address| {
            release_answers(
                address,
                &[
                    ("a.jevblock.zip", set_zip("", &["com.acme.a"])),
                    ("a-copy.jevblock.zip", set_zip("", &["com.acme.a"])),
                ],
            )
        });
        let error = stage(
            "https://github.com/acme/blocks/releases/tag/v1",
            &Network::Local(address),
        )
        .unwrap_err();
        assert_eq!(code(error.clone()), "block.set.duplicate");
        assert_eq!(error.issues[0].param("id"), Some("com.acme.a"));
        assert_eq!(
            server.join().unwrap()[0],
            "/repos/acme/blocks/releases/tags/v1"
        );
    }

    #[test]
    fn trouble_in_any_file_of_a_release_stops_the_whole_set() {
        let slip = zip_of(&[("block.json", MANIFEST.as_bytes()), ("../escape.txt", b"x")]);
        let (address, server) = serve_with(|address| {
            release_answers(
                address,
                &[
                    ("a.jevblock.zip", set_zip("", &["com.acme.a"])),
                    ("b.jevblock.zip", slip),
                ],
            )
        });
        let error = stage("https://github.com/acme/blocks", &Network::Local(address)).unwrap_err();
        assert_eq!(code(error), "block.path.unsafe");
        server.join().unwrap();

        let names: Vec<String> = (0..=MAX_RELEASE_FILES)
            .map(|i| format!("p{i}.jevblock.zip"))
            .collect();
        let (address, server) = serve_with(|address| {
            let files: Vec<(&str, Vec<u8>)> =
                names.iter().map(|n| (n.as_str(), Vec::new())).collect();
            release_answers(address, &files)[..1].to_vec()
        });
        let error = stage("https://github.com/acme/blocks", &Network::Local(address)).unwrap_err();
        assert_eq!(code(error.clone()), "block.download.failed");
        assert!(
            error.issues[0]
                .param("reason")
                .unwrap()
                .contains("more than 20")
        );
        server.join().unwrap();
        let limit = |lang| jevision_l10n::tr_in!(lang, "package.download.tooManyFiles", 20);
        assert_eq!(limit("en"), "the release has more than 20 block files");
        assert_eq!(limit("ru"), "в релизе больше 20 файлов блоков");
        let one = jevision_l10n::tr_in!("ru", "package.download.tooManyFiles", 21);
        assert_eq!(one, "в релизе больше 21 файла блоков");
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
            fs::read(staged.packages[0].join("images/go.png")).unwrap(),
            b"png"
        );
        assert!(!staged.packages[0].join(".DS_Store").exists());
        discard(&staged);
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
