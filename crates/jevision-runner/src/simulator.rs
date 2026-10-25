//! Tests of apps in the iOS Simulator. `xcrun simctl` starts the simulated device, installs and
//! launches the app and takes screenshots in the device's own pixels; clicks, keys and the
//! search for pictures go through the window of Simulator on this Mac, the way a person
//! reaches the device.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use jevision_core::{
    ElementNode, Modifiers, MouseButton, Point, Rect, ScrollDirection, ScrollUnit, WindowPosition,
    WindowSize,
};
use jevision_l10n::tr;
use jevision_vision::image::Image;
use serde::Deserialize;

use crate::{AppStart, Hands, HandsError, Openable, Screenshot};

/// The bundle identifier of Simulator, the Mac app that shows simulated devices.
pub const SIMULATOR_APP: &str = "com.apple.iphonesimulator";

/// Runs `xcrun simctl`; tests put in one that answers as the simulator would.
pub trait Simctl: Send + Sync {
    /// Runs `simctl` with `args` and the variables of `environment` set, and returns what it
    /// printed. `Err` is a sentence for people: what went wrong, as `simctl` said it.
    fn run(&self, args: &[&str], environment: &[(String, String)]) -> Result<Vec<u8>, String>;
}

/// The `simctl` of the Xcode chosen on this Mac.
pub struct Xcrun;

impl Simctl for Xcrun {
    fn run(&self, args: &[&str], environment: &[(String, String)]) -> Result<Vec<u8>, String> {
        let out = Command::new("xcrun")
            .arg("simctl")
            .args(args)
            .envs(environment.iter().map(|(name, value)| (name, value)))
            .output()
            .map_err(|_| tr!("run.simulator.noXcode"))?;
        if out.status.success() {
            return Ok(out.stdout);
        }
        let said = String::from_utf8_lossy(&out.stderr);
        if said.contains("unable to find utility") || said.contains("xcode-select") {
            return Err(tr!("run.simulator.noXcode"));
        }
        Err(tr!("run.simulator.said", reason(&said)))
    }
}

/// The lines of what `simctl` printed on failure that say what happened, without the error
/// domains and codes around them.
fn reason(said: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in said.lines().map(str::trim) {
        let noise = line.is_empty()
            || line.starts_with("An error was encountered processing the command")
            || line.starts_with("Underlying error")
            || line.starts_with("Unhandled error domain");
        if !noise && !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join(" ")
}

/// A simulated iPhone or iPad this Mac has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulatorDevice {
    pub udid: String,
    pub name: String,
    /// The iOS version it runs, such as `27.0`.
    pub os: String,
    pub booted: bool,
    /// Such as `com.apple.CoreSimulator.SimDeviceType.iPhone-17`.
    pub device_type: String,
    /// The folder the device keeps its apps and files in.
    pub data_path: PathBuf,
}

/// An app on a simulated device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulatorApp {
    pub bundle_id: String,
    pub name: String,
    /// Installed by a person, not part of iOS.
    pub user: bool,
}

#[derive(Deserialize)]
struct DeviceList {
    devices: HashMap<String, Vec<DeviceEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceEntry {
    udid: String,
    name: String,
    state: String,
    #[serde(default)]
    device_type_identifier: String,
    #[serde(default = "available")]
    is_available: bool,
    #[serde(default)]
    data_path: PathBuf,
}

fn available() -> bool {
    true
}

const IOS_RUNTIME: &str = "com.apple.CoreSimulator.SimRuntime.iOS-";

/// The simulated iOS devices this Mac has, by name and the newest iOS first.
pub fn simulator_devices(simctl: &dyn Simctl) -> Result<Vec<SimulatorDevice>, HandsError> {
    let out = simctl
        .run(&["list", "devices", "available", "-j"], &[])
        .map_err(HandsError::Failed)?;
    let list: DeviceList =
        serde_json::from_slice(&out).map_err(|e| HandsError::Failed(e.to_string()))?;
    let mut devices: Vec<SimulatorDevice> = list
        .devices
        .into_iter()
        .filter_map(|(runtime, entries)| {
            let os = runtime.strip_prefix(IOS_RUNTIME)?.replace('-', ".");
            Some(
                entries
                    .into_iter()
                    .filter(|e| e.is_available)
                    .map(move |e| SimulatorDevice {
                        udid: e.udid,
                        name: e.name,
                        os: os.clone(),
                        booted: e.state == "Booted",
                        device_type: e.device_type_identifier,
                        data_path: e.data_path,
                    }),
            )
        })
        .flatten()
        .collect();
    devices.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| version(&b.os).cmp(&version(&a.os)))
    });
    Ok(devices)
}

