import AppKit
import SwiftUI

/// Last onboarding step: pick an app and record a first test in it.
struct FirstTestStep: View {
  @Binding var app: String?
  let needsFolder: Bool
  let record: () -> Void
  let runDemo: () -> Void
  @State private var running: [String] = FirstTestStep.runningApps()

  private let columns = [GridItem(.adaptive(minimum: 104, maximum: 120), spacing: 12)]

  var body: some View {
    VStack(spacing: 0) {
      PageHeader(
        symbol: "record.circle", color: .red, title: Text("Make your first test"),
        message: Text("Pick an app, press Start Recording and use it as usual. You can fine-tune the steps later."),
        tileSize: 64)
      // Scrolls only when the tiles don't fit, so there's no scroller beside a grid that fits.
      ViewThatFits(in: .vertical) {
        grid
        ScrollView { grid }
      }
      .frame(maxHeight: .infinity, alignment: .top)
      AppPicker(bundleID: $app, noneTitle: Text("Other App…"))
        .padding(.bottom, 16)
      Button(action: record) {
        Label("Start Recording", systemImage: "record.circle")
          .padding(.horizontal, 8)
      }
      .buttonStyle(.glassProminent)
      .controlSize(.extraLarge)
      .keyboardShortcut(.defaultAction)
      .disabled(app == nil)
      Group {
        if needsFolder {
          Text("Each test is saved as a file in the “JEVision Tests” folder in Documents.")
        } else {
          Text("The test is saved as a file in the open project.")
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .multilineTextAlignment(.center)
      .padding(.top, 10)
      Button("Or run the ready demo test on TextEdit", action: runDemo)
        .buttonStyle(.link)
        .font(.callout)
        .padding(.top, 4)
    }
    .padding(.horizontal, 32)
    .padding(.top, 28)
    .padding(.bottom, 8)
    .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.didLaunchApplicationNotification)) { _ in
      running = Self.runningApps()
    }
    .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.didTerminateApplicationNotification)) { _ in
      running = Self.runningApps()
    }
  }

  private var grid: some View {
    LazyVGrid(columns: columns, spacing: 12) {
      ForEach(tiles, id: \.self) { bundleID in
        AppTile(bundleID: bundleID, isRunning: running.contains(bundleID), isSelected: app == bundleID) {
          app = bundleID
        }
      }
    }
    .padding(.vertical, 12)
  }

  /// Running apps, then installed apps that are easy to try, then the one chosen from the list.
  private var tiles: [String] {
    var seen = Set<String>()
    return (running + Self.suggested + [app].compactMap(\.self)).filter { seen.insert($0).inserted }
  }

  /// Apps that come with every Mac and make a good first test, when they are installed.
  private static let suggested = ["com.apple.TextEdit", "com.apple.Notes", "com.apple.calculator", "com.apple.Safari"]
    .filter { NSWorkspace.shared.urlForApplication(withBundleIdentifier: $0) != nil }

  /// Bundle ids of the apps with a window in the Dock, other than JEVision, in the order they were opened.
  static func runningApps() -> [String] {
    let own = Bundle.main.bundleIdentifier
    var seen = Set<String>()
    return NSWorkspace.shared.runningApplications.compactMap { app in
      guard app.activationPolicy == .regular, let id = app.bundleIdentifier, id != own, seen.insert(id).inserted else { return nil }
      return id
    }
  }
}

private struct AppTile: View {
  let bundleID: String
  let isRunning: Bool
  let isSelected: Bool
  let choose: () -> Void

  var body: some View {
    Button(action: choose) {
      VStack(spacing: 6) {
        Group {
          if let icon = AppCatalog.shared.icon(of: bundleID) {
            Image(nsImage: icon).resizable()
          } else {
            Image(systemName: "app.dashed").resizable().foregroundStyle(.secondary)
          }
        }
        .frame(width: 44, height: 44)
        .accessibilityHidden(true)
        Text(verbatim: AppCatalog.shared.name(of: bundleID))
          .font(.callout)
          .lineLimit(1)
          .truncationMode(.tail)
        (isRunning ? Text("Running") : Text(verbatim: " "))
          .font(.caption)
          .foregroundStyle(.secondary)
      }
      .frame(maxWidth: .infinity)
      .padding(.vertical, 10)
      .padding(.horizontal, 6)
      .contentShape(.rect(cornerRadius: 14))
    }
    .buttonStyle(.plain)
    .background(isSelected ? AnyShapeStyle(.tint.opacity(0.18)) : AnyShapeStyle(.clear), in: .rect(cornerRadius: 14))
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isSelected ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary), lineWidth: isSelected ? 2 : 1)
    }
    .accessibilityAddTraits(isSelected ? .isSelected : [])
    .accessibilityValue(isRunning ? Text("Running") : Text(verbatim: ""))
  }
}
