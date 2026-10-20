use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use jevision_runner::{
    DataRow, Event, Hands, HandsError, Judgement, Limit, Located, MaskedImage, Modifiers,
    MouseButton, Openable, PauseReason, Point, Price, Question, RetryVerdict, Run, RunControl,
    RunOptions, RunReport, Runner, Screenshot, ScrollDirection, ScrollUnit, Search, StepStatus,
    Usage, UsageBook, VIDEO_FILE, Verdict, VideoMode, VisionAnswer, VisionError, VisionJudge,
    describe, html, junit_xml, prune_videos,
};

const SECRET: &str = "s3cr3t-pa55";
const APP: &str = "com.example.App";

/// A screen with a fixed look and an optional window-sized look after the first click / quit.
#[derive(Default)]
struct Fake {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    screen: Vec<u8>,
    dark: bool,
    /// Password fields the window reports.
    secure: Vec<jevision_core::Rect>,
    /// Shown after the first click, hover, drag or scroll.
    after_click: Option<Vec<u8>>,
    after_quit: Option<Vec<u8>>,
    /// Shown from the second look at the screen on.
    after_capture: Option<Vec<u8>>,
    not_findable: bool,
    reject_typing: bool,
    user_input: bool,
    /// Recording a video fails, as without the Screen Recording permission.
    no_video: bool,
    video: Option<PathBuf>,
    /// Areas the video was asked to cover in every window.
    video_hide: Vec<jevision_core::Rect>,
    log: Vec<String>,
    typed: Vec<String>,
    clipboard: String,
    /// Put on the clipboard after the first look at it, as another application would.
    after_look: Option<String>,
}

impl Fake {
    fn new(screen: Vec<u8>) -> Self {
        Self {
            state: Mutex::new(State {
                screen,
                ..State::default()
            }),
        }
    }

    fn with(self, change: impl FnOnce(&mut State)) -> Self {
        change(&mut self.state.lock().unwrap());
        self
    }

    fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    fn typed(&self) -> Vec<String> {
        self.state.lock().unwrap().typed.clone()
    }

    fn clipboard_text(&self) -> String {
        self.state.lock().unwrap().clipboard.clone()
    }

    fn note(&self, entry: String) {
        self.state.lock().unwrap().log.push(entry);
    }

    /// Logs a pointer action; the screen then shows `after_click`, if set.
    fn pointer(&self, entry: String) {
        let mut s = self.state.lock().unwrap();
        s.log.push(entry);
        if let Some(screen) = s.after_click.take() {
            s.screen = screen;
        }
    }
}

impl Hands for Fake {
    fn open_app(&self, app: &str) -> Result<(), HandsError> {
        self.note(format!("open {app}"));
        Ok(())
    }
    fn open(&self, items: &[Openable], app: Option<&str>, _: bool) -> Result<(), HandsError> {
        self.note(format!("open {items:?} in {app:?}"));
        Ok(())
    }
    fn close(&self, items: &[Openable], app: Option<&str>) -> Result<(), HandsError> {
        self.note(format!("close {items:?} in {app:?}"));
        Ok(())
    }
    fn activate(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn quit(&self, app: &str) -> Result<(), HandsError> {
        let mut s = self.state.lock().unwrap();
        s.log.push(format!("quit {app}"));
        if let Some(screen) = s.after_quit.take() {
            s.screen = screen;
        }
        Ok(())
    }
    fn capture(&self, _: &str) -> Result<Screenshot, HandsError> {
        let mut s = self.state.lock().unwrap();
        let png = s.screen.clone();
        if let Some(next) = s.after_capture.take() {
            s.screen = next;
        }
        Ok(Screenshot {
            png,
            scale: 2.0,
            dark: s.dark,
            secure: s.secure.clone(),
        })
    }
    fn locate(&self, _: &str, template: &[u8], _: f64) -> Result<Option<Point>, HandsError> {
        assert!(
            png::Decoder::new(std::io::Cursor::new(template))
                .read_info()
                .is_ok()
        );
        let found = !self.state.lock().unwrap().not_findable;
        Ok(found.then_some(Point { x: 0.5, y: 0.25 }))
    }
    fn click(
        &self,
        _: &str,
        at: Point,
        button: MouseButton,
        _: bool,
        modifiers: Modifiers,
    ) -> Result<(), HandsError> {
        self.pointer(format!(
            "click {:?} {} {}{}",
            button,
            at.x,
            at.y,
            holding(modifiers)
        ));
        Ok(())
    }
    fn hover(&self, _: &str, at: Point) -> Result<(), HandsError> {
        self.pointer(format!("hover {} {}", at.x, at.y));
        Ok(())
    }
    fn drag(
        &self,
        _: &str,
        path: &[Point],
        button: MouseButton,
        modifiers: Modifiers,
    ) -> Result<(), HandsError> {
        let points: Vec<String> = path.iter().map(|p| format!("{} {}", p.x, p.y)).collect();
        let with = if button == MouseButton::Left {
            String::new()
        } else {
            format!(" with {button:?}")
        };
        self.pointer(format!(
            "drag {}{with}{}",
            points.join(", "),
            holding(modifiers)
        ));
        Ok(())
    }
    fn scroll(
        &self,
        _: &str,
        at: Point,
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
    ) -> Result<(), HandsError> {
        self.pointer(format!(
            "scroll {direction:?} {amount} {unit:?} at {} {}",
            at.x, at.y
        ));
        Ok(())
    }
    fn type_text(&self, text: &str) -> Result<(), HandsError> {
        let mut s = self.state.lock().unwrap();
        if s.reject_typing {
            return Err(HandsError::Failed(format!("the field rejected {text}")));
        }
        s.log.push("type".to_owned());
        s.typed.push(text.to_owned());
        Ok(())
    }
    fn press_keys(&self, keys: &str) -> Result<(), HandsError> {
        self.note(format!("keys {keys}"));
        Ok(())
    }
    fn clipboard(&self) -> Result<String, HandsError> {
        let mut s = self.state.lock().unwrap();
        let text = s.clipboard.clone();
        if let Some(next) = s.after_look.take() {
            s.clipboard = next;
        }
        Ok(text)
    }
    fn set_clipboard(&self, text: &str) -> Result<(), HandsError> {
        self.state.lock().unwrap().clipboard = text.to_owned();
        Ok(())
    }
    fn user_input_detected(&self) -> bool {
        std::mem::take(&mut self.state.lock().unwrap().user_input)
    }
    fn start_video(
        &self,
        app: &str,
        path: &Path,
        hide: &[jevision_core::Rect],
    ) -> Result<(), HandsError> {
        let mut s = self.state.lock().unwrap();
        if s.no_video {
            return Err(HandsError::PermissionDenied("screenRecording".to_owned()));
        }
        s.log.push(format!("video {app}"));
        s.video = Some(path.to_path_buf());
        s.video_hide = hide.to_vec();
        Ok(())
    }
    fn stop_video(&self) -> Result<(), HandsError> {
        let mut s = self.state.lock().unwrap();
        s.log.push("video end".to_owned());
        let path = s
            .video
            .take()
            .expect("stopping a video that was not started");
        std::fs::write(path, b"movie").unwrap();
        Ok(())
    }
}

/// ` holding cmd+shift` after a pointer action, or nothing when no key is held.
fn holding(modifiers: Modifiers) -> String {
    if modifiers.is_empty() {
        String::new()
    } else {
        format!(" holding {}", modifiers.names().join("+"))
    }
}

/// A PNG of `w`×`h` pixels coloured by `paint(x, y)`.
fn png(w: u32, h: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| paint(x, y))
        .collect();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    out
}

fn solid(rgb: [u8; 3]) -> Vec<u8> {
    png(40, 20, |_, _| rgb)
}

fn pixel(png_bytes: &[u8], x: u32, y: u32) -> [u8; 3] {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let channels = info.line_size / info.width as usize;
    let at = (y as usize * info.width as usize + x as usize) * channels;
    [buf[at], buf[at + 1], buf[at + 2]]
}

/// A project folder that removes itself.
struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-runner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) -> &Self {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
        self
    }

    fn test(&self, blocks: &str, start: &str, retries: u32) -> &Self {
        self.write(
            "t.flow.json",
            format!(
                r#"{{"format":1,"name":"T","app":"{APP}","retries":{retries},"start":"{start}","blocks":{{{blocks}}}}}"#
            ),
        )
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn options() -> RunOptions {
    RunOptions {
        poll_interval: Duration::from_millis(5),
        ..RunOptions::default()
    }
}

fn secrets() -> HashMap<String, String> {
    HashMap::from([("ACME_PASSWORD".to_owned(), SECRET.to_owned())])
}

/// Runs `t.flow.json` and returns the run with every event it sent.
fn run_with(
    project: &Project,
    hands: &Fake,
    control: RunControl,
    options: &RunOptions,
) -> (Run, Vec<Event>) {
    let events = Mutex::new(Vec::new());
    let sink = |e: &Event| events.lock().unwrap().push(e.clone());
    let secrets = secrets();
    let runner = Runner {
        hands,
        secrets: &secrets,
        events: &sink,
        control,
        vision: None,
        fallback: None,
        providers: &[],
        usage: None,
        locator: None,
    };
    let run = runner
        .run(&project.0, Path::new("t.flow.json"), options)
        .unwrap();
    (run, events.into_inner().unwrap())
}

fn run(project: &Project, hands: &Fake) -> Run {
    run_with(project, hands, RunControl::new(), &options()).0
}

fn statuses(report: &RunReport) -> Vec<(String, StepStatus)> {
    report
        .attempts
        .last()
        .unwrap()
        .steps
        .iter()
        .map(|s| (s.block.clone(), s.status))
        .collect()
}

fn failure_code(report: &RunReport) -> String {
    report
        .attempts
        .last()
        .unwrap()
        .failure
        .as_ref()
        .map(|f| f.code.clone())
        .unwrap_or_default()
}

/// Every text a run leaves behind.
fn outputs(run: &Run, events: &[Event]) -> String {
    let report = std::fs::read_to_string(run.dir.join("report.json")).unwrap();
    let events: Vec<String> = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    format!(
        "{report}\n{}\n{}\n{}",
        html(&run.report, &run.dir, run.report.video.as_deref()),
        junit_xml(std::slice::from_ref(&run.report)),
        events.join("\n")
    )
}

/// Checks that tags nest, attributes are quoted and text has no stray markup characters.
fn assert_well_formed_xml(xml: &str) {
    let body = xml
        .strip_prefix("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        .expect("XML declaration");
    let mut open: Vec<&str> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find('<') {
        let text = &rest[..start];
        assert!(!text.contains('>'), "stray > in {text:?}");
        for (i, _) in text.match_indices('&') {
            let entity = &text[i..];
            assert!(
                ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"]
                    .iter()
                    .any(|e| entity.starts_with(e)),
                "bad entity in {text:?}"
            );
        }
        let end = start + rest[start..].find('>').expect("unclosed tag");
        let tag = &rest[start + 1..end];
        if let Some(name) = tag.strip_prefix('/') {
            assert_eq!(open.pop(), Some(name), "mismatched </{name}>");
        } else {
            let name = tag.split([' ', '/']).next().unwrap();
            assert_eq!(
                tag.matches('"').count() % 2,
                0,
                "unbalanced quotes in <{tag}>"
            );
            assert!(!tag[name.len()..].contains('<'), "< inside <{tag}>");
            if !tag.ends_with('/') {
                open.push(name);
            }
        }
        rest = &rest[end + 1..];
    }
    assert!(rest.trim().is_empty());
    assert!(open.is_empty(), "unclosed {open:?}");
}

#[test]
fn sign_in_example_runs_end_to_end() {
    let p = Project::new();
    let example = include_str!("../../../examples/sign-in.flow.json");
    p.write("t.flow.json", example);
    let login = solid([200, 200, 200]);
    // The inbox; its top-right corner (a clock) and top-left corner (hidden) change per run.
    let inbox = |clock: u8| {
        png(40, 20, move |x, y| match (x, y) {
            (36.., 0..2) => [clock, 0, 0],
            (0..12, 0..2) if x % 2 == 0 => [0, clock, 0],
            (0..12, 0..2) => [clock, clock, clock],
            _ => [30, 90, 160],
        })
    };
    p.write("refs/email-field.png", solid([1, 1, 1]))
        .write("refs/password-field.png", solid([2, 2, 2]))
        .write("refs/sign-in-button.png", solid([3, 3, 3]))
        .write("refs/inbox.png", inbox(10))
        .write("refs/message-list.png", inbox(10));
    let hands = Fake::new(login).with(|s| s.after_click = Some(inbox(250)));

    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());

    assert_eq!(run.report.verdict, Verdict::Passed, "{:#?}", run.report);
    assert!(!run.report.flaky);
    assert_eq!(
        statuses(&run.report),
        ["open", "email", "password", "submit", "inbox", "messages"]
            .map(|b| (b.to_owned(), StepStatus::Passed))
    );
    assert_eq!(hands.typed(), ["demo@example.com", SECRET]);
    assert_eq!(hands.log()[0], "open com.example.AcmeMail");
    assert!(
        run.dir
            .starts_with(std::fs::canonicalize(&p.0).unwrap().join(".jevision/runs"))
    );
    let messages = &run.report.attempts[0].steps[5];
    assert_eq!(messages.similarity, Some(1.0));
    for path in [
        &messages.after,
        &messages.expected,
        &messages.diff,
        &run.report.attempts[0].steps[1].before,
    ] {
        assert!(run.dir.join(path.as_ref().unwrap()).is_file(), "{path:?}");
    }
    // The hidden corner is covered in the stored screenshot.
    let after = std::fs::read(run.dir.join(messages.after.as_ref().unwrap())).unwrap();
    assert_ne!(pixel(&after, 0, 0), [0, 250, 0]);

    let started = events.iter().find_map(|e| match e {
        Event::StepStarted {
            block, variables, ..
        } if block == "password" => Some(variables),
        _ => None,
    });
    assert_eq!(started.unwrap()["secret.ACME_PASSWORD"], "••••");
    assert!(matches!(events.first(), Some(Event::RunStarted { .. })));
    assert!(matches!(
        events.last(),
        Some(Event::RunFinished {
            verdict: Verdict::Passed,
            ..
        })
    ));

    let all = outputs(&run, &events);
    assert!(!all.contains(SECRET), "a secret leaked into the outputs");
    assert!(all.contains("data:image/png;base64,"));
    let saved: RunReport =
        serde_json::from_str(&std::fs::read_to_string(run.dir.join("report.json")).unwrap())
            .unwrap();
    assert_eq!(saved, run.report);
}

#[test]
fn wait_for_keeps_waiting_through_a_window_of_another_size() {
    let p = Project::new();
    p.write("refs/dialog.png", solid([0, 255, 0])).test(
        r#""wait":{"type":"waitFor","until":{"appears":{"image":"refs/dialog.png"}},"timeout":"2s"}"#,
        "wait",
        0,
    );
    // The first look still shows the bigger window; the dialog follows.
    let hands = Fake::new(png(80, 60, |_, _| [9, 9, 9]))
        .with(|s| s.after_capture = Some(solid([0, 255, 0])));
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);
}

