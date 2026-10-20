//! Blocks shared as packages: reading one before it is installed, installing, updating and
//! removing, and writing a block of a project as a package. The rules are in
//! [`jevision_core`] and [`jevision_runner`]; this only carries them to the apps.

use std::path::{Path, PathBuf};

use jevision_core::{
    BlockManifest, Change, ExportMeta, Installed, Package, PackageError, PackageInput, Risk,
};
use jevision_runner::{describe_block, discard_staged, stage_source, zip_package};

use crate::{Flow, Issue};

/// A package can't be read, installed, updated or written; each problem is an issue to show in
/// the user's language.
#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum BlockPackageError {
    #[error("{}", jevision_l10n::tr!("block.package.problem"))]
    Failed { issues: Vec<Issue> },
}

impl From<PackageError> for BlockPackageError {
    fn from(e: PackageError) -> Self {
        Self::Failed { issues: e.issues }
    }
}

/// What a block of a package reads each time it runs: one `${input.NAME}`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockPackageInput {
    pub name: String,
    /// What to call it in the editor.
    pub label: Option<String>,
    /// What the input holds when the test doesn't say.
    pub default_value: Option<String>,
}

/// What `block.json` says about a package.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockManifestInfo {
    /// Like `com.acme.sign-in`.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// SF Symbol the block is shown with.
    pub icon: Option<String>,
    pub version: String,
    pub author: String,
    pub license: String,
    /// Oldest JEVision that can run the block.
    pub min_version: Option<String>,
    pub inputs: Vec<BlockPackageInput>,
}

impl From<BlockManifest> for BlockManifestInfo {
    fn from(m: BlockManifest) -> Self {
        Self {
            id: m.id,
            name: m.name,
            description: m.description,
            icon: m.icon,
            version: m.version.to_string(),
            author: m.author,
            license: m.license,
            min_version: m.min_version.map(|v| v.to_string()),
            inputs: m
                .inputs
                .into_iter()
                .map(|i| BlockPackageInput {
                    name: i.name,
                    label: i.label,
                    default_value: i.default_value,
                })
                .collect(),
        }
    }
}

/// What a step can do that the person installing may not expect, most worrying first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BlockRisk {
    /// Runs code or a shell command on this Mac.
    Script,
    /// Sends a request over the network.
    Http,
    /// Reads or changes a database file.
    Database,
    /// Starts apps, or opens files and web addresses.
    OpensApps,
    /// Types text or presses keys, into whatever has the focus.
    Keyboard,
    /// Replaces or reads what is on the clipboard.
    Clipboard,
}

impl From<Risk> for BlockRisk {
    fn from(r: Risk) -> Self {
        match r {
            Risk::Script => Self::Script,
            Risk::Http => Self::Http,
            Risk::Database => Self::Database,
            Risk::OpensApps => Self::OpensApps,
            Risk::Keyboard => Self::Keyboard,
            Risk::Clipboard => Self::Clipboard,
        }
    }
}

/// One step of a package, in the order it runs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockStep {
    pub block: String,
    /// What the step does, in one line in the current language.
    pub text: String,
    /// Set when the step needs the attention of whoever installs the package.
    pub risk: Option<BlockRisk>,
    /// What exactly the risky step does as the file says it: the code, the address, the text
    /// to type; empty for the other steps.
    pub detail: String,
}

/// How a package compares with the one of the same id that the project has.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BlockInstallChange {
    /// Nothing with this id is installed.
    New,
    /// The same version is installed.
    Same,
    /// An older version is installed.
    Update { from: String },
    /// A newer version is installed.
    Downgrade { from: String },
}

impl From<Change> for BlockInstallChange {
    fn from(c: Change) -> Self {
        match c {
            Change::New => Self::New,
            Change::Same => Self::Same,
            Change::Update { from } => Self::Update {
                from: from.to_string(),
            },
            Change::Downgrade { from } => Self::Downgrade {
                from: from.to_string(),
            },
        }
    }
}

