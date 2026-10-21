//! A made-up project as large as the biggest real ones, for measuring the app and the core on
//! it: tests in nested folders, the project's own blocks, shared tests run as sub-processes,
//! boards, settings, schedules and weeks of saved runs. The same spec writes the same files.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use jevision_core::{
    BLOCK_SUFFIX, BOARD_SUFFIX, BOARDS_FOLDER, BlockKind, Board, BoardNodeKind, DataTable,
    ENVIRONMENTS_FILE, ENVIRONMENTS_FORMAT, FORMAT_VERSION, Point, ProjectSettings, Quarantined,
    Repeat, Schedule, ScheduleTests, TEST_SUFFIX, TimeOfDay, Weekday,
};
use jevision_runner::{
    Attempt, DataRow, Failure, REPORT_FORMAT, RunReport, Step, StepStatus, Verdict, content_hash,
    create_run_dir, html, junit_xml, rfc3339,
};
use serde_json::{Value, json};

/// What [`write_synthetic_project`] makes. Shares are fractions from 0 to 1.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SyntheticSpec {
    /// The same seed and the same other fields write the same files.
    #[uniffi(default = 1)]
    pub seed: u64,
    /// Test files, the shared ones included.
    #[uniffi(default = 1000)]
    pub tests: u32,
    #[uniffi(default = 30)]
    pub min_blocks: u32,
    /// One test has exactly this many blocks; most have far fewer.
    #[uniffi(default = 200)]
    pub max_blocks: u32,
    /// Folders of tests, besides the one of shared tests.
    #[uniffi(default = 40)]
    pub folders: u32,
    /// How deep the deepest folder lies, the project folder not counted.
    #[uniffi(default = 4)]
    pub depth: u32,
    /// Share of tests that run shared tests or the project's own blocks as sub-processes.
    #[uniffi(default = 0.4)]
    pub subprocess_share: f64,
    /// Blocks of the project's own, in its `blocks` folder.
    #[uniffi(default = 8)]
    pub custom_blocks: u32,
    /// Share of tests that need no screen, so Run All goes through them without one.
    #[uniffi(default = 1.0)]
    pub screenless_share: f64,
    /// Saved runs go back this many days.
    #[uniffi(default = 30)]
    pub days: u32,
    /// Saved runs, spread evenly over the tests.
    #[uniffi(default = 5000)]
    pub reports: u32,
    /// Share of saved runs that failed, retry included.
    #[uniffi(default = 0.1)]
    pub failure_share: f64,
    /// Share of saved runs that failed and then passed on the retry.
    #[uniffi(default = 0.05)]
    pub flaky_share: f64,
    /// When the history ends, in milliseconds since 1970: 2026-10-20 12:00 UTC by default.
    #[uniffi(default = 1792497600000)]
    pub end_ms: u64,
}

impl Default for SyntheticSpec {
    fn default() -> Self {
        Self {
            seed: 1,
            tests: 1000,
            min_blocks: 30,
            max_blocks: 200,
            folders: 40,
            depth: 4,
            subprocess_share: 0.4,
            custom_blocks: 8,
            screenless_share: 1.0,
            days: 30,
            reports: 5000,
            failure_share: 0.1,
            flaky_share: 0.05,
            end_ms: 1_792_497_600_000,
        }
    }
}

/// What [`write_synthetic_project`] wrote.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyntheticSummary {
    /// Test files relative to the project folder, in the order of the sidebar.
    pub tests: Vec<String>,
    /// The tests of [`tests`](Self::tests) that need no screen, in the same order.
    pub screenless: Vec<String>,
    /// The test with the most blocks.
    pub largest: String,
    pub largest_blocks: u32,
    /// Blocks in all the tests.
    pub blocks: u64,
    /// Folders with tests, the shared one included.
    pub folders: u32,
    pub depth: u32,
    pub custom_blocks: u32,
    /// Tests that run another test or a block of the project's own.
    pub subprocess_tests: u32,
    pub reports: u32,
    pub failed_reports: u32,
    pub flaky_reports: u32,
    /// Screenshots of failed steps in the saved runs.
    pub screenshots: u32,
    /// Board files, relative to the project folder.
    pub boards: Vec<String>,
    /// Everything in the project folder.
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum SyntheticError {
    /// The spec asks for what can't be made, such as more folders than tests.
    #[error("{reason}")]
    Invalid { reason: String },
    #[error("{path}: {reason}")]
    Unwritable { path: String, reason: String },
}

/// Writes a made-up project of `spec` into the folder `dir`, which must be empty or missing.
/// Every test is valid, those without a screen run as they are, and the saved runs are the
/// files a real run leaves.
#[uniffi::export]
pub fn write_synthetic_project(
    dir: String,
    spec: SyntheticSpec,
) -> Result<SyntheticSummary, SyntheticError> {
    check(&spec)?;
    let root = PathBuf::from(dir);
    if fs::read_dir(&root).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(invalid(format!("{} is not empty", root.display())));
    }
    fs::create_dir_all(&root).map_err(|e| unwritable(&root, &e))?;
    Generator::new(&root, &spec).write()
}

const APP: &str = "com.example.Notes";
const DATA_FILE: &str = "data/users.csv";
const DATA: &str = "name,password\nanna,one\nboris,two\n";
const SHARED_FOLDER: &str = "shared";
const READY_IMAGE: &str = "refs/ready.png";
const SCREEN_IMAGE: &str = "refs/screen.png";
const ENVIRONMENT: &str = "staging";
const NIGHTLY: &str = "Nightly";
/// Share of tests whose runs fail at times; the others always pass.
const UNSTABLE: f64 = 0.2;
const DAY_MS: u64 = 86_400_000;