#[test]
fn a_point_target_is_a_share_of_the_window() {
    let p = Project::new();
    p.test(
        r#""click":{"type":"click","target":{"point":{"x":0.5,"y":0.25}}}"#,
        "click",
        0,
    );
    // 40×20 pixels at 2 pixels per point: a window of 20×10 points.
    let hands = Fake::new(solid([9, 9, 9]));
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);
    assert_eq!(hands.log(), ["click Left 10 2.5"]);
}

#[test]
fn hover_moves_the_pointer_onto_the_target() {
    let p = Project::new();
    p.write("refs/help.png", solid([9, 9, 9])).test(
        r#""tip":{"type":"hover","target":{"image":"refs/help.png"},"waitForChange":true}"#,
        "tip",
        0,
    );
    // The tooltip changes the screen, so waiting for a change passes at once.
    let hands = Fake::new(solid([9, 9, 9])).with(|s| s.after_click = Some(solid([0, 0, 200])));
    let run = run(&p, &hands);
    assert_eq!(run.report.verdict, Verdict::Passed);
    assert_eq!(hands.log(), ["hover 0.5 0.25"]);
    let step = &run.report.attempts[0].steps[0];
    assert_eq!(step.block_type, "hover");
    assert!(step.before.is_some() && step.after.is_some());
}

#[test]
fn drag_presses_on_the_source_and_lets_go_on_the_destination() {
    let p = Project::new();
    p.write("refs/file.png", solid([9, 9, 9])).test(
        r#""open":{"type":"pressKeys","keys":"a","next":"move"},
           "move":{"type":"drag","from":{"image":"refs/file.png"},"to":{"point":{"x":0.5,"y":0.85}},"next":"after"},
           "after":{"type":"pressKeys","keys":"b"}"#,
        "open",
        0,
    );
    // A window of 20×10 points: from the found picture at (0.5, 0.25) down to (10, 8.5).
    let hands = Fake::new(solid([9, 9, 9]));
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);
    let log = hands.log();
    assert_eq!(
        log,
        [
            "keys a",
            "drag 0.5 0.25, 2.875 2.3125, 5.25 4.375, 7.625 6.4375, 10 8.5",
            "keys b"
        ]
    );
}

#[test]
fn a_long_drag_moves_in_small_even_steps() {
    let p = Project::new();
    p.test(
        r#""move":{"type":"drag","from":{"point":{"x":0,"y":0}},"to":{"point":{"x":1,"y":0}}}"#,
        "move",
        0,
    );
    // 1000 points wide: the pointer travels in steps of at most 12 points, 60 at most.
    let hands = Fake::new(png(2000, 4, |_, _| [9, 9, 9]));
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);
    let log = hands.log();
    let points: Vec<f64> = log[0]
        .strip_prefix("drag ")
        .unwrap()
        .split(", ")
        .map(|p| p.split(' ').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(points.len(), 61);
    assert_eq!((points[0], points[60]), (0.0, 1000.0));
    assert!(
        points
            .windows(2)
            .all(|w| (w[1] - w[0] - 1000.0 / 60.0).abs() < 1e-9)
    );
}

#[test]
fn scroll_turns_the_wheel_over_the_middle_or_the_target() {
    let p = Project::new();
    p.write("refs/list.png", solid([9, 9, 9])).test(
        r#""down":{"type":"scroll","direction":"down","amount":5,"next":"left"},
           "left":{"type":"scroll","direction":"left","amount":240,"unit":"pixels","at":{"image":"refs/list.png"}}"#,
        "down",
        0,
    );
    let hands = Fake::new(solid([9, 9, 9]));
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);
    assert_eq!(
        hands.log(),
        [
            "scroll Down 5 Lines at 10 5",
            "scroll Left 240 Pixels at 0.5 0.25"
        ]
    );
}

#[test]
fn a_scroll_target_must_be_on_screen() {
    let p = Project::new();
    p.write("refs/list.png", solid([9, 9, 9])).test(
        r#""s":{"type":"scroll","direction":"up","amount":1,"at":{"image":"refs/list.png"}}"#,
        "s",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.not_findable = true);
    let report = run(&p, &hands).report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.target.notFound");
    assert!(hands.log().is_empty());
}

#[test]
fn wait_for_times_out_with_localizable_reason() {
    let p = Project::new();
    p.write("refs/ok.png", solid([0, 255, 0])).test(
        r#""open":{"type":"openApp","next":"wait"},
           "wait":{"type":"waitFor","until":{"appears":{"image":"refs/ok.png"}},"timeout":"300ms"}"#,
        "open",
        0,
    );
    let hands = Fake::new(solid([255, 0, 0]));
    let clock = Instant::now();
    let report = run(&p, &hands).report;
    assert!(clock.elapsed() >= Duration::from_millis(300));
    assert_eq!(report.verdict, Verdict::Failed);
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.waitFor.timeout");
    assert_eq!(
        [
            failure.param("block"),
            failure.param("target"),
            failure.param("seconds")
        ],
        ["wait", "refs/ok.png", "0.3"]
    );
    let step = &report.attempts[0].steps[1];
    assert_eq!(
        (step.status, step.failure.as_ref()),
        (StepStatus::Failed, Some(failure))
    );
    assert!(step.after.is_some());
}

#[test]
fn a_run_that_never_looks_at_the_screen_says_nothing_was_checked() {
    let p = Project::new();
    p.test(
        r#""keys":{"type":"pressKeys","keys":"cmd+n","next":"settle"},
           "settle":{"type":"waitFor","until":"screenStable","timeout":"1s"}"#,
        "keys",
        0,
    );
    let hands = Fake::new(solid([1, 2, 3]));
    let unchecked = run(&p, &hands);
    assert_eq!(unchecked.report.verdict, Verdict::Passed);
    assert!(unchecked.report.nothing_checked);
    let xml = junit_xml(std::slice::from_ref(&unchecked.report));
    assert!(xml.contains("skipped=\"1\"") && xml.contains("nothing was checked"));
    assert!(html(&unchecked.report, &unchecked.dir, None).contains("Nothing checked"));

    // The wait passes and saw what it waited for: the run checked the screen.
    p.write("refs/screen.png", solid([1, 2, 3]));
    p.test(
        r#""keys":{"type":"pressKeys","keys":"cmd+n","next":"wait"},
           "wait":{"type":"waitFor","until":{"appears":{"image":"refs/screen.png"}},"timeout":"1s"}"#,
        "keys",
        0,
    );
    let checked = run(&p, &hands).report;
    assert_eq!(checked.verdict, Verdict::Passed);
    assert!(!checked.nothing_checked);
    assert!(!junit_xml(&[checked]).contains("skipped=\"1\""));

    // A failed run is a failure, whatever it checked.
    p.test(
        r#""missing":{"type":"openFile","files":["nope.txt"]}"#,
        "missing",
        0,
    );
    let failed = run(&p, &hands).report;
    assert_eq!(failed.verdict, Verdict::Failed);
    assert!(!failed.nothing_checked);
}

#[test]
fn check_creates_baseline_then_passes_then_fails() {
    let p = Project::new();
    p.test(
        r#""check":{"type":"check","reference":{"image":"refs/home.png","compare":"similar","threshold":0.9,
           "hide":[{"x":0,"y":0,"width":0.25,"height":0.5}]}}"#,
        "check",
        0,
    );
    // Private content in the top-left corner, which the reference hides.
    let screen = png(40, 20, |x, y| match (x < 10 && y < 10, (x + y) % 2) {
        (true, 0) => [255, 255, 0],
        (true, _) => [0, 0, 0],
        _ => [10, 20, 30],
    });
    let hands = Fake::new(screen);

    let first = run(&p, &hands).report;
    assert_eq!(first.verdict, Verdict::Passed);
    assert_eq!(
        statuses(&first),
        [("check".to_owned(), StepStatus::BaselineCreated)]
    );
    assert_eq!(first.baselines_created, ["refs/home.png"]);
    let created = &first.attempts[0].steps[0];
    assert_eq!(created.reference.as_deref(), Some("refs/home.png"));
    let baseline = std::fs::read(p.0.join("refs/home.png")).unwrap();
    assert_ne!(
        pixel(&baseline, 0, 0),
        [255, 255, 0],
        "hidden area is covered in the baseline"
    );

    let second = run(&p, &hands).report;
    assert_eq!(
        statuses(&second),
        [("check".to_owned(), StepStatus::Passed)]
    );
    assert!(second.baselines_created.is_empty());

    // Change 60 % of the window, outside the hidden area.
    hands.state.lock().unwrap().screen =
        png(
            40,
            20,
            |x, _| if x < 16 { [10, 20, 30] } else { [200, 0, 0] },
        );
    let third = run(&p, &hands);
    let failure = third.report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.check.mismatch");
    assert_eq!(failure.param("threshold"), "90.0%");
    let step = &third.report.attempts[0].steps[0];
    assert!(step.similarity.unwrap() < 0.9);
    assert_eq!(step.reference.as_deref(), Some("refs/home.png"));
    let diff = std::fs::read(third.dir.join(step.diff.as_ref().unwrap())).unwrap();
    assert_eq!(pixel(&diff, 39, 19), [230, 20, 60], "differences are red");

    assert_eq!(third.report.display_scale, Some(2.0));
    assert_eq!(
        third.report.virtual_machine.is_some(),
        cfg!(target_os = "macos")
    );

    let history = Run::list(&p.0).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].report, third.report, "newest first");
    assert_eq!(Run::load(&third.dir).unwrap().report, third.report);
}

#[test]
fn ignore_masks_area_and_dark_references() {
    let p = Project::new();
    let reference = solid([50, 50, 50]);
    p.write("refs/a.png", &reference).test(
        r#""check":{"type":"check","reference":{"image":"refs/a.png","compare":"exact",
           "ignore":[{"x":0.5,"y":0,"width":0.5,"height":1}]}}"#,
        "check",
        0,
    );
    let right_half_differs = png(80, 40, |x, _| {
        if x < 40 {
            [50, 50, 50]
        } else {
            [255, 255, 255]
        }
    });
    let hands = Fake::new(right_half_differs);
    // A Retina screenshot (80×40) against a 1× reference (40×20), differing only where ignored.
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);

    hands.state.lock().unwrap().screen =
        png(80, 40, |x, _| if x < 2 { [0, 0, 0] } else { [50, 50, 50] });
    // The failure tells that the reference comes from a display of another density.
    let ran = run(&p, &hands);
    let report = &ran.report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.check.mismatchDensity");
    assert_eq!(
        (failure.param("expected"), failure.param("actual")),
        ("40×20", "80×40")
    );
    assert_eq!(report.display_scale, Some(2.0));
    assert!(describe(failure).contains("different pixel density"));
    assert!(html(report, &ran.dir, None).contains("Retina display (2×)</dd>"));

    hands.state.lock().unwrap().dark = true;
    let dark = run(&p, &hands).report;
    assert_eq!(dark.baselines_created, ["refs/a.dark.png"]);
    assert_eq!(
        statuses(&dark),
        [("check".to_owned(), StepStatus::BaselineCreated)]
    );
}

