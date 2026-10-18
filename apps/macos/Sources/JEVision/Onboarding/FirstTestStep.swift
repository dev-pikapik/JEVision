import AppKit
import SwiftUI

/// Last onboarding step: pick an app and record a first test in it.
struct FirstTestStep: View {
  @Binding var app: String?
  let needsFolder: Bool
  let record: () -> Void
  @State private var running: [String] = FirstTestStep.runningApps()

  private let columns = [GridItem(.adaptive(minimum: 104, maximum: 120), spacing: 12)]

  var body: some View {
    VStack(spacing: 0) {
      PageHeader(
        symbol: "record.circle", color: .red, title: Text("Make your first test"),
        message: Text("Pick an app, press Start Recording and use it as usual. You can fine-tune the steps later."),
        tileSize: 64)
      ScrollView {
        LazyVGrid(columns: columns, spacing: 12) {
          ForEach(tiles, id: \.self) { bundleID in
            AppTile(bundleID: bundleID, isRunning: running.contains(bundleID), isSelected: app == bundleID) {
              app = bundleID
            }
          }
        }
        .padding(.vertical, 12)
      }
      .scrollBounceBehavior(.basedOnSize)
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
          Text("First you choose a folder for your tests. JEVision saves each test there as a file.")
        } else {
          Text("The test is saved in the open project.")
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .multilineTextAlignment(.center)
      .padding(.top, 10)
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

  /// Running apps, plus the one chosen from the list if it isn't running.
  private var tiles: [String] {
    guard let app, !running.contains(app) else { return running }
    return running + [app]
  }

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
