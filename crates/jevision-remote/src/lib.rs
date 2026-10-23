//! Remote control of JEVision from a phone.
//!
//! The Mac is the host: tests, reports and pictures never leave it unless a paired phone asks
//! for them. The phone is a remote: it lists projects and tests, starts and stops runs, watches
//! them live and reads reports.
//!
//! Both sides talk over [iroh](https://iroh.computer) QUIC connections dialed by public key:
//! directly on the same network or through a hole-punched path, and through relay servers that
//! only ever see ciphertext when nothing else works. The Mac never opens a port to the internet.
//!
//! A phone pairs once by scanning a [`PairingOffer`] shown on the Mac and proves itself on every
//! later connection with a key that never leaves it ([`DeviceSigner`]). Both signatures cover
//! the endpoint ids of the two sides, which iroh's TLS authenticates, so a relay or anyone in
//! the middle cannot replay them.
//!
//! The Mac side is [`Host`] (feature `host`), the phone side [`Client`] (feature `client`).
//!
//! Assistants on another computer use a second protocol, [`ALPN_ASSISTANT`], on the same
//! endpoint: the same pairing and sign-in, then requests that carry MCP tool calls to the Mac
//! ([`AssistantClient`]). Phones and assistants cannot use each other's protocol.
//!
//! The protocol names say what a connection is for and never change. Which version of the
//! messages the two sides speak is agreed in the first message: each side says the range it
//! knows and the Mac picks the newest version both know, or refuses with
//! [`RemoteError::DeviceTooOld`] or [`RemoteError::MacTooOld`]. There is no list of features:
//! a field one side doesn't know is ignored and a missing one reads as empty, so new fields
//! need no new version.

mod pairing;
mod wire;

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "host")]
mod host;
#[cfg(feature = "host")]
mod project;
#[cfg(feature = "host")]
mod store;
#[cfg(all(test, feature = "host", feature = "client"))]
mod tests;

use iroh::{Endpoint, RelayMode, endpoint::presets};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use serde::{Deserialize, Serialize};

#[cfg(feature = "client")]
pub use client::{AssistantClient, AssistantFailure, Client, HostRecord, RunWatch, mac_version};
#[cfg(feature = "host")]
pub use host::{AssistantRejection, Host, HostConfig, HostDelegate, HostStatus, Project};
pub use jevision_runner::{Failure, RunReport, StepStatus, Verdict};
pub use pairing::{DeviceSigner, PairingOffer};
#[cfg(feature = "host")]
pub use project::running_title;
#[cfg(feature = "host")]
pub use store::{Activity, ActivityKind, Device, DeviceKind};
pub use wire::{
    Area, BlockSummary, EdgeSummary, FrameSummary, LastRun, NoteAttachmentSummary,
    NoteReplySummary, NoteResolvedSummary, NoteSummary, PauseReason, Position, ProjectInfo,
    RunEvent, RunList, RunState, RunStatus, RunSummary, RunUpdate, ScreenHolder, ScreenWait,
    TestEntry, TestSummary,
};

/// The protocol spoken between the Mac and the phone.
pub const ALPN_PHONE: &[u8] = b"jevision/phone/1";

/// The protocol spoken between the Mac and an assistant on another computer.
pub const ALPN_ASSISTANT: &[u8] = b"jevision/assistant/1";

/// This app's version. The Mac app, the phone app and the terminal tool ship together, so one
/// version tells which to update.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a paired phone may do. Every request is checked against the device's current level,
/// so a change on the Mac applies to the next request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    /// Everything `View` allows, plus starting and stopping runs.
    Run,
    /// List projects and tests, look at tests, runs, reports and pictures. A level from a newer
    /// Mac reads as this one, the least.
    #[serde(other)]
    View,
}