fn version(os: &str) -> Vec<u32> {
    os.split('.').filter_map(|p| p.parse().ok()).collect()
}

/// The device a test names: by its UDID, or by its name; of devices with one name, the one
/// that is running, else the one with the newest iOS.
pub fn pick_device<'d>(
    devices: &'d [SimulatorDevice],
    wanted: &str,
) -> Option<&'d SimulatorDevice> {
    let wanted = wanted.trim();
    devices
        .iter()
        .find(|d| d.udid.eq_ignore_ascii_case(wanted))
        .or_else(|| {
            devices
                .iter()
                .filter(|d| d.name == wanted)
                .max_by(|a, b| (a.booted, version(&a.os)).cmp(&(b.booted, version(&b.os))))
        })
}

/// The apps on the device `udid` that people see on its home screen: those a person installed
/// first, then those of iOS, each by name.
pub fn simulator_apps(simctl: &dyn Simctl, udid: &str) -> Result<Vec<SimulatorApp>, HandsError> {
    let out = simctl
        .run(&["listapps", udid], &[])
        .map_err(HandsError::Failed)?;
    let list = plist::Value::from_reader_ascii(out.as_slice())
        .map_err(|e| HandsError::Failed(e.to_string()))?;
    let text = |app: &plist::Dictionary, key: &str| {
        app.get(key)
            .and_then(plist::Value::as_string)
            .unwrap_or_default()
            .to_owned()
    };
    let mut apps: Vec<SimulatorApp> = list
        .as_dictionary()
        .into_iter()
        .flatten()
        .filter_map(|(id, app)| {
            let app = app.as_dictionary()?;
            // The plain text plist simctl prints has numbers that read as text or as numbers.
            let hidden = app
                .get("IsHidden")
                .is_some_and(|v| v.as_signed_integer() == Some(1) || v.as_string() == Some("1"));
            if hidden {
                return None;
            }
            let name = [text(app, "CFBundleDisplayName"), text(app, "CFBundleName")]
                .into_iter()
                .find(|n| !n.is_empty())
                .unwrap_or_else(|| id.clone());
            Some(SimulatorApp {
                bundle_id: id.clone(),
                name,
                user: text(app, "ApplicationType") == "User",
            })
        })
        .collect();
    apps.sort_by(|a, b| b.user.cmp(&a.user).then_with(|| a.name.cmp(&b.name)));
    Ok(apps)
}

/// The apps a person installed on `device`, read from its folder, so they can be listed while it is
/// off; `simctl listapps` answers only for a running device.
pub fn installed_apps(device: &SimulatorDevice) -> Vec<SimulatorApp> {
    let bundles = device.data_path.join("Containers/Bundle/Application");
    let mut apps: Vec<SimulatorApp> = std::fs::read_dir(bundles)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|container| {
            std::fs::read_dir(container.path())
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "app"))
        .filter_map(|entry| {
            let info = plist::Value::from_file(entry.path().join("Info.plist")).ok()?;
            let info = info.as_dictionary()?;
            let text = |key: &str| {
                info.get(key)
                    .and_then(plist::Value::as_string)
                    .filter(|t| !t.is_empty())
            };
            let bundle_id = text("CFBundleIdentifier")?.to_owned();
            let name = text("CFBundleDisplayName")
                .or_else(|| text("CFBundleName"))
                .unwrap_or(&bundle_id)
                .to_owned();
            Some(SimulatorApp {
                bundle_id,
                name,
                user: true,
            })
        })
        .collect();
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    apps
}

