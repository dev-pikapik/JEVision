import SwiftUI

/// The first screen: what the app is, and the one thing to do.
struct WelcomeView: View {
  let scan: () -> Void
  let paste: (PairingLink) -> Void

  var body: some View {
    ScrollView {
      VStack(spacing: 28) {
        VStack(spacing: 14) {
          AppLogo(size: 104)
          Text("Run Your Tests from iPhone")
            .font(.largeTitle.bold())
            .multilineTextAlignment(.center)
            .accessibilityAddTraits(.isHeader)
          Text("This iPhone becomes a remote for JEVision on your Mac.")
            .font(.title3)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
        }
        .padding(.top, 48)

        VStack(alignment: .leading, spacing: 20) {
          Point(
            symbol: "desktopcomputer", color: .blue, title: Text("Your tests stay on the Mac"),
            text: Text("Look at tests, run them and read reports. Pictures come over only when you open them."))
          Point(
            symbol: "qrcode", color: .purple, title: Text("Pair once with a code"),
            text: Text("On the Mac, open JEVision Settings → iPhone & Remote and scan the code it shows."))
          Point(
            symbol: "lock.shield", color: .green, title: Text("Private end to end"),
            text: Text("The connection is encrypted, and the key that proves it’s you never leaves this iPhone."))
        }
        .padding(.horizontal, 8)
      }
      .padding(.horizontal, 24)
      .frame(maxWidth: 560)
      .frame(maxWidth: .infinity)
    }
    .scrollBounceBehavior(.basedOnSize)
    // Text scrolling under the buttons is cut off, so it never reads through them.
    .scrollEdgeEffectStyle(.hard, for: .bottom)
    .safeAreaBar(edge: .bottom) {
      VStack(spacing: 12) {
        Button(action: scan) {
          Label("Scan Pairing Code", systemImage: "qrcode.viewfinder")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.glassProminent)
        .controlSize(.large)
        PasteLinkButton(paste: paste)
      }
      .padding(.horizontal, 24)
      .padding(.bottom, 12)
      .frame(maxWidth: 560)
      // The buttons stay on screen; past this size they would hide most of what they float over.
      .dynamicTypeSize(...DynamicTypeSize.accessibility2)
    }
  }
}

private struct Point: View {
  let symbol: String
  let color: Color
  let title: Text
  let text: Text

  var body: some View {
    HStack(alignment: .top, spacing: 16) {
      IconTile(symbol: symbol, color: color, size: 40)
      VStack(alignment: .leading, spacing: 3) {
        title.font(.headline)
        text.foregroundStyle(.secondary)
      }
    }
    .accessibilityElement(children: .combine)
  }
}

/// Pastes a pairing link copied on the Mac. The system button reads the clipboard only when
/// tapped, so iOS doesn't ask for permission.
struct PasteLinkButton: View {
  let paste: (PairingLink) -> Void
  @State private var notACode = false

  var body: some View {
    PasteButton(payloadType: String.self) { strings in
      if let link = strings.lazy.compactMap(PairingLink.init).first {
        paste(link)
      } else {
        notACode = true
      }
    }
    .buttonBorderShape(.capsule)
    .labelStyle(.titleAndIcon)
    .alert("This Isn’t a Pairing Code", isPresented: $notACode) {
      Button("OK", role: .cancel) {}
    } message: {
      Text("Copy the pairing link from JEVision on the Mac, then paste it here.")
    }
  }
}

#Preview {
  WelcomeView(scan: {}, paste: { _ in })
}
