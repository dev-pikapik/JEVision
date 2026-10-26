import Foundation
import Testing

@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct BoardMinimapControlTests {
  @Test func hidingAndShowingTheMapUpdatesWithoutAnotherBoardAction() async throws {
    try await checkMinimap(board: true)
  }
}

