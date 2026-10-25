//! The Mac side: accepts phones, pairs and signs them in, and answers their requests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use iroh::endpoint::{Connection, Incoming, RecvStream, SendStream, VarInt};
use iroh::{Endpoint, EndpointId, SecretKey, Watcher};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;
use tokio::task::{JoinHandle, JoinSet};

use crate::pairing::{
    CODE_LEN, NONCE_LEN, auth_transcript, code_bytes, device_id, pair_transcript, verify,
};
use crate::store::{Activity, ActivityKind, Device, DeviceKind, Store};
use crate::wire::{
    self, AssistantReply, AssistantRequest, CLOSE_DISABLED, CLOSE_DONE, CLOSE_REFUSED,
    CLOSE_REVOKED, HelloKind, HostHello, MAX_ASSISTANT_REPLY, MAX_PICTURE, PROTOCOLS, Proof,
    Protocols, Reply, Request, Response, RunEvent, RunList, RunState, RunStatus, RunUpdate,
    Welcome,
};
use crate::{
    ALPN_ASSISTANT, ALPN_PHONE, APP_VERSION, Network, PairingOffer, Permission, ProjectInfo,
    RemoteError, bind, edit, hex, now, plain_version, project, random, unhex,
};

/// How long a pairing code works.
const OFFER_LIFETIME: Duration = Duration::from_secs(5 * 60);
/// Wrong codes or signatures an offer takes before it stops working.
const MAX_PAIR_FAILURES: u32 = 5;
/// Time a phone has to pair or sign in after connecting.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// Time a refused phone has to read why before the connection closes.
const REFUSAL_GRACE: Duration = Duration::from_secs(3);
/// Device names are cut to this many characters.
const MAX_NAME: usize = 64;
/// What an assistant says about itself is cut to this many characters.
const MAX_ASSISTANT_TEXT: usize = 64;
/// A tool's name is at most this long.
const MAX_TOOL_NAME: usize = 64;
/// Run events kept for a phone that reads slowly; past that it gets the full status again.
const EVENT_BACKLOG: usize = 256;

/// How the Mac app sets up remote access.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// The endpoint's secret key, 32 bytes, from [`crate::generate_secret_key`]. Phones know
    /// the Mac by the public key derived from it, so it must stay the same between launches.
    pub secret_key: Vec<u8>,
    /// The Mac's name as phones show it.
    pub name: String,
    /// Folder for paired devices and the activity log, such as
    /// `~/Library/Application Support/JEVision/Remote`.
    pub state_dir: String,
    pub network: Network,
}

/// A project phones may see. Its folder never leaves the Mac; phones know it by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub root: String,
}

/// Where remote access stands, for the Mac's settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostStatus {
    pub running: bool,
    /// The Mac's endpoint id, hex.
    pub endpoint_id: String,
    /// The home relay, once one is chosen.
    pub relay_url: Option<String>,
    /// Whether the home relay is connected; phones on other networks need it.
    pub relay_connected: bool,
    /// Direct addresses phones on the same network or VPN can use.
    pub addrs: Vec<String>,
    /// Ids of the devices connected now.
    pub connected: Vec<String>,
}

/// What the Mac app does for remote access. Methods may block; they run on a background
/// thread, never on the network tasks.
///
/// `project_root` is always the `root` of a [`Project`] exactly as the app passed it, and
/// `test` a test file relative to it that exists inside the project. A run is known by the
/// name of its folder in `<project>/.jevision/runs`.
pub trait HostDelegate: Send + Sync {
    /// Whether a run can start now: `Err(RemoteError::MacLocked)` while the screen is locked,
    /// `Err(RemoteError::RunInProgress)` while another run drives the Mac.
    fn can_run(&self) -> Result<(), RemoteError>;
    /// Asks the person at the Mac whether `device` may run `test` of `project` (its name). May
    /// wait for the answer; return `false` when nobody answers in reasonable time. Only asked
    /// while [`Host::set_confirm_runs`] is on.
    fn confirm_run(&self, device: Device, project: String, test: String) -> bool;
    /// Starts a run and returns its id once the run folder exists. `row` limits the run to that
    /// row of the test's data table, counted from 0 and known to exist; `None` runs every row.
    fn start_run(
        &self,
        project_root: String,
        test: String,
        row: Option<u32>,
        requested_by: Device,
    ) -> Result<String, RemoteError>;
    fn stop_run(&self, project_root: String, run: String) -> Result<(), RemoteError>;
    /// Whether a phone may change `test` now: `Err(RemoteError::TestBusyOnMac)` while it is
    /// open on the Mac with a change not saved yet.
    fn can_edit(&self, project_root: String, test: String) -> Result<(), RemoteError>;
    /// A phone changed `test` and the file is saved: an open editor shows it now.
    fn test_edited(&self, project_root: String, test: String);
    /// Status of a run that is going on; `None` once it has finished or was never started,
    /// and the saved report answers instead.
    fn run_status(&self, project_root: String, run: String) -> Option<RunStatus>;
    /// Runs of the project going on now.
    fn active_runs(&self, project_root: String) -> Vec<RunStatus>;
    /// An assistant connected and said who it is: the MCP `initialize` params, cleaned up to
    /// `protocolVersion` and `clientInfo`. `session` names the connection until
    /// [`HostDelegate::assistant_ended`]. Returns the MCP `initialize` result.
    fn assistant_hello(&self, session: String, device: Device, params: Value) -> Value;
    /// The MCP `tools/list` result.
    fn assistant_tools(&self) -> Value;
    /// An assistant called `tool`, with `arguments` that are a JSON object, on the project at
    /// `project_root`. Returns the MCP `tools/call` result, which says by `isError` when the
    /// tool itself failed, or the server's own error. May take as long as a run does.
    fn assistant_call(
        &self,
        session: String,
        device: Device,
        project_root: String,
        tool: String,
        arguments: Value,
    ) -> Result<Value, AssistantRejection>;
    /// The connection `session` ended, was revoked or turned off: stop what it started. May be
    /// called more than once.
    fn assistant_ended(&self, session: String);
}

