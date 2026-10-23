import Foundation
import Testing

@testable import JEVisionRemote

struct AboutTests {
  @Test func versionAndBuildComeFromTheBundle() throws {
    let url = FileManager.default.temporaryDirectory.appending(path: "about-\(UUID().uuidString).bundle")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    let info = [
      "CFBundleIdentifier": "io.github.dev-pikapik.jevision.tests.\(UUID().uuidString)",
      "CFBundleShortVersionString": "1.2.3",
      "CFBundleVersion": "456",
    ]
    try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
      .write(to: url.appending(path: "Info.plist"))
    let version = AppVersion(bundle: try #require(Bundle(url: url)))
    #expect(version.version == "1.2.3")
    #expect(version.build == "456")
  }

  @Test func theAppShowsTheVersionItWasBuiltWith() {
    let version = AppVersion()
    #expect(version.version == Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String)
    #expect(version.build == Bundle.main.infoDictionary?["CFBundleVersion"] as? String)
    #expect(version.version != "–")
  }

  @Test func everyComponentGetsItsLicenseSourceAndTexts() throws {
    let components = ThirdPartyComponent.parse(
      """
      # Third-Party Licenses

      ## Apache-2.0 OR MIT

      - serde 1.0.228 — https://github.com/serde-rs/serde
      - tokio 1.47.1 — https://github.com/tokio-rs/tokio

      ```text
      Permission is hereby granted, free of charge.
      ```

      ## MIT

      - serde 1.0.228 — https://github.com/serde-rs/serde
      - zeroize 1.8.1

      ````text
      A fence inside: ```

      Second paragraph.
      ````
      """)
    #expect(components.map(\.name) == ["serde 1.0.228", "tokio 1.47.1", "zeroize 1.8.1"])
    let serde = try #require(components.first)
    #expect(serde.license == "Apache-2.0 OR MIT")
    #expect(serde.source == URL(string: "https://github.com/serde-rs/serde"))
    #expect(serde.texts == ["Permission is hereby granted, free of charge.", "A fence inside: ```\n\nSecond paragraph."])
    #expect(components[2].license == "MIT")
    #expect(components[2].source == nil)
  }

  @Test func theAppShipsItsLicensesAndPrivacyManifest() throws {
    let license = try #require(Bundle.main.url(forResource: "LICENSE", withExtension: nil))
    #expect(try String(contentsOf: license, encoding: .utf8).hasPrefix("MIT License"))
    let components = try #require(ThirdPartyComponent.bundled(in: .main))
    #expect(components.contains { $0.name.hasPrefix("tokio ") })
    #expect(components.allSatisfy { !$0.license.isEmpty && !$0.texts.isEmpty })

    let manifest = try #require(Bundle.main.url(forResource: "PrivacyInfo", withExtension: "xcprivacy"))
    let privacy = try #require(
      try PropertyListSerialization.propertyList(from: Data(contentsOf: manifest), format: nil) as? [String: Any])
    #expect(privacy["NSPrivacyTracking"] as? Bool == false)
  }
}
