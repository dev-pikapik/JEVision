import JEVisionCore
import SwiftUI

/// Where a shared block comes from, first, and under it the author its package names, marked when
/// that isn't the GitHub account or site the block comes from: anyone can write any name in a block.
struct BlockOriginView: View {
  let origin: BlockOrigin
  let author: String
  /// Whether to say what the origin means for trusting the block.
  var explains = true

  var body: some View {
    let text = describeBlockOrigin(origin: origin, author: author)
    VStack(alignment: .leading, spacing: 2) {
      Label(text.source, systemImage: Self.symbol(origin))
        .fontWeight(.medium)
      if explains {
        Text(text.about)
          .font(.caption)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      Text(text.author)
        .font(.callout)
        .foregroundStyle(.secondary)
      if let mismatch = text.mismatch {
        Label {
          Text(mismatch).fixedSize(horizontal: false, vertical: true)
        } icon: {
          Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
        }
        .font(.caption)
      }
    }
    .accessibilityElement(children: .combine)
  }

  static func symbol(_ origin: BlockOrigin) -> String {
    switch origin {
    case .gitHub: "person.crop.circle"
    case .web: "globe"
    case .thisMac: "desktopcomputer"
    case .unknown: "questionmark.circle"
    }
  }
}
