//! Assistants on other computers. The app serves them the same tools as the assistants on this
//! Mac, over the connection of remote access: what they may run and capture, the question the
//! person is asked before a run and the locked screen are decided by the very same code, with
//! the same [`AssistantHost`]. The command `jevision mcp --remote` on the other computer is the
//! other end: it carries the lines an assistant writes to the tools on the Mac.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use jevision_l10n::tr;
use jevision_remote::{
    self as remote, AssistantClient, AssistantFailure, HostRecord, PairingOffer, RemoteError,
};
use serde_json::{Value, json};

use super::agents::{Agents, HEARTBEAT};
use super::jobs::Jobs;
use super::{
    AssistantHost, AssistantServerError, Backend, INVALID_PARAMS, Server, initialize, serve_lines,
    text, tools,
};

/// The JSON-RPC code for a failure of the connection, not of the request.
const SERVER_ERROR: i64 = -32000;

/// The tools of this Mac for the assistants connected over the network, one [`Link`] each.
pub(crate) struct Assistants {
    host: Arc<dyn AssistantHost>,
    links: Mutex<HashMap<String, Arc<Link>>>,
}

/// What one connection of an assistant has: who it is, its runs, and a server for each project
/// it uses. They share the person's answers, the permissions and the one run at a time, as the
/// tools of one local assistant do.
struct Link {
    agents: Arc<Agents>,
    jobs: Arc<Jobs>,
    servers: Mutex<HashMap<String, Arc<Server>>>,
    /// Dropping it ends the heartbeat.
    hangup: Mutex<Option<Sender<()>>>,
}

impl Assistants {
    pub(crate) fn new(host: Arc<dyn AssistantHost>) -> Self {
        Self {
            host,
            links: Mutex::default(),
        }
    }

    /// The assistant of `session` said who it is.
    pub(crate) fn hello(&self, session: &str, device: &remote::Device, params: Value) -> Value {
        self.link(session, device).agents.hello(&params);
        initialize(&params)
    }

    pub(crate) fn tools(&self) -> Value {
        json!({ "tools": tools::definitions() })
    }

    /// The server's own answer to a call, or the error it gives, as an MCP server would.
    pub(crate) fn call(
        &self,
        session: &str,
        device: &remote::Device,
        project: &str,
        tool: String,
        arguments: Value,
    ) -> Result<Value, remote::AssistantRejection> {
        let link = self.link(session, device);
        let Ok(server) = link.server(project, &self.host) else {
            return Ok(json!({
                "content": [text(tr!("mcp.remote.noProjectFolder"))],
                "isError": true,
            }));
        };
        server
            .call(&json!({ "name": tool, "arguments": arguments }))
            .map_err(|(code, message)| remote::AssistantRejection { code, message })
    }

    /// The connection `session` is over: what it started stops and its reports are saved. Safe
    /// to call again.
    pub(crate) fn ended(&self, session: &str) {
        let Some(link) = lock(&self.links).remove(session) else {
            return;
        };
        lock(&link.hangup).take();
        link.jobs.stop_all();
        link.jobs.wait_all();
        link.agents.end();
    }

    fn link(&self, session: &str, device: &remote::Device) -> Arc<Link> {
        let mut links = lock(&self.links);
        if let Some(link) = links.get(session) {
            return Arc::clone(link);
        }
        let agents = Arc::new(Agents::remote(&self.host.shared_folder(), &device.name));
        let (hangup, hung_up) = mpsc::channel::<()>();
        let beating = Arc::clone(&agents);
        // While the assistant is connected, the app can tell it is.
        let _ = std::thread::Builder::new()
            .name("jevision-assistant-link".into())
            .spawn(move || {
                while hung_up.recv_timeout(HEARTBEAT) == Err(RecvTimeoutError::Timeout) {
                    beating.beat();
                }
            });
        let link = Arc::new(Link {
            agents,
            jobs: Arc::default(),
            servers: Mutex::default(),
            hangup: Mutex::new(Some(hangup)),
        });
        links.insert(session.to_owned(), Arc::clone(&link));
        link
    }
}

