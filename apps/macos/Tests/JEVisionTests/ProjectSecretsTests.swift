import Foundation
import JEVisionHands
import Testing

struct ProjectSecretsTests {
  private final class Fixture {
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-secret-scope-\(UUID().uuidString)")
    let name = "JEVISION_TEST_SECRET_" + UUID().uuidString.replacingOccurrences(of: "-", with: "_")
    let environment = "scope-" + UUID().uuidString
    let first: ProjectSecrets
    let second: ProjectSecrets

    init() throws {
      try FileManager.default.createDirectory(at: root.appending(path: "a"), withIntermediateDirectories: true)
      try FileManager.default.createDirectory(at: root.appending(path: "b"), withIntermediateDirectories: true)
      first = ProjectSecrets(project: root.appending(path: "a"))
      second = ProjectSecrets(project: root.appending(path: "b"))
    }

    deinit {
      for service in [first.service, first.service(environment: environment), second.service, second.service(environment: environment),
        SecretStore.tests, SecretStore.service(for: environment)]
      {
        try? SecretStore.remove(name, in: service)
        try? SecretStore.remove("sharing/1/" + name, in: service)
      }
      try? FileManager.default.removeItem(at: root)
    }
  }

  @Test func sharedLegacyValuesRequireConsentForEachProject() throws {
    let f = try Fixture()
    try SecretStore.save("legacy", for: f.name)
    #expect(!f.first.isSet(f.name, environment: nil))
    #expect(f.first.value(f.name, environment: nil) == nil)
    #expect(f.first.hasShared(f.name, environment: nil))
    try f.first.setUsesShared(true, for: f.name, environment: nil)
    #expect(f.first.value(f.name, environment: nil) == "legacy")
    #expect(f.second.value(f.name, environment: nil) == nil)
    try f.first.setUsesShared(false, for: f.name, environment: nil)
    #expect(!f.first.isSet(f.name, environment: nil))
    #expect(SecretStore.keychainValue(f.name) == "legacy")
  }

  @Test func copyingSharedValuesMakesAnIndependentProjectValue() throws {
    let f = try Fixture()
    try SecretStore.save("original", for: f.name)
    #expect(try f.first.copyShared(f.name, environment: nil))
    try SecretStore.save("changed", for: f.name)
    #expect(f.first.value(f.name, environment: nil) == "original")
    #expect(!f.first.usesShared(f.name, environment: nil))
    #expect(f.second.value(f.name, environment: nil) == nil)
  }

  @Test func environmentsRespectTheirOwnSharingChoiceAndProjectFallback() throws {
    let f = try Fixture()
    try f.first.save("project", for: f.name, environment: nil)
    try SecretStore.save("shared", for: f.name)
    try SecretStore.save("shared-env", for: f.name, in: SecretStore.service(for: f.environment))
    #expect(f.first.value(f.name, environment: f.environment) == "project")
    try f.first.setUsesShared(true, for: f.name, environment: f.environment)
    #expect(f.first.value(f.name, environment: f.environment) == "shared-env")
    #expect(f.first.value(f.name, environment: nil) == "project")
    try f.first.save("private-env", for: f.name, environment: f.environment)
    #expect(f.first.value(f.name, environment: f.environment) == "private-env")
    #expect(SecretStore.keychainValue(f.name, in: SecretStore.service(for: f.environment)) == "shared-env")
    #expect(f.second.value(f.name, environment: f.environment) == nil)
  }

  @Test func renamingKeepsIdentityButCopiesDoNotShareIt() throws {
    let f = try Fixture()
    let moved = f.root.appending(path: "renamed")
    try f.first.save("private", for: f.name, environment: nil)
    try FileManager.default.moveItem(at: f.root.appending(path: "a"), to: moved)
    #expect(ProjectSecrets(project: moved).service == f.first.service)
    let link = f.root.appending(path: "link")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: moved)
    #expect(ProjectSecrets(project: link).service == f.first.service)
    let copy = f.root.appending(path: "copy")
    try FileManager.default.copyItem(at: moved, to: copy)
    #expect(ProjectSecrets(project: copy).service != f.first.service)
    #expect(ProjectSecrets(project: copy).value(f.name, environment: nil) == nil)
    #expect(StoredSecrets(project: moved.path).secret(name: f.name, environment: nil) == "private")
    #expect(StoredSecrets(project: copy.path).secret(name: f.name, environment: nil) == nil)
  }
}
