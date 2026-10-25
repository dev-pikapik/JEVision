import AppKit
import JEVisionHands
import SwiftUI

struct GeneralSettingsView: View {
  @AppPreference(Appearance.key) private var appearance = Appearance.system
  @AppPreference(RunInputSettings.collapsesKey) private var collapses = true
  @AppPreference(RunInputSettings.pausesOnMouseKey) private var pausesOnMouse = true
  @AppPreference(RunInputSettings.escapeStopsKey) private var escapeStops = true
  @AppPreference(GridSnap.key) private var snapsToGrid = true
  @AppPreference(NoteFiles.largeKey) private var largeNoteFile = NoteFiles.largeDefault

  private var language: Binding<String> {
    Binding(get: { AppLanguage.shared.saved }, set: { AppLanguage.shared.choose($0) })
  }

  var body: some View {
    Form {
      SettingsHeader(tab: .general, text: Text("How JEVision looks, which language it speaks and what it does while a test runs."))
      if let folder = Profile.current.folder {
        Section {
          LabeledContent {
            Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([folder]) }
          } label: {
            RowLabel(Text("Separate profile"), Text(verbatim: folder.path(percentEncoded: false)))
          }
        } footer: {
          Text("This copy of JEVision keeps its settings, logs and keys in this folder, apart from yours. Schedules, installing the command line tool, updates and notifications are off in it.")
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      Section {
        ActionRow {
          Picker("Appearance", selection: $appearance) {
            ForEach(Appearance.allCases) { Text($0.title).tag($0) }
          }
          .pickerStyle(.segmented)
          .labelsHidden()
        } label: {
          Text("Appearance")
        }
        .settingAnchor("appearance")
        Picker("Language", selection: language) {
          Text("System").tag("")
          Divider()
          ForEach(AppLanguage.choices.dropFirst(), id: \.self) { Text(verbatim: AppLanguage.name($0)).tag($0) }
        }
        .accessibilityIdentifier(AccessibilityID.Settings.language)
        .settingAnchor("language")
      } footer: {
        VStack(alignment: .leading, spacing: 6) {
          Text("Names that macOS draws itself, like File and Edit in the menu bar, change the next time JEVision opens.")
          Text("JEVision collects no usage data. Nothing leaves this Mac unless a feature you turned on needs it.")
        }
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      }
      Section {
        Toggle(isOn: $collapses) {
          RowLabel(
            Text("Step aside while a test runs"),
            Text("JEVision hides and leaves a small panel, so the app you test is in view."))
        }
        .settingAnchor("runCollapses")
        Toggle(isOn: $pausesOnMouse) {
          RowLabel(
            Text("Pause when you move the mouse"),
            Text("A test uses the mouse and keyboard, so your own hands would spoil it. It waits until you press Resume."))
        }
        .settingAnchor("runPausesOnMouse")
        Toggle(isOn: $escapeStops) {
          RowLabel(Text("Stop with Esc Esc"), Text("Pressing Escape twice, in any app, stops the test."))
        }
        .settingAnchor("runEscapeStops")
      } header: {
        Text("While a test runs")
      } footer: {
        Text("A change counts at once, even for a test that is running.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      Section {
        Toggle(isOn: $snapsToGrid) {
          RowLabel(
            Text("Snap to grid"),
            Text("What you move on a canvas lands on the grid. Hold ⌘ while moving to do the opposite."))
        }
        .settingAnchor("snapToGrid")
      } header: {
        Text("Canvas")
      }
      Section {
        LabeledContent {
          Stepper(value: $largeNoteFile, in: 1...1000) {
            Text(Int64(largeNoteFile) * 1_000_000, format: .byteCount(style: .file))
          }
        } label: {
          RowLabel(
            Text("Large file in a note"),
            Text("From this size, JEVision asks before it copies a file into a note: track it with Git LFS, attach it anyway or leave it."))
        }
        .settingAnchor("noteLargeFile")
      } header: {
        Text("Notes")
      }
      RestoreDefaultsSection(
        message: Text("Appearance and language follow the system again. The options for running tests and for the canvas go back to how they came."),
        isDefault: appearance == .system && language.wrappedValue.isEmpty && collapses && pausesOnMouse && escapeStops
          && snapsToGrid && largeNoteFile == NoteFiles.largeDefault
      ) {
        appearance = .system
        language.wrappedValue = ""
        collapses = true
        pausesOnMouse = true
        escapeStops = true
        snapsToGrid = true
        largeNoteFile = NoteFiles.largeDefault
      }
    }
    .formStyle(.grouped)
    .onChange(of: appearance) { appearance.apply() }
  }
}
