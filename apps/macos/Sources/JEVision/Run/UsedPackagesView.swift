import JEVisionCore
import SwiftUI

/// The community blocks a run used, with the version each one had that day and where it came from.
struct UsedPackagesView: View {
  let packages: [UsedPackage]

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text("Community blocks")
        .font(.headline)
        .accessibilityAddTraits(.isHeader)
      ForEach(packages, id: \.file) { package in
        HStack(alignment: .firstTextBaseline, spacing: 8) {
          Image(systemName: "person.2")
            .foregroundStyle(.secondary)
            .accessibilityHidden(true)
          VStack(alignment: .leading, spacing: 2) {
            Text("\(package.name) \(package.version)")
              .fontWeight(.medium)
            BlockOriginView(
              origin: installedBlockOrigin(source: package.source), author: package.author,
              explains: false)
          }
        }
        .accessibilityElement(children: .combine)
      }
    }
  }
}
