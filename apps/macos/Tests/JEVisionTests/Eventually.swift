import Foundation
import Testing

/// How long a test lets the app or another thread take to reach what it waits for. A test that is
/// told goes on at once, however slow the machine is; only a test that never is told waits it out.
let hangGuard: TimeInterval = 60

/// Returns as soon as `condition` holds and fails the test if it never does. The run loop of the
/// calling thread keeps turning meanwhile, so what the app does on the main thread goes on.
func eventually(
  _ what: String, timeout: TimeInterval = hangGuard, sourceLocation: SourceLocation = #_sourceLocation,
  _ condition: () -> Bool
) throws {
  let deadline = Date().addingTimeInterval(timeout)
  while !condition() {
    try #require(Date() < deadline, "Never happened: \(what)", sourceLocation: sourceLocation)
    RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.005))
    usleep(2_000)
  }
}

/// [`eventually`] for a test that awaits: the condition is read on the actor the test runs on.
func eventuallyAsync(
  _ what: String, timeout: TimeInterval = hangGuard, isolation: isolated (any Actor)? = #isolation,
  sourceLocation: SourceLocation = #_sourceLocation, _ condition: () -> Bool
) async throws {
  let deadline = Date().addingTimeInterval(timeout)
  while !condition() {
    try #require(Date() < deadline, "Never happened: \(what)", sourceLocation: sourceLocation)
    try await Task.sleep(for: .milliseconds(5))
  }
}