/// Why something didn't happen, as a stable code shared by the Mac and the phone.
///
/// Codes travel between the two apps, so a variant is never renamed or reused. Each app shows
/// the text of [`RemoteError::key`] in its own language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteError {
    /// The pairing code is wrong, was cancelled, or the Mac shows no code at the moment.
    PairingCodeInvalid,
    PairingCodeExpired,
    PairingCodeUsed,
    /// Too many wrong attempts: the code no longer works and the Mac must show a new one.
    PairingLocked,
    /// The phone's key or signature does not check out.
    BadSignature,
    /// The Mac does not know this phone: it was never paired or was removed.
    NotPaired,
    /// The Mac removed this phone while it was connected.
    Revoked,
    /// The device's permission does not cover the request.
    NotAllowed,
    /// Runs need the Mac unlocked: input would go to the lock screen.
    MacLocked,
    RunInProgress,
    /// Someone at the Mac declined the run.
    DeclinedOnMac,
    /// The Mac cannot be reached: asleep, offline or JEVision is not running there.
    HostOffline,
    ProjectNotFound,
    TestNotFound,
    /// The test file exists but cannot be read as a test.
    TestUnreadable,
    RunNotFound,
    PictureNotFound,
    /// A path points outside the project folder.
    PathOutsideProject,
    /// A message is bigger than the protocol allows.
    TooLarge,
    /// The other side sent something this version does not understand.
    Protocol,
    /// The pairing QR code is not a JEVision one.
    OfferWrongScheme,
    /// The pairing QR code is damaged or incomplete.
    OfferMalformed,
    /// The secret key the Mac app supplied is not 32 bytes long.
    InvalidSecretKey,
    /// The relay address in the settings is not a valid URL.
    InvalidRelayUrl,
    /// The phone's key could not sign, for example because the person cancelled Face ID.
    SignerFailed,
    /// The network could not be set up.
    Network,
    HostNotRunning,
    /// Paired devices or the activity log could not be saved or read.
    Storage,
    /// The Mac app failed to do what was asked.
    HostFailed,
    /// The test's data table has no such row, or the test has no data table.
    DataRowNotFound,
    /// The app on the device is too old for this Mac or for the code it scanned.
    DeviceTooOld,
    /// JEVision on the Mac is too old for the device.
    MacTooOld,
    /// The other side doesn't know this request.
    Unsupported,
    /// The list of paired devices was saved by a newer JEVision, so this one cannot change it.
    DevicesFromNewer,
    /// A code from a newer JEVision.
    #[serde(other)]
    Unknown,
}

impl RemoteError {
    /// Localization key of the error's text, such as `remote.error.macLocked`.
    pub fn key(&self) -> String {
        self.text_key().to_owned()
    }