const FOLDER_WORDS: &[&str] = &[
    "accounts",
    "billing",
    "calendar",
    "cart",
    "checkout",
    "dashboard",
    "editor",
    "export",
    "import",
    "inbox",
    "invoices",
    "media",
    "messages",
    "notifications",
    "onboarding",
    "orders",
    "payments",
    "preferences",
    "printing",
    "profile",
    "reports",
    "search",
    "settings",
    "sharing",
    "signup",
    "storage",
    "sync",
    "team",
    "upload",
    "widgets",
];
const VERBS: &[&str] = &[
    "add",
    "archive",
    "cancel",
    "change",
    "check",
    "close",
    "copy",
    "create",
    "delete",
    "download",
    "duplicate",
    "edit",
    "filter",
    "find",
    "open",
    "pin",
    "rename",
    "reorder",
    "restore",
    "share",
    "sort",
    "undo",
    "update",
    "upload",
    "view",
];
const OBJECTS: &[&str] = &[
    "address", "avatar", "card", "comment", "contact", "coupon", "draft", "file", "folder",
    "invoice", "item", "label", "message", "note", "order", "password", "photo", "plan",
    "reminder", "report", "tag", "task", "theme", "user", "webhook",
];
const QUALIFIERS: &[&str] = &[
    "",
    "-again",
    "-offline",
    "-twice",
    "-as-guest",
    "-in-dark-mode",
    "-with-keyboard",
    "-after-restart",
];
const BLOCK_NAMES: &[&str] = &[
    "fill-address",
    "pick-date",
    "sign-in-as",
    "set-locale",
    "open-section",
    "reset-cache",
    "wait-for-sync",
    "seed-account",
    "clear-drafts",
    "toggle-flag",
];
const TAGS: &[&str] = &[
    "smoke",
    "regression",
    "nightly",
    "ui",
    "api",
    "slow",
    "critical-path",
];
const AUTHORS: &[&str] = &["Anna", "Boris", "Chen", "Dana", "Emil"];

fn invalid(reason: String) -> SyntheticError {
    SyntheticError::Invalid { reason }
}

fn unwritable(path: &Path, e: &dyn std::fmt::Display) -> SyntheticError {
    SyntheticError::Unwritable {
        path: path.display().to_string(),
        reason: e.to_string(),
    }
}

fn check(spec: &SyntheticSpec) -> Result<(), SyntheticError> {
    let shared = shared_count(spec.tests);
    if spec.folders == 0 || spec.depth == 0 || spec.depth > spec.folders {
        return Err(invalid(format!(
            "{} folders can't be {} deep",
            spec.folders, spec.depth
        )));
    }
    if spec.tests < spec.folders + shared {
        return Err(invalid(format!(
            "{} tests can't fill {} folders and {shared} shared tests",
            spec.tests, spec.folders
        )));
    }
    if spec.min_blocks < 2 || spec.max_blocks < spec.min_blocks {
        return Err(invalid(format!(
            "tests of {} to {} blocks: a test needs at least 2",
            spec.min_blocks, spec.max_blocks
        )));
    }
    let shares = [
        spec.subprocess_share,
        spec.screenless_share,
        spec.failure_share,
        spec.flaky_share,
    ];
    if shares.iter().any(|s| !(0.0..=1.0).contains(s))
        || spec.failure_share + spec.flaky_share > 1.0
    {
        return Err(invalid(
            "shares must be from 0 to 1, failures and flaky runs together too".to_owned(),
        ));
    }
    if spec.reports > 0 && (spec.days == 0 || u64::from(spec.days) * DAY_MS > spec.end_ms) {
        return Err(invalid(format!(
            "{} days of history can't end at {} ms",
            spec.days, spec.end_ms
        )));
    }
    Ok(())
}

fn shared_count(tests: u32) -> u32 {
    (tests / 20).max(1)
}

/// SplitMix64: small, fast and the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// From 0 to `n` - 1; `n` must not be 0.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

struct Folder {
    path: String,
    depth: u32,
}

/// What kind of piece of a test comes next, and how many blocks it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece {
    Pause,
    Stamp,
    Branch,
    Region,
    Loop,
    Data,
    File,
    Shared,
    Custom,
    Click,
    Type,
    Keys,
    Wait,
    Check,
    Look,
}

impl Piece {
    fn size(self) -> u32 {
        match self {
            Self::Branch | Self::Region | Self::Look => 3,
            Self::Loop | Self::Data => 2,
            _ => 1,
        }
    }
}

const SCREENLESS_PIECES: &[(Piece, u32)] = &[
    (Piece::Stamp, 30),
    (Piece::Branch, 25),
    (Piece::Region, 10),
    (Piece::Loop, 15),
    (Piece::File, 15),
    (Piece::Pause, 5),
];
const SCREEN_PIECES: &[(Piece, u32)] = &[
    (Piece::Click, 25),
    (Piece::Type, 15),
    (Piece::Keys, 10),
    (Piece::Wait, 10),
    (Piece::Check, 5),
    (Piece::Look, 10),
    (Piece::Stamp, 10),
    (Piece::Branch, 10),
    (Piece::Loop, 5),
];

/// How a saved run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
    /// Failed, then passed on the retry.
    Flaky,
}

/// One step a run of a test goes through, the same in every saved run.
#[derive(Clone)]
struct Planned {
    test: String,
    block: String,
    kind: &'static str,
    branch: bool,
    /// Name of the variable a date and time block saves and whether it is the date.
    saves: Option<(String, bool)>,
}

/// A test as written, and what its runs look like.
struct Written {
    file: String,
    screen: bool,
    data: bool,
    unstable: bool,
    hash: String,
    name: String,
    steps: Vec<Planned>,
    /// The block a failed run stops at: the last one.
    fails_at: String,
    failure_path: String,
}

/// Blocks of a test as they are made, back to front.
struct Chain {
    blocks: serde_json::Map<String, Value>,
    made: u32,
}

impl Chain {
    fn new() -> Self {
        Self {
            blocks: serde_json::Map::new(),
            made: 0,
        }
    }

    fn add(&mut self, kind: &str, mut block: Value, next: Option<&str>) -> String {
        self.made += 1;
        let id = format!("{kind}-{}", self.made);
        block["type"] = json!(kind);
        if let Some(next) = next {
            block["next"] = json!(next);
        }
        self.blocks.insert(id.clone(), block);
        id
    }
}

struct Generator<'a> {
    root: &'a Path,
    spec: &'a SyntheticSpec,
    rng: Rng,
    folders: Vec<Folder>,
    shared: Vec<String>,
    /// File of each block of the project's own and the inputs it takes.
    custom: Vec<(String, Vec<&'static str>)>,
    /// Steps of the tests and blocks that others run, by file relative to the project.
    callees: HashMap<String, Vec<Planned>>,
    shot: Vec<u8>,
}

impl<'a> Generator<'a> {
    fn new(root: &'a Path, spec: &'a SyntheticSpec) -> Self {
        Self {
            root,
            spec,
            rng: Rng(spec.seed),
            folders: Vec::new(),
            shared: Vec::new(),
            custom: Vec::new(),
            callees: HashMap::new(),
            shot: Vec::new(),
        }
    }

