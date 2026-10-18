import AppKit
import SwiftUI

/// The two permission rows with a hint for the first missing one. Lives inside a grouped `Form`.
struct PermissionsSection: View {
  @Environment(PermissionsMonitor.self) private var monitor
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    Section {
      if let first = monitor.missing.first {
        PermissionGuide(permission: first)
      }
      ForEach(Permission.allCases) { PermissionRow(permission: $0) }
    } footer: {
      if !monitor.allGranted {
        Text("JEVision checks every 1.5 seconds, so there’s no need to restart. If JEVision is already in the list but doesn’t work, remove it with the − button and add it again.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .animation(reduceMotion ? nil : .snappy, value: monitor.granted)
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      monitor.refresh()
    }
  }
}

struct PermissionRow: View {
  @Environment(PermissionsMonitor.self) private var monitor
  let permission: Permission

  var body: some View {
    let granted = monitor.granted.contains(permission)
    LabeledContent {
      HStack(spacing: 10) {
        Group {
          if granted {
            Button("Turn Off", systemImage: "arrow.up.forward.app", action: permission.openSettings)
              .buttonStyle(.glass)
          } else {
            Button("Allow", systemImage: "arrow.up.forward.app", action: permission.request)
              .buttonStyle(.glassProminent)
          }
        }
        .labelStyle(.titleAndIcon)
        .help(Text("Opens this list in System Settings"))
        Image(systemName: granted ? "checkmark.circle.fill" : "xmark.circle.fill")
          .font(.title2)
          .foregroundStyle(granted ? .green : .orange)
          .contentTransition(.symbolEffect(.replace))
          .accessibilityLabel(granted ? Text("Allowed") : Text("Not allowed"))
      }
    } label: {
      HStack(spacing: 10) {
        IconTile(symbol: permission.symbol, color: permission.color, size: 30)
        VStack(alignment: .leading, spacing: 2) {
          Text(permission.title)
          Text(permission.reason)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        .fixedSize(horizontal: false, vertical: true)
      }
    }
  }
}
