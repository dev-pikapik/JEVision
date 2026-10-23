//! Shared by the heavy measurements among this crate's tests: the made-up project they run on,
//! a timer that prints one tab-separated line per measurement, the memory the process holds and
//! a run of a test that needs no screen.
#![allow(dead_code)]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Instant;

use jevision_core::{
    Modifiers, MouseButton, Point, Rect, ScrollDirection, ScrollUnit, WindowPosition, WindowSize,
};
use jevision_ffi::{
    Hands, HandsError, Openable, RunEvent, RunListener, RunOptions, RunOutcome, SecretSource,
    SyntheticSpec, SyntheticSummary, TestRun, write_synthetic_project,
};
use jevision_runner::Screenshot;

/// Environment the made-up project's tests run in.
pub const ENVIRONMENT: &str = "staging";

/// Resident memory of this process in KiB, as `ps` tells it.
pub fn rss_kb() -> u64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("ps prints a number")
}

/// Prints `BENCH\t<name>\t<ms>`.
pub fn report(name: &str, ms: f64) {
    println!("BENCH\t{name}\t{ms:.3}");
}

/// How long something took, in milliseconds: by the clock on the wall, and of processor time
/// the process spent, which other programs keeping the computer busy hardly change.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub wall: f64,
    pub cpu: f64,
}

impl Timing {
    /// Both times divided by `n`, for one of `n` things done.
    pub fn per(self, n: usize) -> Self {
        Self {
            wall: self.wall / n as f64,
            cpu: self.cpu / n as f64,
        }
    }
}

/// Prints `BENCH\t<name>\t<wall ms>\t<cpu ms>`.
pub fn report_timing(name: &str, timing: Timing) {
    println!("BENCH\t{name}\t{:.3}\t{:.3}", timing.wall, timing.cpu);
}

