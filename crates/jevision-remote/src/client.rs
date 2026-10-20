//! The phone side: pairs with a Mac, signs in and makes requests.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use iroh::endpoint::{Connection, ConnectionError, RecvStream, VarInt};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayUrl, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use crate::pairing::{auth_transcript, code_bytes, pair_transcript};
use crate::wire::{
    self, AssistantReply, AssistantRequest, CLOSE_DISABLED, CLOSE_DONE, CLOSE_REVOKED, Hello,
    HostHello, MAX_ASSISTANT_REPLY, MAX_PICTURE, Proof, Reply, Request, Response, Welcome,
};
use crate::{
    ALPN, ALPN_V2, DeviceSigner, Network, PairingOffer, Permission, ProjectInfo, RemoteError,
    RunList, RunReport, RunStatus, RunUpdate, TestEntry, TestSummary, bind, hex, random, unhex,
};

/// Time to reach the Mac and finish signing in.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// A paired Mac as the phone saves it. Addresses are refreshed on every sign-in; read them
/// back with [`Client::record`] and save the record again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRecord {
    /// The Mac's endpoint id, hex.
    pub host_id: String,
    pub name: String,
    /// This phone's id on that Mac.
    pub device_id: String,
    pub relay_url: Option<String>,
    pub addrs: Vec<String>,
}

/// A connection to one Mac. Requests reconnect and sign in again on their own after the
/// connection drops, for example when the phone wakes up or changes networks.
pub struct Client {
    endpoint: Endpoint,
    alpn: &'static [u8],
    signer: Arc<dyn DeviceSigner>,
    host: EndpointId,
    record: Mutex<HostRecord>,
    permission: Mutex<Permission>,
    conn: AsyncMutex<Option<Connection>>,
}

