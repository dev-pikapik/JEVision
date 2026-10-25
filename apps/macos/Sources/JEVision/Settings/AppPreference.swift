import JEVisionHands
import SwiftUI

private struct AppPreferenceStoreKey: EnvironmentKey {
  static var defaultValue: UserDefaults? { nil }
}

extension EnvironmentValues {
  var appPreferenceStore: UserDefaults? {
    get { self[AppPreferenceStoreKey.self] }
    set { self[AppPreferenceStoreKey.self] = newValue }
  }
}

extension View {
  /// AppStorage and its guarded binding must use the same store, including in previews and
  /// isolated interface tests. The store remains UserDefaults; no subclass intercepts SwiftUI.
  nonisolated func appPreferences(_ store: UserDefaults) -> some View {
    defaultAppStorage(store).environment(\.appPreferenceStore, store)
  }
}

/// AppStorage's normal observation and registration, with compatibility checked before its
/// binding writes. A refused change retains the saved value and raises the shared GUI error.
@MainActor
@propertyWrapper
struct AppPreference<Value>: DynamicProperty {
  @AppStorage private var value: Value
  @Environment(\.appPreferenceStore) private var environmentStore
  private var explicitStore: UserDefaults?

  var wrappedValue: Value {
    get { value }
    nonmutating set {
      AppPreferences.write(in: explicitStore ?? environmentStore ?? Profile.current.defaults) { value = newValue }
    }
  }

  var projectedValue: Binding<Value> {
    Binding(get: { wrappedValue }, set: { wrappedValue = $0 })
  }

  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value == Bool {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value == Int {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value == Double {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value == String {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value == Data {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value == URL {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value: RawRepresentable, Value.RawValue == String {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(wrappedValue: Value, _ key: String, store: UserDefaults? = nil) where Value: RawRepresentable, Value.RawValue == Int {
    _value = AppStorage(wrappedValue: wrappedValue, key, store: store); explicitStore = store
  }
  init(_ key: String, store: UserDefaults? = nil) where Value == Double? {
    _value = AppStorage(key, store: store); explicitStore = store
  }
  init(_ key: String, store: UserDefaults? = nil) where Value == String? {
    _value = AppStorage(key, store: store); explicitStore = store
  }
  init(_ key: String, store: UserDefaults? = nil) where Value == Data? {
    _value = AppStorage(key, store: store); explicitStore = store
  }
}
