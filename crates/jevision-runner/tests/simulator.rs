//! Tests of apps in the iOS Simulator, with a simulator and a Simulator window that answer
//! as the real ones do.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

use jevision_runner::{
    AppStart, ElementNode, Event, Hands, HandsError, Modifiers, MouseButton, Openable, Point,
    RunControl, RunOptions, RunReport, Runner, SIMULATOR_APP, Screenshot, ScrollDirection,
    ScrollUnit, Simctl, Verdict, WindowPosition, WindowSize, describe_in,
};
use jevision_vision::image::Image;

const UDID: &str = "D1A2B3C4-0000-4000-8000-000000000001";
const APP: &str = "com.example.Mail";

/// The device screen: 195 × 422 points at 3 pixels a point.
const DEVICE: (u32, u32) = (585, 1266);
/// Where Simulator shows the screen in its window, in window pixels, at 2 pixels a point.
const SCREEN_AT: (u32, u32) = (30, 100);
const SCREEN_WIDTH: u32 = 390;

/// Coloured tiles, so the screen can be told from the window around it.
fn device_screen() -> Image {
    let (width, height) = DEVICE;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let tile = (x / 30).wrapping_mul(31) ^ (y / 30).wrapping_mul(17);
            let v = tile.wrapping_mul(2_654_435_761) >> 8;
            rgba.extend([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// The Simulator window: a title bar and a dark frame around the zoomed device screen.
fn window(screen: &Image) -> Image {
    let (x0, y0) = SCREEN_AT;
    let height = SCREEN_WIDTH * screen.height / screen.width;
    let (width, total) = (SCREEN_WIDTH + 2 * x0, height + y0 + 40);
    let mut rgba = Vec::with_capacity((width * total * 4) as usize);
    for y in 0..total {
        for _ in 0..width {
            rgba.extend(if y < 56 {
                [236, 236, 236, 255]
            } else {
                [28, 28, 30, 255]
            });
        }
    }
    let scaled = screen.resized(SCREEN_WIDTH, height);
    for row in 0..height {
        let to = (((y0 + row) * width + x0) * 4) as usize;
        let from = (row * SCREEN_WIDTH * 4) as usize;
        rgba[to..to + (SCREEN_WIDTH * 4) as usize]
            .copy_from_slice(&scaled.rgba[from..from + (SCREEN_WIDTH * 4) as usize]);
    }
    Image {
        width,
        height: total,
        rgba,
    }
}

static DEVICE_PNG: LazyLock<Vec<u8>> = LazyLock::new(|| device_screen().encode().unwrap());
static WINDOW_PNG: LazyLock<Vec<u8>> = LazyLock::new(|| window(&device_screen()).encode().unwrap());

/// A simulator with one iPhone and the apps installed on it; every command is logged.
struct Sim {
    profiles: PathBuf,
    state: Mutex<SimState>,
}

#[derive(Default)]
struct SimState {
    booted: bool,
    apps: Vec<String>,
    log: Vec<String>,
}

impl Sim {
    fn new(dir: &Path, apps: &[&str]) -> Self {
        let profiles = dir.join("iPhone 17.simdevicetype");
        let resources = profiles.join("Contents/Resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(
            resources.join("capabilities.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>capabilities</key><dict>
<key>ScreenDimensionsCapability</key><dict><key>main-screen-scale</key><integer>3</integer></dict>
</dict></dict></plist>"#,
        )
        .unwrap();
        Self {
            profiles,
            state: Mutex::new(SimState {
                apps: apps.iter().map(|a| (*a).to_owned()).collect(),
                ..SimState::default()
            }),
        }
    }

    fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }
}

impl Simctl for Sim {
    fn run(&self, args: &[&str], environment: &[(String, String)]) -> Result<Vec<u8>, String> {
        let mut s = self.state.lock().unwrap();
        let mut entry = args.join(" ");
        if !environment.is_empty() {
            entry.push_str(&format!(" {environment:?}"));
        }
        let answer = match args {
            ["list", "devices", ..] => format!(
                r#"{{"devices":{{"com.apple.CoreSimulator.SimRuntime.iOS-27-0":[{{"udid":"{UDID}","name":"iPhone 17","state":"{}","isAvailable":true,"deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-17"}}]}}}}"#,
                if s.booted { "Booted" } else { "Shutdown" }
            ),
            ["list", "devicetypes", ..] => format!(
                r#"{{"devicetypes":[{{"identifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-17","bundlePath":{:?}}}]}}"#,
                self.profiles.to_string_lossy()
            ),
            ["bootstatus", ..] => {
                s.booted = true;
                String::new()
            }
            ["listapps", _] => {
                let apps: String = s
                    .apps
                    .iter()
                    .map(|a| {
                        format!("\"{a}\" = {{ ApplicationType = User; CFBundleName = Mail; }};\n")
                    })
                    .collect();
                format!("{{\n{apps}}}")
            }
            ["install", _, path] => {
                s.apps.push(APP.to_owned());
                entry = format!(
                    "install {}",
                    Path::new(path).file_name().unwrap().to_string_lossy()
                );
                String::new()
            }
            ["uninstall", _, app] => {
                s.apps.retain(|a| a != app);
                String::new()
            }
            ["io", _, "screenshot", "--type=png", path] => {
                std::fs::write(path, &*DEVICE_PNG).unwrap();
                return Ok(Vec::new());
            }
            ["ui", _, "appearance"] => return Ok(b"dark\n".to_vec()),
            ["terminate" | "launch" | "openurl", ..] => String::new(),
            _ => return Err(format!("unexpected: {entry}")),
        };
        if !matches!(args, ["list", ..] | ["listapps", ..]) {
            s.log.push(entry);
        }
        Ok(answer.into_bytes())
    }
}

/// The Mac's hands with Simulator open; what they are asked to do is logged.
#[derive(Default)]
struct Mac {
    log: Mutex<Vec<String>>,
    /// Xcode came without Simulator, as when its Simulator component isn't added.
    no_simulator: bool,
}

impl Mac {
    fn note(&self, entry: String) {
        self.log.lock().unwrap().push(entry);
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Hands for Mac {
    fn open_app(
        &self,
        app: &str,
        _: AppStart,
        arguments: &[String],
        _: &HashMap<String, String>,
    ) -> Result<(), HandsError> {
        if self.no_simulator && app == jevision_runner::SIMULATOR_APP {
            return Err(HandsError::AppNotFound(app.to_owned()));
        }
        self.note(format!("open {app} {arguments:?}"));
        Ok(())
    }
    fn sandboxed(&self, _: &str) -> Result<bool, HandsError> {
        unreachable!("apps on a simulated device have no Mac sandbox")
    }
    fn arrange_window(
        &self,
        app: &str,
        _: Option<WindowSize>,
        _: Option<WindowPosition>,
    ) -> Result<(), HandsError> {
        self.note(format!("arrange {app}"));
        Ok(())
    }
    fn open(&self, _: &[Openable], _: Option<&str>, _: bool) -> Result<(), HandsError> {
        unreachable!("links open on the device")
    }
    fn close(&self, _: &[Openable], _: Option<&str>) -> Result<(), HandsError> {
        Ok(())
    }
    fn activate(&self, app: &str) -> Result<(), HandsError> {
        assert_eq!(app, SIMULATOR_APP);
        Ok(())
    }
    fn quit(&self, app: &str) -> Result<(), HandsError> {
        self.note(format!("quit {app}"));
        Ok(())
    }
    fn capture(&self, app: &str) -> Result<Screenshot, HandsError> {
        assert_eq!(app, SIMULATOR_APP);
        Ok(Screenshot {
            png: WINDOW_PNG.clone(),
            scale: 2.0,
            dark: false,
            secure: Vec::new(),
        })
    }
    fn elements(&self, _: &str) -> Result<Vec<ElementNode>, HandsError> {
        let node = |title: &str, y: f64| ElementNode {
            role: "AXButton".into(),
            subrole: String::new(),
            id: String::new(),
            title: title.into(),
            description: String::new(),
            value: String::new(),
            placeholder: String::new(),
            enabled: true,
            selected: false,
            frame: jevision_core::Rect {
                x: 0.4,
                y,
                width: 0.2,
                height: 0.02,
            },
        };
        // A button of Simulator's own above the screen, and the app's on it.
        Ok(vec![node("Send", 0.01), node("Send", 0.5)])
    }
    fn locate(&self, app: &str, template: &[u8], _: f64) -> Result<Option<Point>, HandsError> {
        let picture = Image::decode(template).unwrap();
        self.note(format!(
            "locate in {app} {}x{}",
            picture.width, picture.height
        ));
        // The centre of the window's screen, in window points.
        Ok(Some(Point {
            x: f64::from(SCREEN_AT.0 + SCREEN_WIDTH / 2) / 2.0,
            y: f64::from(SCREEN_AT.1 + 422) / 2.0,
        }))
    }
    fn click(
        &self,
        app: &str,
        at: Point,
        _: MouseButton,
        _: bool,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        self.note(format!("click {app} {} {}", at.x, at.y));
        Ok(())
    }
    fn hover(&self, _: &str, _: Point, _: Modifiers) -> Result<(), HandsError> {
        Ok(())
    }
    fn drag(&self, _: &str, _: &[Point], _: MouseButton, _: Modifiers) -> Result<(), HandsError> {
        Ok(())
    }
    fn scroll(
        &self,
        _: &str,
        _: Point,
        _: ScrollDirection,
        _: u32,
        _: ScrollUnit,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn type_text(&self, text: &str) -> Result<(), HandsError> {
        self.note(format!("type {text}"));
        Ok(())
    }
    fn press_keys(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn clipboard(&self) -> Result<String, HandsError> {
        Ok(String::new())
    }
    fn set_clipboard(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn clipboard_image(&self) -> Result<Option<Vec<u8>>, HandsError> {
        Ok(None)
    }
    fn clipboard_files(&self) -> Result<Vec<String>, HandsError> {
        Ok(Vec::new())
    }
    fn set_clipboard_image(&self, _: &[u8]) -> Result<(), HandsError> {
        Ok(())
    }
    fn user_input_detected(&self) -> bool {
        false
    }
    fn start_video(&self, _: &str, _: &Path, _: &[jevision_core::Rect]) -> Result<(), HandsError> {
        Ok(())
    }
    fn stop_video(&self) -> Result<(), HandsError> {
        Ok(())
    }
}

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-simulator-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, text: &str) -> &Self {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        self
    }

    /// A build of the app for the simulator, as Xcode leaves it.
    fn build(&self, bundle_id: &str) -> &Self {
        self.write(
            "build/Mail.app/Info.plist",
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string></dict></plist>"#
            ),
        )
    }

    fn run(&self, test: &str, sim: &Sim, mac: &Mac) -> RunReport {
        let secrets: HashMap<String, String> = HashMap::new();
        let events = |_: &Event| {};
        let runner = Runner {
            hands: mac,
            simctl: sim,
            secrets: &secrets,
            events: &events,
            control: RunControl::new(),
            vision: None,
            fallback: None,
            providers: &[],
            usage: None,
            locator: None,
        };
        let options = RunOptions {
            poll_interval: std::time::Duration::from_millis(5),
            ..RunOptions::default()
        };
        runner
            .run(&self.0, Path::new(test), &options)
            .unwrap()
            .report
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn reason(report: &RunReport) -> String {
    let failure = report.attempts.last().unwrap().failure.as_ref();
    failure.map(|f| describe_in("en", f)).unwrap_or_default()
}

const SIMULATOR: &str = r#""simulator":{"device":"iPhone 17","install":"build/Mail.app"}"#;

#[test]
fn a_test_installs_launches_and_clicks_its_app_on_the_simulated_device() {
    let p = Project::new();
    p.build(APP).write(
        "t.flow.json",
        &format!(
            r#"{{"format":3,"name":"Mail","app":"{APP}",{SIMULATOR},"start":"open","blocks":{{
              "open":{{"type":"openApp","resetState":true,"arguments":["-demo"],"environment":{{"MODE":"test"}},"next":"link"}},
              "link":{{"type":"openFile","files":["mail://inbox"],"next":"click"}},
              "click":{{"type":"click","target":{{"point":{{"x":0.5,"y":0.25}}}},"next":"send"}},
              "send":{{"type":"click","target":{{"element":{{"title":"Send"}}}},"next":"type"}},
              "type":{{"type":"type","text":"Hello"}}}}}}"#
        ),
    );
    let (sim, mac) = (Sim::new(&p.0, &[]), Mac::default());
    let report = p.run("t.flow.json", &sim, &mac);
    assert_eq!(report.verdict, Verdict::Passed, "{}", reason(&report));
    assert_eq!(
        sim.log(),
        [
            format!("bootstatus {UDID} -b"),
            format!("terminate {UDID} {APP}"),
            format!("uninstall {UDID} {APP}"),
            "install Mail.app".to_owned(),
            format!(
                r#"launch --terminate-running-process {UDID} {APP} -demo [("SIMCTL_CHILD_MODE", "test")]"#
            ),
            format!("openurl {UDID} mail://inbox"),
        ]
    );
    let mac_log = mac.log();
    assert_eq!(
        mac_log[0],
        format!(r#"open {SIMULATOR_APP} ["-CurrentDeviceUDID", "{UDID}"]"#)
    );
    let clicks: Vec<(f64, f64)> = mac_log
        .iter()
        .filter_map(|e| e.strip_prefix(&format!("click {SIMULATOR_APP} ")))
        .map(|at| {
            let (x, y) = at.split_once(' ').unwrap();
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect();
    // Half of the 195-point-wide screen is 97.5 points: 195 window pixels from where the screen
    // starts at 30, at 2 pixels a window point. A quarter of its 422 points down is 211 window
    // pixels below where it starts at 100. The Send button is where the window shows it.
    let expected = [(112.5, 155.5), (112.5, 0.51 * 984.0 / 2.0)];
    assert_eq!(clicks.len(), 2, "{mac_log:?}");
    for (got, want) in clicks.iter().zip(expected) {
        assert!(
            (got.0 - want.0).abs() < 1.0 && (got.1 - want.1).abs() < 1.0,
            "clicked {got:?}, not near {want:?}"
        );
    }
    assert_eq!(mac_log.last().unwrap(), "type Hello");
}

#[test]
fn a_picture_is_looked_for_at_the_zoom_of_the_window() {
    let p = Project::new();
    let cut = device_screen().resized(90, 60).encode().unwrap();
    std::fs::write(p.0.join("button.png"), cut).unwrap();
    p.write(
        "t.flow.json",
        &format!(
            r#"{{"format":3,"name":"Mail","app":"{APP}","simulator":{{"device":"{UDID}"}},"start":"open","blocks":{{
              "open":{{"type":"openApp","next":"click"}},
              "click":{{"type":"click","target":{{"image":"button.png"}}}}}}}}"#
        ),
    );
    let (sim, mac) = (Sim::new(&p.0, &[APP]), Mac::default());
    let report = p.run("t.flow.json", &sim, &mac);
    assert_eq!(report.verdict, Verdict::Passed, "{}", reason(&report));
    let log = mac.log();
    // 390 window pixels show 585 device pixels: the picture is two thirds of its size there.
    assert!(
        log.contains(&format!("locate in {SIMULATOR_APP} 60x40")),
        "{log:?}"
    );
    // The centre of the screen is found where the window shows it.
    assert!(
        log.contains(&format!("click {SIMULATOR_APP} 112.5 261")),
        "{log:?}"
    );
}

#[test]
fn a_missing_device_build_or_app_says_what_to_do() {
    let p = Project::new();
    let test = |simulator: &str| {
        format!(
            r#"{{"format":3,"name":"Mail","app":"{APP}","simulator":{simulator},"start":"open","blocks":{{"open":{{"type":"openApp","resetState":true}}}}}}"#
        )
    };
    p.write("phone.flow.json", &test(r#"{"device":"iPhone 99"}"#))
        .write(
            "other.flow.json",
            &test(SIMULATOR.split_once(':').unwrap().1),
        )
        .write("plain.flow.json", &test(r#"{"device":"iPhone 17"}"#));
    let (sim, mac) = (Sim::new(&p.0, &[]), Mac::default());
    let missing = p.run("phone.flow.json", &sim, &mac);
    assert!(
        reason(&missing).contains("no simulated device “iPhone 99”"),
        "{}",
        reason(&missing)
    );
    let unbuilt = p.run("other.flow.json", &sim, &mac);
    assert!(
        reason(&unbuilt).contains("no app built at"),
        "{}",
        reason(&unbuilt)
    );
    p.build("com.example.Other");
    let other = p.run("other.flow.json", &sim, &mac);
    assert!(
        reason(&other).contains("is com.example.Other, but the test is for com.example.Mail"),
        "{}",
        reason(&other)
    );
    let absent = p.run("plain.flow.json", &sim, &mac);
    assert!(
        reason(&absent).contains("com.example.Mail isn’t installed on “iPhone 17”"),
        "{}",
        reason(&absent)
    );
}

#[test]
fn without_the_simulator_app_the_run_says_how_to_add_it() {
    let p = Project::new();
    p.build(APP).write(
        "t.flow.json",
        &format!(
            r#"{{"format":3,"name":"Mail","app":"{APP}",{SIMULATOR},"start":"open","blocks":{{"open":{{"type":"openApp"}}}}}}"#
        ),
    );
    let sim = Sim::new(&p.0, &[]);
    let mac = Mac {
        no_simulator: true,
        ..Mac::default()
    };
    let report = p.run("t.flow.json", &sim, &mac);
    let failure = report.attempts.last().unwrap().failure.clone().unwrap();
    assert_eq!(
        describe_in("en", &failure),
        "Block “Open App”: Simulator isn’t installed: add it in Xcode › Settings › Components, then run the test again."
    );
    // The run says it in the language of the app; in Russian:
    assert_eq!(
        jevision_l10n::text_in("ru", "run.simulator.noApp", &[]),
        "Приложение Simulator не установлено: добавьте его в Xcode › Настройки › Компоненты и запустите тест снова."
    );
    assert!(
        sim.log().iter().any(|c| c.starts_with("install ")),
        "{:?}",
        sim.log()
    );
    assert!(
        !sim.log().iter().any(|c| c.starts_with("launch ")),
        "{:?}",
        sim.log()
    );
}

#[test]
fn a_test_runs_only_tests_made_for_its_own_device() {
    let p = Project::new();
    let test = |name: &str, simulator: &str, block: &str| {
        format!(
            r#"{{"format":3,"name":"{name}","app":"{APP}"{simulator},"start":"b","blocks":{{"b":{block}}}}}"#
        )
    };
    let keys = r#"{"type":"pressKeys","keys":"cmd+l"}"#;
    let phone = r#","simulator":{"device":"iPhone 17"}"#;
    p.write("mac.flow.json", &test("Mac", "", keys))
        .write("phone.flow.json", &test("Phone", phone, keys))
        .write(
            "same.flow.json",
            &test(
                "Same",
                phone,
                r#"{"type":"runTest","test":"phone.flow.json"}"#,
            ),
        )
        .write(
            "other.flow.json",
            &test(
                "Other",
                phone,
                r#"{"type":"runTest","test":"mac.flow.json"}"#,
            ),
        );
    let (sim, mac) = (Sim::new(&p.0, &[APP]), Mac::default());
    let same = p.run("same.flow.json", &sim, &mac);
    assert_eq!(same.verdict, Verdict::Passed, "{}", reason(&same));
    let other = p.run("other.flow.json", &sim, &mac);
    assert_eq!(
        other.attempts[0].failure.as_ref().unwrap().code,
        "run.runTest.otherDevice"
    );
}
