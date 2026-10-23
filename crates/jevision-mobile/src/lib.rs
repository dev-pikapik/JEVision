//! UniFFI surface of JEVision remote control for the iPhone app.
//!
//! The phone pairs with a Mac by its QR code, signs in with a key kept in the Secure Enclave
//! ([`DeviceSigner`]) and talks to the Mac through a [`RemoteConnection`]. Network work runs
//! on a small runtime owned by this library, so Swift simply awaits the async methods.

use std::fmt;
use std::sync::{Arc, LazyLock};

use jevision_remote::{Client, RunWatch};
use tokio::runtime::{Builder, Runtime};

uniffi::setup_scaffolding!();

mod model;
#[cfg(test)]
mod tests;
mod text;

pub use model::*;
pub use text::*;

/// Runs the network: iroh's endpoint tasks and every request. Two threads are plenty for one
/// connection and keep the phone's footprint small.
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("jevision-remote")
        .enable_all()
        .build()
        .expect("the network runtime starts")
});

/// Runs `task` on the network runtime. A Swift task that stops waiting leaves the request to
/// finish on its own, so a watched run never loses its place in the stream.
async fn on_runtime<T: Send + 'static>(task: impl Future<Output = T> + Send + 'static) -> T {
    match RUNTIME.spawn(task).await {
        Ok(output) => output,
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

async fn spawn<T: Send + 'static>(
    task: impl Future<Output = Result<T, jevision_remote::RemoteError>> + Send + 'static,
) -> Result<T, RemoteError> {
    on_runtime(task).await.map_err(Into::into)
}

macro_rules! remote_error {
    ($($variant:ident),* $(,)?) => {
        /// Why something didn't happen. The cases are the stable codes the Mac and the phone
        /// share; show the text of [`RemoteError::key`] in the app's language.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Error)]
        pub enum RemoteError {
            $($variant),*
        }

        impl From<jevision_remote::RemoteError> for RemoteError {
            fn from(error: jevision_remote::RemoteError) -> Self {
                match error {
                    $(jevision_remote::RemoteError::$variant => Self::$variant),*
                }
            }
        }

        impl From<RemoteError> for jevision_remote::RemoteError {
            fn from(error: RemoteError) -> Self {
                match error {
                    $(RemoteError::$variant => Self::$variant),*
                }
            }
        }
    };
}

remote_error!(
    PairingCodeInvalid,
    PairingCodeExpired,
    PairingCodeUsed,
    PairingLocked,
    BadSignature,
    NotPaired,
    Revoked,
    NotAllowed,
    MacLocked,
    RunInProgress,
    DeclinedOnMac,
    HostOffline,
    ProjectNotFound,
    TestNotFound,
    TestUnreadable,
    RunNotFound,
    PictureNotFound,
    PathOutsideProject,
    TooLarge,
    Protocol,
    OfferWrongScheme,
    OfferMalformed,
    InvalidSecretKey,
    InvalidRelayUrl,
    SignerFailed,
    Network,
    HostNotRunning,
    Storage,
    HostFailed,
    DataRowNotFound,
    DeviceTooOld,
    MacTooOld,
    Unsupported,
    DevicesFromNewer,
    Unknown,
);

#[uniffi::export]
impl RemoteError {
    /// Localization key of the error's text, such as `remote.error.macLocked`.
    pub fn key(&self) -> String {
        jevision_remote::RemoteError::from(*self).key()
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        jevision_remote::RemoteError::from(*self).fmt(f)
    }
}

impl std::error::Error for RemoteError {}

/// Anything else a Swift signer throws, such as an error of the Security framework.
impl From<uniffi::UnexpectedUniFFICallbackError> for RemoteError {
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::SignerFailed
    }
}

/// The phone's own P-256 key, implemented in Swift on top of the Secure Enclave.
#[uniffi::export(with_foreign)]
pub trait DeviceSigner: Send + Sync {
    /// The public key in SEC1 uncompressed form: 65 bytes starting with `0x04`. Called on the
    /// network threads, so return a cached copy instead of asking the Keychain every time.
    fn public_key(&self) -> Vec<u8>;
    /// An ECDSA P-256 signature over the SHA-256 digest of `message`, DER-encoded: what
    /// `SecKeyCreateSignature` returns for `ecdsaSignatureMessageX962SHA256`. May block while
    /// the person confirms with Face ID; throw `RemoteError.signerFailed` when they cancel.
    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError>;
}

struct Signer(Arc<dyn DeviceSigner>);

impl jevision_remote::DeviceSigner for Signer {
    fn public_key(&self) -> Vec<u8> {
        self.0.public_key()
    }

    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, jevision_remote::RemoteError> {
        self.0.sign(message).map_err(Into::into)
    }
}

/// Reads the text of a Mac's pairing QR code, `jevision://pair?…`.
#[uniffi::export]
pub fn parse_pairing_offer(text: String) -> Result<PairingOffer, RemoteError> {
    jevision_remote::PairingOffer::parse(&text).map_err(Into::into)
}

/// This app's version, such as `1.0.0`. A Mac too old for the phone ([`RemoteError::MacTooOld`])
/// needs at least this one.
#[uniffi::export]
pub fn app_version() -> String {
    jevision_remote::APP_VERSION.to_owned()
}

/// The version of JEVision on the Mac `host_id`, as it last told this phone: on every sign-in,
/// and when it refused the phone as too old ([`RemoteError::DeviceTooOld`]), so the phone can
/// say which version to update to. `None` until then.
#[uniffi::export]
pub fn mac_app_version(host_id: String) -> Option<String> {
    jevision_remote::mac_version(&host_id)
}

