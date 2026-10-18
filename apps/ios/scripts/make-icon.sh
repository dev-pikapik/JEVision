#!/bin/bash
# Renders AppIcon.svg into the app icon and the in-app logo.
# Run after editing the SVG; the build uses the committed PNGs as is.
set -euo pipefail
cd "$(dirname "$0")/.."

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

cat > "$WORK/render.swift" <<'SWIFT'
import AppKit

let args = CommandLine.arguments
guard let image = NSImage(contentsOf: URL(fileURLWithPath: args[1])) else { fatalError("cannot read \(args[1])") }
let px = Int(args[2])!
// No alpha channel: the App Store refuses icons that have one.
let context = CGContext(
  data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0,
  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
NSGraphicsContext.current?.imageInterpolation = .high
image.draw(in: NSRect(x: 0, y: 0, width: px, height: px))
NSGraphicsContext.restoreGraphicsState()
let rep = NSBitmapImageRep(cgImage: context.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[3]))
SWIFT
swiftc -O "$WORK/render.swift" -o "$WORK/render"

ASSETS=JEVisionRemote/Assets.xcassets
"$WORK/render" AppIcon.svg 1024 "$ASSETS/AppIcon.appiconset/AppIcon.png"
"$WORK/render" AppIcon.svg 512 "$ASSETS/Logo.imageset/Logo.png"
echo "Wrote the app icon and logo in $PWD/$ASSETS"
