//! Blocks shared as packages: reading them before they are installed, installing them with what
//! they need, restoring what the lock file names, updating and removing, and writing a block of a
//! project as a package. The rules are in [`jevision_core`] and [`jevision_runner`]; this only
//! carries them to the apps.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jevision_core::{
    BlockManifest, BlockOrigin, Change, ExportMeta, Installed, Package, PackageError, PackageInput,
    Risk,
};
use jevision_runner::{StagedSet, describe_block, discard_staged, stage_source_set, zip_package};

use crate::{Flow, Issue, Severity};

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
    /// The packages its steps run, which must be installed with it.
    pub dependencies: Vec<BlockDependency>,
}

/// A package that another one needs, as its `block.json` lists it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockDependency {
    pub id: String,
    /// The versions that work, as written, such as `^1.2.0`; `*` for any.
    pub version: String,
    /// An `https` address to get it from when it isn't installed.
    pub source: Option<String>,
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
            dependencies: m
                .dependencies
                .into_iter()
                .map(|d| BlockDependency {
                    id: d.id,
                    version: d.version.to_string(),
                    source: d.source,
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
    /// The folder the package was read from; give it to `install_block_set`.
    pub folder: String,
    /// The address or path it came from, kept for updates when it is an address.
    pub source: String,
    /// Who stands behind the package, by `source`; show it before the author it names.
    pub origin: BlockOrigin,
    /// Links to other files in the package, by path, which aren't installed; an issue tells
    /// about them.
    pub skipped_links: Vec<String>,
    /// `folder` was unpacked into the temporary files: pass it to `discard_block_staging` when
    /// done with it.
    pub temporary: bool,
}

/// Who stands behind a package, as far as the address it was got from can tell; see
/// `describe_block_origin`.
#[uniffi::remote(Enum)]
pub enum BlockOrigin {
    /// From a repository, a folder in a branch or a release of this GitHub account.
    GitHub { owner: String },
    /// From another `https` address on the site `host`.
    Web { host: String },
    /// From a file or folder on this Mac.
    ThisMac,
    /// The project keeps no `https` address for the installed package.
    Unknown,
}

/// Where a block comes from and the author it names, worded for people.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockOriginText {
    /// Such as “From the GitHub account acme”: show it first.
    pub source: String,
    /// What that means for trusting the block.
    pub about: String,
    /// Such as “Author named in the block: Acme”: show it under `source`.
    pub author: String,
    /// When the author isn't the account or site the block comes from: why to go by `source`.
    pub mismatch: Option<String>,
}

/// Where a block comes from and the `author` its package names, in the current language. The
/// author matches a GitHub account when it is the account name, in any case and with or without
/// `@`; it matches a site when it is the site's host, with or without `www.`.
#[uniffi::export]
pub fn describe_block_origin(origin: BlockOrigin, author: String) -> BlockOriginText {
    describe_block_origin_in(&jevision_l10n::language(), &origin, &author)
}

/// Where an installed block comes from, by the address the project keeps for it: `Unknown`
/// without an `https` one.
#[uniffi::export]
pub fn installed_block_origin(source: Option<String>) -> BlockOrigin {
    BlockOrigin::of_installed(source.as_deref())
}

fn describe_block_origin_in(lang: &str, origin: &BlockOrigin, author: &str) -> BlockOriginText {
    let (author, mismatch) = origin.author_in(lang, author);
    BlockOriginText {
        source: origin.describe_in(lang),
        about: origin.about_in(lang),
        author,
        mismatch,
    }
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
    /// Who stands behind it, by `source`; `Unknown` without an address.
    pub origin: BlockOrigin,
}

impl From<Installed> for InstalledBlock {
    fn from(i: Installed) -> Self {
        Self {
            steps_file: jevision_core::package_flow_path(&i.manifest.id),
            manifest: i.manifest.into(),
            folder: i.folder,
            origin: BlockOrigin::of_installed(i.source.as_deref()),
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

/// `links`: the links the archive the package came from held, which `package` can't see.
fn preview(
    mut package: Package,
    change: Change,
    folder: &Path,
    source: &str,
    links: &[String],
    temporary: bool,
) -> BlockPreview {
    package.skipped_links.extend_from_slice(links);
    package.skipped_links.sort();
    package.skipped_links.dedup();
    let links = jevision_core::skipped_links_warning(&package.skipped_links);
    package.issues.extend(links);
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
        origin: BlockOrigin::of_source(source),
        skipped_links: package.skipped_links,
        temporary,
    }
}

/// The packages a source holds, read and checked, by id.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockSourcePreview {
    /// The address or path the packages came from.
    pub source: String,
    /// The packages were unpacked into the temporary files: pass each folder to
    /// `discard_block_staging` when done with it.
    pub temporary: bool,
    pub packages: Vec<BlockPreview>,
}

/// Reads the packages at `source` — a package folder, a folder with packages in it, a zip file,
/// or an `https` address of a zip, a GitHub repository, a folder in one or a release — and checks
/// each against the project folder `project`, without installing or running anything. Waits for
/// the download, so call it off the main thread.
#[uniffi::export]
pub fn inspect_block_source(
    project: String,
    source: String,
) -> Result<BlockSourcePreview, BlockPackageError> {
    let source = jevision_core::source_address(&source);
    let staged = stage_source_set(&source)?;
    let temporary = staged.temporary;
    Ok(BlockSourcePreview {
        packages: read_set(Path::new(&project), &source, staged)?,
        source,
        temporary,
    })
}

/// Reads each package of `staged`; when one can't be read, the whole set is discarded.
fn read_set(
    project: &Path,
    source: &str,
    staged: StagedSet,
) -> Result<Vec<BlockPreview>, PackageError> {
    let read: Result<Vec<_>, PackageError> = (staged.packages.iter())
        .map(|folder| {
            let package = jevision_core::read_package(folder)?;
            let change = jevision_core::change_for(project, &package.manifest);
            let links = staged
                .skipped_links
                .get(folder)
                .map_or(&[][..], Vec::as_slice);
            Ok(preview(
                package,
                change,
                folder,
                source,
                links,
                staged.temporary,
            ))
        })
        .collect();
    if read.is_err() {
        staged.packages.iter().for_each(|f| discard_staged(f));
    }
    read
}

/// Where a package that one of the packages to install needs stands.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BlockDependencyStatus {
    /// Installed, in a version that fits.
    Installed { version: String },
    /// One of the packages to install, in a version that fits.
    InSelection,
    /// Neither installed nor among the packages to install.
    Missing,
    /// Installed or among the packages to install, in `installed`, a version that doesn't fit.
    Unmet { installed: String },
}

