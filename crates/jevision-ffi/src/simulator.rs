//! The simulated iOS devices of this Mac and their apps, for choosing where a test runs.

use jevision_runner::{HandsError, Xcrun};

/// Simulator can't be asked about its devices, such as when Xcode isn't installed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum SimulatorError {
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<HandsError> for SimulatorError {
    fn from(e: HandsError) -> Self {
        let reason = match e {
            HandsError::Failed(reason) => reason,
            other => format!("{other:?}"),
        };
        Self::Failed { reason }
    }
}

/// A simulated iPhone or iPad a test can run on.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SimulatedDevice {
    pub udid: String,
    pub name: String,
    /// The iOS version it runs, such as `27.0`.
    pub os: String,
    pub booted: bool,
}

/// An app on a simulated device.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SimulatedApp {
    pub bundle_id: String,
    pub name: String,
    /// Installed by a person, not part of iOS.
    pub user: bool,
}

/// The simulated iOS devices of this Mac, by name and the newest iOS first.
#[uniffi::export]
pub fn simulated_devices() -> Result<Vec<SimulatedDevice>, SimulatorError> {
    Ok(jevision_runner::simulator_devices(&Xcrun)?
        .into_iter()
        .map(|d| SimulatedDevice {
            udid: d.udid,
            name: d.name,
            os: d.os,
            booted: d.booted,
        })
        .collect())
}

/// The apps on the device named or numbered `device`, those a person installed first. A device
/// that is off is not started for this: only the apps a person installed are listed then. None
/// when there is no such device.
#[uniffi::export]
pub fn simulated_apps(device: String) -> Result<Vec<SimulatedApp>, SimulatorError> {
    let devices = jevision_runner::simulator_devices(&Xcrun)?;
    let Some(device) = jevision_runner::pick_device(&devices, &device) else {
        return Ok(Vec::new());
    };
    let apps = if device.booted {
        jevision_runner::simulator_apps(&Xcrun, &device.udid)?
    } else {
        jevision_runner::installed_apps(device)
    };
    Ok(apps
        .into_iter()
        .map(|a| SimulatedApp {
            bundle_id: a.bundle_id,
            name: a.name,
            user: a.user,
        })
        .collect())
}

/// Whether `address`, a place an Open File block opens, is a link, such as a web address or
/// `myapp://inbox`, and not a file.
#[uniffi::export]
pub fn address_is_link(address: String) -> bool {
    jevision_core::is_link(&address)
}