#[test]
fn use_data_types_each_row() {
    let p = Project::new();
    p.write(
        "data/users.csv",
        "email,name\na@x.io,Ann\nb@x.io,\"Bo, Jr\"\n",
    )
    .test(
        r#""rows":{"type":"useData","file":"data/users.csv","body":"type","next":"done"},
           "type":{"type":"type","text":"${data.name} <${data.email}>","next":"rows"},
           "done":{"type":"pressKeys","keys":"return"}"#,
        "rows",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());
    assert_eq!(run.report.verdict, Verdict::Passed);
    assert_eq!(hands.typed(), ["Ann <a@x.io>", "Bo, Jr <b@x.io>"]);
    assert_eq!(hands.log().last().unwrap(), "keys return");
    let vars: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::StepStarted {
                block, variables, ..
            } if block == "type" => Some((
                variables["data.email"].clone(),
                variables["useData.rows"].clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        vars,
        [
            ("a@x.io".into(), "row 1 of 2".into()),
            ("b@x.io".into(), "row 2 of 2".into())
        ]
    );
    // Each row is a run of the whole test of its own.
    let rows: Vec<_> = run
        .report
        .attempts
        .iter()
        .map(|a| (a.number, a.row.as_ref().map(DataRow::title)))
        .collect();
    assert_eq!(
        rows,
        [
            (1, Some("Row 1: Ann".into())),
            (1, Some("Row 2: Bo, Jr".into()))
        ]
    );
    assert_eq!(
        run.report.rows_summary().as_deref(),
        Some("2 of 2 rows passed")
    );
    assert!(events.iter().any(|e| matches!(e,
        Event::RowStarted { row: 2, rows: 2, label } if label == "Bo, Jr")));
}

#[test]
fn data_rows_run_one_by_one_with_their_own_retries() {
    let p = Project::new();
    p.write("data/users.csv", "name,expected\nAnn,ok\nBo,bad\nCy,ok\n")
        .write("refs/ok.png", solid([0, 0, 0]))
        .test(
            // The row's values are there before and after the loop too.
            r#""hello":{"type":"type","text":"hi ${data.name}","next":"rows"},
               "rows":{"type":"useData","file":"data/users.csv","body":"type","next":"check"},
               "type":{"type":"type","text":"${data.name}","next":"rows"},
               "check":{"type":"openFile","files":["refs/${data.expected}.png"]}"#,
            "hello",
            1,
        );
    let hands = Fake::new(solid([0, 0, 0]));
    let (run, _) = run_with(&p, &hands, RunControl::new(), &options());
    let report = run.report;
    assert_eq!(report.verdict, Verdict::Failed);
    assert!(!report.flaky);
    let attempts: Vec<_> = report
        .attempts
        .iter()
        .map(|a| (a.row.as_ref().unwrap().number, a.number, a.verdict))
        .collect();
    assert_eq!(
        attempts,
        [
            (1, 1, Verdict::Passed),
            (2, 1, Verdict::Failed),
            (2, 2, Verdict::Failed),
            (3, 1, Verdict::Passed)
        ]
    );
    assert_eq!(report.rows_summary().as_deref(), Some("2 of 3 rows passed"));
    assert_eq!(&hands.typed()[..2], ["hi Ann", "Ann"]);
    let cases = report.split_rows();
    assert_eq!(cases[1].test, "T [Row 2: Bo]");
    assert_eq!(cases[1].attempts.len(), 2);
    assert_eq!(cases[1].verdict, Verdict::Failed);
    let junit = jevision_runner::junit_xml(std::slice::from_ref(&report));
    assert!(junit.contains("tests=\"3\""), "{junit}");
    assert!(junit.contains("name=\"T [Row 2: Bo]\""), "{junit}");
    let page = html(&report, &run.dir, None);
    assert!(page.contains("2 of 3 rows passed."), "{page}");
    assert!(page.contains("Row 2: Bo · Attempt 2"), "{page}");

    // A range of rows, and a range past the end stops at the last row.
    let options = RunOptions {
        rows: Some(1..=7),
        retries: Some(0),
        ..options()
    };
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run_with(&p, &hands, RunControl::new(), &options).0.report;
    assert_eq!(report.rows_summary().as_deref(), Some("1 of 2 rows passed"));
    assert_eq!(hands.typed(), ["hi Bo", "Bo", "hi Cy", "Cy"]);
}

#[test]
fn a_broken_or_empty_table_stops_the_run_before_it_starts() {
    let p = Project::new();
    p.test(
        r#""rows":{"type":"useData","file":"data/users.csv","body":"type"},
           "type":{"type":"type","text":"${data.name}","next":"rows"}"#,
        "rows",
        0,
    );
    for (csv, code) in [
        ("name,name\nAnn,Bo\n", "data.duplicateColumn"),
        ("name\n\"Ann\n", "data.unclosedQuote"),
    ] {
        p.write("data/users.csv", csv);
        let hands = Fake::new(solid([0, 0, 0]));
        let report = run_with(&p, &hands, RunControl::new(), &options()).0.report;
        let failure = report.attempts[0].failure.clone().unwrap();
        assert_eq!(failure.code, "run.data.unreadable");
        assert_eq!(failure.param("problem"), code);
        assert!(hands.log().is_empty());
    }
    p.write("data/users.csv", "name\n");
    let report = run_with(
        &p,
        &Fake::new(solid([0, 0, 0])),
        RunControl::new(),
        &options(),
    )
    .0
    .report;
    assert_eq!(failure_code(&report), "run.data.noRow");
}

#[test]
fn environment_values_and_one_data_row() {
    let p = Project::new();
    p.write(
        "jevision.environments.json",
        r#"{"format":1,"environments":{"staging":{"HOST":"s.example.com"},"empty":{}}}"#,
    )
    .write("data/users.csv", "name\nAnn\nBo\n")
    .test(
        r#""rows":{"type":"useData","file":"data/users.csv","body":"type"},
           "type":{"type":"type","text":"${data.name}@${env.HOST}","next":"rows"}"#,
        "rows",
        0,
    );
    let staging = RunOptions {
        environment: Some("staging".into()),
        rows: Some(1..=1),
        ..options()
    };
    let hands = Fake::new(solid([0, 0, 0]));
    let (run, events) = run_with(&p, &hands, RunControl::new(), &staging);
    assert_eq!(run.report.verdict, Verdict::Passed);
    assert_eq!(hands.typed(), ["Bo@s.example.com"]);
    assert_eq!(run.report.environment.as_deref(), Some("staging"));
    assert!(events.iter().any(|e| matches!(e,
        Event::StepStarted { variables, .. } if variables["env.HOST"] == "s.example.com")));
    let saved = Run::load(&run.dir).unwrap();
    assert_eq!(saved.report.environment.as_deref(), Some("staging"));

    // Nothing touches the computer when a value can't be found.
    for (environment, rows, code) in [
        (None, None, "run.env.notChosen"),
        (Some("empty"), None, "run.env.missing"),
        (Some("prod"), None, "run.env.unknown"),
        (Some("staging"), Some(2..=2), "run.data.noRow"),
    ] {
        let hands = Fake::new(solid([0, 0, 0]));
        let options = RunOptions {
            environment: environment.map(str::to_owned),
            rows,
            ..options()
        };
        let report = run_with(&p, &hands, RunControl::new(), &options).0.report;
        assert_eq!(failure_code(&report), code);
        assert!(hands.typed().is_empty(), "{code}");
    }
}

#[test]
fn cancel_stops_a_long_wait_at_once() {
    let p = Project::new();
    p.write("refs/ok.png", solid([0, 255, 0])).test(
        r#""wait":{"type":"waitFor","until":{"appears":{"image":"refs/ok.png"}},"timeout":"60s","next":"keys"},
           "keys":{"type":"pressKeys","keys":"return"}"#,
        "wait",
        3,
    );
    let hands = Fake::new(solid([255, 0, 0]));
    let control = RunControl::new();
    let remote = control.clone();
    let clock = Instant::now();
    let report = std::thread::scope(|s| {
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            remote.cancel();
        });
        let mut slow = options();
        slow.poll_interval = Duration::from_secs(5);
        run_with(&p, &hands, control, &slow).0.report
    });
    assert!(
        clock.elapsed() < Duration::from_secs(2),
        "{:?}",
        clock.elapsed()
    );
    assert_eq!(report.verdict, Verdict::Cancelled);
    assert_eq!(report.attempts.len(), 1, "a cancelled run is not retried");
    assert_eq!(
        statuses(&report),
        [("wait".to_owned(), StepStatus::Cancelled)]
    );
    assert!(hands.log().is_empty());
    assert_well_formed_xml(&junit_xml(&[report]));
}

#[test]
fn breakpoints_step_mode_and_user_input_pause() {
    let p = Project::new();
    p.test(
        r#""a":{"type":"pressKeys","keys":"a","next":"b"},
           "b":{"type":"pressKeys","keys":"b","next":"c"},
           "c":{"type":"pressKeys","keys":"c","next":"d"},
           "d":{"type":"pressKeys","keys":"d"}"#,
        "a",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.user_input = true);
    let control = RunControl::new();
    control.set_breakpoints(["b".to_owned()]);
    let (tx, rx) = mpsc::channel();
    let sink = move |e: &Event| tx.send(e.clone()).unwrap();
    let secrets = secrets();
    let runner = Runner {
        hands: &hands,
        secrets: &secrets,
        events: &sink,
        control: control.clone(),
        vision: None,
        fallback: None,
        providers: &[],
        usage: None,
        locator: None,
    };

    let pauses = std::thread::scope(|s| {
        let handle = s.spawn(|| {
            runner
                .run(&p.0, Path::new("t.flow.json"), &options())
                .unwrap()
        });
        let mut pauses = Vec::new();
        for event in rx.iter() {
            if let Event::Paused { block, reason, .. } = &event {
                pauses.push((block.clone(), *reason, hands.log().len()));
                match reason {
                    PauseReason::Breakpoint => control.step(),
                    _ => control.resume(),
                }
            }
            if matches!(event, Event::RunFinished { .. }) {
                break;
            }
        }
        assert_eq!(handle.join().unwrap().report.verdict, Verdict::Passed);
        pauses
    });
    assert_eq!(
        pauses,
        [
            ("a".to_owned(), PauseReason::UserInput, 0),
            ("b".to_owned(), PauseReason::Breakpoint, 1),
            ("c".to_owned(), PauseReason::Step, 2),
        ]
    );
    assert_eq!(hands.log(), ["keys a", "keys b", "keys c", "keys d"]);

    // Starting from a chosen block skips the ones before it.
    let hands = Fake::new(solid([0, 0, 0]));
    let from_c = RunOptions {
        start_at: Some("c".to_owned()),
        ..options()
    };
    run_with(&p, &hands, RunControl::new(), &from_c);
    assert_eq!(hands.log(), ["keys c", "keys d"]);
}

#[test]
fn retries_tell_flaky_from_stable_failure() {
    let p = Project::new();
    let good = solid([0, 128, 0]);
    p.write("refs/ok.png", &good).test(
        r#""open":{"type":"openApp","next":"check"},
           "check":{"type":"check","reference":{"image":"refs/ok.png","compare":"exact"}}"#,
        "open",
        2,
    );
    // Fails until the app is restarted, then passes.
    let hands = Fake::new(solid([128, 0, 0])).with(|s| s.after_quit = Some(good));
    let flaky = run(&p, &hands).report;
    assert_eq!(
        (flaky.verdict, flaky.flaky, flaky.attempts.len()),
        (Verdict::Passed, true, 2)
    );
    assert!(hands.log().contains(&format!("quit {APP}")));
    let xml = junit_xml(std::slice::from_ref(&flaky));
    assert_well_formed_xml(&xml);
    assert!(xml.contains("<flakyFailure") && !xml.contains("<failure"));

    let hands = Fake::new(solid([128, 0, 0]));
    let stable = run(&p, &hands).report;
    assert_eq!(
        (stable.verdict, stable.flaky, stable.attempts.len()),
        (Verdict::Failed, false, 3)
    );
    let xml = junit_xml(&[flaky, stable]);
    assert_well_formed_xml(&xml);
    assert!(xml.contains(r#"tests="2" failures="1""#));
    assert_eq!(xml.matches("<failure ").count(), 1);
    assert_eq!(xml.matches("<rerunFailure ").count(), 2);
}

#[test]
fn project_retries_and_quarantine_apply_to_runs() {
    let p = Project::new();
    let failing = format!(
        r#"{{"format":1,"name":"T","app":"{APP}","start":"check","blocks":{{
            "check":{{"type":"check","reference":{{"image":"refs/ok.png","compare":"exact"}}}}}}}}"#
    );
    p.write("refs/ok.png", solid([0, 128, 0]))
        .write("t.flow.json", &failing)
        .write(
            "jevision.project.json",
            r#"{"format":1,"retries":2,"quarantine":[{"test":"t.flow.json","reason":"Bug 12"}]}"#,
        );
    let report = run(&p, &Fake::new(solid([128, 0, 0]))).report;
    assert_eq!(report.attempts.len(), 3, "the project's default retries");
    assert!(report.quarantined && report.is_quarantined_failure());
    assert_eq!(report.quarantine_reason.as_deref(), Some("Bug 12"));
    assert_eq!(report.file_hash.as_ref().map(String::len), Some(64));
    assert_eq!(
        RetryVerdict::of(report.verdict, report.flaky, report.attempts.len())
            .unwrap()
            .describe(),
        "Failed 3 times in a row — a real failure, not flakiness."
    );
    let xml = junit_xml(std::slice::from_ref(&report));
    assert_well_formed_xml(&xml);
    assert!(
        xml.contains(r#"failures="0" errors="0" skipped="1""#),
        "{xml}"
    );
    assert!(
        xml.contains(r#"<skipped message="Quarantined (Bug 12). "#),
        "{xml}"
    );

    // The test's own retries win over the project's, even 0.
    p.write(
        "t.flow.json",
        failing.replace(r#""start""#, r#""retries":0,"start""#),
    );
    let report = run(&p, &Fake::new(solid([128, 0, 0]))).report;
    assert_eq!(report.attempts.len(), 1);

    p.write("jevision.project.json", r#"{"format":1,"retries":99}"#);
    let report = run(&p, &Fake::new(solid([0, 128, 0]))).report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.project.unreadable");
    assert!(!report.quarantined);
}

#[test]
fn retry_verdicts_say_flaky_or_real() {
    use Verdict::{Cancelled, Failed, Passed};
    assert_eq!(RetryVerdict::of(Failed, false, 1), None);
    assert_eq!(RetryVerdict::of(Cancelled, false, 3), None);
    assert_eq!(
        RetryVerdict::of(Passed, true, 3),
        Some(RetryVerdict::PassedOnRetry { retry: 2 })
    );
    assert_eq!(
        RetryVerdict::of(Failed, true, 2),
        Some(RetryVerdict::FailedDifferently { attempts: 2 })
    );
    assert_eq!(
        RetryVerdict::PassedOnRetry { retry: 1 }.describe(),
        "Passed on retry 1 after failing — the test is flaky."
    );
}

#[test]
fn secrets_never_reach_reports_even_in_errors() {
    let p = Project::new();
    p.test(
        r#""login":{"type":"type","text":"user ${secret.ACME_PASSWORD}"}"#,
        "login",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.reject_typing = true);
    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());
    let failure = run.report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.hands.failed");
    assert_eq!(failure.param("reason"), "the field rejected user ••••");
    let all = outputs(&run, &events);
    assert!(!all.contains(SECRET));
    assert!(all.contains("the field rejected user ••••"));
    assert_well_formed_xml(&junit_xml(std::slice::from_ref(&run.report)));

    // A missing secret stops the run before anything touches the computer.
    p.test(
        r#""login":{"type":"type","text":"${secret.OTHER}"}"#,
        "login",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run_with(&p, &hands, RunControl::new(), &options()).0.report;
    assert_eq!(failure_code(&report), "run.secret.missing");
    assert!(hands.log().is_empty());
}

#[test]
fn sub_tests_run_in_place_and_cannot_recurse() {
    let p = Project::new();
    p.write(
        "shared/login.flow.json",
        format!(
            r#"{{"format":1,"name":"Login","app":"{APP}","start":"k","blocks":{{
                "k":{{"type":"pressKeys","keys":"cmd+l"}}}}}}"#
        ),
    )
    .test(
        r#""sub":{"type":"runTest","test":"shared/login.flow.json","next":"after"},
           "after":{"type":"pressKeys","keys":"return"}"#,
        "sub",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run(&p, &hands).report;
    assert_eq!(report.verdict, Verdict::Passed);
    assert_eq!(hands.log(), ["keys cmd+l", "keys return"]);
    let tests: Vec<&str> = report.attempts[0]
        .steps
        .iter()
        .map(|s| s.test.as_str())
        .collect();
    assert_eq!(
        tests,
        ["t.flow.json", "shared/login.flow.json", "t.flow.json"]
    );

    p.test(
        r#""self":{"type":"runTest","test":"t.flow.json"}"#,
        "self",
        0,
    );
    let report = run(&p, &Fake::new(solid([0, 0, 0]))).report;
    assert_eq!(failure_code(&report), "run.runTest.recursion");
}