/// A package that one of the packages to install needs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockDependencyState {
    /// The staged folder of the package that needs it.
    pub folder: String,
    pub id: String,
    /// The versions that work, such as `^1.2.0`.
    pub range: String,
    /// Where to get it from, when the package says: offer it for a missing one.
    pub source: Option<String>,
    pub status: BlockDependencyStatus,
}

/// Something that stops the packages to install from going in.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockPlanProblem {
    /// The staged folder of the package it is about; `None` when it is about several, such as
    /// packages that need each other in a circle or have one id.
    pub folder: Option<String>,
    pub issue: Issue,
}

/// How a set of staged packages goes into a project.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockInstallPlan {
    /// The folders in the order to install them, what another needs first; empty while there
    /// are problems.
    pub order: Vec<String>,
    /// What each package needs, package by package.
    pub dependencies: Vec<BlockDependencyState>,
    /// What stops the install: a missing dependency or one in a version that doesn't fit,
    /// packages that need each other in a circle, two with one id, and installed packages that
    /// need another version of one the set replaces (`block.dependency.breaks`).
    pub problems: Vec<BlockPlanProblem>,
}

/// How the packages staged in `folders` would go into the project folder `project`, in place of
/// the installed packages of the same ids: in what order, what each needs and what stops it.
/// Reads only the folders and the project. A missing dependency with a `source` can be read with
/// `inspect_block_source` and its folder added to `folders`.
#[uniffi::export]
pub fn plan_block_install(
    project: String,
    folders: Vec<String>,
) -> Result<BlockInstallPlan, BlockPackageError> {
    Ok(plan(Path::new(&project), &folders, false)?.0)
}

/// [`plan_block_install`] without `block.dependency.breaks` when `force`, and the packages read.
fn plan(
    project: &Path,
    folders: &[String],
    force: bool,
) -> Result<(BlockInstallPlan, Vec<Package>), PackageError> {
    let packages = (folders.iter())
        .map(|folder| jevision_core::read_package(Path::new(folder)))
        .collect::<Result<Vec<_>, _>>()?;
    let set: Vec<BlockManifest> = packages.iter().map(|p| p.manifest.clone()).collect();
    let installed: Vec<BlockManifest> = (jevision_core::list_installed(project).into_iter())
        .map(|i| i.manifest)
        .collect();
    let folder_of = |id: &str| (set.iter().position(|m| m.id == id)).map(|at| folders[at].clone());
    let dependencies = (set.iter().zip(folders))
        .flat_map(|(manifest, folder)| {
            manifest.dependencies.iter().map(|d| {
                let coming = set.iter().find(|m| m.id == d.id);
                let status = match coming.or_else(|| installed.iter().find(|m| m.id == d.id)) {
                    None => BlockDependencyStatus::Missing,
                    Some(m) if !d.version.matches(&m.version) => BlockDependencyStatus::Unmet {
                        installed: m.version.to_string(),
                    },
                    Some(_) if coming.is_some() => BlockDependencyStatus::InSelection,
                    Some(m) => BlockDependencyStatus::Installed {
                        version: m.version.to_string(),
                    },
                };
                BlockDependencyState {
                    folder: folder.clone(),
                    id: d.id.clone(),
                    range: d.version.to_string(),
                    source: d.source.clone(),
                    status,
                }
            })
        })
        .collect();
    let (order, problems) = match jevision_core::install_order(&installed, &set, force) {
        Ok(ids) => (
            ids.iter().filter_map(|id| folder_of(id)).collect(),
            Vec::new(),
        ),
        Err(e) => (
            Vec::new(),
            (e.issues.into_iter())
                .map(|issue| BlockPlanProblem {
                    // The package that needs it, or the one that breaks an installed package.
                    folder: (issue.param("by").or(issue.param("dependency"))).and_then(folder_of),
                    issue,
                })
                .collect(),
        ),
    };
    let plan = BlockInstallPlan {
        order,
        dependencies,
        problems,
    };
    Ok((plan, packages))
}

/// A staged package to install.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockInstallItem {
    /// The folder of a package from `inspect_block_source` or `restore_block_sources`.
    pub folder: String,
    /// The address or path it came from; an address is kept for updates and restoring.
    pub source: String,
    /// Replace an installed package of the same id.
    pub replace: bool,
}