#[derive(Deserialize)]
struct DeviceTypes {
    devicetypes: Vec<DeviceType>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceType {
    identifier: String,
    bundle_path: PathBuf,
}

/// Device pixels per point of the screen of devices of `device_type`, as the device profile
/// Xcode keeps says: 3 for most iPhones, 2 for iPads.
fn screen_scale(simctl: &dyn Simctl, device_type: &str) -> Result<f64, HandsError> {
    let out = simctl
        .run(&["list", "devicetypes", "-j"], &[])
        .map_err(HandsError::Failed)?;
    let types: DeviceTypes =
        serde_json::from_slice(&out).map_err(|e| HandsError::Failed(e.to_string()))?;
    let unknown = || HandsError::Failed(tr!("run.simulator.unknownScreen", device_type));
    let profile = types
        .devicetypes
        .into_iter()
        .find(|t| t.identifier == device_type)
        .ok_or_else(unknown)?;
    let capabilities = profile
        .bundle_path
        .join("Contents/Resources/capabilities.plist");
    let value = plist::Value::from_file(capabilities).map_err(|_| unknown())?;
    value
        .as_dictionary()
        .and_then(|d| d.get("capabilities")?.as_dictionary())
        .and_then(|d| d.get("ScreenDimensionsCapability")?.as_dictionary())
        .and_then(|d| d.get("main-screen-scale"))
        .and_then(|v| {
            v.as_real()
                .or_else(|| v.as_signed_integer().map(|i| i as f64))
        })
        .filter(|scale| *scale >= 1.0)
        .ok_or_else(unknown)
}

/// The device a run uses, once it is running.
#[derive(Debug, Clone)]
struct Ready {
    udid: String,
    name: String,
    scale: f64,
}

/// Where the device screen is in the Simulator window.
#[derive(Debug, Clone, Copy)]
struct Placed {
    /// The window in its pixels, the size it had when the screen was found in it.
    window: (u32, u32),
    /// Window pixels per window point.
    window_scale: f64,
    /// The screen, in window pixels.
    screen: Rect,
    /// The screen, in device pixels.
    device: (u32, u32),
    /// Device pixels per device point.
    device_scale: f64,
}

impl Placed {
    /// Window pixels per device point.
    fn zoom(&self) -> f64 {
        self.screen.width / f64::from(self.device.0) * self.device_scale
    }

    /// A point on the device screen, in device points, as a point in the window.
    fn window_point(&self, at: Point) -> Point {
        Point {
            x: (self.screen.x + at.x * self.zoom()) / self.window_scale,
            y: (self.screen.y + at.y * self.zoom()) / self.window_scale,
        }
    }

    /// A point in the window as a point on the device screen; `None` off the screen.
    fn device_point(&self, at: Point) -> Option<Point> {
        let p = Point {
            x: (at.x * self.window_scale - self.screen.x) / self.zoom(),
            y: (at.y * self.window_scale - self.screen.y) / self.zoom(),
        };
        let size = (
            f64::from(self.device.0) / self.device_scale,
            f64::from(self.device.1) / self.device_scale,
        );
        ((0.0..=size.0).contains(&p.x) && (0.0..=size.1).contains(&p.y)).then_some(p)
    }

    /// Whether the point `x`, `y` in fractions of the window is on the device screen.
    fn shows(&self, x: f64, y: f64) -> bool {
        let (x, y) = (x * f64::from(self.window.0), y * f64::from(self.window.1));
        let s = &self.screen;
        (s.x..=s.x + s.width).contains(&x) && (s.y..=s.y + s.height).contains(&y)
    }

