import JEVisionCore
import SwiftUI

struct RemoteSettingsView: View {
  @Environment(RemoteAccess.self) private var remote
  @State private var relay = RemoteAccess.RelayChoice.standard
  @State private var customRelay = ""
  @State private var revoking: PairedDevice?
  @State private var showsActivity = false

  var body: some View {
    @Bindable var remote = remote
    Form {
      SettingsHeader(tab: .remote, text: Text("Run tests and read reports from your iPhone. Tests and screenshots stay on this Mac."))
      Section {
        Toggle(isOn: $remote.isEnabled) {
          RowLabel(Text("Allow remote access"), status)
        }
        .settingAnchor("remoteAccess")
        if remote.isEnabled { pairing }
      }
      if remote.isEnabled { devices }
      Section {
        Toggle("Ask on this Mac before a remote run", isOn: $remote.confirmsRuns)
          .settingAnchor("confirmRuns")
        Toggle("Keep this Mac awake while remote access is on", isOn: $remote.keepsAwake)
          .settingAnchor("keepAwake")
        relayPicker
        LabeledContent {
          Button("Show…") { showsActivity = true }
        } label: {
          RowLabel(Text("Activity log"), Text("Who ran what, and when"))
        }
        .settingAnchor("activityLog")
      } footer: {
        Text("End-to-end encrypted. No ports are opened on this Mac. Keys and secrets are never sent to the phone.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      RestoreDefaultsSection(
        message: Text("Remote access turns off, remote runs start without asking, this Mac stays awake while remote access is on, and the standard relay is used. Paired devices stay paired."),
        isDefault: !remote.isEnabled && !remote.confirmsRuns && remote.keepsAwake && remote.relayChoice == .standard
          && remote.customRelay.isEmpty,
        reset: restoreDefaults)
    }
    .formStyle(.grouped)
    .onAppear {
      relay = remote.relayChoice
      customRelay = remote.customRelay
      if remote.isEnabled { remote.showCode() }
    }
    .onDisappear(perform: remote.hideCode)
    .onChange(of: remote.isEnabled) {
      if remote.isEnabled { remote.showCode() }
    }
    .task {
      while !Task.isCancelled {
        remote.refresh()
        try? await Task.sleep(for: .seconds(2))
      }
    }
    .alert(
      "Revoke access for “\(revoking?.name ?? "")”?", isPresented: Binding(get: { revoking != nil }, set: { if !$0 { revoking = nil } }), presenting: revoking
    ) { device in
      Button("Revoke", role: .destructive) {
        remote.revoke(device)
        revoking = nil
      }
      Button("Cancel", role: .cancel) { revoking = nil }
    } message: { _ in
      Text("The device disconnects now. To use it again, pair it with a new code.")
    }
    .alert(
      "Couldn’t change the device", isPresented: Binding(get: { remote.failure != nil }, set: { if !$0 { remote.failure = nil } }), presenting: remote.failure
    ) { _ in
      Button("OK") { remote.failure = nil }
    } message: { failure in
      Text(failure)
    }
    .sheet(isPresented: $showsActivity) { RemoteActivityView() }
  }

  private var status: Text {
    switch remote.phase {
    case .off:
      Text("Paired iPhones can’t reach this Mac.")
    case .starting:
      Text("Turning on…")
    case .on:
      remote.keepsMacAwakeNow ? Text("On. This Mac won’t fall asleep on its own.") : Text("On")
    case .failed(let reason):
      Text("Couldn’t turn on: \(reason)")
        .foregroundStyle(.red)
    }
  }

  private func restoreDefaults() {
    remote.isEnabled = false
    remote.confirmsRuns = false
    remote.keepsAwake = true
    relay = .standard
    customRelay = ""
    remote.setRelay(.standard, customURL: "")
  }

  @ViewBuilder private var pairing: some View {
    if case .failed = remote.phase {
      LabeledContent {
        Button("Try Again", action: remote.retry)
      } label: {
        Text("Remote access is off until it can turn on.")
      }
    } else {
      HStack(alignment: .center, spacing: 18) {
        code
        VStack(alignment: .leading, spacing: 6) {
          Text("1. Open JEVision on your iPhone")
          Text("2. Tap **Pair a Mac**")
          Text("3. Point the camera at this code")
          countdown
            .font(.callout)
            .foregroundStyle(.secondary)
            .padding(.top, 4)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
      }
      .padding(.vertical, 6)
      Picker("The next device can", selection: Bindable(remote).nextPermission) {
        Text("Can run tests").tag(DevicePermission.run)
        Text("View only").tag(DevicePermission.view)
      }
    }
  }

  private var code: some View {
    ZStack {
      RoundedRectangle(cornerRadius: 10).fill(.white)
      if let qr = remote.qrCode {
        Image(decorative: qr, scale: 1)
          .interpolation(.none)
          .resizable()
          .scaledToFit()
          .padding(10)
          .accessibilityLabel(Text("Pairing code"))
      } else if remote.phase == .starting || remote.offer == nil && remote.justPaired == nil {
        ProgressView().controlSize(.small)
      } else {
        Image(systemName: "checkmark.circle.fill")
          .font(.largeTitle)
          .imageScale(.large)
          .foregroundStyle(.green)
      }
    }
    .frame(width: 148, height: 148)
  }

  @ViewBuilder private var countdown: some View {
    if let paired = remote.justPaired {
      VStack(alignment: .leading, spacing: 4) {
        Text("“\(paired)” is paired.")
        Button("Show a Code for Another Device", action: remote.showCode)
      }
    } else if let offer = remote.offer {
      TimelineView(.periodic(from: .now, by: 1)) { context in
        let left = RemoteAccess.secondsLeft(of: offer, at: context.date)
        if left > 0 {
          Text("The code works once and expires in \(Duration.seconds(left).formatted(.time(pattern: .minuteSecond))).")
        } else {
          VStack(alignment: .leading, spacing: 4) {
            Text("The code has expired.")
            Button("Show a New Code", action: remote.showCode)
          }
        }
      }
    }
  }

  @ViewBuilder private var devices: some View {
    Section("Paired devices") {
      if remote.devices.isEmpty {
        Text("No devices yet. Scan the code with your iPhone to pair it.")
          .foregroundStyle(.secondary)
      }
      ForEach(remote.devices, id: \.id) { device in
        let online = remote.connected.contains(device.id)
        LabeledContent {
          HStack {
            Picker(
              "Permission",
              selection: Binding(get: { device.permission }, set: { remote.setPermission($0, of: device) })
            ) {
              Text("Can run tests").tag(DevicePermission.run)
              Text("View only").tag(DevicePermission.view)
            }
            .labelsHidden()
            .fixedSize()
            Button("Revoke", role: .destructive) { revoking = device }
          }
        } label: {
          Label {
            RowLabel(Text(device.name), online ? Text("Connected now") : lastSeen(device))
          } icon: {
            Image(systemName: "iphone")
              .foregroundStyle(online ? .green : .secondary)
          }
        }
      }
    }
  }

  private func lastSeen(_ device: PairedDevice) -> Text {
    guard let seen = device.lastSeen else {
      return Text("Paired \(Date(timeIntervalSince1970: TimeInterval(device.pairedAt)), format: .relative(presentation: .named))")
    }
    return Text("Last connected \(Date(timeIntervalSince1970: TimeInterval(seen)), format: .relative(presentation: .named))")
  }

  @ViewBuilder private var relayPicker: some View {
    Picker(selection: $relay) {
      Text("Standard").tag(RemoteAccess.RelayChoice.standard)
      Text("My own").tag(RemoteAccess.RelayChoice.custom)
      Text("None").tag(RemoteAccess.RelayChoice.none)
    } label: {
      Text("Relay")
      Text("Carries encrypted traffic when the iPhone can’t reach this Mac directly. Without one, only the same Wi-Fi or a VPN works.")
    }
    .settingAnchor("relay")
    .onChange(of: relay) {
      if relay != .custom { remote.setRelay(relay, customURL: customRelay) }
    }
    if relay == .custom {
      LabeledContent("Relay address") {
        HStack {
          TextField("Relay address", text: $customRelay, prompt: Text(verbatim: "https://relay.example.com"))
            .labelsHidden()
            .onSubmit(useCustomRelay)
          Button("Use", action: useCustomRelay)
            .disabled(!RemoteAccess.isRelayAddress(customRelay))
        }
      }
    }
  }

  private func useCustomRelay() {
    guard RemoteAccess.isRelayAddress(customRelay) else { return }
    remote.setRelay(.custom, customURL: customRelay)
  }
}

/// Who did what from afar, newest first.
private struct RemoteActivityView: View {
  @Environment(RemoteAccess.self) private var remote
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    VStack(spacing: 0) {
      if remote.activity.isEmpty {
        ContentUnavailableView(
          "Nothing yet", systemImage: "list.bullet.rectangle",
          description: Text("Pairings, connections and remote runs appear here."))
      } else {
        List(Array(remote.activity.enumerated()), id: \.offset) { _, entry in
          VStack(alignment: .leading, spacing: 2) {
            HStack {
              Text(entry.deviceName ?? String(localized: "Unknown device")).bold()
              Text(RemoteText.kind(entry.kind))
              Spacer()
              Text(Date(timeIntervalSince1970: TimeInterval(entry.time)), format: .dateTime.day().month().hour().minute())
                .foregroundStyle(.secondary)
            }
            if let subject = entry.subject {
              Text(verbatim: subject).foregroundStyle(.secondary)
            }
            if let error = entry.error {
              Text(RemoteText.text(error)).foregroundStyle(.red)
            }
          }
          .accessibilityElement(children: .combine)
        }
      }
      HStack {
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
      .padding()
    }
    .frame(width: 520, height: 420)
  }
}
