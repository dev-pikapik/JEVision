import JEVisionCore

/// The text of a catalog key in the language the host handed to the core, for what the hands
/// report when the system refuses.
func handsWords(_ key: String, _ values: String...) -> String {
  localizedText(key: key, args: values.map { .text(value: $0) })
}