    fn text_key(self) -> &'static str {
        match self {
            Self::PairingCodeInvalid => "remote.error.pairingCodeInvalid",
            Self::PairingCodeExpired => "remote.error.pairingCodeExpired",
            Self::PairingCodeUsed => "remote.error.pairingCodeUsed",
            Self::PairingLocked => "remote.error.pairingLocked",
            Self::BadSignature => "remote.error.badSignature",
            Self::NotPaired => "remote.error.notPaired",
            Self::Revoked => "remote.error.revoked",
            Self::NotAllowed => "remote.error.notAllowed",
            Self::MacLocked => "remote.error.macLocked",
            Self::RunInProgress => "remote.error.runInProgress",
            Self::DeclinedOnMac => "remote.error.declinedOnMac",
            Self::HostOffline => "remote.error.hostOffline",
            Self::ProjectNotFound => "remote.error.projectNotFound",
            Self::TestNotFound => "remote.error.testNotFound",
            Self::TestUnreadable => "remote.error.testUnreadable",
            Self::RunNotFound => "remote.error.runNotFound",
            Self::PictureNotFound => "remote.error.pictureNotFound",
            Self::PathOutsideProject => "remote.error.pathOutsideProject",
            Self::TooLarge => "remote.error.tooLarge",
            Self::Protocol => "remote.error.protocol",
            // Either way the code scanned is not one to pair with, and the apps say so in one text.
            Self::OfferWrongScheme | Self::OfferMalformed => "remote.error.offerMalformed",
            Self::InvalidSecretKey => "remote.error.invalidSecretKey",
            Self::InvalidRelayUrl => "remote.error.invalidRelayUrl",
            Self::SignerFailed => "remote.error.signerFailed",
            Self::Network => "remote.error.network",
            Self::HostNotRunning => "remote.error.hostNotRunning",
            Self::Storage => "remote.error.storage",
            Self::HostFailed => "remote.error.hostFailed",
            Self::DataRowNotFound => "remote.error.dataRowNotFound",
            Self::DeviceTooOld => "remote.error.deviceTooOld",
            Self::MacTooOld => "remote.error.macTooOld",
            Self::Unsupported => "remote.error.unsupported",
            Self::DevicesFromNewer => "remote.error.devicesFromNewer",
            Self::Unknown => "remote.error.unknown",
        }
    }

    /// The error's text in `lang`.
    pub fn text_in(&self, lang: &str) -> String {
        jevision_l10n::text_in(lang, self.text_key(), &[])
    }
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text_in(&jevision_l10n::language()))
    }
}

impl std::error::Error for RemoteError {}

/// How an endpoint reaches the other side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub relay: Relay,
    /// Find the other side on the same Wi-Fi by mDNS. On iOS this needs the multicast
    /// networking entitlement and the local network permission.
    pub local_discovery: bool,
    /// Publish and look up current addresses through the n0 DNS service, so a Mac that changed
    /// networks is still found by its id.
    pub global_discovery: bool,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            relay: Relay::Default,
            local_discovery: true,
            global_discovery: true,
        }
    }
}

/// Relay servers used when no direct path works. They forward ciphertext only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relay {
    /// The public relays run by n0.
    Default,
    /// A relay the user runs, such as `https://relay.example.com`.
    Custom { url: String },
    /// Direct connections only: same network, VPN such as Tailscale, or reachable addresses.
    Disabled,
}

/// A new random secret key for the Mac's endpoint. The Mac app keeps it in the Keychain and
/// passes it to [`HostConfig`]; the endpoint id phones pair with is derived from it.
pub fn generate_secret_key() -> [u8; 32] {
    random()
}

async fn bind(
    secret: iroh::SecretKey,
    network: &Network,
    alpns: Vec<Vec<u8>>,
) -> Result<Endpoint, RemoteError> {
    let relay_mode = match &network.relay {
        Relay::Default => RelayMode::Default,
        Relay::Custom { url } => RelayMode::custom([url
            .parse::<iroh::RelayUrl>()
            .map_err(|_| RemoteError::InvalidRelayUrl)?]),
        Relay::Disabled => RelayMode::Disabled,
    };
    let mut builder = if network.global_discovery {
        Endpoint::builder(presets::N0)
    } else {
        Endpoint::builder(presets::Minimal)
    };
    builder = builder
        .secret_key(secret)
        .alpns(alpns)
        .relay_mode(relay_mode);
    if network.local_discovery {
        builder = builder.address_lookup(MdnsAddressLookup::builder());
    }
    builder.bind().await.map_err(|_| RemoteError::Network)
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the system random number generator works");
    bytes
}

/// Seconds since 1970.
#[cfg(feature = "host")]
fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `text` is a plain app version such as `1.4.0` or `2.0.0-beta.1`. A version another
/// device names is shown only when it is one, never as free text.
fn plain_version(text: &str) -> bool {
    let (numbers, pre) = match text.split_once('-') {
        Some((numbers, pre)) => (numbers, Some(pre)),
        None => (text, None),
    };
    text.len() <= 32
        && numbers
            .split('.')
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|pre| {
            !pre.is_empty()
                && pre
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}
