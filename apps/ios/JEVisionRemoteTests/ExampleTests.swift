import Foundation
import Testing

@testable import JEVisionRemote

struct ExampleSessionTests {
  private func open(step: TimeInterval) async throws -> (any MacSession, String) {
    let session = try await ExampleClient(deviceName: "Test iPhone", step: step).connect(ExampleClient.mac)
    return (session, try await session.listProjects()[0].name)
  }

  /// Follows the run to its end and returns its events.
  private func follow(_ session: any MacSession, _ project: String, _ run: String) async throws -> [RunEvent] {
    var events: [RunEvent] = []
    for try await update in session.runEvents(project: project, run: run) {
      if case .event(let event) = update { events.append(event) }
    }
    return events
  }

  @Test func aRunPlaysEveryRowToItsReport() async throws {
    let (session, project) = try await open(step: 0.01)
    let run = try await session.runTest(project: project, path: ExampleProject.people, row: nil)

    let events = try await follow(session, project, run)

    guard case .finished(.passed, _) = events.last else { Issue.record("ended with \(String(describing: events.last))"); return }
    let report = try await session.report(project: project, run: run)
    #expect(report.verdict == .passed)
    #expect(report.requestedBy == "Test iPhone")
    #expect(report.attempts.map(\.row?.label) == ["Anna", "Boris", "Chen"])
    #expect(report.attempts.allSatisfy { $0.steps.count == 4 })
    let tests = try await session.listTests(project: project)
    #expect(tests.first { $0.path == ExampleProject.people }?.lastRun?.run == run)
  }

  @Test func signInFailsAtItsCheckWithBothPictures() async throws {
    let (session, project) = try await open(step: 0.01)
    let run = try await session.runTest(project: project, path: ExampleProject.signIn, row: nil)
    _ = try await follow(session, project, run)

    let report = try await session.report(project: project, run: run)
    #expect(report.verdict == .failed)
    let step = try #require(report.attempts.last?.steps.last)
    #expect(step.block == "messages" && step.failure?.code == "run.check.mismatch")
    for name in [step.after, step.expected] {
      let picture = try await session.picture(project: project, run: run, name: try #require(name))
      #expect(picture.starts(with: [0x89, 0x50, 0x4E, 0x47]))
    }
  }

  @Test func aStoppedRunIsCancelledAndAnotherCanStart() async throws {
    let (session, project) = try await open(step: 60)
    let run = try await session.runTest(project: project, path: ExampleProject.people, row: 1)
    await #expect(throws: RemoteError.runInProgress) {
      try await session.runTest(project: project, path: ExampleProject.signIn, row: nil)
    }

    try await session.stopRun(project: project, run: run)

    let status = try await session.runStatus(project: project, run: run)
    #expect(status.state == .finished && status.verdict == .cancelled)
    #expect(try await session.report(project: project, run: run).verdict == .cancelled)
    _ = try await session.runTest(project: project, path: ExampleProject.signIn, row: nil)
  }

  @Test func aMissingRowIsRefused() async throws {
    let (session, project) = try await open(step: 0.01)
    await #expect(throws: RemoteError.dataRowNotFound) {
      try await session.runTest(project: project, path: ExampleProject.people, row: 3)
    }
  }
}

@MainActor
struct ExampleModelTests {
  @Test func theExampleOpensUnlockedAndKeepsNothing() async throws {
    let store = makeStore()
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    await model.openExample(deviceName: "Test iPhone")

    let example = try #require(model.example)
    #expect(example.isExample && !model.isExample)
    #expect(example.lock.isUnlocked && example.hosts == [ExampleClient.mac])
    #expect(try await example.session(for: ExampleClient.mac).permission == .run)
    #expect(model.hosts.isEmpty && store.load().isEmpty)

    await example.forget(ExampleClient.mac)
    #expect(example.hosts.isEmpty && !example.storageFailed)
  }

  @Test func pairingARealMacClosesTheExample() async throws {
    let model = AppModel(client: FakeClient(), store: makeStore(), lock: makeLock())
    await model.openExample(deviceName: "Test iPhone")

    _ = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")

    #expect(model.example == nil && model.hosts == [macA])
  }
}
