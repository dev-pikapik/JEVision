//! What the Mac remembers: paired devices in `devices.json` and the activity log in
//! `activity.jsonl`. Both files are readable by the owner only.
//!
//! `devices.json` is `{"format":1,"devices":[…]}`. What this version doesn't know in it, a key
//! or a whole device a newer JEVision wrote, is kept as it was on every write. A file of a newer
//! format, or one that names a newer `minVersion`, is only read, never written.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use jevision_core::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{APP_VERSION, Permission, RemoteError, now};

const DEVICES: &str = "devices.json";
/// Where a `devices.json` that cannot be read is moved, replacing an older one.
const DEVICES_UNREADABLE: &str = "devices.unreadable.json";
/// The format of `devices.json` this version writes.
const DEVICES_FORMAT: u64 = 1;
const ACTIVITY: &str = "activity.jsonl";
const ACTIVITY_OLD: &str = "activity.1.jsonl";
/// The log moves to `activity.1.jsonl` past this size, replacing the older one, so the two
/// files together stay under twice this.
const ACTIVITY_LIMIT: u64 = 512 * 1024;

/// What a paired device is: the iPhone app, or an AI assistant on another computer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    #[default]
    Phone,
    Assistant,
}

/// A device paired with this Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    /// The name the phone gave itself, such as `Anna's iPhone`.
    pub name: String,
    /// The phone's P-256 public key, SEC1 uncompressed, hex.
    pub public_key: String,
    /// Devices saved before assistants existed are phones.
    #[serde(default)]
    pub kind: DeviceKind,
    /// What a phone may do. An assistant has no such level: its tools are limited by the
    /// assistant settings of the app, and by [`Device::assistant`].
    pub permission: Permission,
    /// Whether the owner lets this assistant use the Mac's tools. Always off for a phone.
    #[serde(default)]
    pub assistant: bool,
    /// Seconds since 1970.
    pub paired_at: i64,
    /// When the phone last signed in, in seconds since 1970.
    pub last_seen: Option<i64>,
}

/// One line of the activity log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// Seconds since 1970.
    pub time: i64,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub kind: ActivityKind,
    /// What it was about: the project and test or run, `project/path`.
    pub subject: Option<String>,
    /// Why it failed; `None` when it succeeded.
    pub error: Option<RemoteError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    Paired,
    PairingFailed,
    SignedIn,
    SignInFailed,
    RunStarted,
    RunStopped,
    ReportRead,
    /// A request the device was not allowed to make or that reached outside a project.
    Denied,
    PermissionChanged,
    Revoked,
    /// An assistant called a tool; the subject is the tool's name, never its arguments.
    AssistantCalled,
    /// `devices.json` could not be read, so the Mac started with no paired devices; the subject
    /// is the name the file was moved to.
    DevicesUnreadable,
}

#[derive(Debug, Default)]
pub(crate) struct Store {
    dir: PathBuf,
    devices: Vec<Device>,
    /// `devices.json` as read, without its devices.
    top: Map<String, Value>,
    /// Each usable device's object as read, by id.
    objects: HashMap<String, Map<String, Value>>,
    /// Entries this version cannot read, such as a kind of device a newer JEVision added.
    unreadable: Vec<Value>,
    /// The file is from a newer JEVision.
    newer: bool,
}

