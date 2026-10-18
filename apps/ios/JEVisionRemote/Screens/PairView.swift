import SwiftUI

/// Confirms a scanned code and pairs with the Mac.
struct PairView: View {
  let link: PairingLink
  /// Closes this sheet and opens the camera again.
  let scanAgain: () -> Void
  @Environment(AppModel.self) private var model
  @Environment(\.dismiss) private var dismiss
  @State private var deviceName = UIDevice.current.name
  @State private var step = Step.confirm

  private enum Step: Equatable {
    case confirm
    case pairing
    case paired(HostRecord)
    case failed(RemoteError)
  }

  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(spacing: 24) {
          header
          switch step {
          case .confirm, .pairing:
            VStack(alignment: .leading, spacing: 8) {
              Text("Name of this iPhone on the Mac")
                .font(.footnote)
                .foregroundStyle(.secondary)
              TextField("iPhone", text: $deviceName)
                .textInputAutocapitalization(.words)
                .submitLabel(.go)
                .onSubmit(pair)
                .padding(14)
                .background(.fill.tertiary, in: .rect(cornerRadius: 14))
                .disabled(step == .pairing)
            }
          case .paired, .failed:
            EmptyView()
          }
        }
        .padding(24)
      }
      .scrollBounceBehavior(.basedOnSize)
      .safeAreaInset(edge: .bottom) { actions.dynamicTypeSize(...DynamicTypeSize.accessibility2) }
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          if case .paired = step {
          } else {
            Button("Cancel", systemImage: "xmark", role: .cancel) { dismiss() }
              .disabled(step == .pairing)
          }
        }
      }
    }
    .interactiveDismissDisabled(step == .pairing)
    .presentationDetents([.large])
  }

  @ViewBuilder private var header: some View {
    VStack(spacing: 12) {
      switch step {
      case .confirm, .pairing:
        IconTile(symbol: "desktopcomputer", color: .blue, size: 72)
        Text("Pair with “\(link.macName)”?")
          .font(.title2.bold())
        Text("This iPhone will see the tests on this Mac and, if the Mac allows it, run them. Your tests stay on the Mac.")
          .foregroundStyle(.secondary)
      case .paired(let host):
        IconTile(symbol: "checkmark", color: .green, size: 72)
        Text("Paired with “\(host.name)”")
          .font(.title2.bold())
        Text("You can manage what this iPhone may do in JEVision on the Mac.")
          .foregroundStyle(.secondary)
      case .failed(let error):
        IconTile(symbol: error.symbol, color: .red, size: 72)
        Text(error.title)
          .font(.title2.bold())
        Text(error.message)
          .foregroundStyle(.secondary)
      }
      if model.lock.problem == .noPasscode {
        Text("Set a passcode for this iPhone in Settings first. JEVision needs it to keep your Macs safe.")
          .foregroundStyle(.red)
      }
    }
    .multilineTextAlignment(.center)
    .padding(.top, 24)
  }

  @ViewBuilder private var actions: some View {
    Group {
      switch step {
      case .confirm:
        Button(action: pair) {
          Text("Pair").frame(maxWidth: .infinity)
        }
        .buttonStyle(.glassProminent)
        .accessibilityIdentifier("pair")
        .disabled(deviceName.trimmingCharacters(in: .whitespaces).isEmpty)
      case .pairing:
        Button(action: {}) {
          HStack(spacing: 10) {
            ProgressView()
            Text("Waiting for the Mac…")
          }
          .frame(maxWidth: .infinity)
        }
        .buttonStyle(.glass)
        .disabled(true)
      case .paired:
        Button { dismiss() } label: {
          Text("Done").frame(maxWidth: .infinity)
        }
        .buttonStyle(.glassProminent)
        .accessibilityIdentifier("pairDone")
      case .failed(let error):
        if error == .invalidCode || error == .codeExpired {
          Button(action: scanAgain) {
            Text("Scan a New Code").frame(maxWidth: .infinity)
          }
          .buttonStyle(.glassProminent)
        } else {
          Button(action: pair) {
            Text("Try Again").frame(maxWidth: .infinity)
          }
          .buttonStyle(.glassProminent)
        }
      }
    }
    .controlSize(.large)
    .padding(.horizontal, 24)
    .padding(.bottom, 12)
  }

  private func pair() {
    let name = deviceName.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !name.isEmpty, step != .pairing else { return }
    step = .pairing
    Task {
      do throws(RemoteError) {
        if let host = try await model.pair(link, deviceName: name) {
          step = .paired(host)
          UINotificationFeedbackGenerator().notificationOccurred(.success)
        } else {
          step = .confirm
        }
      } catch {
        step = .failed(error)
        UINotificationFeedbackGenerator().notificationOccurred(.error)
      }
    }
  }
}