/// A paired Mac as JSON, to keep in the app's storage.
#[uniffi::export]
pub fn host_record_to_json(record: HostRecord) -> String {
    serde_json::to_string(&record).expect("a host record always serializes")
}

/// A paired Mac saved by [`host_record_to_json`]. Fails with `storage` when the text is not one.
#[uniffi::export]
pub fn host_record_from_json(json: String) -> Result<HostRecord, RemoteError> {
    serde_json::from_str(&json).map_err(|_| RemoteError::Storage)
}

/// A connection to one Mac. Requests reconnect and sign in again on their own after the
/// connection drops, for example when the phone wakes up or changes networks.
#[derive(uniffi::Object)]
pub struct RemoteConnection {
    client: Arc<Client>,
}

#[uniffi::export]
impl RemoteConnection {
    /// Pairs with the Mac whose QR code was scanned. `device_name` is how the Mac lists this
    /// phone. Save [`RemoteConnection::record`] afterwards to connect again later.
    #[uniffi::constructor]
    pub async fn pair(
        offer: PairingOffer,
        device_name: String,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Arc<Self>, RemoteError> {
        let signer = Arc::new(Signer(signer));
        let client = spawn(Client::pair(offer, device_name, signer, network)).await?;
        Ok(Arc::new(Self {
            client: Arc::new(client),
        }))
    }

    /// Connects to a paired Mac and signs in.
    #[uniffi::constructor]
    pub async fn connect(
        record: HostRecord,
        signer: Arc<dyn DeviceSigner>,
        network: Network,
    ) -> Result<Arc<Self>, RemoteError> {
        let signer = Arc::new(Signer(signer));
        let client = spawn(Client::connect(record, signer, network)).await?;
        Ok(Arc::new(Self {
            client: Arc::new(client),
        }))
    }

    /// The Mac as last seen. Its addresses are refreshed on every sign-in, so save it again
    /// after connecting.
    pub fn record(&self) -> HostRecord {
        self.client.record()
    }

    /// What this phone may do, as of the last sign-in.
    pub fn permission(&self) -> Permission {
        self.client.permission()
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectInfo>, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.list_projects().await }).await
    }

    /// Tests and folders of a project, sorted so the list reads as a tree.
    pub async fn list_tests(&self, project: String) -> Result<Vec<TestEntry>, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.list_tests(project).await }).await
    }

    /// A test laid out for drawing; `path` is a test file from [`RemoteConnection::list_tests`].
    pub async fn get_test(
        &self,
        project: String,
        path: String,
    ) -> Result<TestSummary, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.get_test(project, path).await }).await
    }

    /// Starts the test and returns the run's id. `row` runs only that row of its data table,
    /// counted from 0, one of [`TestSummary::rows`]; `None` runs every row.
    pub async fn run_test(
        &self,
        project: String,
        path: String,
        row: Option<u32>,
    ) -> Result<String, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.run_test(project, path, row).await }).await
    }

    pub async fn stop_run(&self, project: String, run: String) -> Result<(), RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.stop_run(project, run).await }).await
    }

    /// Where a run stands, also after the phone missed its updates.
    pub async fn run_status(&self, project: String, run: String) -> Result<RunStatus, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.run_status(project, run).await }).await
    }

    /// Runs going on now and the saved ones, newest first.
    pub async fn list_runs(&self, project: String) -> Result<RunList, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.list_runs(project).await }).await
    }

    pub async fn report(&self, project: String, run: String) -> Result<RunReport, RemoteError> {
        let client = Arc::clone(&self.client);
        let report = spawn(async move { client.report(project, run).await }).await?;
        Ok(report.into())
    }

    /// PNG bytes of an image a run's report refers to, such as a step's `after`.
    pub async fn picture(
        &self,
        project: String,
        run: String,
        picture: String,
    ) -> Result<Vec<u8>, RemoteError> {
        let client = Arc::clone(&self.client);
        spawn(async move { client.picture(project, run, picture).await }).await
    }

    /// Follows a run live. After the connection drops, watch again: the first update is
    /// always the full status.
    pub async fn watch(
        &self,
        project: String,
        run: String,
    ) -> Result<Arc<RunWatcher>, RemoteError> {
        let client = Arc::clone(&self.client);
        let watch = spawn(async move { client.watch(project, run).await }).await?;
        Ok(Arc::new(RunWatcher {
            watch: Arc::new(watch),
        }))
    }

    /// Disconnects and releases the network. Dropping the connection does the same.
    pub async fn close(&self) {
        let client = Arc::clone(&self.client);
        on_runtime(async move { client.close().await }).await;
    }
}

impl Drop for RemoteConnection {
    fn drop(&mut self) {
        let client = Arc::clone(&self.client);
        RUNTIME.spawn(async move { client.close().await });
    }
}

/// Updates of one watched run, ending after it finishes.
#[derive(uniffi::Object)]
pub struct RunWatcher {
    watch: Arc<RunWatch>,
}

#[uniffi::export]
impl RunWatcher {
    /// The next update, or `nil` after the run finished.
    pub async fn next(&self) -> Result<Option<RunUpdate>, RemoteError> {
        let watch = Arc::clone(&self.watch);
        let update = spawn(async move { watch.next().await }).await?;
        Ok(update.map(Into::into))
    }
}