/// Installs the staged packages `items` into the project folder `project`, what another needs
/// first, and gives them in that order. Nothing is installed while the plan has problems (see
/// `plan_block_install`; with `force` an installed package that needs another version of one of
/// them doesn't stop it), while a package has an error, or while one is installed already and
/// not to be replaced. If writing one fails part way, the ones before it stay installed — each
/// whole, after what it needs — and the rest are not; the error says what failed, and
/// `list_community_blocks` what is there.
#[uniffi::export]
pub fn install_block_set(
    project: String,
    items: Vec<BlockInstallItem>,
    force: bool,
) -> Result<Vec<InstalledBlock>, BlockPackageError> {
    let project = Path::new(&project);
    let folders: Vec<String> = items.iter().map(|i| i.folder.clone()).collect();
    let (plan, packages) = plan(project, &folders, force)?;
    let mut issues: Vec<Issue> = plan.problems.into_iter().map(|p| p.issue).collect();
    for (item, package) in items.iter().zip(&packages) {
        issues.extend(
            (package.issues.iter())
                .filter(|i| i.severity == Severity::Error)
                .cloned(),
        );
        if let Some(old) =
            jevision_core::installed(project, &package.manifest.id).filter(|_| !item.replace)
        {
            issues.push(jevision_core::package_issue(
                "block.install.exists",
                &[
                    ("id", &old.manifest.id),
                    ("version", &old.manifest.version.to_string()),
                ],
            ));
        }
    }
    if !issues.is_empty() {
        return Err(BlockPackageError::Failed { issues });
    }
    let mut done = Vec::new();
    for item in (plan.order.iter()).filter_map(|f| items.iter().find(|i| &i.folder == f)) {
        let address = (item.source.starts_with("https://") || item.source.starts_with("http://"))
            .then_some(item.source.as_str());
        let installed =
            jevision_core::install_folder(project, Path::new(&item.folder), address, item.replace)?;
        done.push(installed.into());
    }
    Ok(done)
}

/// Removes what `inspect_block_source` or `restore_block_sources` unpacked into the temporary
/// files, one package folder at a time; does nothing for a folder the user chose.
#[uniffi::export]
pub fn discard_block_staging(folder: String) {
    discard_staged(Path::new(&folder));
}

/// Compatibility diagnostics for the installed-package list, before using its sources.
#[uniffi::export]
pub fn block_package_issues(project: String) -> Vec<Issue> {
    jevision_core::package_lock_issues(Path::new(&project))
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

/// The installed packages of the project folder `project` that need the package `id`, by id;
/// without it they stop working, so ask before removing it.
#[uniffi::export]
pub fn block_dependents(project: String, id: String) -> Vec<String> {
    jevision_core::block_dependents(Path::new(&project), &id)
}

/// What the installed package `id` needs, and what those need in turn, that isn't installed in a
/// version that fits: `block.dependency.missing` and `block.dependency.unmet`.
#[uniffi::export]
pub fn unmet_dependencies(project: String, id: String) -> Vec<Issue> {
    jevision_core::unmet_dependencies(Path::new(&project), &id)
}

/// Deletes the installed package `id`. The tests that use it and the packages that need it stay
/// as they are and stop passing validation; ask `block_usage` and `block_dependents` first.
#[uniffi::export]
pub fn remove_block_package(project: String, id: String) -> Result<(), BlockPackageError> {
    Ok(jevision_core::remove_package(Path::new(&project), &id)?)
}

/// Reads the newest package `id` at the address the installed one came from, which may hold
/// other packages as well, to show what would change. Ask `plan_block_install` with its folder
/// what it needs and whether it breaks an installed package, then replace the installed one with
/// `install_block_set`.
#[uniffi::export]
pub fn update_block_package(
    project: String,
    id: String,
) -> Result<BlockPreview, BlockPackageError> {
    let issues: Vec<_> = jevision_core::package_lock_issues(Path::new(&project))
        .into_iter()
        .filter(|issue| issue.severity == Severity::Error)
        .collect();
    if !issues.is_empty() {
        return Err(BlockPackageError::Failed { issues });
    }
    let source = jevision_core::installed(Path::new(&project), &id)
        .ok_or_else(|| PackageError::new("block.notInstalled", &[("id", &id)]))?
        .source
        .ok_or_else(|| PackageError::new("block.source.none", &[("id", &id)]))?;
    let mut set = inspect_block_source(project, source)?.packages;
    let found = set
        .iter()
        .position(|p| p.manifest.id == id)
        .map(|at| set.remove(at));
    for other in &set {
        discard_staged(Path::new(&other.folder));
    }
    found.ok_or_else(|| {
        let ids: Vec<&str> = set.iter().map(|p| p.manifest.id.as_str()).collect();
        let code = match ids.len() {
            1 => "block.update.otherId",
            _ => "block.update.missing",
        };
        PackageError::new(code, &[("id", &id), ("found", &ids.join(", "))]).into()
    })
}

/// Where a package that the lock file names stands at its address.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RestoreState {
    /// The address has the version the lock keeps, or the lock keeps none.
    Ready,
    /// The address has another version than the one installed before: install it only when the
    /// person agrees, as it wasn't looked at.
    VersionChanged { found: String },
    /// The address no longer has the package.
    Missing,
    /// The address couldn't be read, for the reasons in `issues`.
    Failed { issues: Vec<Issue> },
}

