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
      SettingsHeader(tab: .remote, text: Text("Run tests and read reports from your iPhone, or let an AI assistant on another computer use this Mac. Tests and screenshots stay on this Mac."))
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
      "Disconnect “\(revoking?.name ?? "")”?", isPresented: Binding(get: { revoking != nil }, set: { if !$0 { revoking = nil } }), presenting: revoking
    ) { device in
      Button("Disconnect", role: .destructive) {
        remote.revoke(device)
        revoking = nil
      }
      Button("Cancel", role: .cancel) { revoking = nil }
    } message: { device in
      device.kind == .assistant
        ? Text("The assistant disconnects now and what it started stops. To use it again, pair it with a new code.")
        : Text("The device disconnects now. To use it again, pair it with a new code.")
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
      Text("Paired iPhones and assistants can’t reach this Mac.")
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
      Picker("Add", selection: Bindable(remote).newcomer) {
        Text("iPhone").tag(RemoteAccess.Newcomer.iPhone)
        Text("Assistant on another computer").tag(RemoteAccess.Newcomer.assistant)
      }
      .pickerStyle(.segmented)
      .settingAnchor("addDevice")
      switch remote.newcomer {
      case .iPhone:
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
          Text("Can edit tests").tag(DevicePermission.edit)
          Text("Can run tests").tag(DevicePermission.run)
          Text("View only").tag(DevicePermission.view)
        }
      case .assistant:
        assistantPairing
      }
    }
  }

  /// The command that pairs an assistant on another computer, and the one its settings run after.
  @ViewBuilder private var assistantPairing: some View {
    VStack(alignment: .leading, spacing: 8) {
      if let offer = remote.offer, remote.justPaired == nil {
        Text("1. On the other computer, run this in Terminal. It needs the jevision command line tool.")
        CommandText(RemoteAccess.pairCommand(for: offer))
      }
      if remote.justPaired != nil {
        Label {
          Text("Add JEVision to the MCP servers of your assistant there, such as Claude, Cursor or Copilot, with:")
        } icon: {
          Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        }
      } else if remote.offer != nil {
        Text("2. Then add JEVision to the MCP servers of your assistant there, such as Claude, Cursor or Copilot, with:")
      } else {
        ProgressView().controlSize(.small)
      }
      if remote.offer != nil || remote.justPaired != nil {
        CommandText(RemoteAccess.connectCommand())
      }
      countdown
        .font(.callout)
        .foregroundStyle(.secondary)
      Text("The assistant may use this Mac’s tools as far as Settings → AI → Assistants allows. Turn it off below at any time.")
        .font(.callout)
        .foregroundStyle(.secondary)
    }
    .fixedSize(horizontal: false, vertical: true)
    .padding(.vertical, 6)
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
        Text("No devices yet. Scan the code with your iPhone, or pair an assistant, to add one.")
          .foregroundStyle(.secondary)
      }
      ForEach(remote.devices, id: \.id) { device in
        let online = remote.connected.contains(device.id)
        LabeledContent {
          HStack {
            switch device.kind {
            case .phone:
              Picker(
                "Permission",
                selection: Binding(get: { device.permission }, set: { remote.setPermission($0, of: device) })
              ) {
                Text("Can edit tests").tag(DevicePermission.edit)
                Text("Can run tests").tag(DevicePermission.run)
                Text("View only").tag(DevicePermission.view)
              }
              .labelsHidden()
              .fixedSize()
            case .assistant:
              Toggle("Assistant", isOn: Binding(get: { device.assistant }, set: { remote.setAssistant($0, of: device) }))
                .help("Lets it use this Mac’s tools. Turning it off disconnects it now.")
            }
            Button("Disconnect", role: .destructive) { revoking = device }
          }
        } label: {
          Label {
            RowLabel(Text(device.name), Text("\(kindName(device)) · \(state(device, online: online))"))
          } icon: {
            Image(systemName: device.kind == .assistant ? "terminal" : "iphone")
              .foregroundStyle(online ? .green : .secondary)
          }
        }
        .accessibilityElement(children: .contain)
      }
    }
  }

  private func kindName(_ device: PairedDevice) -> String {
    device.kind == .assistant ? String(localized: "Assistant") : String(localized: "iPhone")
  }

  private func state(_ device: PairedDevice, online: Bool) -> String {
    if online { return String(localized: "Connected now") }
    func ago(_ seconds: Int64) -> String {
      Date(timeIntervalSince1970: TimeInterval(seconds)).formatted(.relative(presentation: .named))
    }
    guard let seen = device.lastSeen else { return String(localized: "Paired \(ago(device.pairedAt))") }
    return String(localized: "Last connected \(ago(seen))")
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
              // The list of devices is not about one device: its kind is the title.
              if entry.kind == .devicesUnreadable {
                Text(RemoteText.kind(entry.kind)).bold()
              } else {
                Text(entry.deviceName ?? String(localized: "Unknown device")).bold()
                Text(RemoteText.kind(entry.kind))
              }
              Spacer()
              Text(Date(timeIntervalSince1970: TimeInterval(entry.time)), format: .dateTime.day().month().hour().minute())
                .foregroundStyle(.secondary)
            }
            if let subject = RemoteText.subject(entry) {
              Text(verbatim: subject).foregroundStyle(.secondary)
            }
            if let problem = RemoteText.problem(entry) {
              Text(verbatim: problem).foregroundStyle(.red)
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