    /// An area in fractions of the window as fractions of the device screen, cut to it; `None`
    /// when no part of it is on the screen.
    fn window_area_on_screen(&self, r: &Rect) -> Option<Rect> {
        let (w, h) = (f64::from(self.window.0), f64::from(self.window.1));
        let x0 = ((r.x * w - self.screen.x) / self.screen.width).max(0.0);
        let y0 = ((r.y * h - self.screen.y) / self.screen.height).max(0.0);
        let x1 = (((r.x + r.width) * w - self.screen.x) / self.screen.width).min(1.0);
        let y1 = (((r.y + r.height) * h - self.screen.y) / self.screen.height).min(1.0);
        (x1 > x0 && y1 > y0).then_some(Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// An area in fractions of the device screen as fractions of the window.
    fn screen_area_in_window(&self, r: &Rect) -> Rect {
        let (w, h) = (f64::from(self.window.0), f64::from(self.window.1));
        Rect {
            x: (self.screen.x + r.x * self.screen.width) / w,
            y: (self.screen.y + r.y * self.screen.height) / h,
            width: r.width * self.screen.width / w,
            height: r.height * self.screen.height / h,
        }
    }
}

#[derive(Default)]
struct State {
    ready: Option<Ready>,
    /// The device screen the last time it was looked at, to find it in the window.
    last: Option<Image>,
    placed: Option<Placed>,
    /// The app was installed from the test's build in this run.
    installed: bool,
}

/// [`Hands`] for a test of an app on a simulated device: the apps a test names are the apps on
/// the device, points and screenshots are those of the device screen, and what a person does
/// with mouse and keys goes to the window of Simulator through the Mac's own hands.
pub struct SimulatorHands<'a> {
    mac: &'a dyn Hands,
    simctl: &'a dyn Simctl,
    /// The device as the test names it.
    device: String,
    state: Mutex<State>,
}

static SCREENSHOTS: AtomicU64 = AtomicU64::new(0);

impl<'a> SimulatorHands<'a> {
    pub fn new(mac: &'a dyn Hands, simctl: &'a dyn Simctl, device: &str) -> Self {
        Self {
            mac,
            simctl,
            device: device.to_owned(),
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn simctl(&self, args: &[&str]) -> Result<Vec<u8>, HandsError> {
        self.simctl.run(args, &[]).map_err(HandsError::Failed)
    }

    /// The device, started first if it is not running.
    fn ready(&self) -> Result<Ready, HandsError> {
        if let Some(ready) = self.state().ready.clone() {
            return Ok(ready);
        }
        let devices = simulator_devices(self.simctl)?;
        let device = pick_device(&devices, &self.device).ok_or_else(|| {
            HandsError::Failed(tr!("run.simulator.noDevice", self.device.as_str()))
        })?;
        if !device.booted {
            // Starts the device and returns once it has finished starting.
            self.simctl(&["bootstatus", &device.udid, "-b"])?;
        }
        let ready = Ready {
            udid: device.udid.clone(),
            name: device.name.clone(),
            scale: screen_scale(self.simctl, &device.device_type)?,
        };
        self.state().ready = Some(ready.clone());
        Ok(ready)
    }

    /// What the device screen shows now, as PNG; kept to find the screen in the window.
    fn screenshot(&self, ready: &Ready) -> Result<Vec<u8>, HandsError> {
        let n = SCREENSHOTS.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("jevision-simulator-{}-{n}.png", std::process::id()));
        let file = path.to_string_lossy().into_owned();
        let taken = self.simctl(&["io", &ready.udid, "screenshot", "--type=png", &file]);
        let png =
            taken.and_then(|_| std::fs::read(&path).map_err(|e| HandsError::Failed(e.to_string())));
        let _ = std::fs::remove_file(&path);
        let png = png?;
        let image = Image::decode(&png).map_err(HandsError::Failed)?;
        self.state().last = Some(image);
        Ok(png)
    }

    /// The Simulator window, and where the device screen is in it: found again when the window
    /// changed size, and with a fresh screenshot of the device when the last one is not there.
    fn place(&self, ready: &Ready) -> Result<(Screenshot, Placed), HandsError> {
        let window = self
            .mac
            .capture(SIMULATOR_APP)
            .map_err(|e| self.window_error(ready, e))?;
        let image = Image::decode(&window.png).map_err(HandsError::Failed)?;
        let size = (image.width, image.height);
        if let Some(placed) = self.state().placed.filter(|p| p.window == size) {
            return Ok((window, placed));
        }
        let find = || {
            let device = self.state().last.clone()?;
            Some((image.find_scaled(&device)?, device))
        };
        let found = match find() {
            Some(found) => Some(found),
            None => {
                self.screenshot(ready)?;
                find()
            }
        };
        let (area, device) = found.ok_or_else(|| {
            HandsError::Failed(tr!("run.simulator.noScreen", ready.name.as_str()))
        })?;
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let placed = Placed {
            window: size,
            window_scale: window.scale,
            screen: Rect {
                x: area.x * w,
                y: area.y * h,
                width: area.width * w,
                height: area.height * h,
            },
            device: (device.width, device.height),
            device_scale: ready.scale,
        };
        self.state().placed = Some(placed);
        Ok((window, placed))
    }

    fn placed(&self) -> Result<Placed, HandsError> {
        let ready = self.ready()?;
        Ok(self.place(&ready)?.1)
    }

    /// What a failure of the Mac's hands with the Simulator window means for the test.
    fn window_error(&self, ready: &Ready, e: HandsError) -> HandsError {
        match e {
            HandsError::AppNotFound(_) | HandsError::NoWindow(_) => {
                HandsError::Failed(tr!("run.simulator.noWindow", ready.name.as_str()))
            }
            e => e,
        }
    }

    fn mac<T>(
        &self,
        call: impl FnOnce(&dyn Hands) -> Result<T, HandsError>,
    ) -> Result<T, HandsError> {
        let ready = self.ready()?;
        call(self.mac).map_err(|e| self.window_error(&ready, e))
    }

    /// Installs the app built at `path` on the device, once a run, as the build of `app`; with
    /// `fresh` it first goes with all its data, so it starts as on a new device.
    pub fn install(&self, app: &str, path: &Path, fresh: bool) -> Result<(), HandsError> {
        if self.state().installed && !fresh {
            return Ok(());
        }
        let shown = path.to_string_lossy().into_owned();
        let info = plist::Value::from_file(path.join("Info.plist"))
            .map_err(|_| HandsError::Failed(tr!("run.simulator.noBuild", shown.as_str())))?;
        let built = info
            .as_dictionary()
            .and_then(|d| d.get("CFBundleIdentifier")?.as_string())
            .unwrap_or_default();
        if built != app {
            return Err(HandsError::Failed(tr!(
                "run.simulator.otherBuild",
                shown.as_str(),
                built,
                app
            )));
        }
        let ready = self.ready()?;
        if fresh {
            // Not installed yet is as good as gone.
            let _ = self.simctl(&["terminate", &ready.udid, app]);
            let _ = self.simctl(&["uninstall", &ready.udid, app]);
        }
        self.simctl(&["install", &ready.udid, &shown])?;
        self.state().installed = true;
        Ok(())
    }
}

impl Hands for SimulatorHands<'_> {
    fn open_app(
        &self,
        app: &str,
        start: AppStart,
        arguments: &[String],
        environment: &HashMap<String, String>,
    ) -> Result<(), HandsError> {
        let ready = self.ready()?;
        if !simulator_apps(self.simctl, &ready.udid)?
            .iter()
            .any(|a| a.bundle_id == app)
        {
            return Err(HandsError::Failed(tr!(
                "run.simulator.notInstalled",
                app,
                ready.name.as_str()
            )));
        }
        // Simulator opens on this device; one already open shows it among the others.
        let shown = [String::from("-CurrentDeviceUDID"), ready.udid.clone()];
        self.mac
            .open_app(SIMULATOR_APP, AppStart::AsUsual, &shown, &HashMap::new())
            .map_err(|e| match e {
                HandsError::AppNotFound(_) => HandsError::Failed(tr!("run.simulator.noApp")),
                e => e,
            })?;
        let mut args = vec!["launch"];
        if start != AppStart::AsUsual {
            args.push("--terminate-running-process");
        }
        args.extend([ready.udid.as_str(), app]);
        args.extend(arguments.iter().map(String::as_str));
        // simctl hands the app the variables with this prefix, without it.
        let mut variables: Vec<(String, String)> = environment
            .iter()
            .map(|(name, value)| (format!("SIMCTL_CHILD_{name}"), value.clone()))
            .collect();
        variables.sort();
        self.simctl
            .run(&args, &variables)
            .map_err(HandsError::Failed)?;
        self.state().placed = None;
        self.mac(|h| h.activate(SIMULATOR_APP))
    }