/// A package that `jevision.blocks.json` names and the project doesn't have, or has in another
/// version.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RestoreItem {
    pub id: String,
    /// The address it was installed from.
    pub source: String,
    /// `None` for a lock written before versions were kept.
    pub locked_version: Option<String>,
    pub state: RestoreState,
    /// The package as the address has it, staged to show what it does, when it is `Ready` or
    /// `VersionChanged`: install it with `install_block_set`, and pass its folder to
    /// `discard_block_staging` when done.
    pub preview: Option<BlockPreview>,
}

/// What it takes to give the project folder `project` the packages its lock file names: each one
/// it doesn't have, or has in another version, read from its address — each address once — and
/// compared with the version the lock keeps. Installs nothing. Waits for the downloads, so call
/// it off the main thread.
#[uniffi::export]
pub fn restore_block_sources(project: String) -> Result<Vec<RestoreItem>, BlockPackageError> {
    let project = Path::new(&project);
    let mut fetched: BTreeMap<String, Result<Vec<BlockPreview>, Vec<Issue>>> = BTreeMap::new();
    let mut items = Vec::new();
    for entry in jevision_core::restore_entries(project)? {
        let set = fetched.entry(entry.source.clone()).or_insert_with(|| {
            stage_source_set(&entry.source)
                .and_then(|staged| read_set(project, &entry.source, staged))
                .map_err(|e| e.issues)
        });
        let locked_version = entry.locked_version.map(|v| v.to_string());
        let (state, preview) = match set {
            Err(issues) => (
                RestoreState::Failed {
                    issues: issues.clone(),
                },
                None,
            ),
            Ok(set) => match set.iter().position(|p| p.manifest.id == entry.id) {
                None => (RestoreState::Missing, None),
                Some(at) => {
                    let preview = set.remove(at);
                    let found = &preview.manifest.version;
                    let state = match &locked_version {
                        Some(locked) if locked != found => RestoreState::VersionChanged {
                            found: found.clone(),
                        },
                        _ => RestoreState::Ready,
                    };
                    (state, Some(preview))
                }
            },
        };
        items.push(RestoreItem {
            id: entry.id,
            source: entry.source,
            locked_version,
            state,
            preview,
        });
    }
    // The packages of a source that the lock doesn't name.
    for other in fetched.into_values().flatten().flatten() {
        discard_staged(Path::new(&other.folder));
    }
    Ok(items)
}

/// A package that `jevision.blocks.json` names and the project doesn't have, or has in another
/// version, as the lock file alone tells.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UnrestoredBlock {
    pub id: String,
    /// The address it was installed from.
    pub source: String,
    /// `None` for a lock written before versions were kept.
    pub locked_version: Option<String>,
}

/// The packages `restore_block_sources` would fetch for the project folder `project`, read from
/// its lock file without going online: enough to offer a restore.
#[uniffi::export]
pub fn unrestored_blocks(project: String) -> Result<Vec<UnrestoredBlock>, BlockPackageError> {
    Ok(jevision_core::restore_entries(Path::new(&project))?
        .into_iter()
        .map(|entry| UnrestoredBlock {
            id: entry.id,
            source: entry.source,
            locked_version: entry.locked_version.map(|v| v.to_string()),
        })
        .collect())
}

/// A copy of an installed package among the project's own blocks.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OwnBlock {
    /// Relative to the project: `blocks/NAME.block.json`.
    pub file: String,
    pub name: String,
}

/// Copies the installed package `id` of the project folder `project` into the project's own
/// blocks, where it can be changed; the package stays installed. `switch_tests_to_copy` then
/// makes the tests that run the package (`block_usage`) run the copy.
#[uniffi::export]
pub fn make_own_block(project: String, id: String) -> Result<OwnBlock, BlockPackageError> {
    let own = jevision_core::make_own_block(Path::new(&project), &id)?;
    Ok(OwnBlock {
        file: own.file,
        name: own.name,
    })
}

