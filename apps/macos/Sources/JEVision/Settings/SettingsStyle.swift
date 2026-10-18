import SwiftUI

/// The rounded tile in front of each pane in the Settings sidebar and at the top of the pane:
/// a coloured tile with a white symbol in light mode, a dark tile with a coloured symbol in dark mode.
struct SectionIcon: View {
  let symbol: String
  let color: Color
  var size: CGFloat = 20
  @Environment(\.colorScheme) private var scheme

  var body: some View {
    let shape = RoundedRectangle(cornerRadius: size * 0.235, style: .continuous)
    let dark = scheme == .dark
    Image(systemName: symbol)
      .font(.system(size: size * 0.56, weight: .medium))
      .symbolRenderingMode(.monochrome)
      .imageScale(.medium)
      .foregroundStyle(dark ? color : .white)
      .frame(width: size, height: size)
      .background {
        ZStack {
          shape.fill(dark ? Color(white: 0.13) : .white)
          shape.fill(
            LinearGradient(
              colors: dark ? [Color(white: 0.17), Color(white: 0.11)] : [color.opacity(0.82), color],
              startPoint: .top, endPoint: .bottom))
        }
      }
      .overlay(shape.strokeBorder(dark ? Color.white.opacity(0.14) : Color.black.opacity(0.06), lineWidth: 0.5))
      .drawingGroup()
      .accessibilityHidden(true)
  }
}

/// The pane's icon, title and one line of explanation, centred at the top of its form.
struct SettingsHeader: View {
  let tab: SettingsTab
  var warns = false
  var title: Text?
  let text: Text

  var body: some View {
    Section {
      VStack(spacing: 0) {
        tab.icon(size: 48, warns: warns)
          .padding(.bottom, 12)
        (title ?? Text(tab.title))
          .font(.title.bold())
          .padding(.bottom, 4)
          .accessibilityAddTraits(.isHeader)
        text
          .font(.subheadline)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      .multilineTextAlignment(.center)
      .frame(maxWidth: .infinity)
      .padding(.top, 14)
      .padding(.bottom, 10)
      .padding(.horizontal, 24)
    }
  }
}

/// A row's title with an optional line of explanation under it.
struct RowLabel: View {
  let title: Text
  var subtitle: Text?

  init(_ title: Text, _ subtitle: Text? = nil) {
    self.title = title
    self.subtitle = subtitle
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 2) {
      title
      if let subtitle {
        subtitle
          .font(.subheadline)
          .foregroundStyle(.secondary)
      }
    }
    .fixedSize(horizontal: false, vertical: true)
  }
}

/// Label on the left, centred against the control on the right.
struct CenteredLabeledContentStyle: LabeledContentStyle {
  @Environment(\.isEnabled) private var isEnabled

  func makeBody(configuration: Configuration) -> some View {
    HStack(alignment: .center, spacing: 12) {
      configuration.label
        .opacity(isEnabled ? 1 : 0.5)
      Spacer(minLength: 0)
      configuration.content
    }
  }
}

/// Label on the left, switch on the right, centred against each other.
struct CenteredSwitchStyle: ToggleStyle {
  @Environment(\.isEnabled) private var isEnabled

  func makeBody(configuration: Configuration) -> some View {
    HStack(alignment: .center, spacing: 12) {
      configuration.label
        .opacity(isEnabled ? 1 : 0.5)
      Spacer(minLength: 0)
      Toggle(isOn: configuration.$isOn) { configuration.label }
        .labelsHidden()
        .toggleStyle(.switch)
    }
    .accessibilityRepresentation {
      Toggle(isOn: configuration.$isOn) { configuration.label }
        .toggleStyle(.switch)
    }
  }
}

/// The last section of a pane: a button that puts the pane's own settings back, after asking.
struct RestoreDefaultsSection: View {
  let message: Text
  let isDefault: Bool
  let reset: () -> Void
  @State private var confirms = false

  var body: some View {
    Section {
    } footer: {
      HStack {
        Spacer()
        Button("Restore Defaults…", systemImage: "arrow.counterclockwise") { confirms = true }
          .disabled(isDefault)
      }
    }
    .alert("Restore default settings?", isPresented: $confirms) {
      Button("Restore Defaults", action: reset)
      Button("Cancel", role: .cancel) {}
    } message: {
      message
    }
  }
}

extension View {
  /// Marks a row that Settings search can jump to, and lights it up when it does.
  func settingAnchor(_ id: String) -> some View {
    modifier(SettingAnchor(id: id))
  }

  /// The row styles of a Settings pane, and scrolling to the row search jumped to.
  func settingsPage() -> some View {
    modifier(SettingsPage())
  }
}

private struct SettingAnchor: ViewModifier {
  let id: String
  // Missing outside the Settings window, as in the first-launch window, which shares some rows.
  @Environment(SettingsModel.self) private var model: SettingsModel?

  func body(content: Content) -> some View {
    content
      .id(id)
      .background {
        RoundedRectangle(cornerRadius: 8, style: .continuous)
          .fill(Color.accentColor.opacity(model?.highlight == id ? 0.25 : 0))
          .padding(.horizontal, -8)
          .padding(.vertical, -6)
          .animation(.easeInOut(duration: 0.4), value: model?.highlight)
      }
  }
}

private struct SettingsPage: ViewModifier {
  @Environment(SettingsModel.self) private var model

  func body(content: Content) -> some View {
    ScrollViewReader { proxy in
      content
        .labeledContentStyle(CenteredLabeledContentStyle())
        .toggleStyle(CenteredSwitchStyle())
        .labelStyle(.titleAndIcon)
        .onAppear { scroll(proxy) }
        .onChange(of: model.highlight) { scroll(proxy) }
    }
  }

  private func scroll(_ proxy: ScrollViewProxy) {
    guard let id = model.highlight else { return }
    DispatchQueue.main.async {
      withAnimation { proxy.scrollTo(id, anchor: .center) }
    }
  }
}
