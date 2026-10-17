import CoreServices
import Foundation

/// Calls `onChange` on the main thread whenever anything inside the folder changes,
/// including the folder itself being moved or deleted.
final class FolderWatcher {
  private final class Handler: Sendable {
    let onChange: @MainActor @Sendable () -> Void
    init(_ onChange: @escaping @MainActor @Sendable () -> Void) { self.onChange = onChange }
  }

  private let handler: Handler
  private var stream: FSEventStreamRef?

  init?(url: URL, onChange: @escaping @MainActor @Sendable () -> Void) {
    handler = Handler(onChange)
    var context = FSEventStreamContext(
      version: 0, info: Unmanaged.passUnretained(handler).toOpaque(), retain: nil, release: nil, copyDescription: nil)
    let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
      guard let info else { return }
      let handler = Unmanaged<Handler>.fromOpaque(info).takeUnretainedValue()
      MainActor.assumeIsolated { handler.onChange() }
    }
    let flags = FSEventStreamCreateFlags(kFSEventStreamCreateFlagWatchRoot | kFSEventStreamCreateFlagNoDefer)
    guard let stream = FSEventStreamCreate(
      nil, callback, &context, [url.path] as CFArray, FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 0.3, flags)
    else { return nil }
    FSEventStreamSetDispatchQueue(stream, .main)
    guard FSEventStreamStart(stream) else {
      FSEventStreamInvalidate(stream)
      FSEventStreamRelease(stream)
      return nil
    }
    self.stream = stream
  }

  deinit {
    guard let stream else { return }
    FSEventStreamStop(stream)
    FSEventStreamInvalidate(stream)
    FSEventStreamRelease(stream)
  }
}