impl Client {
    /// Pairs with the Mac whose QR code was scanned. `device_name` is how the Mac lists this
    /// phone.
    pub async fn pair(
        offer: PairingOffer,
        device_name: String,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Self, RemoteError> {
        Self::pair_on(ALPN, offer, device_name, signer, network).await
    }

    async fn pair_on(
        alpn: &'static [u8],
        offer: PairingOffer,
        device_name: String,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Self, RemoteError> {
        let host: EndpointId = offer
            .host_id
            .parse()
            .map_err(|_| RemoteError::OfferMalformed)?;
        let code = code_bytes(&offer.code).ok_or(RemoteError::OfferMalformed)?;
        let record = HostRecord {
            host_id: offer.host_id,
            name: offer.host_name,
            device_id: String::new(),
            relay_url: offer.relay_url,
            addrs: offer.addrs,
        };
        let client = Self::new(alpn, host, record, signer, &network).await?;
        let device_key = client.signer.public_key();
        let phone = client.endpoint.id();
        let signature = client
            .sign(pair_transcript(&code, &host, &phone, &device_key))
            .await?;
        let hello = Hello::Pair {
            code: offer.code,
            device_name,
            device_key: hex(&device_key),
            signature: hex(&signature),
        };
        let conn = client.handshake(hello).await?;
        *client.conn.lock().await = Some(conn);
        Ok(client)
    }

    /// Connects to a paired Mac and signs in.
    pub async fn connect(
        record: HostRecord,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Self, RemoteError> {
        let host: EndpointId = record.host_id.parse().map_err(|_| RemoteError::Protocol)?;
        let client = Self::new(ALPN, host, record, signer, &network).await?;
        client.connection().await?;
        Ok(client)
    }

    async fn new(
        alpn: &'static [u8],
        host: EndpointId,
        record: HostRecord,
        signer: Arc<dyn DeviceSigner>,
        network: &Network,
    ) -> Result<Self, RemoteError> {
        // A fresh endpoint key on every launch: the phone is known by its device key instead.
        let endpoint = bind(SecretKey::from_bytes(&random()), network, Vec::new()).await?;
        Ok(Self {
            endpoint,
            alpn,
            signer,
            host,
            record: Mutex::new(record),
            permission: Mutex::new(Permission::View),
            conn: AsyncMutex::new(None),
        })
    }

    /// The Mac as last seen, to save.
    pub fn record(&self) -> HostRecord {
        lock(&self.record).clone()
    }

    /// What this phone may do, as of the last sign-in.
    pub fn permission(&self) -> Permission {
        *lock(&self.permission)
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectInfo>, RemoteError> {
        match self.call(Request::ListProjects).await? {
            (Response::Projects { projects }, _) => Ok(projects),
            _ => Err(RemoteError::Protocol),
        }
    }

    pub async fn list_tests(&self, project: String) -> Result<Vec<TestEntry>, RemoteError> {
        match self.call(Request::ListTests { project }).await? {
            (Response::Tests { tests }, _) => Ok(tests),
            _ => Err(RemoteError::Protocol),
        }
    }

    pub async fn get_test(
        &self,
        project: String,
        path: String,
    ) -> Result<TestSummary, RemoteError> {
        match self.call(Request::GetTest { project, path }).await? {
            (Response::Test { test }, _) => Ok(*test),
            _ => Err(RemoteError::Protocol),
        }
    }

    /// Starts the test and returns the run's id. `row` runs only that row of its data table,
    /// counted from 0, one of [`crate::TestSummary::rows`]; `None` runs every row.
    pub async fn run_test(
        &self,
        project: String,
        path: String,
        row: Option<u32>,
    ) -> Result<String, RemoteError> {
        match self.call(Request::RunTest { project, path, row }).await? {
            (Response::RunStarted { run }, _) => Ok(run),
            _ => Err(RemoteError::Protocol),
        }
    }

    pub async fn stop_run(&self, project: String, run: String) -> Result<(), RemoteError> {
        match self.call(Request::StopRun { project, run }).await? {
            (Response::RunStopped, _) => Ok(()),
            _ => Err(RemoteError::Protocol),
        }
    }

    /// Where a run stands, also after the phone missed its events.
    pub async fn run_status(&self, project: String, run: String) -> Result<RunStatus, RemoteError> {
        match self.call(Request::GetRunStatus { project, run }).await? {
            (Response::Status { status }, _) => Ok(status),
            _ => Err(RemoteError::Protocol),
        }
    }

    pub async fn list_runs(&self, project: String) -> Result<RunList, RemoteError> {
        match self.call(Request::ListRuns { project }).await? {
            (Response::Runs { runs }, _) => Ok(runs),
            _ => Err(RemoteError::Protocol),
        }
    }

    pub async fn report(&self, project: String, run: String) -> Result<RunReport, RemoteError> {
        match self.call(Request::GetReport { project, run }).await? {
            (Response::Report { report }, _) => Ok(*report),
            _ => Err(RemoteError::Protocol),
        }
    }

    /// PNG bytes of an image the run's report refers to, such as a step's `after`.
    pub async fn picture(
        &self,
        project: String,
        run: String,
        picture: String,
    ) -> Result<Vec<u8>, RemoteError> {
        match self
            .call(Request::GetPicture {
                project,
                run,
                picture,
            })
            .await?
        {
            (Response::Picture { .. }, Some(bytes)) => Ok(bytes),
            _ => Err(RemoteError::Protocol),
        }
    }

    /// Follows a run live. After a dropped connection, watch again: the first update is
    /// always the full status.
    pub async fn watch(&self, project: String, run: String) -> Result<RunWatch, RemoteError> {
        let request = Request::Watch { project, run };
        let conn = self.connection().await?;
        let opened = async {
            let (mut send, mut recv) =
                conn.open_bi().await.map_err(|_| RemoteError::HostOffline)?;
            wire::send(&mut send, &request).await?;
            let _ = send.finish();
            let reply: Reply = wire::expect(&mut recv).await?;
            Ok((reply, recv))
        }
        .await;
        let (reply, recv) = match opened {
            Ok(opened) => opened,
            Err(RemoteError::HostOffline) => return Err(lost(&conn)),
            Err(error) => return Err(error),
        };
        match reply {
            Reply::Ok(Response::Status { status }) => Ok(RunWatch {
                first: Mutex::new(Some(RunUpdate::Status { status })),
                recv: AsyncMutex::new(recv),
                conn,
            }),
            Reply::Ok(_) => Err(RemoteError::Protocol),
            Reply::Error(error) => Err(error),
        }
    }

    /// Disconnects and releases the network.
    pub async fn close(&self) {
        if let Some(conn) = self.conn.lock().await.take() {
            conn.close(VarInt::from_u32(CLOSE_DONE), b"");
        }
        self.endpoint.close().await;
    }

    async fn call(&self, request: Request) -> Result<(Response, Option<Vec<u8>>), RemoteError> {
        let conn = self.connection().await?;
        match exchange(&conn, &request).await {
            Err(RemoteError::HostOffline) => match lost(&conn) {
                RemoteError::HostOffline if request.is_idempotent() => {
                    let conn = self.connection().await?;
                    exchange(&conn, &request).await.map_err(|e| match e {
                        RemoteError::HostOffline => lost(&conn),
                        e => e,
                    })
                }
                error => Err(error),
            },
            result => result,
        }
    }

    /// The open connection, or a new one, signed in.
    pub(crate) async fn connection(&self) -> Result<Connection, RemoteError> {
        let mut slot = self.conn.lock().await;
        if let Some(conn) = slot.as_ref() {
            match conn.close_reason() {
                None => return Ok(conn.clone()),
                // Signing in again would only be refused; say why instead.
                Some(_) if lost(conn) == RemoteError::Revoked => {
                    *slot = None;
                    return Err(RemoteError::Revoked);
                }
                Some(_) => {}
            }
        }
        let device_key = hex(&self.signer.public_key());
        let conn = self.handshake(Hello::SignIn { device_key }).await?;
        *slot = Some(conn.clone());
        Ok(conn)
    }

    async fn handshake(&self, hello: Hello) -> Result<Connection, RemoteError> {
        let addr = self.addr()?;
        let attempt = async {
            let conn = self
                .endpoint
                .connect(addr, self.alpn)
                .await
                .map_err(|_| RemoteError::HostOffline)?;
            let welcome = self.greet(&conn, hello).await;
            if welcome.is_err() {
                conn.close(VarInt::from_u32(CLOSE_DONE), b"");
            }
            Ok::<_, RemoteError>((conn, welcome?))
        };
        let (conn, welcome) = tokio::time::timeout(CONNECT_TIMEOUT, attempt)
            .await
            .map_err(|_| RemoteError::HostOffline)??;
        *lock(&self.permission) = welcome.permission;
        let mut record = lock(&self.record);
        record.name = welcome.host_name;
        record.device_id = welcome.device_id;
        if welcome.relay_url.is_some() || !welcome.addrs.is_empty() {
            record.relay_url = welcome.relay_url;
            record.addrs = welcome.addrs;
        }
        Ok(conn)
    }

    async fn greet(&self, conn: &Connection, hello: Hello) -> Result<Welcome, RemoteError> {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|_| RemoteError::HostOffline)?;
        wire::send(&mut send, &hello).await?;
        let mut reply: HostHello = wire::expect(&mut recv).await?;
        if let HostHello::Challenge { nonce } = reply {
            let nonce = unhex(&nonce)
                .and_then(|n| n.try_into().ok())
                .ok_or(RemoteError::Protocol)?;
            let message = auth_transcript(&nonce, &self.host, &self.endpoint.id());
            let signature = self.sign(message).await?;
            wire::send(
                &mut send,
                &Proof {
                    signature: hex(&signature),
                },
            )
            .await?;
            reply = wire::expect(&mut recv).await?;
        }
        let _ = send.finish();
        match reply {
            HostHello::Welcome(welcome) => Ok(welcome),
            HostHello::Refused { error } => Err(error),
            HostHello::Challenge { .. } => Err(RemoteError::Protocol),
        }
    }

    /// Signs off the network tasks: the Secure Enclave may wait for Face ID.
    async fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
        let signer = Arc::clone(&self.signer);
        tokio::task::spawn_blocking(move || signer.sign(message))
            .await
            .map_err(|_| RemoteError::SignerFailed)?
    }

    fn addr(&self) -> Result<EndpointAddr, RemoteError> {
        let record = lock(&self.record);
        let mut addr = EndpointAddr::new(self.host);
        if let Some(relay) = &record.relay_url {
            addr = addr.with_relay_url(
                relay
                    .parse::<RelayUrl>()
                    .map_err(|_| RemoteError::Protocol)?,
            );
        }
        for ip in &record.addrs {
            addr = addr.with_ip_addr(ip.parse().map_err(|_| RemoteError::Protocol)?);
        }
        Ok(addr)
    }
}

/// Why an assistant's request failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantFailure {
    /// The Mac or the connection to it said no, or could not be reached.
    Remote(RemoteError),
    /// The MCP server on the Mac said no: a JSON-RPC error code and message, such as for an
    /// unknown tool.
    Rejected { code: i64, message: String },
}

impl From<RemoteError> for AssistantFailure {
    fn from(error: RemoteError) -> Self {
        Self::Remote(error)
    }
}

/// An assistant's connection to one Mac, over [`ALPN_V2`]. Like [`Client`] it signs in again on
/// its own after the connection drops. Results are what the Mac's MCP server returns.
pub struct AssistantClient {
    pub(crate) client: Client,
    /// The `initialize` params, sent again on a new connection so the Mac still knows who asks.
    hello: Mutex<Option<Value>>,
    /// The connection the Mac was last told who asks on.
    greeted: AsyncMutex<Option<usize>>,
}

impl AssistantClient {
    /// Pairs with the Mac whose assistant link was entered, see
    /// [`PairingOffer::parse_assistant`]. `device_name` is how the Mac lists this computer.
    pub async fn pair(
        offer: PairingOffer,
        device_name: String,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Self, RemoteError> {
        let client = Client::pair_on(ALPN_V2, offer, device_name, signer, network).await?;
        Ok(Self::wrap(client))
    }

    /// Prepares a paired Mac. Nothing is sent until the first request, which signs in; a Mac
    /// that is not there yet fails that request, not this call.
    pub async fn open(
        record: HostRecord,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Self, RemoteError> {
        let host: EndpointId = record.host_id.parse().map_err(|_| RemoteError::Protocol)?;
        let client = Client::new(ALPN_V2, host, record, signer, &network).await?;
        Ok(Self::wrap(client))
    }

    fn wrap(client: Client) -> Self {
        Self {
            client,
            hello: Mutex::new(None),
            greeted: AsyncMutex::new(None),
        }
    }

    /// The Mac as last seen, to save.
    pub fn record(&self) -> HostRecord {
        self.client.record()
    }

    /// Says who is asking: the MCP `initialize` params. Returns the MCP `initialize` result.
    pub async fn hello(&self, params: Value) -> Result<Value, AssistantFailure> {
        self.send(&AssistantRequest::Hello { params }).await
    }

    /// The MCP `tools/list` result.
    pub async fn tools(&self) -> Result<Value, AssistantFailure> {
        self.send(&AssistantRequest::Tools).await
    }

    /// Calls a tool and returns the MCP `tools/call` result. `project` names a project the Mac
    /// shares; without it the Mac uses the only one it shares. A call that was cut off is not
    /// repeated, as the tool may have done its work.
    pub async fn call(
        &self,
        tool: String,
        arguments: Value,
        project: Option<String>,
    ) -> Result<Value, AssistantFailure> {
        self.send(&AssistantRequest::Call {
            tool,
            arguments,
            project,
        })
        .await
    }

    /// Disconnects and releases the network.
    pub async fn close(&self) {
        self.client.close().await;
    }

    async fn send(&self, request: &AssistantRequest) -> Result<Value, AssistantFailure> {
        let attempts = if request.is_idempotent() { 2 } else { 1 };
        let mut attempt = 1;
        loop {
            let conn = self.client.connection().await?;
            let greeted = match request {
                AssistantRequest::Hello { .. } => Ok(()),
                _ => self.greet(&conn).await,
            };
            let result = match greeted {
                Ok(()) => exchange_assistant(&conn, request).await,
                Err(error) => Err(error),
            };
            match result {
                Err(AssistantFailure::Remote(RemoteError::HostOffline)) => {
                    let error = lost(&conn);
                    if error != RemoteError::HostOffline || attempt == attempts {
                        return Err(error.into());
                    }
                    attempt += 1;
                }
                Ok(result) => {
                    if let AssistantRequest::Hello { params } = request {
                        *lock(&self.hello) = Some(params.clone());
                        *self.greeted.lock().await = Some(conn.stable_id());
                    }
                    return Ok(result);
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Tells the Mac who asks, once per connection.
    async fn greet(&self, conn: &Connection) -> Result<(), AssistantFailure> {
        let Some(params) = lock(&self.hello).clone() else {
            return Ok(());
        };
        let mut greeted = self.greeted.lock().await;
        if *greeted != Some(conn.stable_id()) {
            exchange_assistant(conn, &AssistantRequest::Hello { params }).await?;
            *greeted = Some(conn.stable_id());
        }
        Ok(())
    }
}

/// Updates of one watched run, ending after it finishes.
pub struct RunWatch {
    first: Mutex<Option<RunUpdate>>,
    recv: AsyncMutex<RecvStream>,
    conn: Connection,
}

impl RunWatch {
    /// The next update, or `None` after the run finished.
    pub async fn next(&self) -> Result<Option<RunUpdate>, RemoteError> {
        if let Some(first) = lock(&self.first).take() {
            return Ok(Some(first));
        }
        let mut recv = self.recv.lock().await;
        wire::recv(&mut recv).await.map_err(|e| match e {
            RemoteError::HostOffline => lost(&self.conn),
            e => e,
        })
    }
}

async fn exchange(
    conn: &Connection,
    request: &Request,
) -> Result<(Response, Option<Vec<u8>>), RemoteError> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(|_| RemoteError::HostOffline)?;
    wire::send(&mut send, request).await?;
    let _ = send.finish();
    match wire::expect::<Reply>(&mut recv).await? {
        Reply::Ok(Response::Picture { size }) => {
            let bytes = wire::recv_bytes(&mut recv, MAX_PICTURE)
                .await?
                .ok_or(RemoteError::Protocol)?;
            if bytes.len() as u64 != size {
                return Err(RemoteError::Protocol);
            }
            Ok((Response::Picture { size }, Some(bytes)))
        }
        Reply::Ok(response) => Ok((response, None)),
        Reply::Error(error) => Err(error),
    }
}

async fn exchange_assistant(
    conn: &Connection,
    request: &AssistantRequest,
) -> Result<Value, AssistantFailure> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(|_| RemoteError::HostOffline)?;
    wire::send(&mut send, request).await?;
    let _ = send.finish();
    match wire::expect_within::<AssistantReply>(&mut recv, MAX_ASSISTANT_REPLY).await? {
        AssistantReply::Ok { result } => Ok(result),
        AssistantReply::Rejected { code, message } => {
            Err(AssistantFailure::Rejected { code, message })
        }
        AssistantReply::Error { error } => Err(error.into()),
    }
}

/// Why the connection is gone.
fn lost(conn: &Connection) -> RemoteError {
    match conn.close_reason() {
        Some(ConnectionError::ApplicationClosed(close))
            if close.error_code == VarInt::from_u32(CLOSE_REVOKED) =>
        {
            RemoteError::Revoked
        }
        Some(ConnectionError::ApplicationClosed(close))
            if close.error_code == VarInt::from_u32(CLOSE_DISABLED) =>
        {
            RemoteError::NotAllowed
        }
        _ => RemoteError::HostOffline,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