    fn sandboxed(&self, _: &str) -> Result<bool, HandsError> {
        Ok(false)
    }

    fn arrange_window(
        &self,
        _: &str,
        size: Option<WindowSize>,
        position: Option<WindowPosition>,
    ) -> Result<(), HandsError> {
        self.state().placed = None;
        self.mac(|h| h.arrange_window(SIMULATOR_APP, size, position))
    }

    fn open(&self, items: &[Openable], _: Option<&str>, _: bool) -> Result<(), HandsError> {
        let ready = self.ready()?;
        for item in items {
            match item {
                Openable::Url(url) => {
                    self.simctl(&["openurl", &ready.udid, url])?;
                }
                Openable::File(path) => {
                    return Err(HandsError::CannotOpen(path.to_string_lossy().into_owned()));
                }
            }
        }
        Ok(())
    }

    fn close(&self, _: &[Openable], _: Option<&str>) -> Result<(), HandsError> {
        // A link leaves nothing open that the app it opened in would not show anyway.
        Ok(())
    }

    fn activate(&self, _: &str) -> Result<(), HandsError> {
        self.mac(|h| h.activate(SIMULATOR_APP))
    }

    fn quit(&self, app: &str) -> Result<(), HandsError> {
        let ready = self.ready()?;
        // An app that is not running is as good as quit.
        let _ = self.simctl(&["terminate", &ready.udid, app]);
        Ok(())
    }

