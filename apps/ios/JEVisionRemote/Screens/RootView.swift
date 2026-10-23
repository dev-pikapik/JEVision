import SwiftUI

struct RootView: View {
  @Environment(AppModel.self) private var model
  @Environment(\.scenePhase) private var scenePhase
  @State private var unknownLink = false
  @State private var newerLink = false
  @State private var scanAfterPairing = false

  var body: some View {
    @Bindable var model = model
    // The example stands in for the app while it has its Mac; pairing stays with the app.
    let shown = model.example.flatMap { $0.hosts.isEmpty ? nil : $0 } ?? model
    Group {
      if shown.hosts.isEmpty {
        WelcomeView(scan: { model.isScanning = true }, paste: { model.pairingLink = $0 }) {
          Task { await model.openExample(deviceName: UIDevice.current.name) }
        }
      } else if shown.lock.isUnlocked {
        MacsView { model.isScanning = true }
      } else {
        LockView()
      }
    }
    .environment(shown)
    .animation(.smooth, value: shown.hosts.isEmpty)
    .animation(.smooth, value: shown.lock.isUnlocked)
    .animation(.smooth, value: shown.isExample)
    .fullScreenCover(isPresented: $model.isScanning) {
      ScannerView()
    }
    .sheet(item: $model.pairingLink, onDismiss: scanIfAsked) { link in
      PairView(link: link) {
        scanAfterPairing = true
        model.pairingLink = nil
      }
    }
    .onOpenURL { url in
      if let link = PairingLink(url) {
        model.isScanning = false
        model.pairingLink = link
      } else if PairingLink.isFromNewerVersion(url.absoluteString) {
        newerLink = true
      } else {
        unknownLink = true
      }
    }
    .alert("This Isn’t a Pairing Code", isPresented: $unknownLink) {
      Button("OK", role: .cancel) {}
    } message: {
      Text("To pair, scan the code JEVision shows on the Mac in Settings → iPhone & Remote.")
    }
    .alert("Pair Your Macs Again", isPresented: $model.listWasUnreadable) {
      Button("OK", role: .cancel) {}
    } message: {
      Text("The list of paired Macs couldn’t be read. A copy was kept.")
    }
    .alert("Can’t Open Paired Macs", isPresented: $model.listDidNotOpen) {
      Button("OK", role: .cancel) {}
    } message: {
      Text(AppModel.notOpenedMessage)
    }
    .alert("Update JEVision Remote", isPresented: $newerLink) {
      Button("OK", role: .cancel) {}
    } message: {
      Text(RemoteError.newerCodeMessage)
    }
    .overlay {
      // Keeps the Mac's tests out of the app switcher's snapshot.
      if scenePhase != .active && !model.hosts.isEmpty {
        PrivacyCover()
      }
    }
  }
}

extension RootView {
  private func scanIfAsked() {
    if scanAfterPairing {
      scanAfterPairing = false
      model.isScanning = true
    }
  }
}

private struct PrivacyCover: View {
  var body: some View {
    ZStack {
      Rectangle().fill(.background)
      Image("Logo")
        .resizable()
        .frame(width: 96, height: 96)
        .accessibilityHidden(true)
    }
    .ignoresSafeArea()
  }
}

/// The app's icon, for the first screen and the lock screen.
struct AppLogo: View {
  var size: CGFloat = 96

  var body: some View {
    Image("Logo")
      .resizable()
      .frame(width: size, height: size)
      .clipShape(.rect(cornerRadius: size * 0.225))
      .shadow(color: .black.opacity(0.15), radius: 12, y: 6)
      .accessibilityHidden(true)
  }
}

struct LockView: View {
  @Environment(AppModel.self) private var model
  @Environment(\.scenePhase) private var scenePhase

  var body: some View {
    VStack(spacing: 16) {
      Spacer()
      AppLogo()
      Text("JEVision Is Locked")
        .font(.title2.bold())
      Text(message)
        .foregroundStyle(.secondary)
        .multilineTextAlignment(.center)
        .padding(.horizontal, 32)
      Spacer()
      Button(action: unlock) {
        Label("Unlock", systemImage: "faceid")
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.glassProminent)
      .controlSize(.large)
      .disabled(model.lock.state == .unlocking)
      .padding(.horizontal, 24)
      .padding(.bottom, 24)
    }
    .task(id: scenePhase) {
      if scenePhase == .active, model.lock.problem == nil { unlock() }
    }
  }

  private var message: String {
    switch model.lock.problem {
    case .noPasscode:
      String(localized: "Set a passcode for this iPhone in Settings. JEVision needs it to keep your Macs safe.")
    case .failed:
      String(localized: "Face ID or the passcode didn’t work. Try again.")
    case nil:
      String(localized: "Unlock with Face ID or your passcode to reach your Macs.")
    }
  }

  private func unlock() {
    Task { await model.lock.unlock(reason: String(localized: "Unlock to reach your Macs.")) }
  }
}
