import AppKit
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

/// The pages About links to.
enum AboutLink: CaseIterable, Identifiable {
  case sourceCode, documentation, whatsNew, reportProblem, securityPolicy

  static let repository = URL(string: "https://github.com/dev-pikapik/JEVision")!
  /// The license on the project's page, for a copy of the app without its own, as from `swift run`.
  static let license = repository.appending(path: "blob/main/LICENSE")

  var id: Self { self }

  var url: URL {
    switch self {
    case .sourceCode: Self.repository
    case .documentation: Guide.contents.url()
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

/// The About window, in place of the standard panel: which copy of JEVision this is, where the
/// project lives, and the licenses of JEVision and of the software inside it. All of it comes from
/// the app, so it opens without the internet.
struct AboutView: View {
  @Environment(\.openWindow) private var openWindow
  private let version = AppVersion()
  /// JEVision's license, the LICENSE file of the repository that build.sh puts in the app, its
  /// lines joined into paragraphs so the text wraps to the window instead of breaking twice.
  private let license = Bundle.main.url(forResource: "LICENSE", withExtension: nil)
    .flatMap { try? String(contentsOf: $0, encoding: .utf8) }
    .map { $0.replacingOccurrences(of: "(?<!\n)\n(?!\n)", with: " ", options: .regularExpression) }

  var body: some View {
    Form {
      Section {
        VStack(spacing: 0) {
          Image(nsImage: NSApp.applicationIconImage)
            .resizable()
            .frame(width: 96, height: 96)
            .accessibilityHidden(true)
          Text(verbatim: "JEVision")
            .font(.largeTitle.bold())
            .accessibilityAddTraits(.isHeader)
            .padding(.top, 6)
          Text("Version \(version.version) (\(version.build))")
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
            .padding(.top, 2)
          Text("Test any Mac app the way a person would: click, type, look. A test is a flowchart you can read — no code.")
            .fixedSize(horizontal: false, vertical: true)
            .padding(.top, 12)
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
      }
      Section {
        ForEach(AboutLink.allCases) { link in
          Link(destination: link.url) {
            Label(link.title, systemImage: link.symbol)
          }
          .help(link.url.absoluteString)
        }
      }
      Section("License") {
        if let license {
          ScrollView {
            Text(verbatim: license)
              .font(.callout.monospaced())
              .textSelection(.enabled)
              .frame(maxWidth: .infinity, alignment: .leading)
          }
          .frame(height: 160)
        } else {
          Link(destination: AboutLink.license) {
            Text(verbatim: "MIT License")
          }
        }
        ActionRow {
          Button("Show…") { openWindow(id: SceneID.thirdPartyLicenses) }
        } label: {
          RowLabel(Text("Third-Party Licenses"), Text("The open-source software inside JEVision and its licenses."))
        }
      }
    }
    .formStyle(.grouped)
    .frame(width: 480)
    .fixedSize(horizontal: false, vertical: true)
  }
}