    fn write(mut self) -> Result<SyntheticSummary, SyntheticError> {
        self.shot = picture(64, 40, [214, 69, 65])?;
        self.make_folders();
        self.write_file(DATA_FILE, DATA.as_bytes())?;
        self.write_custom_blocks()?;
        let mut written = self.write_shared_tests()?;
        written.extend(self.write_tests()?);
        let order = self.sidebar_order(&written);
        written.sort_by_key(|w| order[w.file.as_str()]);
        if written.iter().any(|w| w.screen) {
            let reference = picture(32, 32, [40, 120, 220])?;
            self.write_file(READY_IMAGE, &reference)?;
            self.write_file(SCREEN_IMAGE, &reference)?;
        }
        let quarantined: Vec<&Written> = written.iter().filter(|w| w.unstable).take(3).collect();
        self.write_settings(&quarantined)?;
        self.write_environments()?;
        self.write_schedules(&written)?;
        let boards = self.write_boards(&written)?;
        let (failed_reports, flaky_reports, screenshots) = self.write_runs(&written)?;
        let (files, bytes) = tally(self.root);
        let blocks = |w: &Written| w.steps.iter().filter(|s| s.test == w.file).count();
        let largest = written
            .iter()
            .max_by_key(|w| (blocks(w), std::cmp::Reverse(w.file.clone())))
            .map(|w| w.file.clone())
            .unwrap_or_default();
        Ok(SyntheticSummary {
            screenless: written
                .iter()
                .filter(|w| !w.screen)
                .map(|w| w.file.clone())
                .collect(),
            largest_blocks: written
                .iter()
                .find(|w| w.file == largest)
                .map_or(0, |w| blocks(w) as u32),
            largest,
            blocks: written.iter().map(|w| blocks(w) as u64).sum(),
            folders: self.folders.len() as u32 + 1,
            depth: self.folders.iter().map(|f| f.depth).max().unwrap_or(0),
            custom_blocks: self.custom.len() as u32,
            subprocess_tests: written
                .iter()
                .filter(|w| w.steps.iter().any(|s| s.kind == "runTest"))
                .count() as u32,
            reports: self.spec.reports,
            failed_reports,
            flaky_reports,
            screenshots,
            boards,
            files,
            bytes,
            tests: written.into_iter().map(|w| w.file).collect(),
        })
    }

    fn write_file(&self, relative: &str, bytes: &[u8]) -> Result<(), SyntheticError> {
        write(&self.root.join(relative), bytes)
    }

    /// A chain of folders as deep as asked, then the rest under random parents above the
    /// deepest level.
    fn make_folders(&mut self) {
        let mut parent: Option<usize> = None;
        while self.folders.len() < self.spec.folders as usize {
            let depth = parent.map_or(0, |p| self.folders[p].depth) + 1;
            let prefix = parent.map_or(String::new(), |p| format!("{}/", self.folders[p].path));
            let taken = |name: &str, folders: &[Folder]| {
                let path = format!("{prefix}{name}");
                path == SHARED_FOLDER || folders.iter().any(|f| f.path == path)
            };
            let mut name = (*self.rng.pick(FOLDER_WORDS)).to_owned();
            let mut n = 2;
            while taken(&name, &self.folders) {
                name = format!("{}-{n}", self.rng.pick(FOLDER_WORDS));
                n += 1;
            }
            self.folders.push(Folder {
                path: format!("{prefix}{name}"),
                depth,
            });
            parent = if (self.folders.len() as u32) < self.spec.depth {
                Some(self.folders.len() - 1)
            } else {
                let roots: Vec<usize> = (0..self.folders.len())
                    .filter(|&i| self.folders[i].depth < self.spec.depth)
                    .collect();
                (!self.rng.chance(0.25)).then(|| *self.rng.pick(&roots))
            };
        }
    }

