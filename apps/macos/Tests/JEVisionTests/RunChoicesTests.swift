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

    try "email;name\nann@example.com;Ann\n;\"Bo\"\"s\"\nc@example.com;\n".write(to: folder.appending(path: "rows.csv"), atomically: true, encoding: .utf8)
    let table = try #require(RunChoices.dataTable(of: flow, folder: folder))
    #expect(RunChoices.rowTitles(of: table) == ["Row 1: Ann", "Row 2: Bo\"s", "Row 3: c@example.com"])
    #expect(RunChoices.dataTitle(row: nil, of: table) == "Data Rows: All 3")
    #expect(RunChoices.dataTitle(row: 1, of: table) == "Data Rows: Row 2")
    // A row the table lost since it was chosen means every row again.
    #expect(RunChoices.dataTitle(row: 5, of: table) == "Data Rows: All 3")
    #expect(RunChoices(dataRow: 1).rows(in: table) == DataRowRange(first: 1, last: 1))
    #expect(RunChoices(dataRow: 5).rows(in: table) == nil)
    #expect(RunChoices().rows(in: table) == nil)
    #expect(RunChoices(dataRow: 0).rows(in: nil) == nil)
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
    #expect(environments.valueNames == ["ENVIRONMENT", "HOST", "USER"], "the name every run has comes with them")

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

  @Test func anEnvironmentIsAddedUnderTheNameGiven() throws {
    let project = try Self.folder()
    defer { try? FileManager.default.removeItem(at: project) }
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let environments = ProjectEnvironments(defaults: defaults)
    environments.open(project)

    #expect(environments.problem(adding: "staging", valueNames: ["HOST"]) == nil)
    try environments.add("staging", values: ["HOST": "https://s.example.com"])
    #expect(environments.current.all == [ProjectEnvironment(name: "staging", values: ["HOST": "https://s.example.com"])])
    #expect(environments.current.selected?.name == "staging")

    #expect(environments.problem(adding: "Staging", valueNames: []) == .Taken(name: "Staging"))
    #expect(environments.problem(adding: "my env", valueNames: []) == .InvalidName(name: "my env"))
    #expect(environments.problem(adding: "production", valueNames: ["1HOST"]) == .InvalidValueName(name: "1HOST"))
    #expect(throws: NewEnvironmentError.self) { try environments.add("staging", values: [:]) }
    #expect(AddEnvironmentSheet.repeated(["HOST", "USER", "HOST"]) == "HOST")
    #expect(AddEnvironmentSheet.repeated(["HOST", "USER"]) == nil)
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

  @Test func theCanvasReadsOutWhatTheTestFindsInTheEnvironment() throws {
    let flow = try Self.flow("${env.HOST} ${env.USER} ${env.ENVIRONMENT}")
    let staging = ProjectEnvironment(name: "staging", values: ["HOST": "s.example.com"])
    let snapshot = ProjectEnvironments.Snapshot(all: [staging], selected: staging)

    let readout = EnvironmentReadout(for: flow, in: snapshot)
    #expect(readout.environment == "staging")
    #expect(readout.values == [.init(name: "HOST", value: "s.example.com"), .init(name: "USER", value: nil)])
    #expect(readout.missing == ["USER"], "the name every run has is never missing")
    #expect(readout.summary() == "HOST s.example.com, USER")
    #expect(readout.summary(limit: 1) == "HOST s.example.com, +1")
    #expect(EnvironmentReadout(for: try Self.flow("plain"), in: snapshot).summary() == nil)

    let none = EnvironmentReadout(for: flow, in: ProjectEnvironments.Snapshot())
    #expect(none.environment == nil)
    #expect(none.missing == ["HOST", "USER"])
  }

  @Test func aBlockSaysWhatItReadsInItsTooltip() {
    let staging = ProjectEnvironment(name: "staging", values: ["HOST": "s.example.com"])
    let snapshot = ProjectEnvironments.Snapshot(all: [staging], selected: staging)
    #expect(snapshot.help(reading: []) == nil)
    #expect(
      snapshot.help(reading: ["ENVIRONMENT", "HOST", "USER"])
        == "ENVIRONMENT = staging\nHOST = s.example.com\nUSER isn’t set in staging")
    #expect(
      ProjectEnvironments.Snapshot().help(reading: ["ENVIRONMENT", "HOST"])
        == "ENVIRONMENT: no environment is chosen\nHOST: no environment is chosen")
    let own = ProjectEnvironment(name: "staging", values: ["ENVIRONMENT": "mine"])
    #expect(
      ProjectEnvironments.Snapshot(all: [own], selected: own).help(reading: ["ENVIRONMENT"]) == "ENVIRONMENT = mine",
      "a value the environment sets itself wins, as in a run")
  }

  @Test func aValueIsChangedInTheEnvironmentsFile() throws {
    let project = try Self.folder()
    defer { try? FileManager.default.removeItem(at: project) }
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    try #"{"format":1,"environments":{"staging":{"HOST":"s"},"production":{"HOST":"p"}}}"#
      .write(to: ProjectEnvironments.file(in: project), atomically: true, encoding: .utf8)
    let environments = ProjectEnvironments(defaults: defaults)
    environments.open(project)
    environments.select("staging")

    try environments.setValue("s2.example.com", named: "HOST", in: "staging")
    try environments.setValue("ann", named: "USER", in: "staging")
    #expect(environments.current.selected?.values == ["HOST": "s2.example.com", "USER": "ann"])
    #expect(environments.current.all.first { $0.name == "production" }?.values == ["HOST": "p"])

    do {
      try environments.setValue("x", named: "HOST", in: "nope")
      Issue.record("an unknown environment has no values to set")
    } catch {
      #expect(ProjectEnvironments.describe(error) == "There’s no environment nope.")
    }
    #expect(throws: EnvironmentValueError.self) { try environments.setValue("x", named: "1HOST", in: "staging") }
  }
}
