//! What the Mac remembers: paired devices in `devices.json` and the activity log in
//! `activity.jsonl`. Both files are readable by the owner only.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Permission, RemoteError};

const DEVICES: &str = "devices.json";
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
}

#[derive(Debug, Serialize, Deserialize)]
struct DevicesFile {
    devices: Vec<Device>,
}

#[derive(Debug)]
pub(crate) struct Store {
    dir: PathBuf,
    devices: Vec<Device>,
}

impl Store {
    pub(crate) fn open(dir: &Path) -> Result<Self, RemoteError> {
        create_private_dir(dir).map_err(|_| RemoteError::Storage)?;
        let devices = match fs::read(dir.join(DEVICES)) {
            Ok(bytes) => {
                serde_json::from_slice::<DevicesFile>(&bytes)
                    .map_err(|_| RemoteError::Storage)?
                    .devices
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err(RemoteError::Storage),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            devices,
        })
    }

    pub(crate) fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub(crate) fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// Applies `change` to the device list and saves it; memory keeps the old list if saving
    /// fails.
    pub(crate) fn change(
        &mut self,
        change: impl FnOnce(&mut Vec<Device>),
    ) -> Result<(), RemoteError> {
        let mut next = self.devices.clone();
        change(&mut next);
        let json = serde_json::to_vec_pretty(&DevicesFile {
            devices: next.clone(),
        })
        .map_err(|_| RemoteError::Storage)?;
        write_atomic(&self.dir.join(DEVICES), &json).map_err(|_| RemoteError::Storage)?;
        self.devices = next;
        Ok(())
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