    fn write_custom_blocks(&mut self) -> Result<(), SyntheticError> {
        for n in 0..self.spec.custom_blocks as usize {
            let stem = match n / BLOCK_NAMES.len() {
                0 => BLOCK_NAMES[n].to_owned(),
                round => format!("{}-{}", BLOCK_NAMES[n % BLOCK_NAMES.len()], round + 1),
            };
            let inputs: Vec<&'static str> = if n % 2 == 0 {
                vec!["value"]
            } else {
                vec!["value", "label"]
            };
            let mut chain = Chain::new();
            let done = chain.add(
                "dateTime",
                json!({"saveAs": "blockDone", "format": "%H:%M"}),
                None,
            );
            let mut first = chain.add(
                "if",
                json!({"value": {"of": "${input.value}", "is": "notEquals", "to": ""}, "then": done, "else": done}),
                None,
            );
            if inputs.contains(&"label") {
                first = chain.add(
                    "if",
                    json!({"value": {"of": "${input.label}", "is": "contains", "to": "a"}, "then": first, "else": first}),
                    None,
                );
            }
            let file = format!("blocks/{stem}{BLOCK_SUFFIX}");
            let flow = json!({
                "format": FORMAT_VERSION,
                "name": title(&stem),
                "description": "A step several tests share, filled in with their own values.",
                "app": APP,
                "start": first,
                "blocks": chain.blocks,
            });
            let (_, steps) = self.save_flow(&file, &flow)?;
            self.callees.insert(file.clone(), steps);
            self.custom.push((file, inputs));
        }
        Ok(())
    }

    fn write_shared_tests(&mut self) -> Result<Vec<Written>, SyntheticError> {
        let mut written = Vec::new();
        for _ in 0..shared_count(self.spec.tests) {
            let file = self.new_test_file(SHARED_FOLDER);
            let blocks = self.rng.between(4, 10) as u32;
            let test = self.write_test(&file, 1, blocks, false, false, false)?;
            self.callees.insert(file.clone(), test.steps.clone());
            self.shared.push(file);
            written.push(test);
        }
        Ok(written)
    }

    fn write_tests(&mut self) -> Result<Vec<Written>, SyntheticError> {
        let count = self.spec.tests - shared_count(self.spec.tests);
        let mut written = Vec::new();
        for n in 0..count as usize {
            // Every folder gets a test first, then a test lands anywhere, the project folder too.
            let folder = if n < self.folders.len() {
                Some(n)
            } else {
                let at = self.rng.below(self.folders.len() + 1);
                (at < self.folders.len()).then_some(at)
            };
            let (path, depth) = folder.map_or((String::new(), 0), |f| {
                (self.folders[f].path.clone(), self.folders[f].depth)
            });
            let file = self.new_test_file(&path);
            let (min, max) = (self.spec.min_blocks, self.spec.max_blocks);
            let blocks = if n == 0 {
                max
            } else {
                min + ((max - min) as f64 * self.rng.unit().powi(3)).round() as u32
            };
            let screen = !self.rng.chance(self.spec.screenless_share);
            let subprocess = self.rng.chance(self.spec.subprocess_share);
            let data = self.rng.chance(0.15);
            written.push(self.write_test(&file, depth, blocks, screen, subprocess, data)?);
        }
        Ok(written)
    }

    /// A file name for a new test in `folder`, not used there yet.
    fn new_test_file(&mut self, folder: &str) -> String {
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        let mut attempt = 0;
        loop {
            let stem = format!(
                "{}-{}{}",
                self.rng.pick(VERBS),
                self.rng.pick(OBJECTS),
                self.rng.pick(QUALIFIERS)
            );
            let stem = if attempt < 20 {
                stem
            } else {
                format!("{stem}-{attempt}")
            };
            let file = format!("{prefix}{stem}{TEST_SUFFIX}");
            if !self.root.join(&file).exists() {
                return file;
            }
            attempt += 1;
        }
    }

    fn write_test(
        &mut self,
        file: &str,
        depth: u32,
        blocks: u32,
        screen: bool,
        subprocess: bool,
        data: bool,
    ) -> Result<Written, SyntheticError> {
        let up = "../".repeat(depth as usize);
        let own = file.rsplit('/').next().unwrap_or(file).to_owned();
        let stem = own.strip_suffix(TEST_SUFFIX).unwrap_or(&own).to_owned();
        let mut pieces = Vec::new();
        let mut left = blocks - 2;
        if data && left >= Piece::Data.size() {
            pieces.push(Piece::Data);
            left -= Piece::Data.size();
        }
        if subprocess {
            for _ in 0..self.rng.between(1, 3) {
                if left == 0 {
                    break;
                }
                let custom = !self.custom.is_empty() && self.rng.chance(0.5);
                pieces.push(if custom { Piece::Custom } else { Piece::Shared });
                left -= 1;
            }
        }
        let menu = if screen {
            SCREEN_PIECES
        } else {
            SCREENLESS_PIECES
        };
        while left > 0 {
            let fits: Vec<(Piece, u32)> = menu
                .iter()
                .copied()
                .filter(|(p, _)| p.size() <= left)
                .collect();
            let total: u32 = fits.iter().map(|(_, w)| w).sum();
            let mut roll = self.rng.below(total as usize) as u32;
            let piece = fits
                .iter()
                .find(|(_, w)| {
                    let hit = roll < *w;
                    roll = roll.saturating_sub(*w);
                    hit
                })
                .map_or(Piece::Stamp, |(p, _)| *p);
            pieces.push(piece);
            left -= piece.size();
        }
        for i in (1..pieces.len()).rev() {
            pieces.swap(i, self.rng.below(i + 1));
        }

        let mut chain = Chain::new();
        let (tail, failure_path) = if screen {
            let image = format!("{up}{READY_IMAGE}");
            let block = json!({"until": {"appears": {"image": image}}, "timeout": "5s"});
            (chain.add("waitFor", block, None), image)
        } else {
            let block = json!({"path": own, "until": "exists", "timeout": "5s"});
            (chain.add("checkFile", block, None), own.clone())
        };
        let mut next = tail.clone();
        for piece in pieces.into_iter().rev() {
            next = self.piece(&mut chain, piece, &up, &next);
        }
        let start = chain.add(
            "dateTime",
            json!({"saveAs": "today", "format": "%F"}),
            Some(&next),
        );

        let shared = file.starts_with(&format!("{SHARED_FOLDER}/"));
        let unstable = !shared
            && self
                .rng
                .chance(UNSTABLE.max(self.spec.failure_share + self.spec.flaky_share));
        let tags: Vec<&str> = (0..self.rng.below(4))
            .map(|_| *self.rng.pick(TAGS))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut flow = json!({
            "format": FORMAT_VERSION,
            "name": title(&stem),
            "description": format!("Makes sure people can {} in the app.", title(&stem).to_lowercase()),
            "app": APP,
            "testType": self.rng.pick(&["smoke", "regression", "acceptance"]),
            "priority": self.rng.pick(&["critical", "high", "medium", "low"]),
            "status": self.rng.pick(&["ready", "ready", "ready", "draft", "outdated"]),
            "tags": tags,
            "author": self.rng.pick(AUTHORS),
            "start": start,
        });
        if unstable && self.rng.chance(0.3) {
            flow["flaky"] = json!(true);
        }
        if !shared && self.rng.chance(0.05) {
            let needs = self.rng.pick(&self.shared).clone();
            flow["dependsOn"] = json!([format!("{up}{needs}")]);
        }
        if self.rng.chance(0.25) {
            let positions: serde_json::Map<String, Value> = chain
                .blocks
                .keys()
                .enumerate()
                .map(|(i, id)| {
                    let (x, y) = ((i % 6) as f64 * 280.0, (i / 6) as f64 * 140.0);
                    (id.clone(), json!({"x": x, "y": y}))
                })
                .collect();
            flow["canvas"] = json!({ "positions": positions });
        }
        flow["blocks"] = Value::Object(chain.blocks);
        let (bytes, steps) = self.save_flow(file, &flow)?;
        let folder = self.root.join(file);
        let folder = folder.parent().unwrap_or(self.root);
        Ok(Written {
            file: file.to_owned(),
            screen,
            data,
            unstable,
            hash: content_hash(&bytes),
            name: title(&stem),
            steps,
            fails_at: tail,
            failure_path: folder.join(failure_path).display().to_string(),
        })
    }

    /// Adds the blocks of `piece` leading to `next` and returns the first.
    fn piece(&mut self, chain: &mut Chain, piece: Piece, up: &str, next: &str) -> String {
        let point = |rng: &mut Rng| json!({"point": {"x": rng.between(5, 95) as f64 / 100.0, "y": rng.between(5, 95) as f64 / 100.0}});
        match piece {
            Piece::Pause => chain.add("pause", json!({"duration": "1ms"}), Some(next)),
            Piece::Stamp => {
                let name = format!("time{}", chain.made + 1);
                chain.add(
                    "dateTime",
                    json!({"saveAs": name, "format": "%H:%M"}),
                    Some(next),
                )
            }
            Piece::Branch | Piece::Region => {
                let name = format!("time{}", chain.made + 1);
                let then = chain.add(
                    "dateTime",
                    json!({"saveAs": name, "format": "%H:%M"}),
                    Some(next),
                );
                let otherwise = chain.add("pause", json!({"duration": "1ms"}), Some(next));
                let value = if piece == Piece::Branch {
                    json!({"of": "${var.today}", "is": "notEquals", "to": ""})
                } else {
                    json!({"of": "${env.REGION}", "is": "equals", "to": "eu"})
                };
                chain.add(
                    "if",
                    json!({"value": value, "then": then, "else": otherwise}),
                    None,
                )
            }
            Piece::Loop | Piece::Data => {
                // The body links back to the loop, which is made last, so its id is known.
                let id = format!("{}-{}", loop_kind(piece), chain.made + 2);
                let name = format!("time{}", chain.made + 1);
                let body = chain.add(
                    "dateTime",
                    json!({"saveAs": name, "format": "%H:%M:%S"}),
                    Some(&id),
                );
                let block = if piece == Piece::Loop {
                    json!({"times": self.rng.between(2, 3), "body": body})
                } else {
                    json!({"file": format!("{up}{DATA_FILE}"), "body": body})
                };
                chain.add(loop_kind(piece), block, Some(next))
            }
            Piece::File => chain.add(
                "checkFile",
                json!({"path": format!("{up}{DATA_FILE}"), "until": {"contains": "anna"}, "timeout": "5s"}),
                Some(next),
            ),
            Piece::Shared => {
                let test = self.rng.pick(&self.shared).clone();
                chain.add("runTest", json!({"test": format!("{up}{test}")}), Some(next))
            }
            Piece::Custom => {
                let (file, inputs) = self.rng.pick(&self.custom).clone();
                let values: serde_json::Map<String, Value> = inputs
                    .iter()
                    .map(|i| ((*i).to_owned(), json!(self.rng.pick(AUTHORS).to_lowercase())))
                    .collect();
                chain.add(
                    "runTest",
                    json!({"test": format!("{up}{file}"), "inputs": values}),
                    Some(next),
                )
            }
            Piece::Click => {
                let target = point(&mut self.rng);
                chain.add(
                    "click",
                    json!({"target": target, "button": "left"}),
                    Some(next),
                )
            }
            Piece::Type => chain.add("type", json!({"text": "Hello from the test"}), Some(next)),
            Piece::Keys => chain.add(
                "pressKeys",
                json!({"keys": self.rng.pick(&["cmd+s", "cmd+n", "return", "esc"])}),
                Some(next),
            ),
            Piece::Wait => chain.add(
                "waitFor",
                json!({"until": {"appears": {"image": format!("{up}{READY_IMAGE}")}}, "timeout": "5s"}),
                Some(next),
            ),
            Piece::Check => chain.add(
                "check",
                json!({"reference": {"image": format!("{up}{SCREEN_IMAGE}")}}),
                Some(next),
            ),
            Piece::Look => {
                let target = point(&mut self.rng);
                let then = chain.add(
                    "click",
                    json!({"target": target, "button": "left"}),
                    Some(next),
                );
                let otherwise = chain.add("pressKeys", json!({"keys": "esc"}), Some(next));
                chain.add(
                    "if",
                    json!({"reference": {"image": format!("{up}{SCREEN_IMAGE}")}, "then": then, "else": otherwise}),
                    None,
                )
            }
        }
    }

    /// Saves the test or block `file` in canonical form and returns its bytes and the steps
    /// a run of it goes through.
    fn save_flow(
        &mut self,
        file: &str,
        flow: &Value,
    ) -> Result<(Vec<u8>, Vec<Planned>), SyntheticError> {
        let text = jevision_core::canonicalize(&flow.to_string()).map_err(|e| {
            let codes: Vec<&str> = e.issues.iter().map(|i| i.code.as_str()).collect();
            invalid(format!("{file} came out invalid: {}", codes.join(", ")))
        })?;
        let parsed = jevision_core::parse(&text)
            .map_err(|_| invalid(format!("{file} doesn't read back")))?;
        let folder = file.rsplit_once('/').map_or("", |(f, _)| f);
        let mut steps = Vec::new();
        for block in parsed.blocks_in_flow_order() {
            steps.push(Planned {
                test: file.to_owned(),
                block: block.id.clone(),
                kind: block.kind.type_name(),
                branch: matches!(block.kind, BlockKind::If { .. }),
                saves: match &block.kind {
                    BlockKind::DateTime {
                        save_as, format, ..
                    } => Some((save_as.clone(), format == "%F")),
                    _ => None,
                },
            });
            if let BlockKind::RunTest { test, .. } = &block.kind
                && let Some(inner) = self.callees.get(&join(folder, test))
            {
                steps.extend(inner.iter().cloned());
            }
        }
        self.write_file(file, text.as_bytes())?;
        Ok((text.into_bytes(), steps))
    }

    /// Where each test lands in the sidebar: a folder's own tests by name, then its folders
    /// by name, each the same way.
    fn sidebar_order(&self, written: &[Written]) -> HashMap<String, usize> {
        let mut by_folder: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for w in written {
            let folder = w.file.rsplit_once('/').map_or("", |(f, _)| f);
            by_folder.entry(folder).or_default().push(&w.file);
        }
        fn walk<'a>(
            folder: &str,
            by_folder: &BTreeMap<&'a str, Vec<&'a str>>,
            out: &mut Vec<&'a str>,
        ) {
            let mut tests = by_folder.get(folder).cloned().unwrap_or_default();
            tests.sort_unstable();
            out.extend(tests);
            let prefix = if folder.is_empty() {
                String::new()
            } else {
                format!("{folder}/")
            };
            let children: Vec<&str> = by_folder
                .keys()
                .filter_map(|f| f.strip_prefix(prefix.as_str()).filter(|_| *f != folder))
                .map(|rest| rest.split('/').next().unwrap_or(rest))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            for child in children {
                walk(&format!("{prefix}{child}"), by_folder, out);
            }
        }
        let mut order = Vec::new();
        walk("", &by_folder, &mut order);
        order
            .into_iter()
            .enumerate()
            .map(|(i, f)| (f.to_owned(), i))
            .collect()
    }

    fn write_settings(&self, quarantined: &[&Written]) -> Result<(), SyntheticError> {
        let settings = ProjectSettings {
            retries: Some(1),
            quarantine: quarantined
                .iter()
                .map(|w| Quarantined {
                    test: w.file.clone(),
                    reason: Some("Fails now and then on the build machine".to_owned()),
                    until: None,
                })
                .collect(),
            ..ProjectSettings::default()
        };
        settings
            .save(self.root)
            .map_err(|e| unwritable(self.root, &e))
    }

    fn write_environments(&self) -> Result<(), SyntheticError> {
        let file = json!({
            "format": ENVIRONMENTS_FORMAT,
            "default": ENVIRONMENT,
            "environments": {
                ENVIRONMENT: {"REGION": "eu", "HOST": "staging.example.com"},
                "production": {"REGION": "us", "HOST": "example.com"},
            },
        });
        let mut text = serde_json::to_string_pretty(&file).unwrap_or_default();
        text.push('\n');
        self.write_file(ENVIRONMENTS_FILE, text.as_bytes())
    }

    fn write_schedules(&self, written: &[Written]) -> Result<(), SyntheticError> {
        let folder = self.folders[0].path.clone();
        let picked = written.iter().step_by(97).map(|w| w.file.clone()).collect();
        let schedules = [
            Schedule {
                name: NIGHTLY.to_owned(),
                tests: ScheduleTests::All,
                repeat: Repeat::Daily,
                at: TimeOfDay { hour: 2, minute: 0 },
                environment: Some(ENVIRONMENT.to_owned()),
                video: None,
            },
            Schedule {
                name: "Morning smoke".to_owned(),
                tests: ScheduleTests::Folder(folder),
                repeat: Repeat::Days(vec![
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ]),
                at: TimeOfDay {
                    hour: 9,
                    minute: 30,
                },
                environment: Some(ENVIRONMENT.to_owned()),
                video: None,
            },
            Schedule {
                name: "Every 4 hours".to_owned(),
                tests: ScheduleTests::Files(picked),
                repeat: Repeat::EveryHours(4),
                at: TimeOfDay { hour: 0, minute: 0 },
                environment: None,
                video: None,
            },
        ];
        jevision_core::save_schedules(self.root, &schedules)
            .map_err(|e| unwritable(self.root, &format!("{e:?}")))
    }

    /// The board of all tests, as the app keeps it, and a board drawn by hand.
    fn write_boards(&self, written: &[Written]) -> Result<Vec<String>, SyntheticError> {
        let tests: Vec<String> = written.iter().map(|w| w.file.clone()).collect();
        let root_title = self
            .root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mut all = Board {
            all_tests: true,
            ..Board::default()
        };
        all.sync_tests(&tests, &root_title);

        let mut plan = Board::default();
        let at = |x: f64, y: f64| Point { x, y };
        let heading = plan.add(
            BoardNodeKind::Text {
                text: "# Release 4.2\nWhat has to pass before it ships.".to_owned(),
            },
            at(0.0, 0.0),
        );
        plan.add(
            BoardNodeKind::Link {
                url: "https://example.com/releases/4.2".to_owned(),
            },
            at(640.0, 0.0),
        );
        plan.add(
            BoardNodeKind::Group {
                label: "Must pass".to_owned(),
                collapsed: false,
                folder: None,
            },
            at(560.0, 700.0),
        );
        let mut previous = heading;
        for (i, file) in tests.iter().step_by(tests.len().div_ceil(12)).enumerate() {
            let card = plan.add(
                BoardNodeKind::File { file: file.clone() },
                at((i % 4) as f64 * 360.0, 400.0 + (i / 4) as f64 * 240.0),
            );
            if i % 3 == 0 {
                plan.connect(&previous, &card);
            }
            previous = card;
        }

        let mut names = Vec::new();
        for (name, board) in [("All Tests", all), ("Release plan", plan)] {
            let file = format!("{BOARDS_FOLDER}/{name}{BOARD_SUFFIX}");
            self.write_file(&file, board.to_json().as_bytes())?;
            names.push(file);
        }
        Ok(names)
    }

    /// Saves the runs of the history; returns how many failed, how many were flaky and how
    /// many screenshots they hold.
    fn write_runs(&mut self, written: &[Written]) -> Result<(u32, u32, u32), SyntheticError> {
        let table = DataTable::parse(DATA).map_err(|e| invalid(format!("{e:?}")))?;
        let rows: Vec<DataRow> = (0..table.rows.len())
            .map(|row| DataRow {
                number: row as u32 + 1,
                label: table.label(row),
                hash: Some(row_hash(&table, row)),
            })
            .collect();
        let quarantine = ProjectSettings::load(self.root).unwrap_or_default();
        let window = u64::from(self.spec.days) * DAY_MS;
        let (mut failed, mut flaky, mut shots) = (0, 0, 0);
        let unstable = UNSTABLE.max(self.spec.failure_share + self.spec.flaky_share);
        let (fail_chance, flaky_chance) = (
            self.spec.failure_share / unstable,
            self.spec.flaky_share / unstable,
        );
        for n in 0..self.spec.reports as usize {
            let test = &written[n % written.len()];
            let started = self.spec.end_ms - self.rng.between(0, window - 1);
            let roll = self.rng.unit();
            let outcome = if !test.unstable {
                Outcome::Passed
            } else if roll < fail_chance {
                Outcome::Failed
            } else if roll < fail_chance + flaky_chance {
                Outcome::Flaky
            } else {
                Outcome::Passed
            };
            let started_at = rfc3339(UNIX_EPOCH + Duration::from_millis(started));
            let dir = create_run_dir(self.root, Path::new(&test.file), &started_at)
                .map_err(|e| unwritable(self.root, &e))?;
            let failure = Failure::new(if test.screen {
                "run.waitFor.timeout"
            } else {
                "run.file.timeout"
            })
            .with("block", &test.fails_at)
            .with("seconds", "5");
            let failure = if test.screen {
                failure.with("target", READY_IMAGE)
            } else {
                failure
                    .with("path", &test.failure_path)
                    .with("state", "exists")
                    .with("text", "")
            };
            let mut attempts = Vec::new();
            let row_list: Vec<Option<DataRow>> = if test.data {
                rows.iter().cloned().map(Some).collect()
            } else {
                vec![None]
            };
            let last_row = row_list.len() - 1;
            for (i, row) in row_list.into_iter().enumerate() {
                let tries: &[bool] = match (i == last_row, outcome) {
                    (true, Outcome::Failed) => &[false, false],
                    (true, Outcome::Flaky) => &[false, true],
                    _ => &[true],
                };
                for (k, passed) in tries.iter().enumerate() {
                    let fail = (!passed).then_some(&failure);
                    let attempt =
                        self.attempt(&dir, k as u32 + 1, row.clone(), test, &started_at, fail)?;
                    shots += u32::from(fail.is_some());
                    attempts.push(attempt);
                }
            }
            let verdict = if outcome == Outcome::Failed {
                failed += 1;
                Verdict::Failed
            } else {
                flaky += u32::from(outcome == Outcome::Flaky);
                Verdict::Passed
            };
            let quarantined = quarantine.quarantine.iter().find(|q| q.test == test.file);
            let report = RunReport {
                format: REPORT_FORMAT,
                test: test.name.clone(),
                file: test.file.clone(),
                app: APP.to_owned(),
                started_at,
                duration_ms: attempts.iter().map(|a| a.duration_ms).sum::<u64>() + 180,
                verdict,
                flaky: outcome == Outcome::Flaky,
                baselines_created: Vec::new(),
                nothing_checked: false,
                requested_by: None,
                schedule: self.rng.chance(0.3).then(|| NIGHTLY.to_owned()),
                ci: None,
                environment: Some(ENVIRONMENT.to_owned()),
                video: None,
                file_hash: Some(test.hash.clone()),
                quarantined: quarantined.is_some(),
                quarantine_reason: quarantined.and_then(|q| q.reason.clone()),
                virtual_machine: Some(false),
                display_scale: test.screen.then_some(2.0),
                blocked: None,
                added_as_dependency: false,
                packages: Vec::new(),
                attempts,
            };
            let mut text =
                serde_json::to_string_pretty(&report).map_err(|e| unwritable(&dir, &e))?;
            text.push('\n');
            write(&dir.join("report.json"), text.as_bytes())?;
            write(
                &dir.join("report.html"),
                html(&report, &dir, None).as_bytes(),
            )?;
            write(
                &dir.join("junit.xml"),
                junit_xml(std::slice::from_ref(&report)).as_bytes(),
            )?;
        }
        Ok((failed, flaky, shots))
    }

    /// One attempt of `test` that stops at its last block with `failure`, or passes.
    fn attempt(
        &mut self,
        dir: &Path,
        number: u32,
        row: Option<DataRow>,
        test: &Written,
        started_at: &str,
        failure: Option<&Failure>,
    ) -> Result<Attempt, SyntheticError> {
        let mut steps = Vec::with_capacity(test.steps.len());
        for (i, planned) in test.steps.iter().enumerate() {
            let seq = i as u32 + 1;
            let screen = !matches!(
                planned.kind,
                "dateTime" | "pause" | "if" | "repeat" | "useData" | "runTest" | "checkFile"
            );
            let duration_ms = if screen {
                self.rng.between(40, 900)
            } else {
                self.rng.between(0, 4)
            };
            let fails = failure.is_some() && i + 1 == test.steps.len();
            let after = if fails {
                let shot = format!("shots/{number}-{seq:03}-{}-after.png", planned.block);
                write(&dir.join(&shot), &self.shot)?;
                Some(shot)
            } else {
                None
            };
            steps.push(Step {
                seq,
                test: planned.test.clone(),
                block: planned.block.clone(),
                block_type: planned.kind.to_owned(),
                status: if fails {
                    StepStatus::Failed
                } else {
                    StepStatus::Passed
                },
                duration_ms,
                before: None,
                after,
                expected: None,
                diff: None,
                reference: None,
                similarity: None,
                vision: None,
                sent: None,
                sent_masked: false,
                found: Vec::new(),
                branch: planned.branch.then(|| "then".to_owned()),
                branches: planned.branch.then_some(2),
                branch_condition: None,
                outputs: planned
                    .saves
                    .iter()
                    .map(|(name, date)| {
                        let value = if *date {
                            &started_at[..10]
                        } else {
                            &started_at[11..16]
                        };
                        (name.clone(), value.to_owned())
                    })
                    .collect(),
                failure: failure.filter(|_| fails).cloned(),
                video_at_ms: None,
            });
        }
        Ok(Attempt {
            number,
            verdict: if failure.is_some() {
                Verdict::Failed
            } else {
                Verdict::Passed
            },
            duration_ms: steps.iter().map(|s| s.duration_ms).sum(),
            failure: failure.cloned(),
            row,
            steps,
        })
    }
}

