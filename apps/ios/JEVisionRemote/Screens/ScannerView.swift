import AVFoundation
import SwiftUI
import VisionKit

/// The camera, looking for the pairing code the Mac shows.
struct ScannerView: View {
  @Environment(AppModel.self) private var model
  @Environment(\.openURL) private var openURL
  @State private var camera = AVCaptureDevice.authorizationStatus(for: .video)
  @State private var otherCode = false
  /// The camera saw a pairing code from a newer JEVision.
  @State private var newerCode = false

  var body: some View {
    NavigationStack {
      content
        .ignoresSafeArea(edges: canScan ? .all : [])
        .navigationTitle("Scan Pairing Code")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
          ToolbarItem(placement: .cancellationAction) {
            Button("Cancel", systemImage: "xmark", role: .cancel) { model.isScanning = false }
          }
        }
        .safeAreaInset(edge: .bottom) {
          VStack(spacing: 12) {
            hint
              .font(.callout)
              .multilineTextAlignment(.center)
              .padding(.horizontal, 20)
              .padding(.vertical, 12)
              .glassEffect(.regular, in: .rect(cornerRadius: 20))
              .animation(.smooth, value: [otherCode, newerCode])
            PasteLinkButton(paste: found)
          }
          .padding(.horizontal, 24)
          .padding(.bottom, 12)
        }
    }
    .task {
      if camera == .notDetermined {
        _ = await AVCaptureDevice.requestAccess(for: .video)
        camera = AVCaptureDevice.authorizationStatus(for: .video)
      }
    }
  }

  private var hint: Text {
    if newerCode { return Text(RemoteError.newerCodeMessage) }
    return Text(otherCode ? "That’s a different code. Point at the one JEVision shows on the Mac." : "On the Mac, open JEVision Settings → iPhone & Remote.")
  }

  private var canScan: Bool {
    camera == .authorized && DataScannerViewController.isSupported
  }

  @ViewBuilder private var content: some View {
    if !DataScannerViewController.isSupported {
      ContentUnavailableView(
        "This iPhone Can’t Scan Codes", systemImage: "camera",
        description: Text("Copy the pairing link from JEVision on the Mac and paste it here instead."))
    } else {
      switch camera {
      case .authorized:
        CodeScanner(found: found, other: { newer in (otherCode, newerCode) = (!newer, newer) })
      case .notDetermined:
        ProgressView()
      default:
        ContentUnavailableView {
          Label("Camera Is Off for JEVision", systemImage: "camera")
        } description: {
          Text("JEVision needs the camera only to read the Mac’s pairing code. You can also paste the link instead.")
        } actions: {
          Button("Open Settings") {
            if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
          }
          .buttonStyle(.glassProminent)
        }
      }
    }
  }

  private func found(_ link: PairingLink) {
    model.isScanning = false
    model.pairingLink = link
  }
}

/// VisionKit's live scanner, reading QR codes only.
private struct CodeScanner: UIViewControllerRepresentable {
  let found: (PairingLink) -> Void
  /// Saw a code that isn't one to pair with; `true` for a pairing code from a newer JEVision.
  let other: (Bool) -> Void

  func makeUIViewController(context: Context) -> DataScannerViewController {
    let scanner = DataScannerViewController(
      recognizedDataTypes: [.barcode(symbologies: [.qr])],
      qualityLevel: .balanced,
      isHighlightingEnabled: true)
    scanner.delegate = context.coordinator
    return scanner
  }

  func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {
    context.coordinator.parent = self
    if !scanner.isScanning, !context.coordinator.done { try? scanner.startScanning() }
  }

  static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
    scanner.stopScanning()
  }

  func makeCoordinator() -> Coordinator { Coordinator(parent: self) }

  @MainActor
  final class Coordinator: NSObject, DataScannerViewControllerDelegate {
    var parent: CodeScanner
    private(set) var done = false

    init(parent: CodeScanner) { self.parent = parent }

    func dataScanner(_ scanner: DataScannerViewController, didAdd items: [RecognizedItem], allItems: [RecognizedItem]) {
      guard !done else { return }
      for item in items {
        guard case .barcode(let code) = item, let text = code.payloadStringValue else { continue }
        if let link = PairingLink(text) {
          done = true
          scanner.stopScanning()
          UINotificationFeedbackGenerator().notificationOccurred(.success)
          parent.found(link)
          return
        }
        parent.other(PairingLink.isFromNewerVersion(text))
      }
    }
  }
}
