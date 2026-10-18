import Foundation
import Testing

@testable import JEVision

@MainActor
struct UpdatesTests {
  @Test func staysOffWithoutAPublicKey() {
    let updates = Updates(bundle: .main)
    #expect(!Updates.hasPublicKey(.main))
    #expect(!updates.isAvailable)
    #expect(!updates.canCheck)
    updates.checksAutomatically = true
    #expect(!updates.checksAutomatically)
  }
}
