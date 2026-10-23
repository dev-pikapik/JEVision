/// Accessibility identifiers of JEVision's own interface: the names JEVision's tests of itself
/// (the selftest folder) find elements by, with an `element` target or check.
///
/// The rules, for every name added here:
/// - `area.element` in lowerCamelCase, such as `onboarding.next` or `settings.language`: the area is
///   the window, screen or part of it, the element says what it is for, never how it looks or what
///   it says.
/// - Never shown and never translated, so a test finds the same element in every language.
/// - Only one element on screen has a name at a time. Rows of a list, such as the tests in the
///   sidebar, share one name and differ in their title: `{ "id": "project.test", "title": "Sign in" }`.
/// - Views use these constants, never the strings. A new area is a new enum here.
/// - A test depends on a name: rename or remove one only together with the tests in selftest.
/// - A container with a name, such as a step of the onboarding, gets
///   `.accessibilityElement(children: .contain)`, so the name stays on it rather than on its contents.
enum AccessibilityID {
  /// The welcome window, shown at the first launch.
  enum Onboarding {
    /// The steps, each a container.
    static let welcome = "onboarding.welcome"
    static let permissions = "onboarding.permissions"
    static let ai = "onboarding.ai"
    static let firstTest = "onboarding.firstTest"
    /// The button at the right that goes to the next step: Set Up Step by Step, Continue, Later
    /// or Skip, depending on the step.
    static let next = "onboarding.next"
    static let back = "onboarding.back"
    /// Closes the window: Skip on the first and on the last step.
    static let skip = "onboarding.skip"
    static let runDemo = "onboarding.runDemo"
  }

  /// The main window.
  enum Main {
    /// Open Project… while no project is open, or the one open last time can't be found.
    static let openProject = "main.openProject"
  }

  /// The open project: its sidebar and what its main window shows.
  enum Project {
    /// A test in the sidebar; its title is the test's name.
    static let test = "project.test"
    /// The buttons of a project without tests.
    static let newTest = "project.newTest"
    static let recordFirstTest = "project.recordFirstTest"
  }

  /// The sheet that makes a new test.
  enum NewTest {
    static let name = "newTest.name"
    static let app = "newTest.app"
    static let create = "newTest.create"
    static let cancel = "newTest.cancel"
  }

  /// The list of apps a test or block can open.
  enum AppPicker {
    static let search = "appPicker.search"
    /// An app of the list; its title is the app's name.
    static let app = "appPicker.app"
  }

  /// The flowchart of the open test.
  enum Canvas {
    /// A block; its title is its kind and place in the test, in the app's language.
    static let block = "canvas.block"
  }

  /// The Settings window.
  enum Settings {
    static let language = "settings.language"
  }

  /// The rows of the permissions, in the onboarding and in Settings.
  enum Permissions {
    /// The mark that says whether the permission is on: its title is Allowed or Not allowed.
    static func status(_ permission: Permission) -> String {
      switch permission {
      case .screenRecording: "permissions.screenRecording"
      case .accessibility: "permissions.accessibility"
      }
    }
  }
}
