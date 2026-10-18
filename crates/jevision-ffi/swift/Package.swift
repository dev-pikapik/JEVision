// swift-tools-version: 6.2
import PackageDescription

// Swift package around the Rust core. scripts/build-core-apple.sh copies this folder to
// build/apple/JEVisionCore and adds the generated bindings and JEVisionCoreFFI.xcframework.
let package = Package(
  name: "JEVisionCore",
  platforms: [.macOS(.v26)],
  products: [
    .library(name: "JEVisionCore", targets: ["JEVisionCore"])
  ],
  targets: [
    .binaryTarget(name: "JEVisionCoreFFI", path: "JEVisionCoreFFI.xcframework"),
    .target(
      name: "JEVisionCore", dependencies: ["JEVisionCoreFFI"],
      // Remote access watches the network through it.
      linkerSettings: [.linkedFramework("SystemConfiguration")]),
    .testTarget(name: "JEVisionCoreTests", dependencies: ["JEVisionCore"]),
  ]
)