impl Store {
    /// Loads paired devices. Fails only when the folder cannot be used: a `devices.json` that
    /// cannot be read is moved aside and the Mac starts with no devices.
    pub(crate) fn open(dir: &Path) -> Result<Self, RemoteError> {
        create_private_dir(dir).map_err(|_| RemoteError::Storage)?;
        let mut store = Self {
            dir: dir.to_path_buf(),
            ..Self::default()
        };
        let path = dir.join(DEVICES);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(store),
            Err(_) => return Err(RemoteError::Storage),
        };
        if !store.read(&bytes) {
            fs::rename(&path, dir.join(DEVICES_UNREADABLE)).map_err(|_| RemoteError::Storage)?;
            store.log(&Activity {
                time: now(),
                device_id: None,
                device_name: None,
                kind: ActivityKind::DevicesUnreadable,
                subject: Some(DEVICES_UNREADABLE.to_owned()),
                error: None,
            });
        }
        Ok(store)
    }

    /// Takes in what `devices.json` holds; `false` when it cannot be read at all.
    fn read(&mut self, bytes: &[u8]) -> bool {
        let Ok(Value::Object(mut top)) = serde_json::from_slice(bytes) else {
            return false;
        };
        let format = match top.get("format") {
            None => DEVICES_FORMAT,
            Some(format) => match format.as_u64() {
                Some(format) if format > 0 => format,
                _ => return false,
            },
        };
        // A version this one cannot read is taken as newer: the file is then left alone.
        let needs_newer = top.get("minVersion").is_some_and(|min| {
            let min = min.as_str().and_then(|min| min.parse::<Version>().ok());
            match (min, APP_VERSION.parse::<Version>()) {
                (Some(min), Ok(own)) => min > own,
                _ => true,
            }
        });
        self.newer = format > DEVICES_FORMAT || needs_newer;
        let entries = match top.remove("devices") {
            Some(Value::Array(entries)) => entries,
            _ if self.newer => Vec::new(),
            _ => return false,
        };
        for entry in entries {
            match (serde_json::from_value::<Device>(entry.clone()), entry) {
                (Ok(device), Value::Object(object)) => {
                    self.objects.insert(device.id.clone(), object);
                    self.devices.push(device);
                }
                (_, entry) => self.unreadable.push(entry),
            }
        }
        self.top = top;
        true
    }

    pub(crate) fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub(crate) fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// Applies `change` to the device list and saves it; memory keeps the old list if saving
    /// fails. A file from a newer JEVision is never changed.
    pub(crate) fn change(
        &mut self,
        change: impl FnOnce(&mut Vec<Device>),
    ) -> Result<(), RemoteError> {
        if self.newer {
            return Err(RemoteError::DevicesFromNewer);
        }
        let mut next = self.devices.clone();
        change(&mut next);
        let mut objects = HashMap::new();
        let mut entries = Vec::new();
        for device in &next {
            let object = self.merged(device)?;
            entries.push(Value::Object(object.clone()));
            objects.insert(device.id.clone(), object);
        }
        // A device paired again replaces the entry of it that could not be read.
        let unreadable: Vec<Value> = self
            .unreadable
            .iter()
            .filter(|e| {
                !e.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| objects.contains_key(id))
            })
            .cloned()
            .collect();
        entries.extend(unreadable.iter().cloned());
        let mut file = self.top.clone();
        file.insert("format".into(), DEVICES_FORMAT.into());
        file.insert("devices".into(), Value::Array(entries));
        let json = serde_json::to_vec_pretty(&file).map_err(|_| RemoteError::Storage)?;
        write_atomic(&self.dir.join(DEVICES), &json).map_err(|_| RemoteError::Storage)?;
        self.devices = next;
        self.objects = objects;
        self.unreadable = unreadable;
        Ok(())
    }

    /// `device` as it goes to the file: its object as read, with only the keys whose value
    /// changed written anew, so what a newer JEVision wrote in it stays.
    fn merged(&self, device: &Device) -> Result<Map<String, Value>, RemoteError> {
        let new = fields(device)?;
        let (Some(mut object), Some(old)) = (
            self.objects.get(&device.id).cloned(),
            self.device(&device.id),
        ) else {
            return Ok(new);
        };
        let old = fields(old)?;
        object.retain(|key, _| new.contains_key(key) || !old.contains_key(key));
        for (key, value) in new {
            if old.get(&key) != Some(&value) {
                object.insert(key, value);
            }
        }
        Ok(object)
    }

    /// Appends to the activity log. A log that cannot be written never stops the work it
    /// describes.
    pub(crate) fn log(&self, entry: &Activity) {
        let _ = self.append(entry);
    }

    fn append(&self, entry: &Activity) -> io::Result<()> {
        let path = self.dir.join(ACTIVITY);
        if fs::metadata(&path).is_ok_and(|m| m.len() >= ACTIVITY_LIMIT) {
            fs::rename(&path, self.dir.join(ACTIVITY_OLD))?;
        }
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        private(&mut options);
        options.open(&path)?.write_all(&line)
    }

    /// The newest `limit` entries, newest first.
    pub(crate) fn activity(&self, limit: usize) -> Vec<Activity> {
        let mut entries: Vec<Activity> = [ACTIVITY_OLD, ACTIVITY]
            .iter()
            .filter_map(|name| File::open(self.dir.join(name)).ok())
            .flat_map(|file| BufReader::new(file).lines().map_while(Result::ok))
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect();
        entries.reverse();
        entries.truncate(limit);
        entries
    }
}

fn fields(device: &Device) -> Result<Map<String, Value>, RemoteError> {
    match serde_json::to_value(device) {
        Ok(Value::Object(fields)) => Ok(fields),
        _ => Err(RemoteError::Storage),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    private(&mut options);
    let mut file = options.open(&temp)?;
    set_private(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
}

/// A file left over from an interrupted write may have been created with other permissions.
fn set_private(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