fn loop_kind(piece: Piece) -> &'static str {
    if piece == Piece::Loop {
        "repeat"
    } else {
        "useData"
    }
}

/// `add-coupon-again` as `Add coupon again`.
fn title(stem: &str) -> String {
    let words = stem.replace('-', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().chain(chars).collect::<String>()
    })
}

/// The project file `path` names, from the folder `folder` of the project.
fn join(folder: &str, path: &str) -> String {
    let mut parts: Vec<&str> = folder.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// The fingerprint a run gives row `row` of `table`, as the runner counts it.
fn row_hash(table: &DataTable, row: usize) -> String {
    let text: String = table
        .columns
        .iter()
        .zip(&table.rows[row])
        .map(|(column, value)| format!("{column}\u{1f}{value}\u{1e}"))
        .collect();
    content_hash(text.as_bytes())[..16].to_owned()
}

fn picture(width: u32, height: u32, [r, g, b]: [u8; 3]) -> Result<Vec<u8>, SyntheticError> {
    let rgba = [r, g, b, 255].repeat((width * height) as usize);
    jevision_vision::image::Image {
        width,
        height,
        rgba,
    }
    .encode()
    .map_err(invalid)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), SyntheticError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| unwritable(parent, &e))?;
    }
    fs::write(path, bytes).map_err(|e| unwritable(path, &e))
}

