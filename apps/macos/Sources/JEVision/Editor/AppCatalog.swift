import AppKit
import Observation
import SwiftUI

/// Apps installed on this Mac, found once in the usual folders. Tests refer to apps by bundle id.
@MainActor
@Observable
final class AppCatalog {
  struct App: Identifiable, Hashable, Sendable {
    var id: String { bundleID }
    let bundleID: String
    let name: String
    let url: URL
  }

  static let shared = AppCatalog()

  private(set) var apps: [App] = []
  @ObservationIgnored private var loading: Task<Void, Never>?

  func load() {
    guard loading == nil else { return }
    loading = Task {
      apps = await Task.detached(priority: .userInitiated) { Self.scan() }.value
    }
  }

  /// The app's name, or its bundle id when it isn't installed here.
  func name(of bundleID: String) -> String {
    if let app = apps.first(where: { $0.bundleID == bundleID }) { return app.name }
    guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) else { return bundleID }
    return FileManager.default.displayName(atPath: url.path).replacing(/\.app$/, with: "")
  }

  func isInstalled(_ bundleID: String) -> Bool {
    NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) != nil
  }

  func icon(of bundleID: String) -> NSImage? {
    NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID).map { NSWorkspace.shared.icon(forFile: $0.path) }
  }

  nonisolated private static func scan() -> [App] {
    let fm = FileManager.default
    let folders = [
      URL(filePath: "/Applications"), URL(filePath: "/Applications/Utilities"),
      URL(filePath: "/System/Applications"), URL(filePath: "/System/Applications/Utilities"),
      fm.homeDirectoryForCurrentUser.appending(path: "Applications"),
    ]
    var seen = Set<String>()
    var found: [App] = []
    for folder in folders {
      let entries = (try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil, options: .skipsHiddenFiles)) ?? []
      for url in entries where url.pathExtension == "app" {
        guard let id = Bundle(url: url)?.bundleIdentifier, seen.insert(id).inserted else { continue }
        let name = fm.displayName(atPath: url.path).replacing(/\.app$/, with: "")
        found.append(App(bundleID: id, name: name, url: url))
      }
    }
    return found.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
  }
}

/// A button showing the chosen app; it opens a searchable list of installed apps.
struct AppPicker: View {
  /// `nil` is allowed only when `noneTitle` is given.
  @Binding var bundleID: String?
  var noneTitle: Text?
  @State private var isPresented = false
  private let catalog = AppCatalog.shared

  var body: some View {
    Button { isPresented = true } label: {
      HStack(spacing: 6) {
        if let bundleID {
          AppIcon(bundleID: bundleID)
          Text(catalog.name(of: bundleID))
          if !catalog.isInstalled(bundleID) {
            Image(systemName: "exclamationmark.triangle.fill")
              .foregroundStyle(.orange)
              .help(Text("This app isn’t installed on this Mac."))
              .accessibilityLabel(Text("Not installed on this Mac"))
          }
        } else {
          noneTitle
        }
        Image(systemName: "chevron.up.chevron.down")
          .imageScale(.small)
          .foregroundStyle(.secondary)
          .accessibilityHidden(true)
      }
      .lineLimit(1)
    }
    .popover(isPresented: $isPresented, arrowEdge: .bottom) {
      AppList(selection: bundleID, noneTitle: noneTitle) { chosen in
        bundleID = chosen
        isPresented = false
      }
    }
    .onAppear { catalog.load() }
  }
}

private struct AppIcon: View {
  let bundleID: String

  var body: some View {
    if let icon = AppCatalog.shared.icon(of: bundleID) {
      Image(nsImage: icon).resizable().frame(width: 18, height: 18).accessibilityHidden(true)
    } else {
      Image(systemName: "app.dashed").accessibilityHidden(true)
    }
  }
}

private struct AppList: View {
  let selection: String?
  let noneTitle: Text?
  let choose: (String?) -> Void
  @State private var query = ""
  @State private var highlighted: String?
  private let catalog = AppCatalog.shared

  private var matches: [AppCatalog.App] {
    let q = query.trimmingCharacters(in: .whitespaces)
    return q.isEmpty ? catalog.apps : catalog.apps.filter { $0.name.localizedStandardContains(q) || $0.bundleID.localizedStandardContains(q) }
  }

  var body: some View {
    VStack(spacing: 0) {
      TextField(text: $query, prompt: Text("Search apps")) { Text("Search apps") }
        .textFieldStyle(.roundedBorder)
        .accessibilityIdentifier(AccessibilityID.AppPicker.search)
        .padding(10)
        .onSubmit { if let first = highlighted ?? matches.first?.bundleID { choose(first) } }
      List(selection: $highlighted) {
        if let noneTitle, query.isEmpty {
          noneTitle.tag(String?.none)
            .onTapGesture { choose(nil) }
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { choose(nil) }
        }
        ForEach(matches) { app in
          Label {
            Text(app.name)
          } icon: {
            Image(nsImage: NSWorkspace.shared.icon(forFile: app.url.path)).resizable().frame(width: 20, height: 20)
          }
          .tag(Optional(app.bundleID))
          .contentShape(.rect)
          .onTapGesture { choose(app.bundleID) }
          .accessibilityAddTraits(.isButton)
          .accessibilityAction { choose(app.bundleID) }
          .accessibilityIdentifier(AccessibilityID.AppPicker.app)
        }
      }
      .overlay {
        if catalog.apps.isEmpty {
          ProgressView().accessibilityLabel(Text("Looking for apps"))
        } else if matches.isEmpty {
          ContentUnavailableView.search(text: query)
        }
      }
      .onKeyPress(.return) {
        guard let highlighted else { return .ignored }
        choose(highlighted)
        return .handled
      }
    }
    .frame(width: 300, height: 380)
    .onAppear { highlighted = selection }
  }
}

/// Chooses the browser that opens links: any installed one, or the Mac's default browser.
struct BrowserPicker: View {
  @Binding var bundleID: String?

  /// Every file is a web link.
  static func areLinks(_ files: [String]) -> Bool {
    !files.isEmpty && files.allSatisfy { $0.hasPrefix("http://") || $0.hasPrefix("https://") }
  }

  private static let probe = URL(string: "https://example.com")!

  /// Installed browsers as `(bundle ID, name)`, by name.
  private var browsers: [(id: String, name: String)] {
    var seen: Set<String> = []
    return NSWorkspace.shared.urlsForApplications(toOpen: Self.probe)
      .compactMap { url in Bundle(url: url)?.bundleIdentifier.map { ($0, FileManager.default.displayName(atPath: url.path)) } }
      .filter { seen.insert($0.0).inserted }
      .map { (id: $0.0, name: ($0.1 as NSString).deletingPathExtension) }
      .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
  }

  private var defaultName: String {
    NSWorkspace.shared.urlForApplication(toOpen: Self.probe)
      .map { (FileManager.default.displayName(atPath: $0.path) as NSString).deletingPathExtension } ?? ""
  }

  var body: some View {
    let browsers = browsers
    Picker(selection: $bundleID) {
      (defaultName.isEmpty ? Text("Default Browser") : Text("Default Browser (\(defaultName))")).tag(String?.none)
      Divider()
      ForEach(browsers, id: \.id) { browser in
        Text(verbatim: browser.name).tag(String?.some(browser.id))
      }
      if let bundleID, !browsers.contains(where: { $0.id == bundleID }) {
        Text(verbatim: AppCatalog.shared.name(of: bundleID)).tag(String?.some(bundleID))
      }
    } label: {
      Text("Browser")
    }
    .help(Text("The browser that opens the links."))
  }
}
