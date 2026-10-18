import Foundation

/// Paired Macs on disk, in Application Support. The records hold no secrets: a Mac trusts
/// this iPhone only by the key in the Secure Enclave.
struct HostStore: Sendable {
  let url: URL

  static let standard = HostStore(
    url: URL.applicationSupportDirectory.appending(path: "PairedMacs.json"))

  func load() -> [HostRecord] {
    guard let data = try? Data(contentsOf: url) else { return [] }
    return (try? JSONDecoder().decode([HostRecord].self, from: data)) ?? []
  }

  func save(_ hosts: [HostRecord]) throws {
    try FileManager.default.createDirectory(
      at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try JSONEncoder().encode(hosts).write(
      to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
  }
}