/// Files under `dir` and their bytes.
fn tally(dir: &Path) -> (u64, u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0);
    };
    entries
        .flatten()
        .fold((0, 0), |(files, bytes), entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                let (f, b) = tally(&entry.path());
                (files + f, bytes + b)
            }
            Ok(_) => (files + 1, bytes + entry.metadata().map_or(0, |m| m.len())),
            Err(_) => (files, bytes),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevision_core::{RunMode, validate_data, validate_environment_values, validate_in};

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-synthetic-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn small() -> SyntheticSpec {
        SyntheticSpec {
            tests: 120,
            min_blocks: 6,
            max_blocks: 60,
            folders: 30,
            depth: 3,
            reports: 360,
            ..SyntheticSpec::default()
        }
    }

    /// Every file under `dir` with its bytes, the folder's own path taken out.
    fn contents(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    let text = fs::read(&path).unwrap();
                    let root_text = root.display().to_string();
                    let text = String::from_utf8(text.clone())
                        .map_or(text, |t| t.replace(&root_text, "<root>").into_bytes());
                    let name = path.strip_prefix(root).unwrap().display().to_string();
                    out.insert(name, text);
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    #[test]
    fn the_same_seed_writes_the_same_bytes_and_another_seed_does_not() {
        let base = scratch("same");
        let spec = SyntheticSpec {
            tests: 60,
            folders: 30,
            reports: 150,
            ..small()
        };
        let (a, b, c) = (
            base.join("a/project"),
            base.join("b/project"),
            base.join("c/project"),
        );
        let first = write_synthetic_project(a.display().to_string(), spec.clone()).unwrap();
        let second = write_synthetic_project(b.display().to_string(), spec.clone()).unwrap();
        let other = SyntheticSpec { seed: 2, ..spec };
        write_synthetic_project(c.display().to_string(), other).unwrap();
        assert_eq!(first, second);
        assert_eq!(contents(&a), contents(&b));
        assert_ne!(contents(&a), contents(&c));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn every_test_is_valid_and_runs_without_a_screen() {
        let root = scratch("valid");
        let summary = write_synthetic_project(root.display().to_string(), small()).unwrap();
        let mut problems = Vec::new();
        let custom: Vec<String> = (0..small().custom_blocks as usize)
            .map(|n| format!("blocks/{}{BLOCK_SUFFIX}", BLOCK_NAMES[n]))
            .collect();
        for file in summary.tests.iter().chain(&custom) {
            let text = fs::read_to_string(root.join(file)).unwrap();
            let flow = jevision_core::parse(&text).unwrap();
            let folder = root.join(file).parent().unwrap().to_path_buf();
            let issues = validate_in(&flow, &folder)
                .into_iter()
                .chain(validate_environment_values(&flow, &folder))
                .chain(validate_data(&flow, &folder))
                .chain(jevision_core::dependency_issues(&flow, &root, file))
                // Blocks rarely check the screen themselves; the tests that run them do.
                .filter(|i| !(custom.contains(file) && i.code == "flow.noCheck"));
            problems.extend(issues.map(|i| format!("{file}: {} {:?}", i.code, i.params)));
        }
        assert!(problems.is_empty(), "{problems:#?}");
        let batch = jevision_core::Batch::new(&root, &summary.tests, RunMode::Sequential);
        let screenless: Vec<&str> = batch
            .tests()
            .iter()
            .filter(|t| !t.uses_screen)
            .map(|t| t.file.as_str())
            .collect();
        assert_eq!(screenless.len(), summary.tests.len());
        assert_eq!(summary.screenless, summary.tests);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_summary_tells_what_is_on_disk() {
        let root = scratch("counts");
        let spec = SyntheticSpec {
            screenless_share: 0.5,
            ..small()
        };
        let summary = write_synthetic_project(root.display().to_string(), spec.clone()).unwrap();
        let on_disk = contents(&root);
        let tests: Vec<&String> = on_disk
            .keys()
            .filter(|f| f.ends_with(TEST_SUFFIX))
            .collect();
        assert_eq!(tests.len(), spec.tests as usize);
        assert_eq!(summary.tests.len(), spec.tests as usize);
        assert_eq!(summary.files, on_disk.len() as u64);
        assert!(summary.folders > spec.folders && summary.depth == spec.depth);
        assert_eq!(summary.largest_blocks, spec.max_blocks);
        assert_eq!(summary.custom_blocks, spec.custom_blocks);
        assert!(summary.subprocess_tests > 0);
        assert!(!summary.screenless.is_empty() && summary.screenless.len() < summary.tests.len());
        let batch = jevision_core::Batch::new(&root, &summary.tests, RunMode::Sequential);
        let needs_no_screen = batch.tests().iter().filter(|t| !t.uses_screen).count();
        assert_eq!(needs_no_screen, summary.screenless.len());

        let runs = crate::list_runs(root.display().to_string()).unwrap();
        assert_eq!(runs.len(), spec.reports as usize);
        let failed = runs
            .iter()
            .filter(|r| r.report.verdict == jevision_runner::Verdict::Failed)
            .count();
        let flaky = runs.iter().filter(|r| r.report.flaky).count();
        assert_eq!(failed, summary.failed_reports as usize);
        assert_eq!(flaky, summary.flaky_reports as usize);
        assert!(failed > 0 && flaky > 0);
        let shots = on_disk.keys().filter(|f| f.ends_with("-after.png")).count();
        assert_eq!(shots, summary.screenshots as usize);
        for run in &runs {
            for name in ["report.html", "junit.xml"] {
                assert!(
                    Path::new(&run.dir).join(name).is_file(),
                    "{} {name}",
                    run.dir
                );
            }
        }
        for board in &summary.boards {
            let text = fs::read_to_string(root.join(board)).unwrap();
            assert!(Board::parse(&text).is_ok(), "{board}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_spec_that_cannot_be_made_is_refused() {
        let root = scratch("refused");
        for spec in [
            SyntheticSpec {
                tests: 10,
                ..SyntheticSpec::default()
            },
            SyntheticSpec {
                depth: 50,
                ..SyntheticSpec::default()
            },
            SyntheticSpec {
                min_blocks: 300,
                ..SyntheticSpec::default()
            },
            SyntheticSpec {
                failure_share: 0.8,
                flaky_share: 0.5,
                ..SyntheticSpec::default()
            },
        ] {
            let result = write_synthetic_project(root.display().to_string(), spec);
            assert!(matches!(result, Err(SyntheticError::Invalid { .. })));
        }
        assert!(!root.exists());
    }
}
