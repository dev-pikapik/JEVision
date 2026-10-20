import Foundation

/// The guide to running tests in a virtual machine: the page of the documentation that ships
/// inside the app, so it opens without the internet and always matches this version.
enum VirtualMachineGuide {
  /// The README section, for an app that runs without a bundle, as from `swift run`.
  private static let online = URL(string: "https://github.com/dev-pikapik/JEVision#in-a-virtual-machine")!

  static func url(in bundle: Bundle = .main) -> URL {
    bundle.url(forResource: "vm", withExtension: "html", subdirectory: "docs") ?? online
  }
}
