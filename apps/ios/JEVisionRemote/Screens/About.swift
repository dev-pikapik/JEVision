import SwiftUI

/// The version and build number of a copy of the app, as its Info.plist states them.
struct AppVersion: Equatable {
  let version: String
  let build: String

  init(bundle: Bundle = .main) {
    version = bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "–"
    build = bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "–"
  }
}

/// The pages About in Settings links to.
enum AboutLink: CaseIterable, Identifiable {
  case sourceCode, documentation, whatsNew, reportProblem, securityPolicy

  static let repository = URL(string: "https://github.com/dev-pikapik/JEVision")!
  /// The license on the project's page, for a copy of the app without its own.
  static let license = repository.appending(path: "blob/main/LICENSE")

  var id: Self { self }

  var url: URL {
    switch self {
    case .sourceCode: Self.repository
    // The guides are pages of the Mac app; the project's front page explains it and links to them.
    case .documentation: URL(string: "https://github.com/dev-pikapik/JEVision#readme")!
    case .whatsNew: Self.repository.appending(path: "blob/main/CHANGELOG.md")
    case .reportProblem: Self.repository.appending(path: "issues")
    case .securityPolicy: Self.repository.appending(path: "security/policy")
    }
  }

  var title: LocalizedStringKey {
    switch self {
    case .sourceCode: "Source Code"
    case .documentation: "Documentation"
    case .whatsNew: "What’s New"
    case .reportProblem: "Report a Problem"
    case .securityPolicy: "Security Policy"
    }
  }

  var symbol: String {
    switch self {
    case .sourceCode: "chevron.left.forwardslash.chevron.right"
    case .documentation: "book"
    case .whatsNew: "sparkles"
    case .reportProblem: "exclamationmark.bubble"
    case .securityPolicy: "lock.shield"
    }
  }
}

/// JEVision's license, the LICENSE file of the repository that the build puts in the app, its
/// lines joined into paragraphs so the text wraps to the screen instead of breaking twice.
struct LicenseView: View {
  private let license = Bundle.main.url(forResource: "LICENSE", withExtension: nil)
    .flatMap { try? String(contentsOf: $0, encoding: .utf8) }
    .map { $0.replacingOccurrences(of: "(?<!\n)\n(?!\n)", with: " ", options: .regularExpression) }

  var body: some View {
    Group {
      if let license {
        ScrollView {
          Text(verbatim: license)
            .font(.footnote.monospaced())
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
        }
      } else {
        Link(destination: AboutLink.license) {
          Text(verbatim: "MIT License")
        }
      }
    }
    .navigationTitle("License")
    .navigationBarTitleDisplayMode(.inline)
  }
}