#[test]
fn click_by_image_fails_when_target_is_not_on_screen() {
    let p = Project::new();
    p.write("refs/button.png", solid([9, 9, 9])).test(
        r#""click":{"type":"click","target":{"image":"refs/button.png"},"button":"left"}"#,
        "click",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.not_findable = true);
    let report = run(&p, &hands).report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(
        (failure.code.as_str(), failure.param("image")),
        ("run.target.notFound", "refs/button.png")
    );
    assert!(
        html(&report, Path::new("/nonexistent"), None)
            .contains("refs/button.png was not found on screen")
    );
}

#[test]
fn repeat_and_if_follow_the_screen() {
    let p = Project::new();
    p.write("refs/on.png", solid([0, 200, 0])).test(
        r#""loop":{"type":"repeat","times":3,"body":"if"},
           "if":{"type":"if","reference":{"image":"refs/on.png"},"then":"yes","else":"no"},
           "yes":{"type":"pressKeys","keys":"y","next":"loop"},
           "no":{"type":"pressKeys","keys":"n","next":"loop"}"#,
        "loop",
        0,
    );
    let hands = Fake::new(solid([0, 200, 0]));
    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());
    assert_eq!(run.report.verdict, Verdict::Passed);
    assert_eq!(hands.log(), ["keys y", "keys y", "keys y"]);
    let steps = &run.report.attempts[0].steps;
    assert_eq!(steps[0].block, "loop");
    assert_eq!(steps[1].branch.as_deref(), Some("then"));
    let passes: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::StepStarted {
                block, variables, ..
            } if block == "yes" => Some(variables["repeat.loop"].clone()),
            _ => None,
        })
        .collect();
    assert_eq!(passes, ["1 of 3", "2 of 3", "3 of 3"]);

    hands.state.lock().unwrap().screen = solid([200, 0, 0]);
    run_with(&p, &hands, RunControl::new(), &options());
    assert_eq!(hands.log()[3..], ["keys n", "keys n", "keys n"]);
}

#[test]
fn if_takes_the_first_matching_case_or_else() {
    let p = Project::new();
    p.write("refs/green.png", solid([0, 200, 0]))
        .write("refs/red.png", solid([200, 0, 0]))
        .write("refs/blue.png", solid([0, 0, 200]))
        .test(
            r#""if":{"type":"if","reference":{"image":"refs/green.png"},"then":"g",
                     "elseIf":[{"reference":{"image":"refs/red.png"},"then":"r"},
                               {"reference":{"image":"refs/blue.png"},"then":"b"}],
                     "else":"other"},
               "g":{"type":"pressKeys","keys":"g"},
               "r":{"type":"pressKeys","keys":"r"},
               "b":{"type":"pressKeys","keys":"b"},
               "other":{"type":"pressKeys","keys":"o"}"#,
            "if",
            0,
        );
    for (screen, keys, branch, text) in [
        ([0, 200, 0], "keys g", "then", "Took “Condition 1: green”"),
        ([200, 0, 0], "keys r", "elseIf1", "Took “Condition 2: red”"),
        ([0, 0, 200], "keys b", "elseIf2", "Took “Condition 3: blue”"),
        ([90, 90, 90], "keys o", "else", "Took “Otherwise”"),
    ] {
        let hands = Fake::new(solid(screen));
        let done = run(&p, &hands);
        assert!(html(&done.report, &done.dir, None).contains(&format!(">{text}<")));
        assert!(
            junit_xml(std::slice::from_ref(&done.report))
                .contains(&format!("<system-out>if: {text}</system-out>"))
        );
        let report = done.report;
        assert_eq!(report.verdict, Verdict::Passed, "{branch}");
        assert_eq!(hands.log(), [keys]);
        let step = &report.attempts[0].steps[0];
        assert_eq!(step.branch.as_deref(), Some(branch));
        assert_eq!(step.branches, Some(4));
        assert_eq!(step.branch_text().as_deref(), Some(text));
        assert!(
            step.similarity.is_some(),
            "{branch}: the comparison is in the report"
        );
    }
}

#[test]
fn open_file_opens_a_fresh_copy() {
    let p = Project::new();
    p.write("docs/note.txt", "original").test(
        r#""open":{"type":"openFile","files":["docs/note.txt","https://example.com/a"],
           "app":"com.apple.TextEdit"}"#,
        "open",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let run = run(&p, &hands);
    assert_eq!(run.report.verdict, Verdict::Passed);
    let copy = run.dir.join("files/1-001-0/note.txt");
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "original");
    let expected = vec![
        Openable::File(copy),
        Openable::Url("https://example.com/a".to_owned()),
    ];
    assert_eq!(
        hands.log(),
        [format!("open {expected:?} in Some(\"com.apple.TextEdit\")")]
    );

    p.test(
        r#""open":{"type":"openFile","files":["../outside.txt"]}"#,
        "open",
        0,
    );
    let report = run_with(&p, &hands, RunControl::new(), &options()).0.report;
    assert_eq!(failure_code(&report), "run.path.outsideProject");
}

#[test]
fn files_opened_to_close_after_the_test_close_even_when_it_fails() {
    let p = Project::new();
    p.write("note.txt", "original").test(
        r#""open":{"type":"openFile","files":["note.txt"],"closeAfter":true,"next":"keep"},
           "keep":{"type":"openFile","files":["https://example.com"],"next":"wait"},
           "wait":{"type":"pause","duration":"20ms","next":"miss"},
           "miss":{"type":"runTest","test":"missing.flow.json"}"#,
        "open",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let started = std::time::Instant::now();
    let run = run(&p, &hands);
    assert!(started.elapsed() >= Duration::from_millis(20));
    assert_eq!(run.report.verdict, Verdict::Failed);
    let copy = Openable::File(run.dir.join("files/1-001-0/note.txt"));
    let log = hands.log();
    assert_eq!(log.len(), 3, "{log:?}");
    assert_eq!(log[2], format!("close {:?} in None", vec![copy]));
    assert_eq!(
        std::fs::read_to_string(p.0.join("note.txt")).unwrap(),
        "original"
    );
}

#[test]
fn a_custom_block_reads_the_inputs_the_test_gives_it() {
    let p = Project::new();
    p.write(
        "blocks/sign in.block.json",
        format!(
            r#"{{"format":1,"name":"Sign in","icon":"person","app":"{APP}","start":"k","blocks":{{
                "k":{{"type":"type","text":"${{input.email}}/${{input.password}}"}}}}}}"#
        ),
    )
    .test(
        r#""sub":{"type":"runTest","test":"blocks/sign in.block.json",
              "inputs":{"email":"me@example.com","password":"${secret.ACME_PASSWORD}"},"next":"after"},
           "after":{"type":"type","text":"${input.email}"}"#,
        "sub",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run(&p, &hands).report;
    assert_eq!(hands.typed(), [format!("me@example.com/{SECRET}")]);
    // Inputs belong to the block: the test itself has none.
    assert_eq!(failure_code(&report), "run.input.missing");
}

#[test]
fn placeholders_fill_the_paths_of_files_blocks_read() {
    let p = Project::new();
    p.write(
        "jevision.environments.json",
        r#"{"format":1,"environments":{"staging":{"SET":"users","HOST":"example.com"}}}"#,
    )
    .write("data/users.csv", "name\nann\n")
    .write("docs/ann.txt", "original")
    .write(
        "subs/ann.flow.json",
        format!(
            r#"{{"format":1,"name":"Ann","app":"{APP}","start":"k","blocks":{{
                "k":{{"type":"pressKeys","keys":"cmd+l"}}}}}}"#
        ),
    )
    .test(
        r#""rows":{"type":"useData","file":"data/${env.SET}.csv","body":"open"},
           "open":{"type":"openFile","files":["docs/${data.name}.txt","https://${env.HOST}/a"],
                   "next":"sub"},
           "sub":{"type":"runTest","test":"subs/${data.name}.flow.json","next":"rows"}"#,
        "rows",
        0,
    );
    let staging = RunOptions {
        environment: Some("staging".into()),
        ..options()
    };
    let hands = Fake::new(solid([0, 0, 0]));
    let run = run_with(&p, &hands, RunControl::new(), &staging).0;
    assert_eq!(run.report.verdict, Verdict::Passed);
    let expected = vec![
        Openable::File(run.dir.join("files/1-002-0/ann.txt")),
        Openable::Url("https://example.com/a".to_owned()),
    ];
    assert_eq!(
        hands.log(),
        [
            format!("open {expected:?} in None"),
            "keys cmd+l".to_owned()
        ]
    );

    // A secret in a path is used but never shown, even when the path is refused.
    p.test(
        r#""open":{"type":"openFile","files":["../${secret.ACME_PASSWORD}.txt"]}"#,
        "open",
        0,
    );
    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());
    assert_eq!(failure_code(&run.report), "run.path.outsideProject");
    assert!(!outputs(&run, &events).contains(SECRET));
}

/// A vision model that gives a fixed answer and keeps what it was shown.
struct Judge {
    name: &'static str,
    id: &'static str,
    local: bool,
    answer: Result<Judgement, VisionError>,
    asked: Mutex<Vec<Asked>>,
}

struct Asked {
    expect: String,
    model: String,
    screenshot: Vec<u8>,
    reference: Option<Vec<u8>>,
}

impl Judge {
    fn new(local: bool, answer: Result<Judgement, VisionError>) -> Self {
        Self {
            name: "Test Vision",
            id: "test-vision",
            local,
            answer,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn says(passed: bool) -> Self {
        Self::new(
            false,
            Ok(Judgement {
                passed,
                reason: if passed {
                    "Two items in the cart."
                } else {
                    "The cart is empty."
                }
                .into(),
                usage: Usage {
                    input_tokens: 4000,
                    output_tokens: 1000,
                },
            }),
        )
    }
}

impl VisionJudge for Judge {
    fn provider(&self) -> &str {
        self.name
    }
    fn provider_id(&self) -> &str {
        self.id
    }
    fn model(&self) -> &str {
        "seer-1"
    }
    fn is_local(&self) -> bool {
        self.local
    }
    fn judge(&self, q: &Question<'_>) -> Result<Judgement, VisionError> {
        self.asked.lock().unwrap().push(Asked {
            expect: q.expect.to_owned(),
            model: q.model.to_owned(),
            screenshot: q.screenshot.png().to_vec(),
            reference: q.reference.map(|r| r.png().to_vec()),
        });
        self.answer.clone()
    }
}

fn run_judged(project: &Project, hands: &Fake, judge: Option<&Judge>, local_only: bool) -> Run {
    run_ai(project, hands, judge, None, None, local_only)
}

fn run_ai(
    project: &Project,
    hands: &Fake,
    judge: Option<&Judge>,
    fallback: Option<&Judge>,
    usage: Option<&UsageBook>,
    local_only: bool,
) -> Run {
    run_with_providers(project, hands, judge, fallback, &[], usage, local_only)
}

fn run_with_providers(
    project: &Project,
    hands: &Fake,
    judge: Option<&Judge>,
    fallback: Option<&Judge>,
    providers: &[&dyn VisionJudge],
    usage: Option<&UsageBook>,
    local_only: bool,
) -> Run {
    let secrets = secrets();
    let runner = Runner {
        hands,
        secrets: &secrets,
        events: &|_: &Event| {},
        control: RunControl::new(),
        vision: judge.map(|j| j as &dyn VisionJudge),
        fallback: fallback.map(|j| j as &dyn VisionJudge),
        providers,
        usage,
        locator: None,
    };
    let options = RunOptions {
        local_only,
        ..options()
    };
    runner
        .run(&project.0, Path::new("t.flow.json"), &options)
        .unwrap()
}

fn size(png_bytes: &[u8]) -> (u32, u32) {
    let info = png::Decoder::new(std::io::Cursor::new(png_bytes))
        .read_info()
        .unwrap()
        .info()
        .clone();
    (info.width, info.height)
}

#[test]
fn checks_by_meaning_show_the_model_only_the_compared_part_with_private_parts_covered() {
    let p = Project::new();
    // Left half of the window compared; its top-left quarter hidden, a stripe ignored.
    p.write("refs/cart.png", solid([0, 0, 200])).test(
        r#""check":{"type":"check","reference":{"image":"refs/cart.png","compare":"meaning",
           "expect":"The cart shows 2 items","area":{"x":0,"y":0,"width":0.5,"height":1},
           "hide":[{"x":0,"y":0,"width":0.25,"height":0.5}],
           "ignore":[{"x":0,"y":0.9,"width":0.5,"height":0.1}]}}"#,
        "check",
        0,
    );
    let screen = png(40, 20, |x, y| match (x, y) {
        (0..10, 0..10) if (x + y) % 2 == 0 => [255, 255, 255],
        (0..10, 0..10) => [0, 0, 0],
        (_, 18..) if x % 2 == 0 => [255, 0, 0],
        (_, 18..) => [0, 0, 255],
        _ => [0, 200, 0],
    });
    let hands = Fake::new(screen);
    let judge = Judge::says(true);
    let report = run_judged(&p, &hands, Some(&judge), false).report;
    assert_eq!(report.verdict, Verdict::Passed);
    let step = &report.attempts[0].steps[0];
    assert_eq!(
        step.vision,
        Some(VisionAnswer {
            provider: "Test Vision".into(),
            model: "seer-1".into(),
            passed: true,
            reason: "Two items in the cart.".into(),
            input_tokens: 4000,
            output_tokens: 1000,
            cost: None,
            warning: None,
            fallback_reason: None,
        })
    );
    assert_eq!(step.similarity, None);
    assert!(step.diff.is_none() && step.expected.is_some());

