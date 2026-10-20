import JEVisionCore

/// A community block a test needs and the project hasn't got: which one, which version, and where
/// to get it when the test knows.
struct BlockNeed: Hashable, Sendable {
  /// Like `com.acme.sign-in`.
  let id: String
  /// The version the test was made with; empty for a test that doesn't say.
  let version: String
  /// The address the block can be installed from; `nil` when the test doesn't know it.
  let source: String?

  /// What the validation of a test says about a step whose block isn't installed.
  init?(_ issue: Issue) {
    guard issue.code == "flow.block.notInstalled", let id = issue.param("id") else { return nil }
    self.init(id: id, version: issue.param("version") ?? "", source: issue.param("source"))
  }

  /// What a run says about a step whose block isn't installed.
  init?(_ failure: Failure) {
    guard failure.code == "run.block.notInstalled", let id = failure.params["package"] else { return nil }
    self.init(id: id, version: failure.params["version"] ?? "", source: failure.params["source"])
  }

  init(id: String, version: String = "", source: String? = nil) {
    self.id = id
    self.version = version
    self.source = source
  }

  /// The block as a person reads it: `com.acme.sign-in 1.2`, or the id alone without a version.
  var label: String { version.isEmpty ? id : "\(id) \(version)" }
}

extension LibraryEntry.Package {
  /// What a step that runs the block keeps about it: the version, and the address when it's a web one.
  var need: PackageNeed {
    PackageNeed(version: version, source: source.flatMap { $0.hasPrefix("https://") ? $0 : nil })
  }
}
