// swift-tools-version: 6.2
import PackageDescription

// Used for `swift test` and for opening the sources in Xcode.
// The app bundle itself is made by scripts/build.sh. Both need the Rust core, built by
// scripts/build-core-apple.sh at the repository root (build.sh runs it when needed).
let package = Package(
  name: "JEVision",
  platforms: [.macOS(.v26)],
  dependencies: [
    .package(path: "../../build/apple/JEVisionCore")
  ],
  targets: [
    .executableTarget(
      name: "JEVision",
      dependencies: ["JEVisionHands", .product(name: "JEVisionCore", package: "JEVisionCore")],
      path: "Sources/JEVision"),
    // The macOS side of a run, without UI, so a command-line tool can use it too.
    .target(
      name: "JEVisionHands",
      dependencies: [.product(name: "JEVisionCore", package: "JEVisionCore")],
      path: "Sources/JEVisionHands"),
    .testTarget(name: "JEVisionTests", dependencies: ["JEVision", "JEVisionHands"], path: "Tests/JEVisionTests"),
  ]
)
