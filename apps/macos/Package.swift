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
      dependencies: [.product(name: "JEVisionCore", package: "JEVisionCore")],
      path: "Sources/JEVision"),
    .testTarget(name: "JEVisionTests", dependencies: ["JEVision"], path: "Tests/JEVisionTests"),
  ]
)