/// Makes the tests and blocks of the project folder `project` that run the installed package
/// `id` run `own_file`, its copy from `make_own_block`, instead; the files changed, relative to
/// the project.
#[uniffi::export]
pub fn switch_tests_to_copy(
    project: String,
    id: String,
    own_file: String,
) -> Result<Vec<String>, BlockPackageError> {
    Ok(jevision_core::switch_tests_to_copy(
        Path::new(&project),
        &id,
        &own_file,
    )?)
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

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// Writes the package `id` into the folder `shelf`, needing `needs` as `(id, range, source)`,
    /// and gives its folder.
    fn package(shelf: &Path, id: &str, version: &str, needs: &[(&str, &str, &str)]) -> String {
        let needs: Vec<String> = (needs.iter())
            .map(|(id, range, source)| match *source {
                "" => format!(r#"{{"id":"{id}","version":"{range}"}}"#),
                _ => format!(r#"{{"id":"{id}","version":"{range}","source":"{source}"}}"#),
            })
            .collect();
        let dir = shelf.join(format!("{id}.jevblock"));
        write(
            &dir,
            jevision_core::MANIFEST_FILE,
            &format!(
                r#"{{"format":1,"id":"{id}","name":"{id}","version":"{version}","author":"Acme",
                    "license":"MIT","dependencies":[{}]}}"#,
                needs.join(",")
            ),
        );
        write(
            &dir,
            jevision_core::PACKAGE_FLOW_FILE,
            &format!(
                r#"{{"format":1,"name":"{id}","app":"{APP}","start":"a","blocks":{{
                    "a":{{"type":"pressKeys","keys":"cmd+s"}}}}}}"#
            ),
        );
        text(&dir)
    }

    fn item(folder: &str, source: &str) -> BlockInstallItem {
        BlockInstallItem {
            folder: folder.into(),
            source: source.into(),
            replace: false,
        }
    }

    fn ids(blocks: &[InstalledBlock]) -> Vec<&str> {
        blocks.iter().map(|b| b.manifest.id.as_str()).collect()
    }

    fn problems(plan: &BlockInstallPlan) -> Vec<(&str, Option<&str>)> {
        (plan.problems.iter())
            .map(|p| (p.issue.code.as_str(), p.folder.as_deref()))
            .collect()
    }

    #[test]
    fn a_block_travels_as_a_zip_and_is_read_before_it_is_installed() {
        let mine = project("share-mine");
        write(&mine, "blocks/Sign in.block.json", &sign_in_block());
        let zip = mine.join("out/sign-in.zip");
        std::fs::create_dir_all(zip.parent().unwrap()).unwrap();
        let out = export_block_package(
            text(&mine),
            "blocks/Sign in.block.json".into(),
            share(),
            text(&zip),
            true,
        )
        .unwrap();
        assert_eq!(out.manifest.name, "Sign in");
        assert!(zip.is_file());

        let theirs = project("share-theirs");
        let source = text(&zip);
        let set = inspect_block_source(text(&theirs), source.clone()).unwrap();
        assert!(set.temporary);
        let [seen] = <[BlockPreview; 1]>::try_from(set.packages).unwrap();
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
        assert!(list_community_blocks(text(&theirs)).is_empty());

        let installed =
            install_block_set(text(&theirs), vec![item(&seen.folder, &source)], false).unwrap();
        discard_block_staging(seen.folder.clone());
        assert!(!Path::new(&seen.folder).exists());
        assert_eq!(
            installed[0].steps_file,
            "blocks/com.anna.sign-in.jevblock/flow.json"
        );
        assert_eq!(installed[0].source, None);
        assert_eq!(installed[0].origin, BlockOrigin::Unknown);
        assert_eq!(seen.origin, BlockOrigin::ThisMac);
        assert!(seen.skipped_links.is_empty());
        assert_eq!(list_community_blocks(text(&theirs)), installed);

        // The same package again is the same version: nothing to replace.
        let again = inspect_block_source(text(&theirs), source)
            .unwrap()
            .packages;
        assert_eq!(again[0].change, BlockInstallChange::Same);
        let refused =
            install_block_set(text(&theirs), vec![item(&again[0].folder, "")], false).unwrap_err();
        discard_block_staging(again[0].folder.clone());
        assert_eq!(codes(refused), ["block.install.exists"]);
    }

    #[test]
    fn a_block_that_runs_an_installed_package_is_shared_with_it_as_a_dependency() {
        let root = project("share-calls");
        let path = text(&root);
        write(&root, "blocks/Sign in.block.json", &sign_in_block());
        let helper = export_block_package(
            path.clone(),
            "blocks/Sign in.block.json".into(),
            share(),
            text(&root.join("out")),
            false,
        )
        .unwrap();
        install_block_set(path.clone(), vec![item(&helper.path, "")], false).unwrap();
        write(
            &root,
            "blocks/Start.block.json",
            &format!(
                r#"{{"format":1,"name":"Start","app":"{APP}","start":"s","blocks":{{
                    "s":{{"type":"runTest","test":"com.anna.sign-in.jevblock/flow.json",
                          "inputs":{{"email":"${{input.email}}"}}}}}}}}"#
            ),
        );
        let mut start = share();
        start.id = "com.anna.start".into();
        let zip = root.join("start.zip");
        export_block_package(
            path.clone(),
            "blocks/Start.block.json".into(),
            start,
            text(&zip),
            true,
        )
        .unwrap();
        let other = project("share-calls-other");
        let seen = inspect_block_source(text(&other), text(&zip)).unwrap();
        let preview = &seen.packages[0];
        discard_block_staging(preview.folder.clone());
        assert!(preview.issues.is_empty(), "{:?}", preview.issues);
        let needs = &preview.manifest.dependencies;
        assert_eq!(needs.len(), 1);
        assert_eq!(
            (needs[0].id.as_str(), needs[0].version.as_str()),
            ("com.anna.sign-in", "^1.0.0")
        );
        assert_eq!(preview.manifest.inputs.len(), 1);
    }

    #[test]
    fn a_package_in_use_is_named_and_can_be_removed() {
        let root = project("usage");
        write(&root, "blocks/Sign in.block.json", &sign_in_block());
        let out = export_block_package(
            text(&root),
            "blocks/Sign in.block.json".into(),
            share(),
            text(&root.join("out")),
            false,
        )
        .unwrap();
        std::fs::remove_file(root.join("blocks/Sign in.block.json")).unwrap();
        let project_path = text(&root);
        install_block_set(project_path.clone(), vec![item(&out.path, "")], false).unwrap();
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
        let path = text(&root);
        let err =
            inspect_block_source(path.clone(), "http://example.com/block.zip".into()).unwrap_err();
        assert_eq!(codes(err), ["block.download.notHttps"]);

        write(&root, "blocks/Sign in.block.json", &sign_in_block());
        let out = export_block_package(
            path.clone(),
            "blocks/Sign in.block.json".into(),
            share(),
            text(&root.join("out")),
            false,
        )
        .unwrap();
        install_block_set(path.clone(), vec![item(&out.path, "")], false).unwrap();
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

    #[test]
    fn a_folder_of_packages_is_a_set_by_id_with_what_each_needs() {
        let root = project("set");
        let shelf = root.join("shelf");
        package(
            &shelf,
            "com.acme.b",
            "1.0.0",
            &[("com.acme.a", "^1.0.0", "https://example.com/a.zip")],
        );
        package(&shelf, "com.acme.a", "1.0.0", &[]);
        let set = inspect_block_source(text(&root), text(&shelf)).unwrap();
        assert!(!set.temporary);
        let ids: Vec<_> = set
            .packages
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect();
        assert_eq!(ids, ["com.acme.a", "com.acme.b"]);
        assert_eq!(
            set.packages[1].manifest.dependencies,
            [BlockDependency {
                id: "com.acme.a".into(),
                version: "^1.0.0".into(),
                source: Some("https://example.com/a.zip".into()),
            }]
        );
        // A folder the user chose stays where it is.
        discard_block_staging(set.packages[0].folder.clone());
        assert!(shelf.join("com.acme.a.jevblock").is_dir());
    }

    #[test]
    fn the_plan_says_what_each_package_needs_and_what_stops_it() {
        let root = project("plan");
        let path = text(&root);
        let shelf = root.join("shelf");
        package(&root.join("blocks"), "com.acme.base", "1.0.0", &[]);
        let helper = package(&shelf, "com.acme.helper", "2.0.0", &[]);
        let app = package(
            &shelf,
            "com.acme.app",
            "1.0.0",
            &[
                ("com.acme.base", "^1.0.0", ""),
                ("com.acme.helper", "^2.0.0", ""),
                ("com.acme.extra", "^1.0.0", "https://example.com/extra.zip"),
            ],
        );
        let plan = plan_block_install(path.clone(), vec![app.clone(), helper.clone()]).unwrap();
        let states: Vec<_> = (plan.dependencies.iter())
            .map(|d| {
                (
                    d.folder.as_str(),
                    d.id.as_str(),
                    d.source.as_deref(),
                    &d.status,
                )
            })
            .collect();
        assert_eq!(
            states,
            [
                (
                    app.as_str(),
                    "com.acme.base",
                    None,
                    &BlockDependencyStatus::Installed {
                        version: "1.0.0".into()
                    }
                ),
                (
                    app.as_str(),
                    "com.acme.helper",
                    None,
                    &BlockDependencyStatus::InSelection
                ),
                (
                    app.as_str(),
                    "com.acme.extra",
                    Some("https://example.com/extra.zip"),
                    &BlockDependencyStatus::Missing
                ),
            ]
        );
        assert_eq!(
            problems(&plan),
            [("block.dependency.missing", Some(app.as_str()))]
        );
        assert!(plan.order.is_empty());
        assert!(crate::describe_issue(plan.problems[0].issue.clone()).contains("com.acme.extra"));

        let picky = package(
            &shelf,
            "com.acme.picky",
            "1.0.0",
            &[("com.acme.helper", "^3.0.0", "")],
        );
        let plan = plan_block_install(path.clone(), vec![helper.clone(), picky.clone()]).unwrap();
        assert_eq!(
            plan.dependencies[0].status,
            BlockDependencyStatus::Unmet {
                installed: "2.0.0".into()
            }
        );
        assert_eq!(
            problems(&plan),
            [("block.dependency.unmet", Some(picky.as_str()))]
        );

        let ring = root.join("ring");
        let x = package(&ring, "com.acme.x", "1.0.0", &[("com.acme.y", "*", "")]);
        let y = package(&ring, "com.acme.y", "1.0.0", &[("com.acme.x", "*", "")]);
        let plan = plan_block_install(path.clone(), vec![x, y]).unwrap();
        assert_eq!(problems(&plan), [("block.dependency.cycle", None)]);

        // A newer base breaks the installed package that needs the old one.
        package(
            &root.join("blocks"),
            "com.acme.user",
            "1.0.0",
            &[("com.acme.base", "^1.0.0", "")],
        );
        let base = package(&root.join("next"), "com.acme.base", "2.0.0", &[]);
        let plan = plan_block_install(path.clone(), vec![base.clone()]).unwrap();
        assert_eq!(
            problems(&plan),
            [("block.dependency.breaks", Some(base.as_str()))]
        );

        let plan = plan_block_install(path, vec![text(&root.join("nothing"))]).unwrap_err();
        assert!(!codes(plan).is_empty());
    }

    #[test]
    fn a_set_goes_in_what_another_needs_first_and_not_at_all_with_a_problem() {
        let root = project("install-set");
        let path = text(&root);
        let shelf = root.join("shelf");
        let lonely = package(
            &shelf,
            "com.acme.lonely",
            "1.0.0",
            &[("com.acme.gone", "^1.0.0", "")],
        );
        let refused = install_block_set(path.clone(), vec![item(&lonely, "")], false).unwrap_err();
        assert_eq!(codes(refused), ["block.dependency.missing"]);
        assert!(list_community_blocks(path.clone()).is_empty());

        let app = package(
            &shelf,
            "com.acme.app",
            "1.0.0",
            &[("com.acme.helper", "^1.0.0", "")],
        );
        let helper = package(&shelf, "com.acme.helper", "1.0.0", &[]);
        let done = install_block_set(path.clone(), vec![item(&app, ""), item(&helper, "")], false)
            .unwrap();
        assert_eq!(ids(&done), ["com.acme.helper", "com.acme.app"]);

        // A helper the app can't use stops the install unless forced.
        let next = package(&root.join("next"), "com.acme.helper", "2.0.0", &[]);
        let replace = BlockInstallItem {
            replace: true,
            ..item(&next, "")
        };
        let refused = install_block_set(path.clone(), vec![replace.clone()], false).unwrap_err();
        assert_eq!(codes(refused), ["block.dependency.breaks"]);
        let done = install_block_set(path.clone(), vec![replace], true).unwrap();
        assert_eq!(done[0].manifest.version, "2.0.0");
        assert_eq!(
            unmet_dependencies(path.clone(), "com.acme.app".into())
                .into_iter()
                .map(|i| i.code)
                .collect::<Vec<_>>(),
            ["block.dependency.unmet"]
        );
        assert_eq!(
            block_dependents(path, "com.acme.helper".into()),
            ["com.acme.app"]
        );
    }

    /// A zip of the package folder `dir`, with a link at `images/logo.png` when `link`.
    fn zip_with_link(dir: &Path, zip: &Path, link: bool) {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::fs::File::create(zip).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for file in [
            jevision_core::MANIFEST_FILE,
            jevision_core::PACKAGE_FLOW_FILE,
        ] {
            writer.start_file(file, options).unwrap();
            writer
                .write_all(&std::fs::read(dir.join(file)).unwrap())
                .unwrap();
        }
        if link {
            writer
                .add_symlink("images/logo.png", "/etc/hosts", options)
                .unwrap();
        }
        writer.finish().unwrap();
    }

    fn link_warnings(preview: &BlockPreview) -> Vec<(Option<&str>, Option<&str>)> {
        (preview.issues.iter())
            .filter(|i| i.code == "block.links.skipped")
            .map(|i| (i.param("count"), i.param("links")))
            .collect()
    }

    #[test]
    fn a_zip_with_a_link_installs_without_it_and_says_which_link_was_left_out() {
        let root = project("links-zip");
        let dir = PathBuf::from(package(&root.join("shelf"), "com.acme.a", "1.0.0", &[]));
        for (link, skipped) in [(true, vec!["images/logo.png"]), (false, vec![])] {
            let zip = root.join(format!("a-{link}.zip"));
            zip_with_link(&dir, &zip, link);
            let set = inspect_block_source(text(&root), text(&zip)).unwrap();
            let [seen] = <[BlockPreview; 1]>::try_from(set.packages).unwrap();
            assert!(seen.can_install, "{:?}", seen.issues);
            assert_eq!(seen.skipped_links, skipped);
            let warnings = link_warnings(&seen);
            if link {
                assert_eq!(warnings, [(Some("1"), Some("images/logo.png"))]);
                let installed =
                    install_block_set(text(&root), vec![item(&seen.folder, "")], false).unwrap();
                let folder = root.join(&installed[0].folder);
                assert!(folder.join(jevision_core::PACKAGE_FLOW_FILE).is_file());
                assert!(std::fs::symlink_metadata(folder.join("images/logo.png")).is_err());
                remove_block_package(text(&root), "com.acme.a".into()).unwrap();
            } else {
                assert!(warnings.is_empty());
            }
            discard_block_staging(seen.folder);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_with_a_link_says_which_link_is_left_out() {
        let root = project("links-folder");
        let dir = PathBuf::from(package(&root.join("shelf"), "com.acme.a", "1.0.0", &[]));
        std::os::unix::fs::symlink("/etc/hosts", dir.join("hosts")).unwrap();
        let set = inspect_block_source(text(&root), text(&dir)).unwrap();
        assert!(!set.temporary);
        let seen = &set.packages[0];
        assert_eq!(seen.skipped_links, ["hosts"]);
        assert_eq!(link_warnings(seen), [(Some("1"), Some("hosts"))]);
        assert!(!seen.files.contains(&"hosts".to_owned()));
    }

    #[test]
    fn the_origin_comes_first_and_a_different_author_is_marked() {
        let account = BlockOrigin::GitHub {
            owner: "acme".into(),
        };
        let same = describe_block_origin_in("en", &account, "Acme");
        assert_eq!(same.source, "From the GitHub account acme");
        assert_eq!(same.author, "Author named in the block: Acme");
        assert_eq!(same.mismatch, None);
        let other = describe_block_origin_in("ru", &account, "Anna");
        assert_eq!(other.source, "Из аккаунта GitHub acme");
        assert!(
            other
                .mismatch
                .unwrap()
                .contains("ориентируйтесь на аккаунт")
        );
        let local = describe_block_origin_in("en", &BlockOrigin::ThisMac, "Acme");
        assert_eq!(local.mismatch, None);
        assert!(local.about.contains("Anyone could have made this file"));

        let root = project("origin");
        let first = package(&root.join("first"), "com.acme.a", "1.0.0", &[]);
        let address = "https://github.com/acme/blocks";
        install_block_set(text(&root), vec![item(&first, address)], false).unwrap();
        assert_eq!(list_community_blocks(text(&root))[0].origin, account);
        // A lock written before an address was kept, or by hand, names none.
        write(
            &root,
            jevision_core::PACKAGE_LOCK_FILE,
            r#"{"format":1,"sources":{}}"#,
        );
        assert_eq!(
            list_community_blocks(text(&root))[0].origin,
            BlockOrigin::Unknown
        );
    }

    #[test]
    fn an_address_is_kept_with_the_version_and_updates_come_from_its_set() {
        let root = project("lock");
        let path = text(&root);
        let first = package(&root.join("first"), "com.acme.a", "1.0.0", &[]);
        let address = "https://example.com/blocks.zip";
        let done = install_block_set(path.clone(), vec![item(&first, address)], false).unwrap();
        assert_eq!(done[0].source.as_deref(), Some(address));
        assert_eq!(
            done[0].origin,
            BlockOrigin::Web {
                host: "example.com".into()
            }
        );
        let lock: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(jevision_core::PACKAGE_LOCK_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(lock["sources"]["com.acme.a"], address);
        assert_eq!(lock["versions"]["com.acme.a"], "1.0.0");

        let point_at = |shelf: &Path| {
            write(
                &root,
                jevision_core::PACKAGE_LOCK_FILE,
                &format!(
                    r#"{{"format":1,"sources":{{"com.acme.a":"{}"}},"versions":{{"com.acme.a":"1.0.0"}}}}"#,
                    text(shelf)
                ),
            )
        };
        let next = root.join("next");
        package(&next, "com.acme.a", "1.1.0", &[]);
        package(&next, "com.acme.b", "1.0.0", &[]);
        point_at(&next);
        let update = update_block_package(path.clone(), "com.acme.a".into()).unwrap();
        assert_eq!(update.manifest.version, "1.1.0");
        assert_eq!(
            update.change,
            BlockInstallChange::Update {
                from: "1.0.0".into()
            }
        );

        let other = root.join("other");
        package(&other, "com.acme.b", "1.0.0", &[]);
        point_at(&other);
        let err = update_block_package(path.clone(), "com.acme.a".into()).unwrap_err();
        assert_eq!(codes(err), ["block.update.otherId"]);
        package(&other, "com.acme.c", "1.0.0", &[]);
        let err = update_block_package(path, "com.acme.a".into()).unwrap_err();
        assert_eq!(codes(err), ["block.update.missing"]);
    }

    #[test]
    fn restoring_reads_each_address_once_and_says_where_each_package_stands() {
        let root = project("restore");
        let path = text(&root);
        let shelf = root.join("shelf");
        package(&shelf, "com.acme.a", "1.0.0", &[]);
        package(&shelf, "com.acme.b", "2.0.0", &[]);
        package(&shelf, "com.acme.unlisted", "1.0.0", &[]);
        package(&root.join("blocks"), "com.acme.here", "1.0.0", &[]);
        let shelf = text(&shelf);
        let nowhere = text(&root.join("nowhere"));
        write(
            &root,
            jevision_core::PACKAGE_LOCK_FILE,
            &format!(
                r#"{{"format":1,
                    "sources":{{"com.acme.a":"{shelf}","com.acme.b":"{shelf}","com.acme.gone":"{shelf}",
                                "com.acme.here":"{shelf}","com.acme.lost":"{nowhere}"}},
                    "versions":{{"com.acme.a":"1.0.0","com.acme.b":"1.5.0","com.acme.here":"1.0.0"}}}}"#
            ),
        );
        let items = restore_block_sources(path.clone()).unwrap();
        let states: Vec<_> = (items.iter())
            .map(|i| (i.id.as_str(), &i.state, i.preview.is_some()))
            .collect();
        assert_eq!(states.len(), 4, "{states:?}");
        assert_eq!(states[0], ("com.acme.a", &RestoreState::Ready, true));
        assert_eq!(
            states[1],
            (
                "com.acme.b",
                &RestoreState::VersionChanged {
                    found: "2.0.0".into()
                },
                true
            )
        );
        assert_eq!(states[2], ("com.acme.gone", &RestoreState::Missing, false));
        assert_eq!(states[3].0, "com.acme.lost");
        assert!(
            matches!(&items[3].state, RestoreState::Failed { issues } if !issues.is_empty()),
            "{:?}",
            items[3].state
        );
        assert_eq!(items[1].locked_version.as_deref(), Some("1.5.0"));
        assert_eq!(items[0].source, shelf);
        let unrestored: Vec<_> = (unrestored_blocks(path.clone()).unwrap().into_iter())
            .map(|b| (b.id, b.locked_version))
            .collect();
        assert_eq!(
            unrestored,
            [
                ("com.acme.a".to_owned(), Some("1.0.0".to_owned())),
                ("com.acme.b".to_owned(), Some("1.5.0".to_owned())),
                ("com.acme.gone".to_owned(), None),
                ("com.acme.lost".to_owned(), None),
            ]
        );

        let ready = items[0].preview.as_ref().unwrap();
        install_block_set(path.clone(), vec![item(&ready.folder, &shelf)], false).unwrap();
        let left: Vec<_> = (restore_block_sources(path).unwrap().into_iter())
            .map(|i| i.id)
            .collect();
        assert_eq!(left, ["com.acme.b", "com.acme.gone", "com.acme.lost"]);
        let unrestored = unrestored_blocks(text(&root)).unwrap();
        assert_eq!(unrestored.len(), 3);
        assert_eq!(unrestored[0].source, shelf);
    }
}