    fn capture(&self, _: &str) -> Result<Screenshot, HandsError> {
        let ready = self.ready()?;
        let png = self.screenshot(&ready)?;
        let dark = self
            .simctl(&["ui", &ready.udid, "appearance"])
            .is_ok_and(|out| String::from_utf8_lossy(&out).trim() == "dark");
        // The window tells where the password fields are; without it there is still the screen.
        let secure = self
            .place(&ready)
            .map(|(window, placed)| {
                window
                    .secure
                    .iter()
                    .filter_map(|r| placed.window_area_on_screen(r))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Screenshot {
            png,
            scale: ready.scale,
            dark,
            secure,
        })
    }

    fn elements(&self, _: &str) -> Result<Vec<ElementNode>, HandsError> {
        let placed = self.placed()?;
        let nodes = self.mac(|h| h.elements(SIMULATOR_APP))?;
        Ok(nodes
            .into_iter()
            .filter_map(|mut node| {
                // Simulator's own buttons are around the screen, the app's elements on it.
                let f = &node.frame;
                if !placed.shows(f.x + f.width / 2.0, f.y + f.height / 2.0) {
                    return None;
                }
                node.frame = placed.window_area_on_screen(&node.frame)?;
                Some(node)
            })
            .collect())
    }

    fn locate(
        &self,
        _: &str,
        template: &[u8],
        min_similarity: f64,
    ) -> Result<Option<Point>, HandsError> {
        let placed = self.placed()?;
        // The picture was cut from a screenshot of the device; the window shows it at its zoom.
        let picture = Image::decode(template).map_err(HandsError::Failed)?;
        let factor = placed.screen.width / f64::from(placed.device.0);
        let side = |v: u32| ((f64::from(v) * factor).round() as u32).max(1);
        let scaled = picture
            .resized(side(picture.width), side(picture.height))
            .encode()
            .map_err(HandsError::Failed)?;
        let found = self.mac(|h| h.locate(SIMULATOR_APP, &scaled, min_similarity))?;
        Ok(found.and_then(|at| placed.device_point(at)))
    }

    fn click(
        &self,
        _: &str,
        at: Point,
        button: MouseButton,
        double: bool,
        modifiers: Modifiers,
    ) -> Result<(), HandsError> {
        let at = self.placed()?.window_point(at);
        self.mac(|h| h.click(SIMULATOR_APP, at, button, double, modifiers))
    }

    fn hover(&self, _: &str, at: Point, modifiers: Modifiers) -> Result<(), HandsError> {
        let at = self.placed()?.window_point(at);
        self.mac(|h| h.hover(SIMULATOR_APP, at, modifiers))
    }

    fn drag(
        &self,
        _: &str,
        path: &[Point],
        button: MouseButton,
        modifiers: Modifiers,
    ) -> Result<(), HandsError> {
        let placed = self.placed()?;
        let path: Vec<Point> = path.iter().map(|p| placed.window_point(*p)).collect();
        self.mac(|h| h.drag(SIMULATOR_APP, &path, button, modifiers))
    }

    fn scroll(
        &self,
        _: &str,
        at: Point,
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
        modifiers: Modifiers,
    ) -> Result<(), HandsError> {
        let at = self.placed()?.window_point(at);
        self.mac(|h| h.scroll(SIMULATOR_APP, at, direction, amount, unit, modifiers))
    }

    fn type_text(&self, text: &str) -> Result<(), HandsError> {
        self.mac.type_text(text)
    }

    fn press_keys(&self, keys: &str) -> Result<(), HandsError> {
        self.mac.press_keys(keys)
    }

    // Simulator keeps the device's clipboard in step with the Mac's.
    fn clipboard(&self) -> Result<String, HandsError> {
        self.mac.clipboard()
    }

    fn set_clipboard(&self, text: &str) -> Result<(), HandsError> {
        self.mac.set_clipboard(text)
    }

    fn clipboard_image(&self) -> Result<Option<Vec<u8>>, HandsError> {
        self.mac.clipboard_image()
    }

    fn clipboard_files(&self) -> Result<Vec<String>, HandsError> {
        self.mac.clipboard_files()
    }

    fn set_clipboard_image(&self, png: &[u8]) -> Result<(), HandsError> {
        self.mac.set_clipboard_image(png)
    }

    fn user_input_detected(&self) -> bool {
        self.mac.user_input_detected()
    }

    fn start_video(&self, _: &str, path: &Path, hide: &[Rect]) -> Result<(), HandsError> {
        // Areas the project hides are on the device screen; the video shows the whole window,
        // so they are found in it first, and without them found there is no video at all.
        let hide = if hide.is_empty() {
            Vec::new()
        } else {
            let placed = self.placed()?;
            hide.iter()
                .map(|r| placed.screen_area_in_window(r))
                .collect()
        };
        self.mac.start_video(SIMULATOR_APP, path, &hide)
    }

    fn stop_video(&self) -> Result<(), HandsError> {
        self.mac.stop_video()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICES: &str = r#"{
  "devices" : {
    "com.apple.CoreSimulator.SimRuntime.watchOS-12-0" : [
      { "udid" : "W1", "isAvailable" : true, "deviceTypeIdentifier" : "watch", "state" : "Shutdown", "name" : "Apple Watch" }
    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-26-2" : [
      { "udid" : "OLD", "isAvailable" : true, "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-17", "state" : "Shutdown", "name" : "iPhone 17" }
    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-27-0" : [
      { "udid" : "B646D6A7-966E-44FA-A97C-7F3BE3207274", "isAvailable" : true, "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-17", "state" : "Shutdown", "name" : "iPhone 17" },
      { "udid" : "PRO", "isAvailable" : true, "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-18-Pro", "state" : "Booted", "name" : "iPhone 18 Pro" },
      { "udid" : "GONE", "isAvailable" : false, "deviceTypeIdentifier" : "x", "state" : "Shutdown", "name" : "iPhone 18 Pro" }
    ]
  }
}"#;

    /// What `simctl listapps` prints, cut short.
    const APPS: &str = r#"{
    "com.apple.Preferences" =     {
        ApplicationType = System;
        CFBundleDisplayName = Settings;
        CFBundleIdentifier = "com.apple.Preferences";
        CFBundleName = Preferences;
        GroupContainers =         {
        };
        IsHidden = 0;
        Path = "/Library/Developer/CoreSimulator/Volumes/iOS 27.0.simruntime/Applications/Preferences.app";
    };
    "com.apple.WebSheet" =     {
        ApplicationType = System;
        CFBundleIdentifier = "com.apple.WebSheet";
        IsHidden = 1;
    };
    "com.example.Mail" =     {
        ApplicationType = User;
        CFBundleIdentifier = "com.example.Mail";
        CFBundleName = Mail;
        SBAppTags =         (
            "hidden-from-search"
        );
    };
}"#;

