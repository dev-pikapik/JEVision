// Checks an update feed and its entry for an update archive the way Sparkle does on a user's Mac:
// the feed's own EdDSA signature (the app requires a signed feed) and the entry's signature of the
// archive must match the public key built into the app. The entry must also carry the data format
// of the version and release notes in Russian next to the English ones (make-appcast.sh).
//   swift verify-appcast.swift appcast.xml JEVision-1.0.0.dmg PUBLIC_ED_KEY
import CryptoKit
import Foundation

func fail(_ message: String) -> Never {
  FileHandle.standardError.write(Data((message + "\n").utf8))
  exit(1)
}

let arguments = CommandLine.arguments
guard arguments.count == 4 else {
  FileHandle.standardError.write(Data("Usage: verify-appcast.swift appcast.xml archive PUBLIC_ED_KEY\n".utf8))
  exit(2)
}
guard let appcast = try? String(contentsOfFile: arguments[1], encoding: .utf8) else { fail("Can't read \(arguments[1])") }
guard let archive = try? Data(contentsOf: URL(fileURLWithPath: arguments[2])) else { fail("Can't read \(arguments[2])") }
guard let raw = Data(base64Encoded: arguments[3]), let key = try? Curve25519.Signing.PublicKey(rawRepresentation: raw)
else { fail("The public key isn't a base64 EdDSA key: \(arguments[3])") }

/// The value of `name="…"` inside one XML tag.
func attribute(_ name: String, in tag: String) -> String? {
  guard let match = tag.firstMatch(of: try! Regex("\\b\(name)=\"([^\"]*)\"")) else { return nil }
  return match.output[1].substring.map(String.init)
}

// The feed is signed up to the comment at its end that holds the signature.
guard
  let block = appcast.firstMatch(of: #/<!-- sparkle-signatures:\nedSignature: ([A-Za-z0-9+\/=]+)\nlength: ([0-9]+)\n-->\s*$/#),
  let feedSignature = Data(base64Encoded: String(block.output.1)), let length = Int(block.output.2)
else { fail("\(arguments[1]) isn't signed, so the app would refuse it.") }
guard key.isValidSignature(feedSignature, for: Data(appcast.utf8).prefix(length)) else {
  fail("The signature of \(arguments[1]) doesn't match the update key built into the app.")
}

let fileName = URL(fileURLWithPath: arguments[2]).lastPathComponent
let found = appcast.matches(of: #/<item>[\s\S]*?<\/item>/#).lazy.compactMap { match -> (String, String)? in
  let item = String(appcast[match.range])
  let entry = item.matches(of: #/<enclosure\b[^>]*>/#).map { String(item[$0.range]) }
    .first { attribute("url", in: $0)?.hasSuffix("/" + fileName) == true }
  return entry.map { (item, $0) }
}.first
guard let (item, entry) = found else { fail("The appcast has no entry for \(fileName).") }
guard item.contains(#/<jevision:dataFormat>[1-9][0-9]*<\/jevision:dataFormat>/#) else {
  fail("The entry for \(fileName) has no jevision:dataFormat.")
}
guard item.contains(#"<description xml:lang="ru" sparkle:format="markdown">"#),
  item.contains(#"<description sparkle:format="markdown">"#)
else { fail("The entry for \(fileName) lacks the release notes in English or in Russian.") }
guard let signature = attribute("sparkle:edSignature", in: entry), let signed = Data(base64Encoded: signature) else {
  fail("The appcast entry for \(fileName) has no EdDSA signature.")
}
guard attribute("length", in: entry) == String(archive.count) else {
  fail("The appcast says \(attribute("length", in: entry) ?? "nothing") bytes for \(fileName), the file has \(archive.count).")
}
guard key.isValidSignature(signed, for: archive) else {
  fail("The signature of \(fileName) in the appcast doesn't match the update key built into the app.")
}
print("\(URL(fileURLWithPath: arguments[1]).lastPathComponent): signed, and the entry for \(fileName) matches the update key of the app.")