/// Processor time, user and system, all threads of this process have spent, in milliseconds.
pub fn cpu_ms() -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` fills the struct it is given; `RUSAGE_SELF` is always valid.
    let usage = unsafe {
        assert_eq!(libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()), 0);
        usage.assume_init()
    };
    let ms = |t: libc::timeval| t.tv_sec as f64 * 1000.0 + f64::from(t.tv_usec) / 1000.0;
    ms(usage.ru_utime) + ms(usage.ru_stime)
}

/// Times `work` three times, each on a fresh `setup`, prints the fastest by the wall clock and
/// by processor time and returns them; setting up isn't timed.
pub fn bench_with<S, T>(
    name: &str,
    mut setup: impl FnMut() -> S,
    mut work: impl FnMut(S) -> T,
) -> Timing {
    let best = (0..3)
        .map(|_| {
            let input = setup();
            let cpu = cpu_ms();
            let clock = Instant::now();
            std::hint::black_box(work(input));
            Timing {
                wall: clock.elapsed().as_secs_f64() * 1000.0,
                cpu: cpu_ms() - cpu,
            }
        })
        .fold(
            Timing {
                wall: f64::INFINITY,
                cpu: f64::INFINITY,
            },
            |best, t| Timing {
                wall: best.wall.min(t.wall),
                cpu: best.cpu.min(t.cpu),
            },
        );
    report_timing(name, best);
    best
}

/// [`bench_with`] without setting up.
pub fn bench<T>(name: &str, mut work: impl FnMut() -> T) -> Timing {
    bench_with(name, || (), |()| work())
}

/// The project of [`SyntheticSpec::default`], written once per process into the same
/// temporary folder, which a later process empties first. Prints how long writing it took.
pub fn project() -> &'static (PathBuf, SyntheticSummary) {
    static PROJECT: OnceLock<(PathBuf, SyntheticSummary)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = std::env::temp_dir()
            .join("jevision-scale-bench")
            .join("Scale");
        let _ = fs::remove_dir_all(&root);
        let clock = Instant::now();
        let summary = write_synthetic_project(root.display().to_string(), SyntheticSpec::default())
            .expect("the project is written");
        report("generate.project", clock.elapsed().as_secs_f64() * 1000.0);
        (root, summary)
    })
}

/// The folders of saved runs of `project` now.
pub fn saved_runs(project: &Path) -> HashSet<OsString> {
    fs::read_dir(project.join(".jevision/runs"))
        .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default()
}

/// Deletes the runs of `project` saved since `before`, so the history stays as it was written.
pub fn forget_runs_since(project: &Path, before: &HashSet<OsString>) {
    for name in saved_runs(project).difference(before) {
        fs::remove_dir_all(project.join(".jevision/runs").join(name)).expect("the run is deleted");
    }
}

/// Runs the test `file` of `project` as the app does and waits for it to end.
pub fn run_test(project: &Path, file: &str) -> RunOutcome {
    let (sender, receiver) = mpsc::channel();
    let options = RunOptions {
        environment: Some(ENVIRONMENT.to_owned()),
        ..RunOptions::default()
    };
    TestRun::new().start(
        project.display().to_string(),
        file.to_owned(),
        options,
        Arc::new(NoScreen),
        Arc::new(NoSecrets),
        Arc::new(Ending(sender)),
    );
    receiver.recv().expect("the run ends")
}

/// Hands of a computer without a screen: what needs one fails, the rest does nothing.
struct NoScreen;

fn no_window(app: String) -> HandsError {
    HandsError::NoWindow { app }
}

impl Hands for NoScreen {
    fn sandboxed(&self, _: String) -> Result<bool, HandsError> {
        Ok(false)
    }
    fn elements(&self, app: String) -> Result<Vec<jevision_core::ElementNode>, HandsError> {
        Err(HandsError::NoWindow { app })
    }
    fn open_app(
        &self,
        _: String,
        _: jevision_runner::AppStart,
        _: Vec<String>,
        _: std::collections::HashMap<String, String>,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn arrange_window(
        &self,
        app: String,
        _: Option<WindowSize>,
        _: Option<WindowPosition>,
    ) -> Result<(), HandsError> {
        Err(no_window(app))
    }
    fn open_items(&self, _: Vec<Openable>, _: Option<String>, _: bool) -> Result<(), HandsError> {
        Ok(())
    }
    fn close_items(&self, _: Vec<Openable>, _: Option<String>) -> Result<(), HandsError> {
        Ok(())
    }
    fn activate(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn quit(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn capture(&self, app: String) -> Result<Screenshot, HandsError> {
        Err(no_window(app))
    }
    fn locate(&self, app: String, _: Vec<u8>, _: f64) -> Result<Option<Point>, HandsError> {
        Err(no_window(app))
    }
    fn click(
        &self,
        app: String,
        _: Point,
        _: MouseButton,
        _: bool,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Err(no_window(app))
    }
    fn hover(&self, app: String, _: Point, _: Modifiers) -> Result<(), HandsError> {
        Err(no_window(app))
    }
    fn drag(
        &self,
        app: String,
        _: Vec<Point>,
        _: MouseButton,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Err(no_window(app))
    }
    fn scroll(
        &self,
        app: String,
        _: Point,
        _: ScrollDirection,
        _: u32,
        _: ScrollUnit,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Err(no_window(app))
    }
    fn type_text(&self, _: String) -> Result<(), HandsError> {
        Err(HandsError::ScreenLocked)
    }
    fn press_keys(&self, _: String) -> Result<(), HandsError> {
        Err(HandsError::ScreenLocked)
    }
    fn clipboard(&self) -> Result<String, HandsError> {
        Ok(String::new())
    }
    fn set_clipboard(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn clipboard_image(&self) -> Result<Option<Vec<u8>>, HandsError> {
        Ok(None)
    }
    fn clipboard_files(&self) -> Result<Vec<String>, HandsError> {
        Ok(Vec::new())
    }
    fn set_clipboard_image(&self, _: Vec<u8>) -> Result<(), HandsError> {
        Ok(())
    }
    fn user_input_detected(&self) -> bool {
        false
    }
    fn start_video(&self, _: String, _: String, _: Vec<Rect>) -> Result<(), HandsError> {
        Ok(())
    }
    fn stop_video(&self) -> Result<(), HandsError> {
        Ok(())
    }
}

struct NoSecrets;

impl SecretSource for NoSecrets {
    fn secret(&self, _: String, _: Option<String>) -> Option<String> {
        None
    }
}

/// Hands the outcome of a run back to the thread waiting for it.
struct Ending(mpsc::Sender<RunOutcome>);

impl RunListener for Ending {
    fn event(&self, _: RunEvent) {}
    fn finished(&self, outcome: RunOutcome) {
        let _ = self.0.send(outcome);
    }
}
