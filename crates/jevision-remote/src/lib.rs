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
pub use client::{Client, HostRecord, RunWatch};
#[cfg(feature = "host")]
pub use host::{Host, HostConfig, HostDelegate, HostStatus, Project};
pub use jevision_runner::{Failure, RunReport, StepStatus, Verdict};
pub use pairing::{DeviceSigner, PairingOffer};
#[cfg(feature = "host")]
pub use store::{Activity, ActivityKind, Device};
pub use wire::{
    Area, BlockSummary, EdgeSummary, FrameSummary, LastRun, PauseReason, Position, ProjectInfo,
    RunEvent, RunList, RunState, RunStatus, RunSummary, RunUpdate, TestEntry, TestSummary,
};

/// The one protocol spoken between the Mac and the phone.
pub const ALPN: &[u8] = b"jevision/remote/1";

/// What a paired phone may do. Every request is checked against the device's current level,
/// so a change on the Mac applies to the next request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    /// List projects and tests, look at tests, runs, reports and pictures.
    View,
    /// Everything `View` allows, plus starting and stopping runs.
    Run,
}

/// Why something didn't happen, as a stable code shared by the Mac and the phone.
///
/// Codes travel between the two apps, so a variant is never renamed or reused. Each app shows
/// the text of [`RemoteError::key`] in its own language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum RemoteError {
    /// The pairing code is wrong, was cancelled, or the Mac shows no code at the moment.
    #[error("the pairing code is not valid")]
    PairingCodeInvalid,
    #[error("the pairing code has expired")]
    PairingCodeExpired,
    #[error("the pairing code has already been used")]
    PairingCodeUsed,
    /// Too many wrong attempts: the code no longer works and the Mac must show a new one.
    #[error("pairing is locked after too many attempts")]
    PairingLocked,
    /// The phone's key or signature does not check out.
    #[error("the device could not prove who it is")]
    BadSignature,
    /// The Mac does not know this phone: it was never paired or was removed.
    #[error("this device is not paired with the Mac")]
    NotPaired,
    /// The Mac removed this phone while it was connected.
    #[error("the Mac removed this device")]
    Revoked,
    /// The device's permission does not cover the request.
    #[error("this device is not allowed to do that")]
    NotAllowed,
    /// Runs need the Mac unlocked: input would go to the lock screen.
    #[error("the Mac is locked")]
    MacLocked,
    #[error("a run is already in progress")]
    RunInProgress,
    /// Someone at the Mac declined the run.
    #[error("the run was declined on the Mac")]
    DeclinedOnMac,
    /// The Mac cannot be reached: asleep, offline or JEVision is not running there.
    #[error("the Mac cannot be reached")]
    HostOffline,
    #[error("there is no such project")]
    ProjectNotFound,
    #[error("there is no such test")]
    TestNotFound,
    /// The test file exists but cannot be read as a test.
    #[error("the test cannot be read")]
    TestUnreadable,
    #[error("there is no such run")]
    RunNotFound,
    #[error("there is no such picture")]
    PictureNotFound,
    /// A path points outside the project folder.
    #[error("the path is outside the project")]
    PathOutsideProject,
    /// A message is bigger than the protocol allows.
    #[error("the message is too large")]
    TooLarge,
    /// The other side sent something this version does not understand.
    #[error("the other side sent an unexpected message")]
    Protocol,
    /// The pairing QR code is not a JEVision one.
    #[error("this is not a JEVision pairing code")]
    OfferWrongScheme,
    /// The pairing QR code is damaged or incomplete.
    #[error("the pairing code is damaged")]
    OfferMalformed,
    /// The secret key the Mac app supplied is not 32 bytes long.
    #[error("the secret key is not valid")]
    InvalidSecretKey,
    /// The relay address in the settings is not a valid URL.
    #[error("the relay address is not valid")]
    InvalidRelayUrl,
    /// The phone's key could not sign, for example because the person cancelled Face ID.
    #[error("the device key could not sign")]
    SignerFailed,
    /// The network could not be set up.
    #[error("the network is not available")]
    Network,
    #[error("remote access is not turned on")]
    HostNotRunning,
    /// Paired devices or the activity log could not be saved or read.
    #[error("the remote access settings cannot be saved")]
    Storage,
    /// The Mac app failed to do what was asked.
    #[error("the Mac could not do that")]
    HostFailed,
}

impl RemoteError {
    /// Localization key of the error's text, such as `remote.error.macLocked`.
    pub fn key(&self) -> String {
        let code = serde_json::to_value(self).expect("an error code always serializes");
        format!(
            "remote.error.{}",
            code.as_str().expect("error codes are strings")
        )
    }
}

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

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}
