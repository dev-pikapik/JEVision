import AppKit
import SwiftUI

/// The two permission rows with a hint for the first missing one. Lives inside a grouped `Form`.
struct PermissionsSection: View {
  @Environment(PermissionsMonitor.self) private var monitor
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    Section {
      if let first = monitor.missing.first, !monitor.stale.contains(first) {
        PermissionGuide(permission: first)
      }
      ForEach(Permission.allCases) { PermissionRow(permission: $0) }
    } footer: {
      if !monitor.allGranted {
        Text("JEVision checks every 1.5 seconds and when you come back, so there’s no need to restart.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .animation(reduceMotion ? nil : .snappy, value: monitor.granted)
    .animation(reduceMotion ? nil : .snappy, value: monitor.stale)
  }
}

struct PermissionRow: View {
  @Environment(PermissionsMonitor.self) private var monitor
  let permission: Permission
  @State private var isResetting = false

  var body: some View {
    let granted = monitor.granted.contains(permission)
    let stale = !granted && monitor.stale.contains(permission)
    LabeledContent {
      HStack(spacing: 10) {
        Group {
          if granted {
            Button("Turn Off", systemImage: "arrow.up.forward.app", action: permission.openSettings)
              .buttonStyle(.glass)
              .help(Text("Opens this list in System Settings"))
          } else if stale {
            Button("Reset and Allow", systemImage: "arrow.counterclockwise") {
              isResetting = true
              Task {
                await monitor.reset(permission)
                isResetting = false
              }
            }
            .buttonStyle(.glassProminent)
            .disabled(isResetting)
            .help(Text("Makes macOS forget the old switch for JEVision, then opens this list in System Settings"))
          } else {
            Button("Allow", systemImage: "arrow.up.forward.app") { monitor.request(permission) }
              .buttonStyle(.glassProminent)
              .help(Text("Opens this list in System Settings"))
          }
        }
        .labelStyle(.titleAndIcon)
        Image(systemName: granted ? "checkmark.circle.fill" : "xmark.circle.fill")
          .font(.title2)
          .foregroundStyle(granted ? .green : .orange)
          .contentTransition(.symbolEffect(.replace))
          .accessibilityLabel(granted ? Text("Allowed") : Text("Not allowed"))
          .accessibilityIdentifier(AccessibilityID.Permissions.status(permission))
      }
    } label: {
      HStack(spacing: 10) {
        IconTile(symbol: permission.symbol, color: permission.color, size: 30)
        VStack(alignment: .leading, spacing: 2) {
          Text(permission.title)
          Group {
            if stale, monitor.olderCopy.contains(permission) {
              Text("macOS still remembers an older copy of JEVision, so its switch doesn’t work: press Reset and Allow, then turn JEVision on.")
            } else if stale {
              Text("Switch already on but still ✕? Press Reset and Allow, then turn JEVision on again.")
            } else {
              Text(permission.reason)
            }
          }
          .font(.subheadline)
          .foregroundStyle(stale ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
        }
        .fixedSize(horizontal: false, vertical: true)
      }
    }
    .settingAnchor("permission.\(permission.id)")
  }
}