    let asked = judge.asked.lock().unwrap();
    let Asked {
        expect,
        screenshot: shot,
        reference,
        ..
    } = &asked[0];
    assert_eq!(expect, "The cart shows 2 items");
    assert_eq!(size(shot), (20, 20));
    assert_eq!(size(reference.as_ref().unwrap()), (20, 20));
    // The checkerboard under the hidden part became one flat colour; the ignored stripe too.
    for y in 0..10 {
        for x in 0..10 {
            assert_eq!(pixel(shot, x, y), [128, 128, 128], "({x}, {y})");
        }
    }
    assert_eq!(pixel(shot, 14, 19), pixel(shot, 15, 19));
    assert_eq!(pixel(shot, 15, 5), [0, 200, 0]);

    // The model says no: the check fails with its reason.
    let judge = Judge::says(false);
    let report = run_judged(&p, &hands, Some(&judge), false).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.check.meaningMismatch");
    assert_eq!(failure.param("reason"), "The cart is empty.");
    assert_eq!(failure.param("provider"), "Test Vision");
}

#[test]
fn an_expectation_needs_no_reference_and_if_follows_the_verdict() {
    let p = Project::new();
    p.test(
        r#""if":{"type":"if","reference":{"compare":"meaning","expect":"A dialog is open"},
           "then":"yes","else":"no"},
           "yes":{"type":"pressKeys","keys":"return"},
           "no":{"type":"pressKeys","keys":"escape"}"#,
        "if",
        0,
    );
    let hands = Fake::new(solid([9, 9, 9]));
    let judge = Judge::says(false);
    let report = run_judged(&p, &hands, Some(&judge), false).report;
    assert_eq!(report.verdict, Verdict::Passed);
    assert_eq!(report.attempts[0].steps[0].branch.as_deref(), Some("else"));
    assert_eq!(
        report.attempts[0].steps[0].branch_text().as_deref(),
        Some("Took “No”")
    );
    assert!(judge.asked.lock().unwrap()[0].reference.is_none());
    assert!(report.attempts[0].steps[0].expected.is_none());

    let report = run_judged(&p, &hands, Some(&Judge::says(true)), false).report;
    assert_eq!(
        report.attempts[0].steps[0].branch_text().as_deref(),
        Some("Took “Yes: A dialog is open”")
    );
}

#[test]
fn local_only_keeps_screenshots_away_from_cloud_models() {
    let p = Project::new();
    p.test(
        r#""check":{"type":"check","reference":{"compare":"meaning","expect":"Signed in"}}"#,
        "check",
        0,
    );
    let hands = Fake::new(solid([9, 9, 9]));

    let cloud = Judge::says(true);
    let report = run_judged(&p, &hands, Some(&cloud), true).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.vision.cloudBlocked");
    assert_eq!(failure.param("provider"), "Test Vision");
    assert!(cloud.asked.lock().unwrap().is_empty());

    let local = Judge {
        local: true,
        ..Judge::says(true)
    };
    let report = run_judged(&p, &hands, Some(&local), true).report;
    assert_eq!(report.verdict, Verdict::Passed);
    assert_eq!(local.asked.lock().unwrap().len(), 1);

    let report = run_judged(&p, &hands, None, false).report;
    assert_eq!(failure_code(&report), "run.compare.meaningUnavailable");
}

#[test]
fn project_areas_and_password_fields_are_covered_wherever_a_screenshot_goes() {
    let p = Project::new();
    // The project hides a stripe on the left of every window.
    p.write(
        "jevision.project.json",
        r#"{"format":1,"hide":[{"x":0,"y":0,"width":0.25,"height":1}]}"#,
    )
    .test(
        r#""click":{"type":"click","target":{"point":{"x":0.5,"y":0.5}},"next":"check"},
           "check":{"type":"check","reference":{"compare":"meaning","expect":"Signed in"}}"#,
        "click",
        0,
    );
    // A password field in the bottom-right corner.
    let field = jevision_core::Rect {
        x: 0.75,
        y: 0.5,
        width: 0.25,
        height: 0.5,
    };
    let hands = Fake::new(png(40, 20, |x, y| [x as u8 * 5, y as u8 * 10, 99]))
        .with(|s| s.secure = vec![field]);
    let judge = Judge::says(true);
    let run = run_judged(&p, &hands, Some(&judge), false);
    assert_eq!(run.report.verdict, Verdict::Passed);

    let covered = |x: u32, y: u32| x < 10 || (x >= 30 && y >= 10);
    let check_covered = |picture: &[u8], what: &str| {
        for y in 0..20 {
            for x in 0..40 {
                let expected = if covered(x, y) {
                    [128, 128, 128]
                } else {
                    [x as u8 * 5, y as u8 * 10, 99]
                };
                assert_eq!(pixel(picture, x, y), expected, "{what} ({x}, {y})");
            }
        }
    };
    check_covered(&judge.asked.lock().unwrap()[0].screenshot, "sent");
    let steps = &run.report.attempts[0].steps;
    let saved = [
        steps[0].before.as_ref().unwrap(),
        steps[0].after.as_ref().unwrap(),
        steps[1].after.as_ref().unwrap(),
        steps[1].sent.as_ref().unwrap(),
    ];
    for path in saved {
        check_covered(&std::fs::read(run.dir.join(path)).unwrap(), path);
    }
    assert!(steps[1].sent_masked);
    let page = html(&run.report, &run.dir, None);
    assert!(page.contains("Sent to the AI model, masked before sending"));
}

#[test]
fn a_local_only_project_keeps_screenshots_away_from_cloud_models() {
    let p = Project::new();
    p.write("jevision.project.json", r#"{"format":1,"localOnly":true}"#)
        .test(
            r#""check":{"type":"check","reference":{"compare":"meaning","expect":"Signed in"}}"#,
            "check",
            0,
        );
    let hands = Fake::new(solid([9, 9, 9]));
    let cloud = Judge::says(true);
    let report = run_judged(&p, &hands, Some(&cloud), false).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.vision.cloudBlocked");
    assert_eq!(failure.param("setting"), "project");
    assert!(cloud.asked.lock().unwrap().is_empty());
    assert!(report.attempts[0].steps[0].sent.is_none());

    let local = Judge {
        local: true,
        ..Judge::says(true)
    };
    let report = run_judged(&p, &hands, Some(&local), false).report;
    assert_eq!(report.verdict, Verdict::Passed);
    // Nothing was covered, and the report says so.
    assert!(!report.attempts[0].steps[0].sent_masked);
}

#[test]
fn vision_problems_are_localizable_failures() {
    let p = Project::new();
    p.test(
        r#""wait":{"type":"waitFor","until":{"appears":{"compare":"meaning","expect":"Done"}},
           "timeout":"1s"}"#,
        "wait",
        0,
    );
    let hands = Fake::new(solid([9, 9, 9]));
    let slow = Judge::new(false, Err(VisionError::Timeout { seconds: 120 }));
    let failure = run_judged(&p, &hands, Some(&slow), false).report.attempts[0]
        .failure
        .clone()
        .unwrap();
    assert_eq!(failure.code, "run.vision.timeout");
    assert_eq!(
        failure.params.keys().collect::<Vec<_>>(),
        ["block", "model", "provider", "seconds"]
    );
    assert_eq!(failure.param("seconds"), "120");
    assert_eq!(
        jevision_runner::describe(&failure),
        "Block wait: Test Vision did not answer within 120 s."
    );

    // A model that keeps saying no: the wait times out on the expectation, the last answer kept.
    let report = run_judged(&p, &hands, Some(&Judge::says(false)), false).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.waitFor.timeout");
    assert_eq!(failure.param("target"), "Done");
    let step = &report.attempts[0].steps[0];
    assert_eq!(step.vision.as_ref().unwrap().reason, "The cart is empty.");
}

#[test]
fn waiting_by_meaning_asks_again_only_when_the_screen_changes() {
    let p = Project::new();
    p.test(
        r#""wait":{"type":"waitFor","until":{"appears":{"compare":"meaning","expect":"Done"}},
                   "timeout":"1s"}"#,
        "wait",
        0,
    );
    let asked = |hands: &Fake| {
        let judge = Judge::says(false);
        let report = run_judged(&p, hands, Some(&judge), false).report;
        assert_eq!(failure_code(&report), "run.waitFor.timeout");
        let step = &report.attempts[0].steps[0];
        assert_eq!(step.vision.as_ref().unwrap().reason, "The cart is empty.");
        assert!(step.sent.is_some());
        judge.asked.lock().unwrap().len()
    };
    assert_eq!(asked(&Fake::new(solid([9, 9, 9]))), 1);
    let changing = Fake::new(solid([9, 9, 9])).with(|s| s.after_capture = Some(solid([0, 9, 9])));
    assert_eq!(asked(&changing), 2);
}

#[test]
fn a_model_quoting_a_secret_from_the_screen_is_masked() {
    let p = Project::new();
    p.test(
        r#""login":{"type":"type","text":"${secret.ACME_PASSWORD}","next":"check"},
           "check":{"type":"check","reference":{"compare":"meaning","expect":"Signed in"}}"#,
        "login",
        0,
    );
    let hands = Fake::new(solid([0, 200, 0]));
    let judge = Judge::new(
        false,
        Ok(Judgement {
            passed: false,
            reason: format!("The password field shows {SECRET}."),
            usage: Usage::default(),
        }),
    );
    let run = run_judged(&p, &hands, Some(&judge), false);
    let step = &run.report.attempts[0].steps[1];
    assert_eq!(
        step.vision.as_ref().unwrap().reason,
        "The password field shows ••••."
    );
    let saved = std::fs::read_to_string(run.dir.join("report.json")).unwrap();
    assert!(!saved.contains(SECRET));
}

#[test]
fn a_pause_shows_the_variables_of_the_block_ahead() {
    let p = Project::new();
    p.write("data/users.csv", "email\na@x.io\nb@x.io\n").test(
        r#""rows":{"type":"useData","file":"data/users.csv","body":"type"},
           "type":{"type":"type","text":"${data.email}","next":"rows"}"#,
        "rows",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let control = RunControl::new();
    control.set_breakpoints(["type".to_owned()]);
    let (tx, rx) = mpsc::channel();
    let sink = move |e: &Event| tx.send(e.clone()).unwrap();
    let secrets = secrets();
    let runner = Runner {
        hands: &hands,
        secrets: &secrets,
        events: &sink,
        control: control.clone(),
        vision: None,
        fallback: None,
        providers: &[],
        usage: None,
        locator: None,
    };
    let seen = std::thread::scope(|s| {
        let handle = s.spawn(|| {
            runner
                .run(&p.0, Path::new("t.flow.json"), &options())
                .unwrap()
        });
        let mut seen = Vec::new();
        for event in rx.iter() {
            if let Event::Paused { variables, .. } = &event {
                seen.push(variables["data.email"].clone());
                control.resume();
            }
            if matches!(event, Event::RunFinished { .. }) {
                break;
            }
        }
        assert_eq!(handle.join().unwrap().report.verdict, Verdict::Passed);
        seen
    });
    assert_eq!(seen, ["a@x.io", "b@x.io"]);
}

fn with_video(mode: VideoMode) -> RunOptions {
    RunOptions {
        video: mode,
        ..options()
    }
}

fn markers(report: &RunReport) -> Vec<Option<u64>> {
    report
        .attempts
        .iter()
        .flat_map(|a| a.steps.iter().map(|s| s.video_at_ms))
        .collect()
}

#[test]
fn video_has_step_markers_and_is_kept_as_asked() {
    let p = Project::new();
    p.test(
        r#""a":{"type":"pressKeys","keys":"a","next":"b"},
           "b":{"type":"pressKeys","keys":"b"}"#,
        "a",
        0,
    );

    let hands = Fake::new(solid([0, 0, 0]));
    let (run, _) = run_with(
        &p,
        &hands,
        RunControl::new(),
        &with_video(VideoMode::Always),
    );
    assert_eq!(run.report.video.as_deref(), Some(VIDEO_FILE));
    assert!(run.dir.join(VIDEO_FILE).is_file());
    assert_eq!(
        hands.log(),
        ["video com.example.App", "keys a", "keys b", "video end"]
    );
    let at: Vec<u64> = markers(&run.report)
        .into_iter()
        .map(Option::unwrap)
        .collect();
    assert_eq!(at.len(), 2);
    assert!(at[0] <= at[1], "{at:?}");
    assert!(at[1] <= run.report.duration_ms);
    let saved = Run::load(&run.dir).unwrap();
    assert_eq!(saved.report, run.report);

    // A passed run records, then lets the video go.
    let hands = Fake::new(solid([0, 0, 0]));
    let (run, _) = run_with(
        &p,
        &hands,
        RunControl::new(),
        &with_video(VideoMode::OnFailure),
    );
    assert_eq!(run.report.verdict, Verdict::Passed);
    assert_eq!(run.report.video, None);
    assert!(!run.dir.join(VIDEO_FILE).exists());
    assert_eq!(markers(&run.report), [None, None]);
    assert!(hands.log().contains(&"video end".to_owned()));

    let hands = Fake::new(solid([0, 0, 0]));
    let (run, _) = run_with(&p, &hands, RunControl::new(), &options());
    assert!(!hands.log().iter().any(|l| l.starts_with("video")));
    assert_eq!(markers(&run.report), [None, None]);

    // Without a recording the run goes on and simply has no video.
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.no_video = true);
    let (run, _) = run_with(
        &p,
        &hands,
        RunControl::new(),
        &with_video(VideoMode::Always),
    );
    assert_eq!(run.report.verdict, Verdict::Passed);
    assert_eq!(run.report.video, None);
    assert_eq!(markers(&run.report), [None, None]);
}

