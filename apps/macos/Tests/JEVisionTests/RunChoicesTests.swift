import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct RunChoicesTests {
  private static func flow(_ text: String) throws -> Flow {
    try loadFlow(
      json: #"""
        {"format":1,"name":"T","app":"com.example.App","start":"data","retries":2,"blocks":{
          "data":{"type":"useData","file":"rows.csv","body":"type"},
          "type":{"type":"type","text":"\#(text)"}}}
        """#)
  }

  private static func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appending(path: "jevision-choices-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
  }

  @Test func dataRowsAreTitledAndChosen() throws {
    let folder = try Self.folder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let flow = try Self.flow("${data.email}")
    #expect(RunChoices.dataTable(of: flow, folder: folder) == nil)

    try "email,name\nann@example.com,Ann\n,Bo\n".write(to: folder.appending(path: "rows.csv"), atomically: true, encoding: .utf8)
    let table = try #require(RunChoices.dataTable(of: flow, folder: folder))
    #expect(RunChoices.rowTitles(of: table) == ["Row 1: ann@example.com", "Row 2: Bo"])
    #expect(RunChoices.dataTitle(row: nil, of: table) == "Data Rows: All 2")
    #expect(RunChoices.dataTitle(row: 1, of: table) == "Data Rows: Row 2")
    // A row the table lost since it was chosen means every row again.
    #expect(RunChoices.dataTitle(row: 5, of: table) == "Data Rows: All 2")
    #expect(RunChoices(dataRow: 1).row(in: table) == 1)
    #expect(RunChoices(dataRow: 5).row(in: table) == nil)
    #expect(RunChoices().row(in: table) == nil)
    #expect(RunChoices(dataRow: 0).row(in: nil) == nil)
  }

  @Test func retriesKeepTheTestsNumberUnlessChosen() {
    #expect(RunChoices.retryChoices == [0, 1, 2, 3])
    #expect(RunChoices.retriesTitle(nil, test: 2) == "Retries: As in Test (2)")
    #expect(RunChoices.retriesTitle(0, test: 2) == "Retries: 0")
  }

  @Test func environmentsAreReadAndTheChoiceIsKeptPerProject() throws {
    let project = try Self.folder()
    defer { try? FileManager.default.removeItem(at: project) }
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }

    #expect(ProjectEnvironments.read(project, defaults: defaults).all.isEmpty)
    try #"{"format":1,"environments":{"staging":{"HOST":"s"},"production":{"HOST":"p","USER":"u"}}}"#
      .write(to: ProjectEnvironments.file(in: project), atomically: true, encoding: .utf8)
    let environments = ProjectEnvironments(defaults: defaults)
    environments.open(project)
    #expect(environments.current.all.map(\.name) == ["production", "staging"])
    #expect(environments.current.selected?.name == "production")
    #expect(environments.valueNames == ["HOST", "USER"])

    try #"{"format":1,"default":"staging","environments":{"staging":{"HOST":"s"},"production":{"HOST":"p","USER":"u"}}}"#
      .write(to: ProjectEnvironments.file(in: project), atomically: true, encoding: .utf8)
    environments.reload()
    #expect(environments.current.selected?.name == "staging", "the file's default until one is chosen")
    environments.select("production")
    #expect(environments.current.selected?.name == "production")
    #expect(ProjectEnvironments.read(project, defaults: defaults).selected?.name == "production")

    try "{".write(to: ProjectEnvironments.file(in: project), atomically: true, encoding: .utf8)
    environments.reload()
    #expect(environments.current.all.isEmpty)
    #expect(environments.current.problem?.contains("jevision.environments.json") == true)
  }

  @Test func missingValuesNameWhatTheChosenEnvironmentLacks() throws {
    let flow = try Self.flow("${env.HOST}${env.USER}${secret.PASS}${secret.TOKEN}")
    let staging = ProjectEnvironment(name: "staging", values: ["HOST": "s"])
    let snapshot = ProjectEnvironments.Snapshot(all: [staging], selected: staging)
    var asked: [String?] = []
    let missing = MissingValues(for: flow, in: snapshot) { name, environment in
      asked.append(environment)
      return name == "TOKEN"
    }
    #expect(missing == MissingValues(environment: "staging", values: ["USER"], secrets: ["PASS"]))
    #expect(asked == ["staging", "staging"])

    let none = MissingValues(for: flow, in: ProjectEnvironments.Snapshot()) { _, _ in true }
    #expect(none == MissingValues(values: ["HOST", "USER"]))
    #expect(!none.isEmpty)

    // A broken environments file matters only to tests that read environment values.
    let broken = ProjectEnvironments.Snapshot(problem: "broken")
    #expect(MissingValues(for: flow, in: broken) { _, _ in true }.problem == "broken")
    #expect(MissingValues(for: try Self.flow("${secret.PASS}"), in: broken) { _, _ in true }.isEmpty)
  }
}