/// The MCP server's own error, such as an unknown tool: a JSON-RPC error code and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantRejection {
    pub code: i64,
    pub message: String,
}

/// Remote access on the Mac.
pub struct Host {
    shared: Arc<Shared>,
}

struct Shared {
    secret: SecretKey,
    id: EndpointId,
    name: String,
    network: Network,
    delegate: Arc<dyn HostDelegate>,
    state: Mutex<State>,
    /// Phones watching a run, by project root and run.
    watchers: Mutex<HashMap<(String, String), broadcast::Sender<RunUpdate>>>,
}

struct State {
    store: Store,
    endpoint: Option<Endpoint>,
    accept: Option<JoinHandle<()>>,
    offer: Option<Offer>,
    projects: Vec<Project>,
    confirm_runs: bool,
    live: Vec<(String, Connection)>,
    protocols: Protocols,
}

pub(crate) struct Offer {
    code: [u8; CODE_LEN],
    kind: DeviceKind,
    permission: Permission,
    pub(crate) expires: Instant,
    used: bool,
    failures: u32,
}

impl Host {
    /// Loads paired devices from `config.state_dir`. Nothing listens until [`Host::start`].
    pub fn new(config: HostConfig, delegate: Arc<dyn HostDelegate>) -> Result<Self, RemoteError> {
        let key: [u8; 32] = config
            .secret_key
            .as_slice()
            .try_into()
            .map_err(|_| RemoteError::InvalidSecretKey)?;
        let secret = SecretKey::from_bytes(&key);
        let store = Store::open(Path::new(&config.state_dir))?;
        Ok(Self {
            shared: Arc::new(Shared {
                id: secret.public(),
                secret,
                name: config.name,
                network: config.network,
                delegate,
                state: Mutex::new(State {
                    store,
                    endpoint: None,
                    accept: None,
                    offer: None,
                    projects: Vec::new(),
                    confirm_runs: false,
                    live: Vec::new(),
                    protocols: PROTOCOLS,
                }),
                watchers: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// Starts accepting phones. Does nothing when already running.
    pub async fn start(&self) -> Result<(), RemoteError> {
        if self.shared.state().endpoint.is_some() {
            return Ok(());
        }
        let endpoint = bind(
            self.shared.secret.clone(),
            &self.shared.network,
            vec![ALPN_PHONE.to_vec(), ALPN_ASSISTANT.to_vec()],
        )
        .await?;
        let spare = {
            let mut state = self.shared.state();
            if state.endpoint.is_some() {
                Some(endpoint)
            } else {
                let shared = Arc::clone(&self.shared);
                let listener = endpoint.clone();
                state.accept = Some(tokio::spawn(async move {
                    while let Some(incoming) = listener.accept().await {
                        tokio::spawn(Arc::clone(&shared).serve(incoming));
                    }
                }));
                state.endpoint = Some(endpoint);
                None
            }
        };
        // Another call started the host meanwhile.
        if let Some(endpoint) = spare {
            endpoint.close().await;
        }
        Ok(())
    }

    /// Disconnects every phone and stops accepting new ones. A pairing code shown stops
    /// working.
    pub async fn stop(&self) {
        let endpoint = {
            let mut state = self.shared.state();
            state.offer = None;
            if let Some(task) = state.accept.take() {
                task.abort();
            }
            for (_, conn) in state.live.drain(..) {
                conn.close(VarInt::from_u32(CLOSE_DONE), b"");
            }
            state.endpoint.take()
        };
        if let Some(endpoint) = endpoint {
            endpoint.close().await;
        }
    }

    pub fn status(&self) -> HostStatus {
        let state = self.shared.state();
        let mut connected: Vec<String> = state.live.iter().map(|(id, _)| id.clone()).collect();
        connected.sort();
        connected.dedup();
        let (relay_url, relay_connected, addrs) = match &state.endpoint {
            Some(endpoint) => {
                let (relay_url, addrs) = addresses(endpoint);
                let connected = endpoint
                    .home_relay_status()
                    .get()
                    .iter()
                    .any(|r| r.is_connected());
                (relay_url, connected, addrs)
            }
            None => (None, false, Vec::new()),
        };
        HostStatus {
            running: state.endpoint.is_some(),
            endpoint_id: self.shared.id.to_string(),
            relay_url,
            relay_connected,
            addrs,
            connected,
        }
    }

    /// A new pairing code for a phone that gets `permission`. It replaces any code shown
    /// before, works once and for five minutes.
    pub fn create_offer(&self, permission: Permission) -> Result<PairingOffer, RemoteError> {
        self.offer(DeviceKind::Phone, permission)
    }

    /// A new pairing code for an assistant, to show as [`PairingOffer::to_assistant_url`]. The
    /// assistant may use the Mac's tools from the moment it pairs, until the owner turns it
    /// off with [`Host::set_assistant`]. Otherwise like [`Host::create_offer`]: it replaces any
    /// code shown before, works once and for five minutes, and only an assistant can use it.
    pub fn create_assistant_offer(&self) -> Result<PairingOffer, RemoteError> {
        self.offer(DeviceKind::Assistant, Permission::View)
    }

    fn offer(&self, kind: DeviceKind, permission: Permission) -> Result<PairingOffer, RemoteError> {
        let mut state = self.shared.state();
        let endpoint = state.endpoint.as_ref().ok_or(RemoteError::HostNotRunning)?;
        let (relay_url, addrs) = addresses(endpoint);
        let code = random::<CODE_LEN>();
        state.offer = Some(Offer {
            code,
            kind,
            permission,
            expires: Instant::now() + OFFER_LIFETIME,
            used: false,
            failures: 0,
        });
        Ok(PairingOffer {
            host_id: self.shared.id.to_string(),
            host_name: self.shared.name.clone(),
            code: hex(&code),
            expires_at: now() + OFFER_LIFETIME.as_secs() as i64,
            relay_url,
            addrs,
        })
    }

    /// Makes the code shown stop working.
    pub fn cancel_offer(&self) {
        self.shared.state().offer = None;
    }

    pub fn devices(&self) -> Vec<Device> {
        self.shared.state().store.devices().to_vec()
    }

    /// Applies to the device's next request. An assistant has no level: use
    /// [`Host::set_assistant`].
    pub fn set_permission(
        &self,
        device_id: &str,
        permission: Permission,
    ) -> Result<(), RemoteError> {
        let mut state = self.shared.state();
        let mut device = state
            .store
            .device(device_id)
            .cloned()
            .ok_or(RemoteError::NotPaired)?;
        if device.kind != DeviceKind::Phone {
            return Err(RemoteError::NotAllowed);
        }
        device.permission = permission;
        state.store.change(|devices| {
            for d in devices.iter_mut().filter(|d| d.id == device.id) {
                d.permission = permission;
            }
        })?;
        state.store.log(&activity(
            Some(&device),
            ActivityKind::PermissionChanged,
            None,
            None,
        ));
        Ok(())
    }

    /// Lets an assistant use the Mac's tools or not, from its next request. Turning it off
    /// disconnects the assistant and stops what it started. A phone cannot be one.
    pub fn set_assistant(&self, device_id: &str, enabled: bool) -> Result<(), RemoteError> {
        let mut state = self.shared.state();
        let device = state
            .store
            .device(device_id)
            .cloned()
            .ok_or(RemoteError::NotPaired)?;
        if device.kind != DeviceKind::Assistant {
            return Err(RemoteError::NotAllowed);
        }
        state.store.change(|devices| {
            for d in devices.iter_mut().filter(|d| d.id == device.id) {
                d.assistant = enabled;
            }
        })?;
        state.store.log(&activity(
            Some(&device),
            ActivityKind::PermissionChanged,
            None,
            None,
        ));
        if !enabled {
            state.live.retain(|(id, conn)| {
                if *id == device.id {
                    conn.close(VarInt::from_u32(CLOSE_DISABLED), b"");
                }
                *id != device.id
            });
        }
        Ok(())
    }

    /// Forgets the device and disconnects it at once. It has to pair again to come back.
    pub fn revoke(&self, device_id: &str) -> Result<(), RemoteError> {
        let mut state = self.shared.state();
        let device = state
            .store
            .device(device_id)
            .cloned()
            .ok_or(RemoteError::NotPaired)?;
        state
            .store
            .change(|devices| devices.retain(|d| d.id != device.id))?;
        state
            .store
            .log(&activity(Some(&device), ActivityKind::Revoked, None, None));
        state.live.retain(|(id, conn)| {
            if *id == device.id {
                conn.close(VarInt::from_u32(CLOSE_REVOKED), b"");
            }
            *id != device.id
        });
        Ok(())
    }

    /// Whether someone at the Mac must approve each run a phone asks for.
    pub fn set_confirm_runs(&self, confirm: bool) {
        self.shared.state().confirm_runs = confirm;
    }

    /// The projects phones may see, replacing the previous list.
    pub fn set_projects(&self, projects: Vec<Project>) {
        self.shared.state().projects = projects;
    }

    /// The newest `limit` activity entries, newest first.
    pub fn activity(&self, limit: u32) -> Vec<Activity> {
        self.shared.state().store.activity(limit as usize)
    }

    /// Passes progress of a run to the phones watching it. `project_root` is the project's
    /// `root` as given to [`Host::set_projects`]. Send [`RunEvent::Finished`] last, once
    /// [`HostDelegate::run_status`] no longer reports the run.
    pub fn publish(&self, project_root: String, run: String, event: RunEvent) {
        let finished = matches!(event, RunEvent::Finished { .. });
        self.send_update(project_root, run, RunUpdate::Event { event }, finished);
    }

    /// Passes the run's new status to the phones watching it, for a change no event tells,
    /// such as the run waiting for the screen. Phones that don't know the change read it as
    /// any other status.
    pub fn publish_status(&self, project_root: String, run: String, status: RunStatus) {
        let finished = status.state == RunState::Finished;
        self.send_update(project_root, run, RunUpdate::Status { status }, finished);
    }

    fn send_update(&self, project_root: String, run: String, update: RunUpdate, finished: bool) {
        let mut watchers = self
            .shared
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let key = (project_root, run);
        if let Some(sender) = watchers.get(&key)
            && (sender.send(update).is_err() || finished)
        {
            watchers.remove(&key);
        }
    }

    #[cfg(test)]
    pub(crate) fn with_offer(&self, f: impl FnOnce(&mut Offer)) {
        f(self
            .shared
            .state()
            .offer
            .as_mut()
            .expect("an offer is shown"));
    }

    /// Makes this Mac speak other protocol versions, as an older or newer one would.
    #[cfg(test)]
    pub(crate) fn set_protocols(&self, protocols: Protocols) {
        self.shared.state().protocols = protocols;
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let mut state = self.shared.state();
        if let Some(task) = state.accept.take() {
            task.abort();
        }
        for (_, conn) in state.live.drain(..) {
            conn.close(VarInt::from_u32(CLOSE_DONE), b"");
        }
        state.endpoint = None;
    }
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn log(
        &self,
        device: Option<&Device>,
        kind: ActivityKind,
        subject: Option<String>,
        error: Option<RemoteError>,
    ) {
        self.state()
            .store
            .log(&activity(device, kind, subject, error));
    }

    async fn serve(self: Arc<Self>, incoming: Incoming) {
        let Ok(conn) = incoming.await else {
            return;
        };
        let kind = if conn.alpn() == ALPN_ASSISTANT {
            DeviceKind::Assistant
        } else {
            DeviceKind::Phone
        };
        let device =
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, self.handshake(&conn, kind)).await {
                Ok(Ok(device)) => device,
                Ok(Err(_)) => {
                    let _ = tokio::time::timeout(REFUSAL_GRACE, conn.closed()).await;
                    conn.close(VarInt::from_u32(CLOSE_REFUSED), b"");
                    return;
                }
                Err(_) => {
                    conn.close(VarInt::from_u32(CLOSE_REFUSED), b"");
                    return;
                }
            };
        {
            let mut state = self.state();
            if state.store.device(&device).is_none() || state.endpoint.is_none() {
                conn.close(VarInt::from_u32(CLOSE_REVOKED), b"");
                return;
            }
            state.live.push((device.clone(), conn.clone()));
        }
        if kind == DeviceKind::Assistant {
            Arc::clone(&self).serve_assistant(device, &conn).await;
        } else {
            while let Ok((send, recv)) = conn.accept_bi().await {
                tokio::spawn(Arc::clone(&self).request(device.clone(), conn.clone(), send, recv));
            }
        }
        let id = conn.stable_id();
        self.state().live.retain(|(_, c)| c.stable_id() != id);
    }

    /// Answers an assistant's requests until the connection ends, then stops what it started.
    async fn serve_assistant(self: Arc<Self>, device: String, conn: &Connection) {
        let session = conn.stable_id().to_string();
        let mut requests = JoinSet::new();
        while let Ok((send, recv)) = conn.accept_bi().await {
            requests.spawn(Arc::clone(&self).assistant_request(
                session.clone(),
                device.clone(),
                conn.clone(),
                send,
                recv,
            ));
        }
        // Stopping the runs lets the calls waiting for them return; one that began just
        // before the connection ended may have started something new, so stop again after.
        self.end_session(&session).await;
        while requests.join_next().await.is_some() {}
        self.end_session(&session).await;
    }

    async fn end_session(&self, session: &str) {
        let delegate = Arc::clone(&self.delegate);
        let session = session.to_owned();
        let _ = blocking(move || delegate.assistant_ended(session)).await;
    }

    /// Pairs or signs in the phone and returns its device id. A refusal is sent to the phone
    /// before the error is returned.
    async fn handshake(&self, conn: &Connection, kind: DeviceKind) -> Result<String, RemoteError> {
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|_| RemoteError::HostOffline)?;
        let phone = conn.remote_id();
        let protocols = self.state().protocols;
        let result = match wire::expect::<Value>(&mut recv).await {
            Ok(frame) => {
                // Versions that are missing or unreadable count as protocol 1 and no version.
                let theirs = frame
                    .get("protocols")
                    .and_then(|p| Protocols::deserialize(p).ok())
                    .unwrap_or_default();
                let app_version = frame
                    .get("appVersion")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match protocols.agree(theirs) {
                    Ok(protocol) => match HelloKind::deserialize(&frame) {
                        Ok(HelloKind::Pair {
                            code,
                            device_name,
                            device_key,
                            signature,
                        }) => self.pair(phone, kind, &code, &device_name, &device_key, &signature),
                        Ok(HelloKind::SignIn { device_key }) => {
                            self.sign_in(phone, kind, &device_key, &mut send, &mut recv)
                                .await
                        }
                        Ok(HelloKind::Unknown) => Err(RemoteError::Unsupported),
                        Err(_) => Err(RemoteError::Protocol),
                    }
                    .map(|device| (device, protocol)),
                    Err(error) => {
                        self.log_version_refusal(&frame, app_version, error);
                        Err(error)
                    }
                }
            }
            Err(e) => Err(e),
        };
        let reply = match &result {
            Ok((device, protocol)) => {
                let (relay_url, addrs) = self
                    .state()
                    .endpoint
                    .as_ref()
                    .map(addresses)
                    .unwrap_or_default();
                HostHello::Welcome(Welcome {
                    protocol: *protocol,
                    app_version: APP_VERSION.to_owned(),
                    host_name: self.name.clone(),
                    device_id: device.id.clone(),
                    permission: device.permission,
                    relay_url,
                    addrs,
                })
            }
            Err(error) => HostHello::Refused {
                error: *error,
                app_version: (*error == RemoteError::DeviceTooOld).then(|| APP_VERSION.to_owned()),
            },
        };
        wire::send(&mut send, &reply).await?;
        let _ = send.finish();
        result.map(|(device, _)| device.id)
    }

    /// Notes a phone turned away for its protocol version, with the app version that would do:
    /// this Mac's when the phone is too old, the phone's own when the Mac is and it is a plain
    /// version. The rest of `frame` is read as far as it can be: its shape may be a newer one.
    fn log_version_refusal(&self, frame: &Value, app_version: &str, error: RemoteError) {
        let subject = match error {
            RemoteError::DeviceTooOld => Some(APP_VERSION.to_owned()),
            RemoteError::MacTooOld => plain_version(app_version).then(|| app_version.to_owned()),
            _ => return,
        };
        let text = |key: &str| frame.get(key).and_then(Value::as_str);
        let id = text("deviceKey")
            .and_then(unhex)
            .filter(|k| !k.is_empty())
            .map(|k| device_id(&k));
        let state = self.state();
        let (kind, device_name) = if text("type") == Some("signIn") {
            let name = id
                .as_deref()
                .and_then(|id| state.store.device(id))
                .map(|d| d.name.clone());
            (ActivityKind::SignInFailed, name)
        } else {
            (
                ActivityKind::PairingFailed,
                text("deviceName").map(str::to_owned),
            )
        };
        state.store.log(&Activity {
            time: now(),
            device_id: id,
            device_name,
            kind,
            subject,
            error: Some(error),
        });
    }

    fn pair(
        &self,
        phone: EndpointId,
        kind: DeviceKind,
        code: &str,
        name: &str,
        device_key: &str,
        signature: &str,
    ) -> Result<Device, RemoteError> {
        let key = unhex(device_key).unwrap_or_default();
        let result = self.check_pairing(phone, kind, code, name, &key, signature);
        let device_id = (!key.is_empty()).then(|| device_id(&key));
        match &result {
            Ok(device) => self.log(Some(device), ActivityKind::Paired, None, None),
            Err(error) => self.state().store.log(&Activity {
                time: now(),
                device_id,
                device_name: Some(name.to_owned()),
                kind: ActivityKind::PairingFailed,
                subject: None,
                error: Some(*error),
            }),
        }
        result
    }

    fn check_pairing(
        &self,
        phone: EndpointId,
        kind: DeviceKind,
        code: &str,
        name: &str,
        key: &[u8],
        signature: &str,
    ) -> Result<Device, RemoteError> {
        let mut state = self.state();
        let offer = state
            .offer
            .as_mut()
            .ok_or(RemoteError::PairingCodeInvalid)?;
        if offer.failures >= MAX_PAIR_FAILURES {
            return Err(RemoteError::PairingLocked);
        }
        let presented = code_bytes(code).unwrap_or_default();
        if !bool::from(presented.as_slice().ct_eq(offer.code.as_slice())) {
            offer.failures += 1;
            return Err(RemoteError::PairingCodeInvalid);
        }
        // The right code, but shown for the other kind of device: it is not valid here.
        if offer.kind != kind {
            offer.failures += 1;
            return Err(RemoteError::PairingCodeInvalid);
        }
        if offer.used {
            return Err(RemoteError::PairingCodeUsed);
        }
        if Instant::now() >= offer.expires {
            return Err(RemoteError::PairingCodeExpired);
        }
        let signature = unhex(signature).unwrap_or_default();
        if !verify(
            key,
            &pair_transcript(&offer.code, &self.id, &phone, key),
            &signature,
        ) {
            offer.failures += 1;
            return Err(RemoteError::BadSignature);
        }
        let name: String = name.trim().chars().take(MAX_NAME).collect();
        if name.is_empty() {
            return Err(RemoteError::Protocol);
        }
        let device = Device {
            id: device_id(key),
            name,
            public_key: hex(key),
            kind,
            permission: offer.permission,
            assistant: kind == DeviceKind::Assistant,
            paired_at: now(),
            last_seen: Some(now()),
        };
        state.store.change(|devices| {
            devices.retain(|d| d.id != device.id);
            devices.push(device.clone());
        })?;
        if let Some(offer) = state.offer.as_mut() {
            offer.used = true;
        }
        Ok(device)
    }

    /// Challenges the phone and checks the answer. Whether a key is paired is told only to
    /// whoever holds it.
    async fn sign_in(
        &self,
        phone: EndpointId,
        kind: DeviceKind,
        device_key: &str,
        send: &mut SendStream,
        recv: &mut RecvStream,
    ) -> Result<Device, RemoteError> {
        let nonce = random::<NONCE_LEN>();
        wire::send(send, &HostHello::Challenge { nonce: hex(&nonce) }).await?;
        let proof: Proof = wire::expect(recv).await?;
        let key = unhex(device_key).unwrap_or_default();
        let signature = unhex(&proof.signature).unwrap_or_default();
        let id = device_id(&key);
        let result = if !verify(&key, &auth_transcript(&nonce, &self.id, &phone), &signature) {
            Err(RemoteError::BadSignature)
        } else {
            let mut state = self.state();
            match state.store.device(&id).cloned() {
                Some(device) if device.public_key == hex(&key) && device.kind == kind => {
                    let seen = now();
                    // A sign-in still counts when the time of it cannot be saved.
                    let _ = state.store.change(|devices| {
                        for d in devices.iter_mut().filter(|d| d.id == id) {
                            d.last_seen = Some(seen);
                        }
                    });
                    Ok(Device {
                        last_seen: Some(seen),
                        ..device
                    })
                }
                _ => Err(RemoteError::NotPaired),
            }
        };
        match &result {
            Ok(device) => self.log(Some(device), ActivityKind::SignedIn, None, None),
            Err(error) => self.state().store.log(&Activity {
                time: now(),
                device_id: Some(id),
                device_name: None,
                kind: ActivityKind::SignInFailed,
                subject: None,
                error: Some(*error),
            }),
        }
        result
    }

    async fn request(
        self: Arc<Self>,
        device_id: String,
        conn: Connection,
        mut send: SendStream,
        mut recv: RecvStream,
    ) {
        let request = match wire::expect::<Request>(&mut recv).await {
            Ok(request) => request,
            Err(error) => {
                let _ = wire::send(&mut send, &Reply::Error(error)).await;
                let _ = send.finish();
                return;
            }
        };
        let Some(device) = self.state().store.device(&device_id).cloned() else {
            let _ = wire::send(&mut send, &Reply::Error(RemoteError::Revoked)).await;
            conn.close(VarInt::from_u32(CLOSE_REVOKED), b"");
            return;
        };
        if let Request::Watch { project, run } = &request {
            return self.watch(&device, project, run, send).await;
        }
        let result = self.answer(&device, &request).await;
        self.audit(&device, &request, result.as_ref().err().copied());
        match result {
            Ok((response, picture)) => {
                if wire::send(&mut send, &Reply::Ok(response)).await.is_ok()
                    && let Some(bytes) = picture
                {
                    let _ = wire::send_bytes(&mut send, &bytes, MAX_PICTURE).await;
                }
            }
            Err(error) => {
                let _ = wire::send(&mut send, &Reply::Error(error)).await;
            }
        }
        let _ = send.finish();
    }

    async fn answer(
        &self,
        device: &Device,
        request: &Request,
    ) -> Result<(Response, Option<Vec<u8>>), RemoteError> {
        if !device.permission.allows(request.needs()) {
            return Err(RemoteError::NotAllowed);
        }
        let response = match request.clone() {
            Request::ListProjects => Response::Projects {
                projects: self
                    .state()
                    .projects
                    .iter()
                    .map(|p| ProjectInfo {
                        name: p.name.clone(),
                    })
                    .collect(),
            },
            Request::ListTests { project } => {
                let (_, root) = self.project(&project)?;
                let tests = blocking(move || project::tests(&root)).await??;
                Response::Tests { tests }
            }
            Request::GetTest { project, path } => {
                let (_, root) = self.project(&project)?;
                let test = blocking(move || project::summary(&root, &path)).await??;
                Response::Test {
                    test: Box::new(test),
                }
            }
            Request::RunTest { project, path, row } => {
                let (given, root) = self.project(&project)?;
                let checked = path.clone();
                blocking(move || match row {
                    Some(row) => match project::data_rows(&root, &checked)?.len() {
                        rows if (row as usize) < rows => Ok(()),
                        _ => Err(RemoteError::DataRowNotFound),
                    },
                    None => project::test_file(&root, &checked).map(drop),
                })
                .await??;
                let delegate = Arc::clone(&self.delegate);
                let confirm = self.state().confirm_runs;
                let device = device.clone();
                let run = blocking(move || {
                    delegate.can_run()?;
                    if confirm && !delegate.confirm_run(device.clone(), project, path.clone()) {
                        return Err(RemoteError::DeclinedOnMac);
                    }
                    delegate.start_run(given, path, row, device)
                })
                .await??;
                Response::RunStarted { run }
            }
            Request::StopRun { project, run } => {
                let (given, _) = self.project(&project)?;
                project::check_run(&run)?;
                let delegate = Arc::clone(&self.delegate);
                blocking(move || delegate.stop_run(given, run)).await??;
                Response::RunStopped
            }
            Request::GetRunStatus { project, run } => Response::Status {
                status: self.run_status(&project, &run).await?,
            },
            Request::ListRuns { project } => {
                let (given, root) = self.project(&project)?;
                let delegate = Arc::clone(&self.delegate);
                let runs = blocking(move || RunList {
                    active: delegate.active_runs(given),
                    saved: project::saved_runs(&root),
                })
                .await?;
                Response::Runs { runs }
            }
            Request::GetReport { project, run } => {
                let (_, root) = self.project(&project)?;
                let report = blocking(move || project::report(&root, &run)).await??;
                Response::Report {
                    report: Box::new(report),
                }
            }
            Request::GetPicture {
                project,
                run,
                picture,
            } => {
                let (_, root) = self.project(&project)?;
                let bytes = blocking(move || project::picture(&root, &run, &picture)).await??;
                return Ok((
                    Response::Picture {
                        size: bytes.len() as u64,
                    },
                    Some(bytes),
                ));
            }
            Request::EditTest {
                project,
                path,
                revision,
                edit,
            } => {
                let (given, root) = self.project(&project)?;
                let delegate = Arc::clone(&self.delegate);
                let (test, select) = blocking(move || {
                    let (file, text, mut flow) = project::read_test(&root, &path)?;
                    if edit::revision(&text) != revision {
                        return Err(RemoteError::TestChanged);
                    }
                    delegate.can_edit(given.clone(), path.clone())?;
                    let select = edit::apply(&mut flow, edit)?;
                    edit::save(&root, &file, &flow)?;
                    delegate.test_edited(given, path.clone());
                    Ok((project::summary(&root, &path)?, select))
                })
                .await??;
                Response::Edited {
                    test: Box::new(test),
                    select,
                }
            }
            Request::Watch { .. } => return Err(RemoteError::Protocol),
            Request::Unknown => return Err(RemoteError::Unsupported),
        };
        Ok((response, None))
    }

    async fn assistant_request(
        self: Arc<Self>,
        session: String,
        device_id: String,
        conn: Connection,
        mut send: SendStream,
        mut recv: RecvStream,
    ) {
        let reply = match wire::expect::<AssistantRequest>(&mut recv).await {
            Ok(request) => self.assistant_answer(session, &device_id, request).await,
            Err(error) => AssistantReply::Error { error },
        };
        let mut sent = wire::send_within(&mut send, &reply, MAX_ASSISTANT_REPLY).await;
        if sent == Err(RemoteError::TooLarge) {
            let error = AssistantReply::Error {
                error: RemoteError::TooLarge,
            };
            sent = wire::send(&mut send, &error).await;
        }
        let _ = send.finish();
        if sent.is_ok()
            && matches!(
                reply,
                AssistantReply::Error {
                    error: RemoteError::Revoked
                }
            )
        {
            conn.close(VarInt::from_u32(CLOSE_REVOKED), b"");
        }
    }

    /// Passes an assistant's request to the app if the owner lets the device use assistants.
    /// Only the tool's name is ever logged: its arguments may hold anything.
    async fn assistant_answer(
        &self,
        session: String,
        device_id: &str,
        request: AssistantRequest,
    ) -> AssistantReply {
        let Some(device) = self.state().store.device(device_id).cloned() else {
            return AssistantReply::Error {
                error: RemoteError::Revoked,
            };
        };
        let subject = match &request {
            AssistantRequest::Call { tool, .. } if valid_tool(tool) => Some(tool.clone()),
            _ => None,
        };
        let reply = if device.kind != DeviceKind::Assistant || !device.assistant {
            AssistantReply::Error {
                error: RemoteError::NotAllowed,
            }
        } else {
            self.assistant_reply(session, &device, request).await
        };
        let error = match &reply {
            AssistantReply::Error { error } => Some(*error),
            _ => None,
        };
        if subject.is_some() || error == Some(RemoteError::NotAllowed) {
            let kind = match error {
                Some(RemoteError::NotAllowed) => ActivityKind::Denied,
                _ => ActivityKind::AssistantCalled,
            };
            self.log(Some(&device), kind, subject, error);
        }
        reply
    }

    async fn assistant_reply(
        &self,
        session: String,
        device: &Device,
        request: AssistantRequest,
    ) -> AssistantReply {
        let delegate = Arc::clone(&self.delegate);
        let device = device.clone();
        let outcome = match request {
            AssistantRequest::Hello { params } => {
                let Some(params) = clean_hello(&params) else {
                    return protocol_error();
                };
                blocking(move || Ok(delegate.assistant_hello(session, device, params))).await
            }
            AssistantRequest::Tools => blocking(move || Ok(delegate.assistant_tools())).await,
            AssistantRequest::Call {
                tool,
                arguments,
                project,
            } => {
                if !valid_tool(&tool) || !arguments.is_object() {
                    return protocol_error();
                }
                let root = match self.assistant_project(project) {
                    Ok(root) => root,
                    Err(error) => return AssistantReply::Error { error },
                };
                blocking(move || delegate.assistant_call(session, device, root, tool, arguments))
                    .await
            }
            AssistantRequest::Unknown => {
                return AssistantReply::Error {
                    error: RemoteError::Unsupported,
                };
            }
        };
        match outcome {
            Ok(Ok(result)) => AssistantReply::Ok { result },
            Ok(Err(AssistantRejection { code, message })) => {
                AssistantReply::Rejected { code, message }
            }
            Err(error) => AssistantReply::Error { error },
        }
    }

    /// The root, as the app gave it, of the project an assistant names; without a name, of the
    /// only project the Mac shares.
    fn assistant_project(&self, name: Option<String>) -> Result<String, RemoteError> {
        let name = match name {
            Some(name) => name,
            None => match self.state().projects.as_slice() {
                [only] => only.name.clone(),
                _ => return Err(RemoteError::ProjectNotFound),
            },
        };
        self.project(&name).map(|(given, _)| given)
    }

    /// Runs, stops, changes, reports read and anything refused for permission or path go to the log.
    fn audit(&self, device: &Device, request: &Request, error: Option<RemoteError>) {
        let (kind, subject) = match request {
            Request::RunTest { project, path, .. } => {
                (ActivityKind::RunStarted, format!("{project}/{path}"))
            }
            Request::StopRun { project, run } => {
                (ActivityKind::RunStopped, format!("{project}/{run}"))
            }
            Request::GetReport { project, run } => {
                (ActivityKind::ReportRead, format!("{project}/{run}"))
            }
            Request::EditTest { project, path, .. } => {
                (ActivityKind::TestEdited, format!("{project}/{path}"))
            }
            _ if matches!(
                error,
                Some(RemoteError::NotAllowed | RemoteError::PathOutsideProject)
            ) =>
            {
                let subject = match request {
                    Request::ListTests { project } | Request::ListRuns { project } => {
                        project.clone()
                    }
                    Request::GetTest { project, path } => format!("{project}/{path}"),
                    Request::GetRunStatus { project, run }
                    | Request::Watch { project, run }
                    | Request::GetPicture { project, run, .. } => format!("{project}/{run}"),
                    _ => String::new(),
                };
                (ActivityKind::Denied, subject)
            }
            _ => return,
        };
        self.log(Some(device), kind, Some(subject), error);
    }

    /// The project's root as the app gave it and its folder with symlinks resolved.
    fn project(&self, name: &str) -> Result<(String, PathBuf), RemoteError> {
        let given = self
            .state()
            .projects
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.root.clone())
            .ok_or(RemoteError::ProjectNotFound)?;
        let root = project::root(Path::new(&given))?;
        Ok((given, root))
    }

    async fn run_status(&self, project: &str, run: &str) -> Result<RunStatus, RemoteError> {
        let (given, root) = self.project(project)?;
        project::check_run(run)?;
        let delegate = Arc::clone(&self.delegate);
        let run = run.to_owned();
        blocking(move || match delegate.run_status(given, run.clone()) {
            Some(status) => Ok(status),
            None => project::saved_status(&root, &run),
        })
        .await?
    }

    async fn watch(&self, device: &Device, project: &str, run: &str, mut send: SendStream) {
        let started = async {
            let (given, _) = self.project(project)?;
            project::check_run(run)?;
            // Subscribe before asking for the status, so nothing falls between the two.
            let events = self
                .watchers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry((given, run.to_owned()))
                .or_insert_with(|| broadcast::channel(EVENT_BACKLOG).0)
                .subscribe();
            let status = self.run_status(project, run).await?;
            Ok((events, status))
        }
        .await;
        let (mut events, status) = match started {
            Ok(started) => started,
            Err(error) => {
                self.audit(
                    device,
                    &Request::Watch {
                        project: project.to_owned(),
                        run: run.to_owned(),
                    },
                    Some(error),
                );
                let _ = wire::send(&mut send, &Reply::Error(error)).await;
                let _ = send.finish();
                return;
            }
        };
        let finished = status.state == RunState::Finished;
        if wire::send(&mut send, &Reply::Ok(Response::Status { status }))
            .await
            .is_err()
            || finished
        {
            let _ = send.finish();
            return;
        }
        let stopped = send.stopped();
        tokio::pin!(stopped);
        loop {
            let update = tokio::select! {
                update = events.recv() => match update {
                    Ok(update) => update,
                    Err(broadcast::error::RecvError::Lagged(_)) => match self.run_status(project, run).await {
                        Ok(status) => RunUpdate::Status { status },
                        Err(_) => break,
                    },
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = &mut stopped => return,
            };
            let last = matches!(
                &update,
                RunUpdate::Event {
                    event: RunEvent::Finished { .. }
                }
            ) || matches!(&update, RunUpdate::Status { status } if status.state == RunState::Finished);
            if wire::send(&mut send, &update).await.is_err() || last {
                break;
            }
        }
        let _ = send.finish();
    }
}

fn protocol_error() -> AssistantReply {
    AssistantReply::Error {
        error: RemoteError::Protocol,
    }
}

/// A tool name that is safe to show and to write to the log.
fn valid_tool(tool: &str) -> bool {
    !tool.is_empty()
        && tool.len() <= MAX_TOOL_NAME
        && tool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// What an assistant says about itself, cut down to what the Mac shows: short text without
/// control characters.
fn clean_hello(params: &Value) -> Option<Value> {
    let params = params.as_object()?;
    let text = |value: Option<&Value>| -> String {
        value
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_ASSISTANT_TEXT)
            .collect()
    };
    let info = params.get("clientInfo").and_then(Value::as_object);
    let said = |key: &str| text(info.and_then(|i| i.get(key)));
    Some(json!({
        "protocolVersion": text(params.get("protocolVersion")),
        "clientInfo": {
            "name": said("name"),
            "title": said("title"),
            "version": said("version"),
        },
    }))
}

fn addresses(endpoint: &Endpoint) -> (Option<String>, Vec<String>) {
    let addr = endpoint.addr();
    (
        addr.relay_urls().next().map(ToString::to_string),
        addr.ip_addrs().map(ToString::to_string).collect(),
    )
}

fn activity(
    device: Option<&Device>,
    kind: ActivityKind,
    subject: Option<String>,
    error: Option<RemoteError>,
) -> Activity {
    Activity {
        time: now(),
        device_id: device.map(|d| d.id.clone()),
        device_name: device.map(|d| d.name.clone()),
        kind,
        subject,
        error,
    }
}

/// Runs file access and the app's callbacks off the network tasks. A callback that panics
/// fails the request instead of the host.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, RemoteError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| RemoteError::HostFailed)
}
