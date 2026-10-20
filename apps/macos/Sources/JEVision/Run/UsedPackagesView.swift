import JEVisionCore
import SwiftUI

/// The community blocks a run used, with the version each one had that day.
struct UsedPackagesView: View {
  let packages: [UsedPackage]

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text("Community blocks")
        .font(.headline)
        .accessibilityAddTraits(.isHeader)
      ForEach(packages, id: \.file) { package in
        HStack(spacing: 8) {
          Image(systemName: "person.2")
            .foregroundStyle(.secondary)
            .accessibilityHidden(true)
          Text("\(package.name) \(package.version)")
            .fontWeight(.medium)
          Text("by \(package.author)")
            .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
      }
    }
  }
}