/// A package read and checked, ready to be shown before it is installed.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockPreview {
    pub manifest: BlockManifestInfo,
    /// The steps of the package, for showing each block the way the editor does.
    pub flow: Flow,
    /// Every step in the order it runs.
    pub steps: Vec<BlockStep>,
    /// Problems and warnings; the package can't be installed while one is an error.
    pub issues: Vec<Issue>,
    /// Whether no issue is an error.
    pub can_install: bool,
    /// Files that would be installed, relative to the package.
    pub files: Vec<String>,
    pub bytes: u64,
    pub change: BlockInstallChange,
    /// The folder the package was read from; give it to `install_block_package`.
    pub folder: String,
    /// The address or path it came from, kept for updates when it is an address.
    pub source: String,
    /// `folder` was unpacked into the temporary files: pass it to `discard_block_staging` when
    /// done with it.
    pub temporary: bool,
}

/// A package a project has installed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InstalledBlock {
    pub manifest: BlockManifestInfo,
    /// Relative to the project: `blocks/ID.jevblock`.
    pub folder: String,
    /// The steps file a test names to run it, relative to the project.
    pub steps_file: String,
    /// The address it was installed from; `None` for one installed from a file or folder.
    pub source: Option<String>,
}

impl From<Installed> for InstalledBlock {
    fn from(i: Installed) -> Self {
        Self {
            steps_file: jevision_core::package_flow_path(&i.manifest.id),
            manifest: i.manifest.into(),
            folder: i.folder,
            source: i.source,
        }
    }
}

/// What the author says about a block they share.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockShare {
    /// Like `com.acme.sign-in`; see `suggest_block_id`.
    pub id: String,
    /// Empty uses the name of the block.
    pub name: String,
    /// Empty uses the description of the block.
    pub description: String,
    pub author: String,
    /// Semantic version, such as `1.0.0`.
    pub version: String,
    /// Name of the license, such as `MIT`.
    pub license: String,
    /// Labels and defaults for the inputs the block reads.
    pub inputs: Vec<BlockPackageInput>,
}

/// A package written by `export_block_package`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ExportedBlock {
    /// The folder or the zip file that was written.
    pub path: String,
    pub manifest: BlockManifestInfo,
    pub files: Vec<String>,
    pub bytes: u64,
    /// Things worth a look that didn't stop the export.
    pub warnings: Vec<Issue>,
}

fn preview(
    package: Package,
    change: Change,
    folder: &Path,
    source: &str,
    temporary: bool,
) -> BlockPreview {
    let app = package.flow.app.clone();
    let risks = jevision_core::risky_steps(&package.flow);
    let steps = package
        .flow
        .blocks_in_flow_order()
        .into_iter()
        .map(|b| {
            let risky = risks.iter().find(|r| r.block == b.id);
            BlockStep {
                block: b.id.clone(),
                text: describe_block(&b.kind, &app),
                risk: risky.map(|r| r.risk.into()),
                detail: risky.map(|r| r.detail.clone()).unwrap_or_default(),
            }
        })
        .collect();
    BlockPreview {
        can_install: !package.has_errors(),
        flow: package.flow,
        files: package.files.iter().map(|f| f.path.clone()).collect(),
        bytes: package.bytes,
        manifest: package.manifest.into(),
        steps,
        issues: package.issues,
        change: change.into(),
        folder: folder.to_string_lossy().into_owned(),
        source: source.to_owned(),
        temporary,
    }
}

/// Reads the package at `source` — a package folder, a zip file, or an `https` address of a zip,
/// a GitHub repository or a release — and checks it against the project folder `project`,
/// without installing or running anything. Waits for the download, so call it off the main
/// thread.
#[uniffi::export]
pub fn inspect_block_package(
    project: String,
    source: String,
) -> Result<BlockPreview, BlockPackageError> {
    let source = source.trim().to_owned();
    let staged = stage_source(&source)?;
    match jevision_core::read_package(&staged.folder) {
        Ok(package) => {
            let change = jevision_core::change_for(Path::new(&project), &package.manifest);
            Ok(preview(
                package,
                change,
                &staged.folder,
                &source,
                staged.temporary,
            ))
        }
        Err(e) => {
            if staged.temporary {
                discard_staged(&staged.folder);
            }
            Err(e.into())
        }
    }
}

