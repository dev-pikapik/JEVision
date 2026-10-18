use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use jevision_runner::{
    Event, Hands, HandsError, Judgement, MouseButton, Openable, PauseReason, Point, Question, Run,
    RunControl, RunOptions, RunReport, Runner, Screenshot, StepStatus, VIDEO_FILE, Verdict,
    VideoMode, VisionAnswer, VisionError, VisionJudge, html, junit_xml, prune_videos,
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
    log: Vec<String>,
    typed: Vec<String>,
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

    fn note(&self, entry: String) {
        self.state.lock().unwrap().log.push(entry);
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
    fn click(&self, _: &str, at: Point, button: MouseButton, _: bool) -> Result<(), HandsError> {
        let mut s = self.state.lock().unwrap();
        s.log.push(format!("click {:?} {} {}", button, at.x, at.y));
        if let Some(screen) = s.after_click.take() {
            s.screen = screen;
        }
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
    fn user_input_detected(&self) -> bool {
        std::mem::take(&mut self.state.lock().unwrap().user_input)
    }
    fn start_video(&self, app: &str, path: &Path) -> Result<(), HandsError> {
        let mut s = self.state.lock().unwrap();
        if s.no_video {
            return Err(HandsError::PermissionDenied("screenRecording".to_owned()));
        }
        s.log.push(format!("video {app}"));
        s.video = Some(path.to_path_buf());
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
    assert_eq!(failure_code(&run(&p, &hands).report), "run.check.mismatch");

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
        data_row: Some(1),
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
    for (environment, data_row, code) in [
        (None, None, "run.env.notChosen"),
        (Some("empty"), None, "run.env.missing"),
        (Some("prod"), None, "run.env.unknown"),
        (Some("staging"), Some(2), "run.data.noRow"),
    ] {
        let hands = Fake::new(solid([0, 0, 0]));
        let options = RunOptions {
            environment: environment.map(str::to_owned),
            data_row,
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

/// A vision model that gives a fixed answer and keeps what it was shown.
struct Judge {
    local: bool,
    answer: Result<Judgement, VisionError>,
    asked: Mutex<Vec<Asked>>,
}

struct Asked {
    expect: String,
    screenshot: Vec<u8>,
    reference: Option<Vec<u8>>,
}

impl Judge {
    fn new(local: bool, answer: Result<Judgement, VisionError>) -> Self {
        Self {
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
            }),
        )
    }
}

impl VisionJudge for Judge {
    fn provider(&self) -> &str {
        "Test Vision"
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
            screenshot: q.screenshot.to_vec(),
            reference: q.reference.map(<[u8]>::to_vec),
        });
        self.answer.clone()
    }
}

fn run_judged(project: &Project, hands: &Fake, judge: Option<&Judge>, local_only: bool) -> Run {
    let secrets = secrets();
    let runner = Runner {
        hands,
        secrets: &secrets,
        events: &|_: &Event| {},
        control: RunControl::new(),
        vision: judge.map(|j| j as &dyn VisionJudge),
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
        })
    );
    assert_eq!(step.similarity, None);
    assert!(step.diff.is_none() && step.expected.is_some());

    let asked = judge.asked.lock().unwrap();
    let Asked {
        expect,
        screenshot: shot,
        reference,
    } = &asked[0];
    assert_eq!(expect, "The cart shows 2 items");
    assert_eq!(size(shot), (20, 20));
    assert_eq!(size(reference.as_ref().unwrap()), (20, 20));
    // The checkerboard under the hidden part became one flat colour; the ignored stripe too.
    for y in 0..10 {
        for x in 0..10 {
            assert_eq!(pixel(shot, x, y), [127, 127, 127], "({x}, {y})");
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
    assert!(judge.asked.lock().unwrap()[0].reference.is_none());
    assert!(report.attempts[0].steps[0].expected.is_none());
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
