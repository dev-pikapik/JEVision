import SwiftUI

/// Name and app of a new test. The test starts with one block that opens the app.
struct NewTestSheet: View {
  /// The sheet leads to a recording: it says so, and `created` starts it once the sheet is gone.
  var records = false
  var created: () -> Void = {}
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @State private var name = ""
  @State private var app: String?
  @State private var error: String?

  private var canCreate: Bool { !name.trimmingCharacters(in: .whitespaces).isEmpty && app != nil }

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      (records ? Text("Record a Test") : Text("New Test"))
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      Form {
        TextField(text: $name, prompt: Text("Sign in with a valid password")) { Text("Name") }
        LabeledContent("App") {
          AppPicker(bundleID: $app, noneTitle: Text("Choose App"))
        }
      }
      .formStyle(.columns)
      if let error {
        Text(error).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button(records ? "Record" : "Create") {
          guard let app else { return }
          do {
            try project.createTest(name: name, app: app)
            created()
            dismiss()
          } catch {
            self.error = error.localizedDescription
          }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(!canCreate)
      }
    }
    .padding(20)
    .frame(width: 420)
  }
}