    struct Answers(Vec<(&'static str, &'static str)>);

    impl Simctl for Answers {
        fn run(&self, args: &[&str], _: &[(String, String)]) -> Result<Vec<u8>, String> {
            let asked = args.join(" ");
            self.0
                .iter()
                .find(|(start, _)| asked.starts_with(start))
                .map(|(_, answer)| answer.as_bytes().to_vec())
                .ok_or(asked)
        }
    }

    #[test]
    fn only_ios_devices_are_listed_by_name_and_newest_ios_first() {
        let devices = simulator_devices(&Answers(vec![("list devices", DEVICES)])).unwrap();
        let shown: Vec<(&str, &str, bool)> = devices
            .iter()
            .map(|d| (d.udid.as_str(), d.os.as_str(), d.booted))
            .collect();
        assert_eq!(
            shown,
            [
                ("B646D6A7-966E-44FA-A97C-7F3BE3207274", "27.0", false),
                ("OLD", "26.2", false),
                ("PRO", "27.0", true),
            ]
        );
    }

    #[test]
    fn a_device_is_found_by_udid_or_by_name_the_running_or_newest_one() {
        let mut devices = simulator_devices(&Answers(vec![("list devices", DEVICES)])).unwrap();
        let udid = |wanted: &str, devices: &[SimulatorDevice]| {
            pick_device(devices, wanted).map(|d| d.udid.clone())
        };
        assert_eq!(udid("pro", &devices).as_deref(), Some("PRO"));
        assert_eq!(
            udid("iPhone 17", &devices).as_deref(),
            Some("B646D6A7-966E-44FA-A97C-7F3BE3207274")
        );
        devices[1].booted = true;
        assert_eq!(udid(" iPhone 17 ", &devices).as_deref(), Some("OLD"));
        assert_eq!(udid("iPhone 16", &devices), None);
    }