#[test]
fn the_video_covers_what_the_project_hides() {
    let p = Project::new();
    p.write(
        "jevision.project.json",
        r#"{"format":1,"hide":[{"x":0,"y":0,"width":0.25,"height":1}]}"#,
    )
    .test(r#""a":{"type":"pressKeys","keys":"a"}"#, "a", 0);
    let hands = Fake::new(solid([0, 0, 0]));
    run_with(
        &p,
        &hands,
        RunControl::new(),
        &with_video(VideoMode::Always),
    );
    assert_eq!(
        hands.state.lock().unwrap().video_hide,
        [jevision_core::Rect {
            x: 0.0,
            y: 0.0,
            width: 0.25,
            height: 1.0,
        }]
    );
}

#[test]
fn a_failed_run_keeps_one_video_for_all_its_tries() {
    let p = Project::new();
    p.test(r#""login":{"type":"type","text":"x"}"#, "login", 1);
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.reject_typing = true);
    let (run, _) = run_with(
        &p,
        &hands,
        RunControl::new(),
        &with_video(VideoMode::OnFailure),
    );
    assert_eq!(run.report.verdict, Verdict::Failed);
    assert_eq!(run.report.attempts.len(), 2);
    assert_eq!(run.report.video.as_deref(), Some(VIDEO_FILE));
    let at = markers(&run.report);
    assert!(at[0].unwrap() <= at[1].unwrap(), "{at:?}");
    let starts = hands
        .log()
        .iter()
        .filter(|l| *l == "video com.example.App")
        .count();
    assert_eq!(starts, 1);
    let page = html(&run.report, &run.dir, run.report.video.as_deref());
    assert!(page.contains(r#"<video id="video" controls preload="metadata" src="video.mp4">"#));
    assert_eq!(page.matches("class=\"seek\"").count(), 2);
}

#[test]
fn old_videos_are_pruned_and_reports_stay() {
    let p = Project::new();
    let now = SystemTime::now();
    let day = Duration::from_secs(24 * 60 * 60);
    p.write(".jevision/runs/old/report.json", "{}")
        .write(".jevision/runs/old/video.mp4", "movie")
        .write(".jevision/runs/new/video.mp4", "movie")
        .write(".jevision/runs/none/report.json", "{}");
    std::fs::File::options()
        .write(true)
        .open(p.0.join(".jevision/runs/old/video.mp4"))
        .unwrap()
        .set_modified(now - 31 * day)
        .unwrap();

    assert_eq!(prune_videos(&p.0, 30 * day, now).unwrap(), 1);
    assert!(!p.0.join(".jevision/runs/old/video.mp4").exists());
    assert!(p.0.join(".jevision/runs/old/report.json").exists());
    assert!(p.0.join(".jevision/runs/new/video.mp4").exists());
    assert_eq!(prune_videos(&p.0, 30 * day, now).unwrap(), 0);
    assert_eq!(prune_videos(&p.0.join("elsewhere"), day, now).unwrap(), 0);
}

#[test]
fn an_unreadable_test_says_what_is_wrong_in_plain_words() {
    let p = Project::new();
    p.write(
        "t.flow.json",
        r#"{"format":1,"name":"T","start":"a","blocks":{"a":{"type":"openApp","colour":1}}}"#,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let failure = run(&p, &hands).report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.test.unreadable");
    assert_eq!(
        jevision_runner::describe(&failure),
        "Test file t.flow.json cannot be read: Field \"app\": required, but missing. \
         Block \"a\", field \"colour\": not a known field."
    );
}

fn meaning_check(p: &Project, model: &str) {
    p.test(
        &format!(
            r#""check":{{"type":"check","reference":{{"compare":"meaning","expect":"The cart shows 2 items"{model}}}}}"#
        ),
        "check",
        0,
    );
}

fn usage_book(p: &Project, daily: Option<f64>) -> UsageBook {
    UsageBook {
        journal: p.0.join("ai-usage.jsonl"),
        prices: vec![Price {
            provider: "test-vision".into(),
            model: "seer".into(),
            input: 1.0,
            output: 2.0,
        }],
        limits: vec![Limit {
            provider: "test-vision".into(),
            daily,
            monthly: None,
        }],
    }
}

#[test]
fn ai_calls_are_logged_priced_and_stopped_at_the_limit() {
    let p = Project::new();
    meaning_check(&p, "");
    let hands = Fake::new(solid([0, 200, 0]));
    let judge = Judge::says(true);
    // Each call costs $0.006 of $0.01 a day.
    let book = usage_book(&p, Some(0.01));
    let run = || run_ai(&p, &hands, Some(&judge), None, Some(&book), false).report;

    let first = run();
    let answer = first.attempts[0].steps[0].vision.clone().unwrap();
    assert!((answer.cost.unwrap() - 0.006).abs() < 1e-12);
    assert_eq!(answer.warning, None);

    let second = run();
    assert_eq!(second.verdict, Verdict::Passed);
    let warning = second.attempts[0].steps[0].vision.clone().unwrap().warning;
    assert_eq!(warning.unwrap().code, "run.vision.limitReached");

    let third = run();
    assert_eq!(failure_code(&third), "run.vision.limitReached");
    let failure = third.attempts[0].steps[0].failure.clone().unwrap();
    assert_eq!(failure.param("period"), "day");
    assert_eq!(failure.param("spent"), "0.01");
    assert_eq!(failure.param("block"), "check");
    assert_eq!(judge.asked.lock().unwrap().len(), 2);
    assert!(describe(&failure).contains("Test Vision has used $0.01 of its daily limit of $0.01"));

    let records = book.records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        (
            records[0].provider.as_str(),
            records[0].model.as_str(),
            records[0].test.as_str(),
            records[0].block.as_str(),
            records[0].input_tokens,
            records[0].output_tokens
        ),
        ("test-vision", "seer-1", "t.flow.json", "check", 4000, 1000)
    );

    // Without a journal nothing is logged and nothing is held back.
    let free = run_ai(&p, &hands, Some(&judge), None, None, false).report;
    assert_eq!(free.verdict, Verdict::Passed);
}

#[test]
fn a_check_naming_a_provider_is_judged_by_that_provider() {
    let p = Project::new();
    meaning_check(&p, r#","provider":"ollama","model":"llava""#);
    let hands = Fake::new(solid([0, 200, 0]));
    let main = Judge::says(false);
    let ollama = Judge {
        name: "Ollama",
        id: "ollama",
        local: true,
        ..Judge::says(true)
    };
    let other = Judge {
        name: "OpenAI",
        id: "openai",
        ..Judge::says(false)
    };
    let providers: [&dyn VisionJudge; 2] = [&other, &ollama];
    let report = run_with_providers(&p, &hands, Some(&main), None, &providers, None, true).report;
    assert_eq!(report.verdict, Verdict::Passed);
    let answer = report.attempts[0].steps[0].vision.clone().unwrap();
    assert_eq!(
        (answer.provider.as_str(), answer.model.as_str()),
        ("Ollama", "llava")
    );
    assert_eq!(ollama.asked.lock().unwrap()[0].model, "llava");
    assert!(main.asked.lock().unwrap().is_empty());
    assert!(other.asked.lock().unwrap().is_empty());

    // Only models on this Mac holds for a named provider too.
    meaning_check(&p, r#","provider":"openai""#);
    let report = run_with_providers(&p, &hands, Some(&main), None, &providers, None, true).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.vision.cloudBlocked");
    assert_eq!(failure.param("provider"), "OpenAI");
    assert!(other.asked.lock().unwrap().is_empty());

    // A provider that isn't set up fails as if its key were missing, naming it.
    meaning_check(&p, r#","provider":"gemini""#);
    let report = run_with_providers(&p, &hands, Some(&main), None, &providers, None, false).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.vision.noKey");
    assert_eq!(failure.param("provider"), "Google Gemini");
}

fn named_judge(name: &'static str, id: &'static str, local: bool, passed: bool) -> Judge {
    Judge {
        name,
        id,
        local,
        ..Judge::says(passed)
    }
}

#[test]
fn the_project_chooses_the_ai_for_checks_between_the_block_and_the_app() {
    let p = Project::new();
    p.write(
        "jevision.project.json",
        r#"{"format":1,"ai":{"screen":{"provider":"ollama","model":"llava-project"}}}"#,
    );
    let hands = Fake::new(solid([0, 200, 0]));
    let app = Judge::says(false);
    let ollama = named_judge("Ollama", "ollama", true, true);
    let openai = named_judge("OpenAI", "openai", false, true);
    let providers: [&dyn VisionJudge; 2] = [&openai, &ollama];
    let run = |p: &Project| {
        run_with_providers(p, &hands, Some(&app), None, &providers, None, false).report
    };
    let asked = |j: &Judge| {
        j.asked
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.model.clone())
            .collect::<Vec<_>>()
    };

    // The block says nothing: the project's provider and model, not the app's.
    meaning_check(&p, "");
    let report = run(&p);
    assert_eq!(report.verdict, Verdict::Passed);
    let answer = report.attempts[0].steps[0].vision.clone().unwrap();
    assert_eq!(
        (answer.provider.as_str(), answer.model.as_str()),
        ("Ollama", "llava-project")
    );
    assert_eq!(asked(&ollama), ["llava-project"]);
    assert!(app.asked.lock().unwrap().is_empty());

    // A model of the block's own goes to the provider in force.
    meaning_check(&p, r#","model":"llava-block""#);
    assert_eq!(run(&p).verdict, Verdict::Passed);
    assert_eq!(asked(&ollama), ["llava-project", "llava-block"]);

    // A provider of the block's own beats the project, and takes none of its model.
    meaning_check(&p, r#","provider":"openai""#);
    assert_eq!(run(&p).verdict, Verdict::Passed);
    assert_eq!(asked(&openai), [""]);
    assert_eq!(asked(&ollama).len(), 2);

    // The project's choice holds for a project without one: the app's.
    let plain = Project::new();
    meaning_check(&plain, "");
    assert_eq!(run(&plain).verdict, Verdict::Failed);
    assert_eq!(app.asked.lock().unwrap().len(), 1);

    // Only models on this Mac holds for the project's choice too.
    meaning_check(&p, "");
    let report = run_with_providers(&p, &hands, Some(&app), None, &providers, None, true).report;
    assert_eq!(report.verdict, Verdict::Passed);
    p.write(
        "jevision.project.json",
        r#"{"format":1,"ai":{"screen":{"provider":"openai"}}}"#,
    );
    let report = run_with_providers(&p, &hands, Some(&app), None, &providers, None, true).report;
    assert_eq!(failure_code(&report), "run.vision.cloudBlocked");

    // The project may name a provider that isn't set up on this Mac: it fails naming it.
    p.write(
        "jevision.project.json",
        r#"{"format":1,"ai":{"screen":{"provider":"gemini"}}}"#,
    );
    let report = run(&p);
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.vision.noKey");
    assert_eq!(failure.param("provider"), "Google Gemini");
}

#[test]
fn a_fallback_that_is_the_provider_in_force_is_not_asked_again() {
    let p = Project::new();
    p.write(
        "jevision.project.json",
        r#"{"format":1,"ai":{"screen":{"provider":"ollama"}}}"#,
    );
    meaning_check(&p, "");
    let hands = Fake::new(solid([0, 200, 0]));
    let down = || {
        Err(VisionError::Unreachable {
            address: "http://127.0.0.1:11434".into(),
        })
    };
    let ollama = Judge {
        name: "Ollama",
        id: "ollama",
        local: true,
        ..Judge::new(true, down())
    };
    // The app's fallback is the project's provider: asking it again can't help.
    let fallback = Judge {
        name: "Ollama",
        id: "ollama",
        local: true,
        ..Judge::says(true)
    };
    let providers: [&dyn VisionJudge; 1] = [&ollama];
    let app = Judge::says(true);
    let report = run_with_providers(
        &p,
        &hands,
        Some(&app),
        Some(&fallback),
        &providers,
        None,
        false,
    )
    .report;
    assert_eq!(failure_code(&report), "run.vision.unreachable");
    assert_eq!(ollama.asked.lock().unwrap().len(), 1);
    assert!(fallback.asked.lock().unwrap().is_empty());

    // Another provider does answer in its place.
    let other = named_judge("OpenAI", "openai", false, true);
    let report = run_with_providers(
        &p,
        &hands,
        Some(&app),
        Some(&other),
        &providers,
        None,
        false,
    )
    .report;
    assert_eq!(report.verdict, Verdict::Passed);
    let answer = report.attempts[0].steps[0].vision.clone().unwrap();
    assert_eq!(answer.provider, "OpenAI");
    assert_eq!(
        answer.fallback_reason.unwrap().code,
        "run.vision.unreachable"
    );
}

#[test]
fn the_fallback_provider_answers_when_the_main_one_is_down_or_out_of_money() {
    let p = Project::new();
    meaning_check(&p, r#","model":"seer-2""#);
    let hands = Fake::new(solid([0, 200, 0]));
    let down = Judge::new(
        false,
        Err(VisionError::Unreachable {
            address: "http://127.0.0.1:9".into(),
        }),
    );
    let backup = Judge {
        name: "Backup",
        id: "backup",
        ..Judge::says(true)
    };
    let report = run_ai(&p, &hands, Some(&down), Some(&backup), None, false).report;
    assert_eq!(report.verdict, Verdict::Passed);
    let answer = report.attempts[0].steps[0].vision.clone().unwrap();
    assert_eq!(
        (answer.provider.as_str(), answer.model.as_str()),
        ("Backup", "seer-1")
    );
    assert_eq!(
        answer.fallback_reason.unwrap().code,
        "run.vision.unreachable"
    );
    // The block's model is asked of the main provider; the fallback uses its own.
    assert_eq!(down.asked.lock().unwrap()[0].model, "seer-2");
    assert_eq!(backup.asked.lock().unwrap()[0].model, "");

    // A main provider over its limit isn't asked at all.
    let main = Judge::says(false);
    let book = usage_book(&p, Some(0.0));
    let report = run_ai(&p, &hands, Some(&main), Some(&backup), Some(&book), false).report;
    assert_eq!(report.verdict, Verdict::Passed);
    let answer = report.attempts[0].steps[0].vision.clone().unwrap();
    assert_eq!(
        answer.fallback_reason.unwrap().code,
        "run.vision.limitReached"
    );
    assert!(main.asked.lock().unwrap().is_empty());
    assert_eq!(book.records().unwrap()[0].provider, "backup");

    // A setup mistake is shown, not covered up by the fallback.
    let wrong = Judge::new(false, Err(VisionError::BadKey));
    let report = run_ai(&p, &hands, Some(&wrong), Some(&backup), None, false).report;
    assert_eq!(failure_code(&report), "run.vision.badKey");

    // Only models on this Mac: a fallback in the cloud is not used.
    let local_down = Judge {
        local: true,
        ..Judge::new(false, Err(VisionError::Timeout { seconds: 120 }))
    };
    let report = run_ai(&p, &hands, Some(&local_down), Some(&backup), None, true).report;
    assert_eq!(failure_code(&report), "run.vision.timeout");
}

fn last_steps(report: &RunReport) -> &[jevision_runner::Step] {
    &report.attempts.last().unwrap().steps
}

#[test]
fn scripts_and_commands_hand_values_to_later_blocks() {
    let p = Project::new();
    p.test(
        r#""calc":{"type":"script","language":"sh","code":"echo \"count=$((2 + 1))\" >> \"$JEV_OUTPUT\"\nprintf 'who=%s\\nnote<<END\\nl1\\nl2\\nEND\\n' \"$WHO\" >> \"$JEV_OUTPUT\"","inputs":[{"name":"WHO","value":"${secret.ACME_PASSWORD}"}],"outputs":[{"name":"count","type":"number"},{"name":"who","type":"text"},{"name":"note","type":"text"}],"next":"echo"},
        "echo":{"type":"command","command":"echo ${var.count}; echo oops >&2; exit 3","exitCode":3,"saveOutput":"out","next":"branch"},
        "branch":{"type":"if","value":{"of":"${var.out}","is":"greaterThan","to":"2"},"then":"yes","else":"no"},
        "yes":{"type":"type","text":"${var.out} ${var.note}"},
        "no":{"type":"type","text":"no"}"#,
        "calc",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());
    assert_eq!(run.report.verdict, Verdict::Passed, "{:?}", run.report);
    assert_eq!(hands.typed(), ["3 l1\nl2"]);
    let steps = last_steps(&run.report);
    assert_eq!(steps[0].outputs["count"], "3");
    assert_eq!(steps[0].outputs["who"], "••••");
    assert_eq!(steps[1].outputs["out"], "3");
    assert_eq!(
        steps[2].branch_text().as_deref(),
        Some("Took “Yes: ${var.out} > 2”")
    );
    let all = outputs(&run, &events);
    assert!(!all.contains(SECRET));
    assert!(html(&run.report, &run.dir, None).contains("count = 3"));
    assert!(junit_xml(std::slice::from_ref(&run.report)).contains("calc: count = 3"));
}

#[test]
fn scripts_and_commands_fail_with_their_reason() {
    let cases = [
        (
            r#""a":{"type":"command","command":"echo broken >&2; exit 2"}"#,
            "run.command.exitCode",
            ("reason", "broken"),
        ),
        (
            r#""a":{"type":"script","language":"zsh","code":"exit 0","outputs":[{"name":"x","type":"text"}]}"#,
            "run.script.missingOutput",
            ("name", "x"),
        ),
        (
            r#""a":{"type":"script","language":"sh","code":"echo x=many >> $JEV_OUTPUT","outputs":[{"name":"x","type":"number"}]}"#,
            "run.script.badOutput",
            ("value", "many"),
        ),
        (
            r#""a":{"type":"script","language":"sh","code":"echo pic=gone.png >> $JEV_OUTPUT","outputs":[{"name":"pic","type":"image"}]}"#,
            "run.script.badOutput",
            ("type", "image"),
        ),
        (
            r#""a":{"type":"script","language":"sh","code":"echo box=2,2,3,3 >> $JEV_OUTPUT","outputs":[{"name":"box","type":"area"}]}"#,
            "run.script.badOutput",
            ("type", "area"),
        ),
        (
            r#""a":{"type":"script","language":"sh","code":"exit 0","inputs":[{"name":"N","type":"number","value":"${secret.ACME_PASSWORD}"}]}"#,
            "run.script.badInput",
            ("value", "••••"),
        ),
        (
            r#""a":{"type":"script","language":"sh","code":"exit 0","inputs":[{"name":"PIC","type":"image","value":"${secret.ACME_PASSWORD}"}]}"#,
            "run.script.badInput",
            ("type", "image"),
        ),
        (
            r#""a":{"type":"script","language":"sh","code":"echo nope >&2; exit 4"}"#,
            "run.script.failed",
            ("code", "4"),
        ),
        (
            r#""a":{"type":"clipboard","action":"check","is":"equals","to":"done","timeout":"100ms"}"#,
            "run.clipboard.mismatch",
            ("expected", "done"),
        ),
        (
            r#""a":{"type":"command","command":"sleep 5","timeout":"200ms"}"#,
            "run.process.timeout",
            ("seconds", "0.2"),
        ),
        (
            r#""a":{"type":"type","text":"${var.never}"}"#,
            "run.var.missing",
            ("name", "never"),
        ),
        (
            r#""a":{"type":"if","value":{"of":"1","is":"lessThan","to":"${secret.ACME_PASSWORD}"},"then":"b"},"b":{"type":"pressKeys","keys":"cmd+s"}"#,
            "run.value.notNumber",
            ("value", "••••"),
        ),
    ];
    for (blocks, code, (param, value)) in cases {
        let p = Project::new();
        p.test(blocks, "a", 0);
        let clock = Instant::now();
        let report = run(&p, &Fake::new(solid([0, 0, 0]))).report;
        let failure = report.attempts[0].failure.as_ref().unwrap();
        assert_eq!(failure.code, code, "{blocks} {failure:?}");
        assert_eq!(failure.param(param), value, "{blocks}");
        assert!(!describe(failure).is_empty());
        assert!(clock.elapsed() < Duration::from_secs(3), "{blocks}");
    }
}

#[test]
fn clicks_and_drags_hold_their_keys_and_button() {
    let p = Project::new();
    p.test(
        r#""one":{"type":"click","target":{"point":{"x":0.5,"y":0.5}},"modifiers":["cmd","shift"],"next":"two"},
           "two":{"type":"click","target":{"point":{"x":0.5,"y":0.5}},"button":"right","double":true,"next":"three"},
           "three":{"type":"drag","from":{"point":{"x":0,"y":0}},"to":{"point":{"x":1,"y":0}},"button":"right","modifiers":["option","ctrl"],"next":"four"},
           "four":{"type":"drag","from":{"point":{"x":0,"y":0}},"to":{"point":{"x":1,"y":0}}}"#,
        "one",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);
    let log = hands.log();
    assert_eq!(log[0], "click Left 10 5 holding cmd+shift");
    assert_eq!(log[1], "click Right 10 5");
    assert!(
        log[2].ends_with(", 20 0 with Right holding option+ctrl"),
        "{log:?}"
    );
    assert!(log[3].ends_with(", 20 0"), "{log:?}");
}

#[test]
fn the_clipboard_takes_text_gives_it_back_and_hides_secrets() {
    let p = Project::new();
    p.test(
        r#""put":{"type":"clipboard","action":"put","text":"code ${secret.ACME_PASSWORD}","next":"read"},
           "read":{"type":"clipboard","action":"read","saveAs":"copied","next":"check"},
           "check":{"type":"clipboard","action":"check","is":"contains","to":"code","next":"paste"},
           "paste":{"type":"type","text":"${var.copied}"}"#,
        "put",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let (run, events) = run_with(&p, &hands, RunControl::new(), &options());
    assert_eq!(run.report.verdict, Verdict::Passed, "{:?}", run.report);
    assert_eq!(hands.clipboard_text(), format!("code {SECRET}"));
    assert_eq!(hands.typed(), [format!("code {SECRET}")]);
    assert_eq!(last_steps(&run.report)[1].outputs["copied"], "code ••••");
    assert!(!outputs(&run, &events).contains(SECRET));
}

#[test]
fn a_clipboard_check_waits_for_another_application_to_fill_it() {
    let p = Project::new();
    p.test(
        r#""wait":{"type":"clipboard","action":"check","is":"equals","to":"done","timeout":"5s"}"#,
        "wait",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| {
        s.clipboard = "old".to_owned();
        s.after_look = Some("done".to_owned());
    });
    assert_eq!(run(&p, &hands).report.verdict, Verdict::Passed);

    // What the clipboard holds and what is waited for may both be secret.
    p.test(
        r#""wait":{"type":"clipboard","action":"check","is":"equals","to":"${secret.ACME_PASSWORD}","timeout":"100ms"}"#,
        "wait",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.clipboard = format!("old {SECRET}"));
    let report = run(&p, &hands).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.clipboard.mismatch");
    assert_eq!(failure.param("actual"), "old ••••");
    assert_eq!(failure.param("test"), "equals");
    let text = describe(&failure);
    assert!(
        text.contains("“old ••••”") && text.contains("equal to “••••”"),
        "{text}"
    );

    // A number is waited for until the clipboard holds one; a number the other side lacks fails.
    p.test(
        r#""wait":{"type":"clipboard","action":"check","is":"greaterThan","to":"${secret.ACME_PASSWORD}","timeout":"100ms"}"#,
        "wait",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0])).with(|s| s.clipboard = "5".to_owned());
    let failure = run(&p, &hands).report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.value.notNumber");
    assert_eq!(failure.param("value"), "••••");
}

#[test]
fn the_date_and_time_are_saved_as_text_and_can_be_moved() {
    let p = Project::new();
    p.test(
        r#""today":{"type":"dateTime","saveAs":"today","format":"%F","next":"later"},
           "later":{"type":"dateTime","saveAs":"later","format":"%F","shift":{"amount":2,"unit":"weeks"},"next":"stamp"},
           "stamp":{"type":"dateTime","saveAs":"stamp","next":"show"},
           "show":{"type":"type","text":"${var.today}|${var.later}|${var.stamp}"}"#,
        "today",
        0,
    );
    let day = |text: &str| chrono::NaiveDate::parse_from_str(text, "%F").unwrap();
    let before = chrono::Local::now().date_naive();
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run(&p, &hands).report;
    let after = chrono::Local::now().date_naive();
    assert_eq!(report.verdict, Verdict::Passed, "{report:?}");
    let typed = hands.typed();
    let parts: Vec<&str> = typed[0].split('|').collect();
    assert!((before..=after).contains(&day(parts[0])));
    let later = day(parts[1]);
    assert!(
        [before, after]
            .iter()
            .any(|d| *d + chrono::Days::new(14) == later)
    );
    assert!(
        chrono::NaiveDateTime::parse_from_str(parts[2], "%Y-%m-%d %H:%M:%S").is_ok(),
        "{}",
        parts[2]
    );
}

#[test]
fn script_values_keep_their_types_between_blocks() {
    let p = Project::new();
    p.write("refs/pic.png", solid([1, 2, 3])).test(
        r#""calc":{"type":"script","language":"sh","code":"printf 'pic=%s\nbox=%s\nflag=%s\nn=%s\n' \"$PIC\" \"$BOX\" \"$FLAG\" \"$N\" >> \"$JEV_OUTPUT\"",
             "inputs":[{"name":"PIC","type":"image","value":"refs/pic.png"},{"name":"BOX","type":"area","value":"0.1, 0.2, 0.5, 0.5"},{"name":"FLAG","type":"boolean","value":"yes"},{"name":"N","type":"number","value":"2.5"}],
             "outputs":[{"name":"pic","type":"image"},{"name":"box","type":"area"},{"name":"flag","type":"boolean"},{"name":"n","type":"number"}],"next":"show"},
           "show":{"type":"type","text":"${var.box} ${var.flag} ${var.n}"}"#,
        "calc",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run(&p, &hands).report;
    assert_eq!(report.verdict, Verdict::Passed, "{report:?}");
    // An area is kept in one form, a yes/no answer as true or false, a picture as its place.
    assert_eq!(hands.typed(), ["0.1,0.2,0.5,0.5 true 2.5"]);
    let pic = &last_steps(&report)[0].outputs["pic"];
    assert!(
        pic.ends_with("refs/pic.png") && Path::new(pic).is_absolute(),
        "{pic}"
    );
}

/// Answers one request on 127.0.0.1 and hands back what was sent.
fn mock_http(answer: &'static str) -> (String, mpsc::Receiver<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            request.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if body.len() >= length || n == 0 {
                    tx.send(text).unwrap();
                    break;
                }
            }
        }
        stream.write_all(answer.as_bytes()).unwrap();
    });
    (address, rx)
}

#[test]
fn http_requests_send_and_save_parts_of_the_answer() {
    let body = r#"{"items":[{"id":42}]}"#;
    let answer: &'static str = Box::leak(
        format!(
            "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nX-Request-Id: r-7\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_boxed_str(),
    );
    let (address, sent) = mock_http(answer);
    let p = Project::new();
    p.test(
        &format!(
            r#""ask":{{"type":"http","method":"POST","url":"{address}/items","headers":[{{"name":"Authorization","value":"Bearer ${{secret.ACME_PASSWORD}}"}}],"body":"{{\"n\": 1}}","expectStatus":201,"save":[{{"name":"id","from":"body.items[0].id"}},{{"name":"rid","from":"header.x-request-id"}},{{"name":"code","from":"status"}}],"next":"show"}},
            "show":{{"type":"type","text":"${{var.id}} ${{var.rid}} ${{var.code}}"}}"#
        ),
        "ask",
        0,
    );
    let hands = Fake::new(solid([0, 0, 0]));
    let report = run(&p, &hands).report;
    assert_eq!(report.verdict, Verdict::Passed, "{report:?}");
    assert_eq!(hands.typed(), ["42 r-7 201"]);
    let request = sent.recv().unwrap();
    assert!(request.starts_with("POST /items HTTP/1.1"), "{request}");
    assert!(
        request.contains(&format!("authorization: Bearer {SECRET}"))
            || request.contains(&format!("Authorization: Bearer {SECRET}")),
        "{request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    assert!(request.ends_with(r#"{"n": 1}"#));

    let (address, _sent) =
        mock_http("HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngone");
    p.test(
        &format!(r#""ask":{{"type":"http","url":"{address}/x"}}"#),
        "ask",
        0,
    );
    let report = run(&p, &Fake::new(solid([0, 0, 0]))).report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.http.status");
    assert_eq!(failure.param("status"), "404");
    assert_eq!(failure.param("body"), "gone");
}

#[test]
fn files_and_databases_are_waited_for_and_read() {
    let p = Project::new();
    p.write("app.sqlite", "");
    p.test(
        r#""late":{"type":"command","command":"(sleep 0.2; echo done > out.txt) &","next":"appear"},
        "appear":{"type":"checkFile","path":"out.txt","until":{"contains":"done"},"timeout":"5s","next":"changed"},
        "changed":{"type":"checkFile","path":"out.txt","until":"changed","next":"gone"},
        "gone":{"type":"checkFile","path":"nope.txt","until":"missing","next":"make"},
        "make":{"type":"database","file":"app.sqlite","query":"create table items(name text)","next":"fill"},
        "fill":{"type":"database","file":"app.sqlite","query":"insert into items values ('b'), ('a')","next":"read"},
        "read":{"type":"database","file":"app.sqlite","query":"select name from items order by name","saveAs":"first","expect":{"rows":2},"next":"wrong"},
        "wrong":{"type":"database","file":"app.sqlite","query":"select count(*) from items","expect":{"value":"3"},"timeout":"50ms"}"#,
        "late",
        0,
    );
    let report = run(&p, &Fake::new(solid([0, 0, 0]))).report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.database.value", "{report:?}");
    assert_eq!(failure.param("actual"), "2");
    let steps = last_steps(&report);
    assert_eq!(steps.len(), 8);
    assert_eq!(steps[6].outputs["first"], "a");

    p.test(
        r#""wait":{"type":"checkFile","path":"~/jevision-surely-not-here.txt","until":"exists","timeout":"50ms"}"#,
        "wait",
        0,
    );
    let report = run(&p, &Fake::new(solid([0, 0, 0]))).report;
    let failure = report.attempts[0].failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.file.timeout");
    assert_eq!(failure.param("state"), "exists");
    assert!(!failure.param("path").starts_with('~'));
}

#[test]
fn a_blocked_test_is_saved_with_its_reason_and_skipped_in_junit() {
    let p = Project::new();
    p.test(r#""a":{"type":"pressKeys","keys":"cmd+n"}"#, "a", 0);
    let blocker = jevision_core::Blocker {
        dependency: "login.flow.json".into(),
        cause: "login.flow.json".into(),
        cause_name: "Login".into(),
        reason: jevision_core::BlockReason::Failed,
    };
    let options = RunOptions {
        added_as_dependency: true,
        ..RunOptions::default()
    };
    let run = Run::blocked(&p.0, Path::new("t.flow.json"), blocker, &options).unwrap();
    let saved = Run::load(&run.dir).unwrap().report;
    assert_eq!(saved, run.report);
    assert_eq!(saved.verdict, Verdict::Blocked);
    assert_eq!((saved.test.as_str(), saved.app.as_str()), ("T", APP));
    assert!(saved.attempts.is_empty() && saved.added_as_dependency);
    assert_eq!(Run::list(&p.0).unwrap().len(), 1, "history lists it");

    let xml = junit_xml(std::slice::from_ref(&saved));
    assert_well_formed_xml(&xml);
    assert!(
        xml.contains(r#"failures="0" errors="0" skipped="1""#),
        "{xml}"
    );
    assert!(
        xml.contains(r#"<skipped message="Blocked: Login failed."/>"#),
        "{xml}"
    );
    let page = html(&saved, &run.dir, None);
    assert!(page.contains("Blocked: Login failed. The test didn’t run"));
    assert!(page.contains("Added to the run because other tests depend on it."));
}

/// Finds every described element in one place, or nowhere, and keeps what it was asked.
struct Locator {
    found: Option<jevision_core::Rect>,
    asked: Mutex<Vec<(String, Vec<u8>)>>,
}

impl Locator {
    fn new(found: Option<jevision_core::Rect>) -> Self {
        Self {
            found,
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl VisionJudge for Locator {
    fn provider(&self) -> &str {
        "Test Finder"
    }
    fn provider_id(&self) -> &str {
        "test-finder"
    }
    fn model(&self) -> &str {
        "finder-1"
    }
    fn is_local(&self) -> bool {
        false
    }
    fn judge(&self, _: &Question<'_>) -> Result<Judgement, VisionError> {
        unreachable!("a locator only finds")
    }
    fn locate(&self, screenshot: &MaskedImage, what: &str) -> Result<Search, VisionError> {
        self.asked
            .lock()
            .unwrap()
            .push((what.to_owned(), screenshot.png().to_vec()));
        Ok(Search {
            found: self.found.map(|area| Located {
                area,
                confidence: 0.9,
            }),
            usage: Usage {
                input_tokens: 3000,
                output_tokens: 10,
            },
        })
    }
}

fn run_located(project: &Project, hands: &Fake, locator: Option<&dyn VisionJudge>) -> Run {
    run_located_logged(project, hands, locator, None)
}

fn run_located_logged(
    project: &Project,
    hands: &Fake,
    locator: Option<&dyn VisionJudge>,
    usage: Option<&UsageBook>,
) -> Run {
    let secrets = secrets();
    let runner = Runner {
        hands,
        secrets: &secrets,
        events: &|_: &Event| {},
        control: RunControl::new(),
        vision: None,
        fallback: None,
        providers: &[],
        usage,
        locator,
    };
    runner
        .run(&project.0, Path::new("t.flow.json"), &options())
        .unwrap()
}

const DESCRIBED: &str = r#""save":{"type":"click","target":{"description":"the blue Save button"},"next":"name"},
    "name":{"type":"type","text":"Ann","into":{"description":"the name field"}}"#;

#[test]
fn a_described_element_is_clicked_in_the_middle_of_where_the_model_found_it() {
    let p = Project::new();
    p.write(
        "jevision.project.json",
        r#"{"format":1,"hide":[{"x":0,"y":0,"width":0.25,"height":1}]}"#,
    )
    .test(DESCRIBED, "save", 0);
    let hands = Fake::new(png(40, 20, |_, _| [0, 200, 0]));
    let locator = Locator::new(Some(jevision_core::Rect {
        x: 0.5,
        y: 0.25,
        width: 0.2,
        height: 0.5,
    }));
    let run = run_located(&p, &hands, Some(&locator));
    let report = &run.report;
    assert_eq!(report.verdict, Verdict::Passed);
    // The middle of the box, (0.6, 0.5) of a 40×20 window shot at scale 2, in points.
    assert_eq!(hands.log(), ["click Left 12 5", "click Left 12 5", "type"]);
    let asked = locator.asked.lock().unwrap();
    let what: Vec<&str> = asked.iter().map(|(what, _)| what.as_str()).collect();
    assert_eq!(what, ["the blue Save button", "the name field"]);
    let (_, shot) = &asked[0];
    assert_eq!(size(shot), (40, 20));
    assert_eq!(pixel(shot, 2, 10), [128, 128, 128]);
    assert_eq!(pixel(shot, 20, 10), [0, 200, 0]);
    let step = &report.attempts[0].steps[0];
    assert!(step.sent.is_some() && step.sent_masked);
    assert_eq!(step.found, [locator.found.unwrap()]);
    // The report outlines the box on the picture sent: 40×20 pixels.
    assert!(
        html(report, &run.dir, None)
            .contains(r#"<rect class="found" x="20.0" y="5.0" width="8.0" height="10.0"/>"#)
    );
}

#[test]
fn hover_drag_and_scroll_find_described_elements_too() {
    let p = Project::new();
    p.test(
        r#""tip":{"type":"hover","target":{"description":"the help button"},"next":"move"},
           "move":{"type":"drag","from":{"description":"the report file"},"to":{"point":{"x":0.5,"y":0.85}},"next":"down"},
           "down":{"type":"scroll","direction":"down","amount":3,"at":{"description":"the message list"}}"#,
        "tip",
        0,
    );
    let hands = Fake::new(png(40, 20, |_, _| [0, 200, 0]));
    let locator = Locator::new(Some(jevision_core::Rect {
        x: 0.5,
        y: 0.25,
        width: 0.2,
        height: 0.5,
    }));
    let report = run_located(&p, &hands, Some(&locator)).report;
    assert_eq!(report.verdict, Verdict::Passed);
    let log = hands.log();
    assert_eq!(log[0], "hover 12 5");
    // From the found element to the point (0.5, 0.85) of a 20×10 point window.
    assert!(
        log[1].starts_with("drag 12 5, ") && log[1].ends_with(", 10 8.5"),
        "{log:?}"
    );
    assert_eq!(log[2], "scroll Down 3 Lines at 12 5");
    let asked = locator.asked.lock().unwrap();
    let what: Vec<&str> = asked.iter().map(|(what, _)| what.as_str()).collect();
    assert_eq!(
        what,
        ["the help button", "the report file", "the message list"]
    );
    assert!(report.attempts[0].steps.iter().all(|s| s.found.len() == 1));
}

#[test]
fn a_described_element_that_is_not_on_screen_fails_with_its_description() {
    let p = Project::new();
    p.test(DESCRIBED, "save", 0);
    let hands = Fake::new(solid([9, 9, 9]));
    let report = run_located(&p, &hands, Some(&Locator::new(None))).report;
    assert_eq!(failure_code(&report), "run.target.descriptionNotFound");
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.param("description"), "the blue Save button");
    assert_eq!(
        describe(&failure),
        "Block save: “the blue Save button” was not found on screen."
    );
    assert!(report.attempts[0].steps[0].before.is_some());
    assert!(hands.log().is_empty());
}

#[test]
fn finding_by_description_is_logged_priced_and_held_to_the_limits() {
    let p = Project::new();
    p.test(DESCRIBED, "save", 0);
    let hands = Fake::new(png(40, 20, |_, _| [0, 200, 0]));
    let locator = Locator::new(Some(jevision_core::Rect {
        x: 0.5,
        y: 0.25,
        width: 0.2,
        height: 0.5,
    }));
    // Each search costs $0.0031 of $0.005 a day.
    let book = UsageBook {
        journal: p.0.join("ai-usage.jsonl"),
        prices: vec![Price {
            provider: "test-finder".into(),
            model: "finder".into(),
            input: 1.0,
            output: 10.0,
        }],
        limits: vec![Limit {
            provider: "test-finder".into(),
            daily: Some(0.005),
            monthly: None,
        }],
    };

    // The click and the typing each search for their element: two entries in the journal.
    let report = run_located_logged(&p, &hands, Some(&locator), Some(&book)).report;
    assert_eq!(report.verdict, Verdict::Passed);
    let records = book.records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        (
            records[0].provider.as_str(),
            records[0].model.as_str(),
            records[0].test.as_str(),
            records[0].block.as_str(),
            records[0].input_tokens,
            records[0].output_tokens
        ),
        ("test-finder", "finder-1", "t.flow.json", "save", 3000, 10)
    );
    assert_eq!(records[1].block, "name");
    let warning = report.attempts[0].steps[1].vision.clone();
    assert!(warning.is_none(), "a search is not a check by meaning");

    // The limit is used up: the next search is not made, and the run says which limit.
    let asked = locator.asked.lock().unwrap().len();
    let report = run_located_logged(&p, &hands, Some(&locator), Some(&book)).report;
    assert_eq!(failure_code(&report), "run.vision.limitReached");
    let failure = report.attempts[0].steps[0].failure.clone().unwrap();
    assert_eq!(failure.param("block"), "save");
    assert_eq!(failure.param("period"), "day");
    assert_eq!(locator.asked.lock().unwrap().len(), asked);

    // A search that finds nothing cost tokens too.
    let q = Project::new();
    q.test(DESCRIBED, "save", 0);
    let empty = UsageBook {
        journal: q.0.join("ai-usage.jsonl"),
        ..book
    };
    let nothing = Locator::new(None);
    let report = run_located_logged(&q, &hands, Some(&nothing), Some(&empty)).report;
    assert_eq!(failure_code(&report), "run.target.descriptionNotFound");
    assert_eq!(empty.records().unwrap().len(), 1);
}

#[test]
fn finding_by_description_needs_jev_and_a_cloud_service_allowed() {
    let p = Project::new();
    p.test(DESCRIBED, "save", 0);
    let hands = Fake::new(solid([9, 9, 9]));
    let report = run_located(&p, &hands, None).report;
    assert_eq!(failure_code(&report), "run.target.noLocator");
    assert!(describe(&report.attempts[0].failure.clone().unwrap()).contains("Settings → AI"));

    // A provider other than JEV can't find elements.
    let other = Judge::says(true);
    let report = run_located(&p, &hands, Some(&other)).report;
    let failure = report.attempts[0].failure.clone().unwrap();
    assert_eq!(failure.code, "run.vision.cannotLocate");
    assert_eq!(failure.param("provider"), "Test Vision");

    let secrets = secrets();
    let locator = Locator::new(None);
    let runner = Runner {
        hands: &hands,
        secrets: &secrets,
        events: &|_: &Event| {},
        control: RunControl::new(),
        vision: None,
        fallback: None,
        providers: &[],
        usage: None,
        locator: Some(&locator),
    };
    let local_only = RunOptions {
        local_only: true,
        ..options()
    };
    let report = runner
        .run(&p.0, Path::new("t.flow.json"), &local_only)
        .unwrap()
        .report;
    assert_eq!(failure_code(&report), "run.vision.cloudBlocked");
    assert!(locator.asked.lock().unwrap().is_empty());
}
