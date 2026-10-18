import SwiftUI

struct WelcomeStep: View {
  let runDemo: () -> Void

  var body: some View {
    VStack(spacing: 0) {
      IconTile(symbol: "flowchart.fill", color: .accentColor, size: 72)
        .padding(.bottom, 18)
      Text("Welcome to JEVision")
        .font(.largeTitle.bold())
        .accessibilityAddTraits(.isHeader)
        .padding(.bottom, 8)
      Text("Test any Mac app the way a person would: click, type, look. A test is a flowchart you can read — no code.")
        .font(.title3)
        .foregroundStyle(.secondary)
        .multilineTextAlignment(.center)
        .frame(maxWidth: 500)
        .fixedSize(horizontal: false, vertical: true)
        .padding(.bottom, 22)
      VStack(alignment: .leading, spacing: 12) {
        point("record.circle", .red, Text("**Record once.** Use the app as usual; JEVision writes down the steps."))
        point("eye", .blue, Text("**Check by eye.** A test compares the screen with a picture, so it notices what looks wrong."))
        point("play.circle", .green, Text("**Run from anywhere.** From the app, the Terminal or CI, with a report you can open."))
      }
      .frame(maxWidth: 460)
      .padding(.bottom, 26)
      Button(action: runDemo) {
        Label("Run the Demo Test", systemImage: "play.fill")
          .padding(.horizontal, 8)
      }
      .buttonStyle(.glassProminent)
      .controlSize(.extraLarge)
      .keyboardShortcut(.defaultAction)
      Text("Opens TextEdit, adds a line to a note and checks the window. Takes about 10 seconds.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .multilineTextAlignment(.center)
        .padding(.top, 10)
    }
    .padding(.horizontal, 40)
  }

  private func point(_ symbol: String, _ color: Color, _ text: Text) -> some View {
    Label {
      text.fixedSize(horizontal: false, vertical: true)
    } icon: {
      Image(systemName: symbol).foregroundStyle(color)
    }
    .font(.body)
  }
}
