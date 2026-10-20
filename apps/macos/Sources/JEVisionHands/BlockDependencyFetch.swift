import JEVisionCore

/// Reads the block packages that packages about to be installed need and the project hasn't got,
/// from the address each package gives for them, the same way for the app's Install Block… and
/// `jevision blocks install`.
public enum BlockDependencyFetch {
  /// Packages that need packages that need packages are followed this many steps deep at most.
  public static let rounds = 5
  /// At most this many addresses are read.
  public static let sources = 20

  /// A package read because another one needs it.
  public struct Found: Sendable {
    public let preview: BlockPreview
    /// The name of the package that needs it.
    public let neededBy: String
  }

  /// The packages `packages` need, and those need in turn, read from the addresses they name.
  /// `plan` plans the install of staged folders, `inspect` reads an address, `discard` throws away
  /// a staged package nobody needs, and `reading` hears of each package before its address is read.
  /// Gives what was found and, by address, why one couldn't be read; a need that can't be met stays
  /// a problem of the plan.
  public static func fetch(
    for packages: [BlockPreview],
    plan: ([String]) throws -> BlockInstallPlan,
    inspect: (String) throws -> BlockSourcePreview,
    discard: (String) -> Void,
    reading: (BlockDependencyState) -> Void = { _ in }
  ) -> (found: [Found], unread: [String: any Error]) {
    var all = packages
    var found: [Found] = []
    var unread: [String: any Error] = [:]
    var read: Set<String> = []
    for _ in 0..<rounds {
      guard let plan = try? plan(all.map(\.folder)) else { break }
      let missing = plan.dependencies.filter { need in
        need.status == .missing && need.source.map { !read.contains($0) } == true
      }
      let before = found.count
      for source in missing.compactMap(\.source) where !read.contains(source) && read.count < sources {
        read.insert(source)
        let needs = missing.filter { $0.source == source }
        needs.forEach(reading)
        do {
          let set = try inspect(source)
          for package in set.packages {
            let id = package.manifest.id
            if let need = needs.first(where: { $0.id == id }), !all.contains(where: { $0.manifest.id == id }) {
              let by = all.first { $0.folder == need.folder }?.manifest.name ?? need.folder
              found.append(Found(preview: package, neededBy: by))
              all.append(package)
            } else if set.temporary {
              discard(package.folder)
            }
          }
        } catch {
          unread[source] = error
        }
      }
      if found.count == before { break }
    }
    return (found, unread)
  }
}
