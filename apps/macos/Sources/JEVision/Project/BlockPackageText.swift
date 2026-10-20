import Foundation
import JEVisionCore

/// Problems with a shared block, in words: the core words each problem.
enum BlockPackageText {
  /// All the problems of `issues` as one text, one sentence after the other.
  static func message(_ error: BlockPackageError) -> String {
    switch error {
    case .Failed(let issues): issues.map { IssueText.message($0) }.joined(separator: " ")
    }
  }

  /// A size in bytes, written the way the Mac writes file sizes.
  static func size(_ bytes: Int64) -> String {
    ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
  }
}