/// Installs the package read by `inspect_block_package` into the project folder `project`.
/// An installed package of the same id is replaced only with `replace`. `source` is kept for
/// updates when it is an address.
#[uniffi::export]
pub fn install_block_package(
    project: String,
    folder: String,
    source: String,
    replace: bool,
) -> Result<InstalledBlock, BlockPackageError> {
    let address = (source.starts_with("https://") || source.starts_with("http://"))
        .then_some(source.as_str());
    Ok(
        jevision_core::install_folder(Path::new(&project), Path::new(&folder), address, replace)?
            .into(),
    )
}

/// Removes what `inspect_block_package` unpacked into the temporary files; does nothing for a
/// folder the user chose.
#[uniffi::export]
pub fn discard_block_staging(folder: String) {
    discard_staged(Path::new(&folder));
}

/// The packages installed in the project folder `project`, by name.
#[uniffi::export]
pub fn list_community_blocks(project: String) -> Vec<InstalledBlock> {
    jevision_core::list_installed(Path::new(&project))
        .into_iter()
        .map(Into::into)
        .collect()
}

/// The tests and blocks of the project, as files relative to it, that run the package `id`.
#[uniffi::export]
pub fn block_usage(project: String, id: String) -> Vec<String> {
    jevision_core::usage(Path::new(&project), &id)
}

/// Deletes the installed package `id`. The tests that use it stay as they are and stop passing
/// validation; ask `block_usage` first.
#[uniffi::export]
pub fn remove_block_package(project: String, id: String) -> Result<(), BlockPackageError> {
    Ok(jevision_core::remove_package(Path::new(&project), &id)?)
}

/// Reads the newest package at the address the installed package `id` came from, to show what
/// would change before `install_block_package` replaces it.
#[uniffi::export]
pub fn update_block_package(
    project: String,
    id: String,
) -> Result<BlockPreview, BlockPackageError> {
    let source = jevision_core::installed(Path::new(&project), &id)
        .ok_or_else(|| PackageError::new("block.notInstalled", &[("id", &id)]))?
        .source
        .ok_or_else(|| PackageError::new("block.source.none", &[("id", &id)]))?;
    let preview = inspect_block_package(project, source)?;
    if preview.manifest.id != id {
        discard_block_staging(preview.folder.clone());
        return Err(PackageError::new(
            "block.update.otherId",
            &[("id", &id), ("found", &preview.manifest.id)],
        )
        .into());
    }
    Ok(preview)
}

/// Writes the block `block_file` of the project folder `project`, such as
/// `blocks/Sign in.block.json`, as a package. With `as_zip` `destination` is the zip file to
/// write, otherwise the folder to put `ID.jevblock` in.
#[uniffi::export]
pub fn export_block_package(
    project: String,
    block_file: String,
    share: BlockShare,
    destination: String,
    as_zip: bool,
) -> Result<ExportedBlock, BlockPackageError> {
    let meta = ExportMeta {
        id: share.id,
        name: Some(share.name).filter(|n| !n.trim().is_empty()),
        description: Some(share.description).filter(|d| !d.trim().is_empty()),
        author: share.author,
        version: share.version,
        license: share.license,
        inputs: share
            .inputs
            .into_iter()
            .map(|i| PackageInput {
                name: i.name,
                label: i.label,
                default_value: i.default_value,
            })
            .collect(),
    };
    let project = Path::new(&project);
    let (parent, staging) = if as_zip {
        let staging = scratch_folder();
        (staging.clone(), Some(staging))
    } else {
        (PathBuf::from(&destination), None)
    };
    let result =
        jevision_core::export_package(project, &block_file, &meta, &parent).and_then(|out| {
            if as_zip {
                zip_package(&out.folder, Path::new(&destination))?;
            }
            Ok(out)
        });
    if let Some(staging) = staging {
        let _ = std::fs::remove_dir_all(staging);
    }
    let out = result?;
    Ok(ExportedBlock {
        path: if as_zip {
            destination
        } else {
            out.folder.to_string_lossy().into_owned()
        },
        files: out.files.iter().map(|f| f.path.clone()).collect(),
        bytes: out.bytes,
        warnings: out.warnings,
        manifest: out.manifest.into(),
    })
}

