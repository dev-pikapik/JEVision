import JEVisionCore
import SwiftUI

/// How the tests of a folder run when the folder's Run or Run All starts them: one after another
/// or side by side. Kept in the project settings file, so everyone who runs the project gets the
/// same. Shown in menus.
struct RunModePicker: View {
  @Environment(ProjectStore.self) private var project
  /// Relative to the project; `.` for Run All.
  let folder: String
  /// Says why the choice couldn't be saved.
  let failed: (String) -> Void

  var body: some View {
    let current = project.runMode(of: folder)
    let choices = Self.choices.contains(current) ? Self.choices : Self.choices + [current]
    Picker(
      "Run Tests",
      selection: Binding(
        get: { current },
        set: { mode in
          do { try project.setRunMode(mode, of: folder) } catch { failed(ProjectStore.message(error)) }
        })
    ) {
      ForEach(choices, id: \.self) { mode in
        Text(RunHUD.mode(mode)).tag(mode)
      }
    }
    .pickerStyle(.inline)
    // A test that clicks or looks at the screen would spoil another one doing the same.
    Text("Tests that use the screen still take turns")
  }

  private static let choices: [RunMode] = [.sequential, .parallel(limit: 2), .parallel(limit: 4), .parallel(limit: 8)]
}
