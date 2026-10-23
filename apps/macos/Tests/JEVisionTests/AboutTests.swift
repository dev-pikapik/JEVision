import Foundation
import Testing

@testable import JEVision

struct AboutTests {
  /// A bundle on disk with the given Info.plist keys and resource files.
  private func bundle(info: [String: String], resources: [String: String] = [:]) throws -> Bundle {
    let url = FileManager.default.temporaryDirectory.appending(path: "about-\(UUID().uuidString).bundle")
    let folder = url.appending(path: "Contents/Resources")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    var plist = info
    plist["CFBundleIdentifier"] = "io.github.dev-pikapik.jevision.tests.\(UUID().uuidString)"
    try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
      .write(to: url.appending(path: "Contents/Info.plist"))
    for (name, text) in resources {
      try Data(text.utf8).write(to: folder.appending(path: name))
    }
    return try #require(Bundle(url: url))
  }

  @Test func versionAndBuildComeFromTheBundle() throws {
    let version = AppVersion(bundle: try bundle(info: ["CFBundleShortVersionString": "1.2.3", "CFBundleVersion": "456"]))
    #expect(version.version == "1.2.3")
    #expect(version.build == "456")
  }

  @Test func aBundleWithoutAVersionShowsADash() throws {
    let version = AppVersion(bundle: try bundle(info: [:]))
    #expect(version.version == "–")
    #expect(version.build == "–")
  }

  private let list = """
    # Third-Party Licenses

    JEVision for Mac includes the open-source software below.

    ## Apache-2.0 OR MIT

    - serde 1.0.228 — https://github.com/serde-rs/serde
    - tokio 1.47.1 — https://github.com/tokio-rs/tokio

    ```text
    Permission is hereby granted, free of charge.
    ```

    ## Apache-2.0 OR MIT

    - serde 1.0.228 — https://github.com/serde-rs/serde

    ````text
    Licensed under the Apache License.
    A `code` span and a fence: ```
    ````

    ## MIT

    - Sparkle 2.10.0 — https://github.com/sparkle-project/Sparkle
    - zeroize 1.8.1

    ```text
    Copyright (c) Sparkle

    Second paragraph.
    ```
    """

  @Test func everyComponentGetsItsLicenseSourceAndTexts() throws {
    let components = ThirdPartyComponent.parse(list)
    #expect(components.map(\.name) == ["serde 1.0.228", "Sparkle 2.10.0", "tokio 1.47.1", "zeroize 1.8.1"])

    let serde = try #require(components.first)
    #expect(serde.license == "Apache-2.0 OR MIT")
    #expect(serde.source == URL(string: "https://github.com/serde-rs/serde"))
    #expect(
      serde.texts == [
        "Permission is hereby granted, free of charge.",
        "Licensed under the Apache License.\nA `code` span and a fence: ```",
      ])

    let sparkle = components[1]
    #expect(sparkle.license == "MIT")
    #expect(sparkle.texts == ["Copyright (c) Sparkle\n\nSecond paragraph."])
    #expect(components[2].texts == ["Permission is hereby granted, free of charge."])
    #expect(components[3].source == nil)
  }

  @Test func theListComesFromTheBundle() throws {
    let with = try bundle(info: [:], resources: ["ThirdPartyLicenses.md": list])
    #expect(ThirdPartyComponent.bundled(in: with)?.count == 4)
    #expect(ThirdPartyComponent.bundled(in: try bundle(info: [:])) == nil)
  }
}