    #[test]
    fn apps_people_installed_come_first_and_hidden_ones_are_left_out() {
        let apps = simulator_apps(&Answers(vec![("listapps X", APPS)]), "X").unwrap();
        assert_eq!(
            apps,
            [
                SimulatorApp {
                    bundle_id: "com.example.Mail".into(),
                    name: "Mail".into(),
                    user: true,
                },
                SimulatorApp {
                    bundle_id: "com.apple.Preferences".into(),
                    name: "Settings".into(),
                    user: false,
                },
            ]
        );
    }

    #[test]
    fn apps_on_a_device_that_is_off_are_read_from_its_folder() {
        let data = std::env::temp_dir().join(format!("jevision-sim-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let app = |container: &str, folder: &str, info: &str| {
            let dir = data
                .join("Containers/Bundle/Application")
                .join(container)
                .join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Info.plist"), info).unwrap();
        };
        let info = |keys: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>{keys}</dict></plist>"#
            )
        };
        app(
            "A",
            "Shop.app",
            &info(
                "<key>CFBundleIdentifier</key><string>com.example.Shop</string><key>CFBundleDisplayName</key><string>Shop</string>",
            ),
        );
        app(
            "B",
            "Notes.app",
            &info("<key>CFBundleIdentifier</key><string>com.example.Notes</string>"),
        );
        app("C", "Broken.app", &info(""));
        let mut devices = simulator_devices(&Answers(vec![("list devices", DEVICES)])).unwrap();
        devices[0].data_path = data.clone();
        let apps = installed_apps(&devices[0]);
        std::fs::remove_dir_all(&data).unwrap();
        let shown: Vec<(&str, &str)> = apps
            .iter()
            .map(|a| (a.bundle_id.as_str(), a.name.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("com.example.Shop", "Shop"),
                ("com.example.Notes", "com.example.Notes")
            ]
        );
        assert!(installed_apps(&devices[1]).is_empty());
    }

    #[test]
    fn what_simctl_says_keeps_only_the_sentences() {
        let said = "An error was encountered processing the command (domain=NSPOSIXErrorDomain, code=2):\nSimulator device failed to install the application.\nUnhandled error domain NSPOSIXErrorDomain, code 2\nUnderlying error (domain=NSPOSIXErrorDomain, code=2):\n\tlstat of /x/My.app failed: No such file or directory\n\tUnhandled error domain NSPOSIXErrorDomain, code 2\n";
        assert_eq!(
            reason(said),
            "Simulator device failed to install the application. lstat of /x/My.app failed: No such file or directory"
        );
    }

    #[test]
    fn points_go_between_the_device_screen_and_the_window() {
        // A 390 × 844 point screen at 3 pixels a point, shown at half its pixels from 40, 120
        // window pixels on a Retina display.
        let placed = Placed {
            window: (700, 1400),
            window_scale: 2.0,
            screen: Rect {
                x: 40.0,
                y: 120.0,
                width: 585.0,
                height: 1266.0,
            },
            device: (1170, 2532),
            device_scale: 3.0,
        };
        let at = placed.window_point(Point { x: 100.0, y: 200.0 });
        assert_eq!((at.x, at.y), (95.0, 210.0));
        let back = placed.device_point(at).unwrap();
        assert!((back.x - 100.0).abs() < 1e-9 && (back.y - 200.0).abs() < 1e-9);
        assert_eq!(placed.device_point(Point { x: 5.0, y: 5.0 }), None);
        let area = placed
            .window_area_on_screen(&Rect {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            })
            .unwrap();
        let expected = Rect {
            x: 0.0,
            y: 0.0,
            width: (350.0 - 40.0) / 585.0,
            height: (700.0 - 120.0) / 1266.0,
        };
        assert!((area.width - expected.width).abs() < 1e-9);
        assert!((area.height - expected.height).abs() < 1e-9);
        let whole = placed.screen_area_in_window(&Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        });
        assert!((whole.x * 700.0 - 40.0).abs() < 1e-9);
        assert!((whole.width * 700.0 - 585.0).abs() < 1e-9);
    }
}
