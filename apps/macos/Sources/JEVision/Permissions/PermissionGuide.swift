import AppKit
import SwiftUI

/// Shows where to click in System Settings: a small picture of the list with a switch turning on.
struct PermissionGuide: View {
  let permission: Permission
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var isOn = false

  var body: some View {
    HStack(spacing: 14) {
      miniSettings
      Text("In System Settings, open **Privacy & Security** › **\(permission.listName)** and turn on **JEVision**.")
        .font(.callout)
        .fixedSize(horizontal: false, vertical: true)
    }
    .padding(.vertical, 4)
  }

  private var miniSettings: some View {
    VStack(alignment: .leading, spacing: 5) {
      Capsule().fill(.quaternary).frame(width: 80, height: 6)
      HStack(spacing: 5) {
        Image(nsImage: NSApp.applicationIconImage)
          .resizable()
          .frame(width: 14, height: 14)
        Text(verbatim: "JEVision")
          .font(.caption2.bold())
        Spacer(minLength: 4)
        Toggle(isOn: .constant(isOn)) { EmptyView() }
          .toggleStyle(.switch)
          .controlSize(.mini)
          .labelsHidden()
      }
      .padding(.horizontal, 5)
      .padding(.vertical, 3)
      .background(Color.accentColor.opacity(0.14), in: .rect(cornerRadius: 5))
      Capsule().fill(.quaternary).frame(width: 110, height: 6)
    }
    .padding(8)
    .frame(width: 150)
    .background(.background, in: .rect(cornerRadius: 9))
    .shadow(color: .black.opacity(0.12), radius: 4, y: 2)
    .allowsHitTesting(false)
    .accessibilityHidden(true)
    .task(id: reduceMotion) {
      guard !reduceMotion else {
        isOn = true
        return
      }
      while !Task.isCancelled {
        try? await Task.sleep(for: .seconds(1.2))
        withAnimation(.snappy) { isOn.toggle() }
      }
    }
  }
}
