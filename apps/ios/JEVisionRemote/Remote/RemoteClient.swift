import Foundation

/// Opens connections to Macs. The app reaches a Mac only through this protocol; the network
/// layer behind it lives in the Rust core and signs in with `DeviceKey`.
protocol RemoteClient: Sendable {
  /// Pairs with the Mac whose code was scanned. `offer` is the whole `jevision://pair?…` text;
  /// `deviceName` is how the Mac lists this iPhone.
  func pair(offer: String, deviceName: String) async throws(RemoteError) -> any MacSession
  /// Connects to a paired Mac and signs in.
  func connect(_ host: HostRecord) async throws(RemoteError) -> any MacSession
}

/// A signed-in connection to one Mac. It reconnects on its own after the network changes.
protocol MacSession: AnyObject, Sendable {
  /// The Mac as last seen, with fresh addresses.
  var record: HostRecord { get }
  /// What this iPhone may do, as of the last sign-in.
  var permission: Permission { get }

  func listProjects() async throws(RemoteError) -> [Project]
  /// Every test and folder of the project, folders included, paths relative to its folder.
  func listTests(project: String) async throws(RemoteError) -> [TestEntry]
  func getTest(project: String, path: String) async throws(RemoteError) -> TestFlow
  /// Starts a run and returns its id. `row` runs only that row of the test's data table, an
  /// index into `TestFlow.rows`; `nil` runs every row.
  func runTest(project: String, path: String, row: Int?) async throws(RemoteError) -> String
  func stopRun(project: String, run: String) async throws(RemoteError)
  /// Makes `edit` on the test as read at `revision`; `.testChanged` when it changed since.
  func editTest(project: String, path: String, revision: String, edit: TestEdit) async throws(RemoteError) -> EditedTest
  func runStatus(project: String, run: String) async throws(RemoteError) -> RunStatus
  /// The run as it happens: the full status first, then events. The stream ends after the run
  /// finishes and throws a `RemoteError` if the connection drops.
  func runEvents(project: String, run: String) -> AsyncThrowingStream<RunUpdate, any Error>
  func listRuns(project: String) async throws(RemoteError) -> RunList
  func report(project: String, run: String) async throws(RemoteError) -> RunReport
  /// PNG bytes of a picture the report refers to, such as a step's `after`.
  func picture(project: String, run: String, name: String) async throws(RemoteError) -> Data
  /// Disconnects and releases the network.
  func close() async
}

/// Why a Mac didn't do what was asked. Mirrors the codes of the core's `RemoteError`.
enum RemoteError: Error, Hashable, Sendable {
  /// Asleep, offline, or JEVision isn't running there.
  case macOffline
  /// Runs need the Mac unlocked: input would go to the lock screen.
  case macLocked
  case runInProgress
  case notAllowed
  case declinedOnMac
  /// The pairing code is wrong, already used, or the Mac shows no code now.
  case invalidCode
  case codeExpired
  /// The Mac no longer knows this iPhone.
  case revoked
  /// The test's data table no longer has the chosen row.
  case dataRowNotFound
  /// The test changed on the Mac since it was read here.
  case testChanged
  /// The test is open on the Mac with a change not saved yet.
  case testBusyOnMac
  /// The Mac doesn't take the value or the change.
  case editInvalid
  /// The change needs a question answered at the Mac first.
  case editNeedsMac
  /// More than the Mac takes from a phone, or sends to one.
  case tooLarge
  /// This app is too old for the Mac or its pairing code. `version` is the Mac's JEVision, when
  /// the Mac told it.
  case updateApp(version: String?)
  /// JEVision on the Mac is too old for this app. `version` is the least it needs, when known.
  case updateMac(version: String?)
  case other
}
