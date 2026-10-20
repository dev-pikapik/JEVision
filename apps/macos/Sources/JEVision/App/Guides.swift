import Foundation

/// The pages of the documentation the Help menu opens. They come from the copy inside the app, so
/// they open without the internet and match this version; an app without one, as from `swift run`,
/// opens the documentation folder of the repository.
enum Guide: String, CaseIterable {
  case contents = "index"
  case gettingStarted = "getting-started"
  case writingTests = "writing-tests"
  case checks
  case faq

  private static let online = URL(string: "https://github.com/dev-pikapik/JEVision/tree/main/docs")!

  func url(in bundle: Bundle = .main) -> URL {
    bundle.url(forResource: rawValue, withExtension: "html", subdirectory: "docs") ?? Self.online
  }
}