impl Link {
    fn server(
        &self,
        project: &str,
        host: &Arc<dyn AssistantHost>,
    ) -> Result<Arc<Server>, AssistantServerError> {
        let mut servers = lock(&self.servers);
        if let Some(server) = servers.get(project) {
            return Ok(Arc::clone(server));
        }
        let server = Arc::new(Server::shared(
            Path::new(project),
            Arc::clone(host),
            Arc::clone(&self.agents),
            Arc::clone(&self.jobs),
        )?);
        servers.insert(project.to_owned(), Arc::clone(&server));
        Ok(server)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// This computer's own P-256 key, implemented in Swift on top of the Keychain.
#[uniffi::export(with_foreign)]
pub trait AssistantKey: Send + Sync {
    /// The public key in SEC1 uncompressed form: 65 bytes starting with `0x04`.
    fn public_key(&self) -> Vec<u8>;
    /// An ECDSA P-256 signature over the SHA-256 digest of `message`, DER-encoded, or `None`
    /// when the key can't sign.
    fn sign(&self, message: Vec<u8>) -> Option<Vec<u8>>;
}

struct Key(Arc<dyn AssistantKey>);

impl remote::DeviceSigner for Key {
    fn public_key(&self) -> Vec<u8> {
        self.0.public_key()
    }

    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
        self.0.sign(message).ok_or(RemoteError::SignerFailed)
    }
}

/// Pairs this computer with the Mac showing `link`, the text `jevision://assistant?…`, and
/// returns the Mac to keep. The Mac lists this computer as `computer_name`. The error says in
/// words for the person at the terminal why it didn't work.
#[uniffi::export]
pub fn pair_remote_assistant(
    link: String,
    computer_name: String,
    key: Arc<dyn AssistantKey>,
) -> Result<HostRecord, AssistantServerError> {
    let mac = PairingOffer::parse_assistant(&link).ok().map(|o| o.host_id);
    pair(link, computer_name, key, remote::Network::default()).map_err(|e| {
        AssistantServerError::Io {
            reason: describe_pairing(e, mac.as_deref()),
        }
    })
}

pub(crate) fn pair(
    link: String,
    computer_name: String,
    key: Arc<dyn AssistantKey>,
    network: remote::Network,
) -> Result<HostRecord, RemoteError> {
    let offer = PairingOffer::parse_assistant(&link)?;
    let runtime = runtime()?;
    runtime.block_on(async {
        let client =
            AssistantClient::pair(offer, computer_name, Arc::new(Key(key)), network).await?;
        let record = client.record();
        client.close().await;
        Ok(record)
    })
}

/// Serves the tools of the paired Mac `mac` over standard input and output until the input
/// ends, as `serve_assistants` does for this Mac. `project` names the project the Mac shares
/// that the assistant works on; without it the Mac uses the only one it shares.
#[uniffi::export]
pub fn serve_remote_assistant(
    mac: HostRecord,
    key: Arc<dyn AssistantKey>,
    project: Option<String>,
) -> Result<(), AssistantServerError> {
    serve(
        mac,
        key,
        project,
        remote::Network::default(),
        io::stdin().lock(),
        io::stdout(),
    )
}

pub(crate) fn serve(
    mac: HostRecord,
    key: Arc<dyn AssistantKey>,
    project: Option<String>,
    network: remote::Network,
    input: impl io::BufRead,
    output: impl io::Write + Send,
) -> Result<(), AssistantServerError> {
    let io_error = |reason: &str| AssistantServerError::Io {
        reason: reason.to_owned(),
    };
    let runtime = runtime().map_err(|_| io_error(&tr!("mcp.remote.networkSetup")))?;
    let host_id = mac.host_id.clone();
    let client = runtime
        .block_on(AssistantClient::open(mac, Arc::new(Key(key)), network))
        .map_err(|e| io_error(&describe(e, Some(&host_id))))?;
    let bridge = Bridge {
        client,
        runtime,
        project,
    };
    serve_lines(&bridge, input, output).map_err(|e| AssistantServerError::Io {
        reason: e.to_string(),
    })
}

fn runtime() -> Result<tokio::runtime::Runtime, RemoteError> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("jevision-assistant-link")
        .enable_all()
        .build()
        .map_err(|_| RemoteError::Network)
}