fn scratch_folder() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "jevision-export-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// An id for a package, like `com.anna-lee.sign-in`, from the author's name and the block's.
#[uniffi::export]
pub fn suggest_block_id(author: String, name: String) -> String {
    jevision_core::suggest_id(&author, &name)
}

/// Whether `id` can be the id of a package.
#[uniffi::export]
pub fn is_valid_block_id(id: String) -> bool {
    jevision_core::is_package_id(&id)
}

/// The path `path`, from a test, names a custom block: one the project made
/// (`NAME.block.json`) or the steps of an installed package.
#[uniffi::export]
pub fn is_block_path(path: String) -> bool {
    jevision_core::is_block_path(&path)
}

/// What a kind of risk means, in the current language.
#[uniffi::export]
pub fn describe_block_risk(risk: BlockRisk) -> String {
    describe_block_risk_in(&jevision_l10n::language(), risk)
}

fn describe_block_risk_in(lang: &str, risk: BlockRisk) -> String {
    use jevision_l10n::tr_in;
    match risk {
        BlockRisk::Script => tr_in!(lang, "block.risk.script"),
        BlockRisk::Http => tr_in!(lang, "block.risk.http"),
        BlockRisk::Database => tr_in!(lang, "block.risk.database"),
        BlockRisk::OpensApps => tr_in!(lang, "block.risk.opensApps"),
        BlockRisk::Keyboard => tr_in!(lang, "block.risk.keyboard"),
        BlockRisk::Clipboard => tr_in!(lang, "block.risk.clipboard"),
    }
}

