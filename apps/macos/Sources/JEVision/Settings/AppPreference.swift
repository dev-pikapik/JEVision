import JEVisionHands
import SwiftUI

private struct AppPreferenceContextKey: EnvironmentKey {
  static var defaultValue: PreferenceContext? { nil }
}

extension EnvironmentValues {
  var appPreferenceContext: PreferenceContext? {
    get { self[AppPreferenceContextKey.self] }
    set { self[AppPreferenceContextKey.self] = newValue }
  }
}

private struct PreferenceContextModifier: ViewModifier {
  @State private var context: PreferenceContext

  init(_ source: any PreferenceSource) {
    _context = State(initialValue: PreferenceContext(source))
  }

  func body(content: Content) -> some View {
    content.environment(\.appPreferenceContext, context)
  }
}

extension View {
  @MainActor func appPreferences(_ source: any PreferenceSource) -> some View {
    modifier(PreferenceContextModifier(source))
  }
}

/// Reads the selected backend directly. Only the observation context is retained by SwiftUI;
/// preference values and failed writes never become a second, in-memory source of truth.
@MainActor
@propertyWrapper
struct AppPreference<Value>: DynamicProperty {
  @Environment(\.appPreferenceContext) private var environmentContext
  @State private var explicitContext: PreferenceContext?
  private let key: String
  private let fallback: Value
  private let decode: (PreferenceStore, String) -> Value
  private let encode: (Value) -> Any?

  private var context: PreferenceContext {
    explicitContext ?? environmentContext ?? PreferenceContext.current
  }

  var wrappedValue: Value {
    get { context.read(key, fallback: fallback, decode: decode) }
    nonmutating set {
      let store = context.store
      if let value = encode(newValue) { AppPreferences.set(value, forKey: key, in: store) }
      else { AppPreferences.removeObject(forKey: key, in: store) }
    }
  }

  var projectedValue: Binding<Value> {
    Binding(get: { wrappedValue }, set: { wrappedValue = $0 })
  }

  private init(_ key: String, fallback: Value, store: (any PreferenceSource)?,
    decode: @escaping (PreferenceStore, String) -> Value, encode: @escaping (Value) -> Any?) {
    self.key = key
    self.fallback = fallback
    self.decode = decode
    self.encode = encode
    _explicitContext = State(initialValue: store.map { PreferenceContext($0) })
  }

  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil) where Value == Bool {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { $0.object(forKey: $1) == nil ? wrappedValue : $0.bool(forKey: $1) }, encode: { $0 })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil) where Value == Int {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { $0.object(forKey: $1) == nil ? wrappedValue : $0.integer(forKey: $1) }, encode: { $0 })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil) where Value == Double {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { $0.object(forKey: $1) == nil ? wrappedValue : $0.double(forKey: $1) }, encode: { $0 })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil) where Value == String {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { $0.string(forKey: $1) ?? wrappedValue }, encode: { $0 })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil) where Value == Data {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { $0.data(forKey: $1) ?? wrappedValue }, encode: { $0 })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil) where Value == URL {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { $0.url(forKey: $1) ?? wrappedValue }, encode: { $0 })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil)
    where Value: RawRepresentable, Value.RawValue == String {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { store, key in
        guard let raw = store.string(forKey: key) else { return wrappedValue }
        return Value(rawValue: raw) ?? wrappedValue
      },
      encode: { $0.rawValue })
  }
  init(wrappedValue: Value, _ key: String, store: (any PreferenceSource)? = nil)
    where Value: RawRepresentable, Value.RawValue == Int {
    self.init(key, fallback: wrappedValue, store: store,
      decode: { store, key in
        store.object(forKey: key) == nil ? wrappedValue : Value(rawValue: store.integer(forKey: key)) ?? wrappedValue
      }, encode: { $0.rawValue })
  }
  init(_ key: String, store: (any PreferenceSource)? = nil) where Value == Double? {
    self.init(key, fallback: nil, store: store,
      decode: { $0.object(forKey: $1) == nil ? nil : $0.double(forKey: $1) }, encode: { $0 })
  }
  init(_ key: String, store: (any PreferenceSource)? = nil) where Value == String? {
    self.init(key, fallback: nil, store: store, decode: { $0.string(forKey: $1) }, encode: { $0 })
  }
  init(_ key: String, store: (any PreferenceSource)? = nil) where Value == Data? {
    self.init(key, fallback: nil, store: store, decode: { $0.data(forKey: $1) }, encode: { $0 })
  }
  init(_ key: String, store: (any PreferenceSource)? = nil) where Value == URL? {
    self.init(key, fallback: nil, store: store, decode: { $0.url(forKey: $1) }, encode: { $0 })
  }
}
