import SwiftUI

/// First-launch window. Every step can be skipped; Help › Welcome to JEVision opens it again.
struct OnboardingView: View {
  static let doneKey = "onboardingDone"

  private enum Step: Int, CaseIterable {
    case welcome, permissions
  }

  @AppStorage(doneKey) private var done = false
  @Environment(PermissionsMonitor.self) private var monitor
  @Environment(\.dismissWindow) private var dismissWindow
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var step = Step.welcome

  var body: some View {
    VStack(spacing: 0) {
      Group {
        switch step {
        case .welcome: WelcomeStep()
        case .permissions: PermissionsStep()
        }
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .transition(reduceMotion ? .opacity : .push(from: .trailing))
      bottomBar
    }
    .frame(width: 700, height: 580)
    .task { await monitor.watch() }
    .onDisappear { done = true }
  }

  private var bottomBar: some View {
    HStack {
      HStack {
        if step == .welcome {
          Button("Skip", action: finish)
            .keyboardShortcut(.cancelAction)
        } else {
          Button("Back", systemImage: "chevron.left") { go(to: .welcome) }
            .keyboardShortcut("[", modifiers: .command)
        }
      }
      .buttonStyle(.glass)
      .frame(width: 140, alignment: .leading)
      Spacer()
      dots
      Spacer()
      HStack {
        switch step {
        case .welcome:
          Button("Continue") { go(to: .permissions) }
            .buttonStyle(.glassProminent)
        case .permissions where monitor.allGranted:
          Button("Done", action: finish)
            .buttonStyle(.glassProminent)
        case .permissions:
          Button("Later", action: finish)
            .buttonStyle(.glass)
        }
      }
      .keyboardShortcut(.defaultAction)
      .frame(width: 140, alignment: .trailing)
    }
    .controlSize(.large)
    .padding(.horizontal, 20)
    .padding(.vertical, 16)
  }

  private var dots: some View {
    HStack(spacing: 8) {
      ForEach(Step.allCases, id: \.self) { item in
        Circle()
          .fill(item == step ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary))
          .frame(width: 7, height: 7)
      }
    }
    .accessibilityElement()
    .accessibilityLabel(Text("Step \(step.rawValue + 1) of \(Step.allCases.count)"))
  }

  private func go(to next: Step) {
    withAnimation(.snappy) { step = next }
  }

  private func finish() {
    done = true
    dismissWindow(id: SceneID.onboarding)
  }
}