/// The id of the installed block whose steps `path` names, as in
/// `blocks/com.acme.sign-in.jevblock/flow.json`; `None` for any other file.
#[uniffi::export]
pub fn block_package_id(path: String) -> Option<String> {
    jevision_core::package_of_path(&path).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "com.example.App";

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("jevision-ffi-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("blocks")).unwrap();
        root
    }

    fn write(root: &Path, file: &str, text: &str) {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn share() -> BlockShare {
        BlockShare {
            id: "com.anna.sign-in".into(),
            name: String::new(),
            description: String::new(),
            author: "Anna".into(),
            version: "1.0.0".into(),
            license: "MIT".into(),
            inputs: vec![BlockPackageInput {
                name: "email".into(),
                label: Some("Email".into()),
                default_value: None,
            }],
        }
    }

    fn sign_in_block() -> String {
        format!(
            r#"{{"format":1,"name":"Sign in","icon":"person","app":"{APP}","start":"a","blocks":{{
                "a":{{"type":"type","text":"${{input.email}}","next":"b"}},
                "b":{{"type":"command","command":"say hello"}}}}}}"#
        )
    }

    fn codes(error: BlockPackageError) -> Vec<String> {
        let BlockPackageError::Failed { issues } = error;
        issues.into_iter().map(|i| i.code).collect()
    }

    #[test]
    fn a_block_travels_as_a_zip_and_is_read_before_it_is_installed() {
        let mine = project("share-mine");
        write(&mine, "blocks/Sign in.block.json", &sign_in_block());
        let zip = mine.join("out/sign-in.zip");
        std::fs::create_dir_all(zip.parent().unwrap()).unwrap();
        let out = export_block_package(
            mine.to_string_lossy().into_owned(),
            "blocks/Sign in.block.json".into(),
            share(),
            zip.to_string_lossy().into_owned(),
            true,
        )
        .unwrap();
        assert_eq!(out.manifest.name, "Sign in");
        assert!(zip.is_file());

        let theirs = project("share-theirs");
        let source = zip.to_string_lossy().into_owned();
        let seen =
            inspect_block_package(theirs.to_string_lossy().into_owned(), source.clone()).unwrap();
        assert!(seen.can_install, "{:?}", seen.issues);
        assert_eq!(seen.change, BlockInstallChange::New);
        assert_eq!(seen.manifest.id, "com.anna.sign-in");
        let steps: Vec<_> = seen
            .steps
            .iter()
            .map(|s| (s.block.as_str(), s.risk))
            .collect();
        assert_eq!(
            steps,
            [
                ("a", Some(BlockRisk::Keyboard)),
                ("b", Some(BlockRisk::Script))
            ]
        );
        assert_eq!(seen.steps[1].detail, "say hello");
        // Reading installs nothing.
        assert!(list_community_blocks(theirs.to_string_lossy().into_owned()).is_empty());

        let installed = install_block_package(
            theirs.to_string_lossy().into_owned(),
            seen.folder.clone(),
            source.clone(),
            false,
        )
        .unwrap();
        discard_block_staging(seen.folder);
        assert_eq!(
            installed.steps_file,
            "blocks/com.anna.sign-in.jevblock/flow.json"
        );
        assert_eq!(installed.source, None);
        let listed = list_community_blocks(theirs.to_string_lossy().into_owned());
        assert_eq!(listed, [installed]);

        // The same package again is the same version: nothing to replace.
        let again = inspect_block_package(theirs.to_string_lossy().into_owned(), source).unwrap();
        assert_eq!(again.change, BlockInstallChange::Same);
        let refused = install_block_package(
            theirs.to_string_lossy().into_owned(),
            again.folder,
            String::new(),
            false,
        )
        .unwrap_err();
        assert_eq!(codes(refused), ["block.install.exists"]);
    }

    #[test]
    fn a_package_in_use_is_named_and_can_be_removed() {
        let root = project("usage");
        write(&root, "blocks/Sign in.block.json", &sign_in_block());
        let out = export_block_package(
            root.to_string_lossy().into_owned(),
            "blocks/Sign in.block.json".into(),
            share(),
            root.join("out").to_string_lossy().into_owned(),
            false,
        )
        .unwrap();
        std::fs::remove_file(root.join("blocks/Sign in.block.json")).unwrap();
        let project_path = root.to_string_lossy().into_owned();
        install_block_package(project_path.clone(), out.path, String::new(), false).unwrap();
        write(
            &root,
            "login.flow.json",
            &format!(
                r#"{{"format":1,"name":"Login","app":"{APP}","start":"s","blocks":{{
                    "s":{{"type":"runTest","test":"blocks/com.anna.sign-in.jevblock/flow.json",
                          "inputs":{{"email":"a@b.c"}}}}}}}}"#
            ),
        );
        assert_eq!(
            block_usage(project_path.clone(), "com.anna.sign-in".into()),
            ["login.flow.json"]
        );
        remove_block_package(project_path.clone(), "com.anna.sign-in".into()).unwrap();
        assert!(list_community_blocks(project_path.clone()).is_empty());
        let gone = remove_block_package(project_path, "com.anna.sign-in".into()).unwrap_err();
        assert_eq!(codes(gone), ["block.notInstalled"]);
    }

    #[test]
    fn only_https_addresses_are_downloaded_and_a_package_from_a_file_has_nothing_to_update() {
        let root = project("addresses");
        let path = root.to_string_lossy().into_owned();
        let err =
            inspect_block_package(path.clone(), "http://example.com/block.zip".into()).unwrap_err();
        assert_eq!(codes(err), ["block.download.notHttps"]);

        write(&root, "blocks/Sign in.block.json", &sign_in_block());
        let out = export_block_package(
            path.clone(),
            "blocks/Sign in.block.json".into(),
            share(),
            root.join("out").to_string_lossy().into_owned(),
            false,
        )
        .unwrap();
        install_block_package(path.clone(), out.path, String::new(), false).unwrap();
        let err = update_block_package(path, "com.anna.sign-in".into()).unwrap_err();
        assert_eq!(codes(err), ["block.source.none"]);
    }

    #[test]
    fn risks_read_in_russian_and_english() {
        assert_eq!(
            describe_block_risk_in("en", BlockRisk::Http),
            "Sends requests over the network"
        );
        assert_eq!(
            describe_block_risk_in("ru", BlockRisk::Http),
            "Отправляет запросы по сети"
        );
        assert_eq!(
            describe_block_risk(BlockRisk::Script),
            "Runs a script on this Mac"
        );
    }
}
