import SwiftUI

@main
struct JEVisionRemoteApp: App {
  @State private var model: AppModel
  @Environment(\.scenePhase) private var scenePhase

  init() {
    let key = DeviceKey()
    let lock = AppLock()
    let model = AppModel(client: CoreRemoteClient(signer: key), lock: lock)
    lock.onChange = { [weak model] context in
      key.authenticate(with: context)
      if context == nil { model?.disconnectAll() }
    }
    _model = State(initialValue: model)
  }

  var body: some Scene {
    WindowGroup {
      RootView()
        .environment(model)
    }
    .onChange(of: scenePhase) { _, phase in
      switch phase {
      case .background: model.lock.didEnterBackground()
      case .active:
        model.lock.checkExpiry()
        // Back from the background: try again the Macs that were out of reach.
        if model.lock.isUnlocked { Task { await model.connectAll() } }
      default: break
      }
    }
  }
}