/// The tools on another Mac, as the assistant sees them.
struct Bridge {
    client: AssistantClient,
    runtime: tokio::runtime::Runtime,
    project: Option<String>,
}

impl Backend for Bridge {
    fn initialize(&self, params: &Value) -> Result<Value, (i64, String)> {
        self.runtime
            .block_on(self.client.hello(params.clone()))
            .map_err(|f| self.failure(f))
    }

    fn tools(&self) -> Result<Value, (i64, String)> {
        self.runtime
            .block_on(self.client.tools())
            .map_err(|f| self.failure(f))
    }

    fn call(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "Invalid params: name the tool".to_owned()))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err((
                INVALID_PARAMS,
                "Invalid params: arguments must be an object".to_owned(),
            ));
        }
        match self.runtime.block_on(self.client.call(
            name.to_owned(),
            arguments,
            self.project.clone(),
        )) {
            Ok(result) => Ok(result),
            Err(AssistantFailure::Rejected { code, message }) => Err((code, message)),
            // A tool that couldn't run is an error of the tool, which the assistant reads.
            Err(AssistantFailure::Remote(error)) => Ok(json!({
                "content": [text(self.describe(error))],
                "isError": true,
            })),
        }
    }

    fn beat(&self) {}

    /// Cutting the connection stops what the Mac started for this assistant.
    fn hang_up(&self) {
        self.runtime.block_on(self.client.close());
    }

    fn finish(&self) {}
}

impl Bridge {
    fn failure(&self, failure: AssistantFailure) -> (i64, String) {
        match failure {
            AssistantFailure::Rejected { code, message } => (code, message),
            AssistantFailure::Remote(error) => (SERVER_ERROR, self.describe(error)),
        }
    }

    fn describe(&self, error: RemoteError) -> String {
        describe(error, Some(&self.client.record().host_id))
    }
}

/// Why pairing didn't work, in words for the person at the terminal. `mac` is the id of the
/// Mac, when the link was readable.
fn describe_pairing(error: RemoteError, mac: Option<&str>) -> String {
    use RemoteError::*;
    match error {
        OfferWrongScheme | OfferMalformed | PairingCodeInvalid => tr!("mcp.pairing.invalidCode"),
        PairingCodeExpired => tr!("mcp.pairing.codeExpired"),
        PairingCodeUsed => tr!("mcp.pairing.codeUsed"),
        PairingLocked => tr!("mcp.pairing.locked"),
        HostOffline => tr!("mcp.pairing.hostOffline"),
        other => describe(other, mac),
    }
}

/// What went wrong between this computer and the Mac `mac`, in words for the assistant to pass
/// on.
fn describe(error: RemoteError, mac: Option<&str>) -> String {
    use RemoteError::*;
    match error {
        NotPaired | Revoked | BadSignature => tr!("mcp.remote.notPaired"),
        NotAllowed => tr!("mcp.remote.notAllowed"),
        HostOffline => tr!("mcp.remote.hostOffline"),
        Network => tr!("mcp.remote.network"),
        SignerFailed => tr!("mcp.remote.signerFailed"),
        ProjectNotFound => tr!("mcp.remote.projectNotFound"),
        TooLarge => tr!("mcp.remote.tooLarge"),
        // The Mac tells its version when it finds this computer too old; a link of a newer
        // format comes before any contact, so then the newest version is the one to get.
        DeviceTooOld => match mac.and_then(remote::mac_version) {
            Some(version) => tr!("mcp.remote.deviceTooOld", version),
            None => tr!("mcp.remote.deviceTooOldLatest"),
        },
        MacTooOld => tr!("mcp.remote.macTooOld", remote::APP_VERSION),
        Unsupported => tr!("mcp.remote.unsupported"),
        Unknown => tr!("mcp.remote.unknown"),
        other => other.to_string(),
    }
}
