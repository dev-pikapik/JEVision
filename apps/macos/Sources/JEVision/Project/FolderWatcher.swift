import CoreServices
import Foundation

/// Calls `onChange` on the main thread with the folders, relative to the watched one, in which
/// something changed: `""` for the folder itself. `nil` asks to look at everything again: the
/// folder itself was moved or deleted, or the system lost track of what changed.
final class FolderWatcher {
  private final class Handler: Sendable {
    /// The real path of the folder, as the system reports changes in it, with a slash at the end.
    let root: String
    let onChange: @MainActor @Sendable (Set<String>?) -> Void

    init(root: String, onChange: @escaping @MainActor @Sendable (Set<String>?) -> Void) {
      self.root = root.hasSuffix("/") ? root : root + "/"
      self.onChange = onChange
    }

    /// The folder at `path` relative to the watched one; `nil` outside it.
    func relative(_ path: String) -> String? {
      let folder = path.hasSuffix("/") ? path : path + "/"
      guard folder.hasPrefix(root) else { return nil }
      return String(folder.dropFirst(root.count).dropLast())
    }
  }

  private static let everything = FSEventStreamEventFlags(
    kFSEventStreamEventFlagMustScanSubDirs | kFSEventStreamEventFlagUserDropped | kFSEventStreamEventFlagKernelDropped
      | kFSEventStreamEventFlagEventIdsWrapped | kFSEventStreamEventFlagRootChanged | kFSEventStreamEventFlagMount
      | kFSEventStreamEventFlagUnmount)

  private let handler: Handler
  private var stream: FSEventStreamRef?

  init?(url: URL, onChange: @escaping @MainActor @Sendable (Set<String>?) -> Void) {
    // The system reports changes by the real path: through no symbolic link, in the case the
    // names have on the disk.
    let real = realpath(url.path, nil)
    handler = Handler(root: real.map { String(cString: $0) } ?? url.path, onChange: onChange)
    free(real)
    var context = FSEventStreamContext(
      version: 0, info: Unmanaged.passUnretained(handler).toOpaque(), retain: nil, release: nil, copyDescription: nil)
    let callback: FSEventStreamCallback = { _, info, count, paths, flags, _ in
      guard let info else { return }
      let handler = Unmanaged<Handler>.fromOpaque(info).takeUnretainedValue()
      let paths = paths.assumingMemoryBound(to: UnsafePointer<CChar>.self)
      var folders: Set<String>? = []
      for index in 0..<count {
        guard flags[index] & FolderWatcher.everything == 0, let folder = handler.relative(String(cString: paths[index]))
        else {
          folders = nil
          break
        }
        folders?.insert(folder)
      }
      MainActor.assumeIsolated { handler.onChange(folders) }
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
