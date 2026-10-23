import Foundation
import MachO
import Testing

@testable import JEVision

private func temporaryFolder() -> URL {
  FileManager.default.temporaryDirectory.appending(path: "jevision-diagnostics-\(UUID().uuidString)", directoryHint: .isDirectory)
}

/// The top folder of this repository, found from this file.
private let repository = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
  .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

struct LogTests {
  @Test func theLogKeepsOnlyTheLastFiles() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let log = RotatingLog(folder: folder, maxBytes: 200, files: 3)
    for number in 1...60 { try log.append("line \(number) " + String(repeating: "x", count: 20)) }

    let names = try FileManager.default.contentsOfDirectory(atPath: folder.path).sorted()
    #expect(names == ["JEVision.1.log", "JEVision.2.log", "JEVision.log"])
    for index in 0..<3 {
      let size = try #require(try log.url(index).resourceValues(forKeys: [.fileSizeKey]).fileSize)
      #expect(size <= 200)
    }
    let newest = try String(contentsOf: log.url(0), encoding: .utf8)
    let oldest = try String(contentsOf: log.url(2), encoding: .utf8)
    #expect(newest.hasSuffix("line 60 " + String(repeating: "x", count: 20) + "\n"))
    #expect(!oldest.contains("line 1 "))
  }

  @Test func aSingleLineLongerThanTheLimitIsStillWritten() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let log = RotatingLog(folder: folder, maxBytes: 10, files: 2)
    try log.append("a line much longer than ten bytes")
    try log.append("another long line, it goes to a new file")
    #expect(try String(contentsOf: log.url(1), encoding: .utf8) == "a line much longer than ten bytes\n")
    #expect(try String(contentsOf: log.url(0), encoding: .utf8) == "another long line, it goes to a new file\n")
  }

  @Test func aBugReportGetsTheNewestWholeLinesAlsoFromTheFilesBefore() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let log = RotatingLog(folder: folder, maxBytes: 40, files: 3)
    #expect(log.tail(limit: 1000).isEmpty)
    // 15 bytes a line, two lines a file: the files kept hold lines 5 to 9.
    for number in 1...9 { try log.append("line \(number) padding") }
    #expect(log.tail(limit: 1000) == (5...9).map { "line \($0) padding" })
    #expect(log.tail(limit: 45) == ["line 7 padding", "line 8 padding", "line 9 padding"])
    #expect(log.tail(limit: 44) == ["line 8 padding", "line 9 padding"])

    #expect(DiagnosticsSection.log([]) == nil)
    #expect(DiagnosticsSection.log(["a", "b"]) == "### App log\nThe newest lines of JEVision’s log on this Mac, the oldest first:\n```\na\nb\n```")
  }

  @Test func levelsIncludeTheOnesBefore() {
    #expect(LogLevel.error < .info && LogLevel.info < .debug)
    #expect(LogLevel.default == .info)
    #expect(LogLevel(rawValue: "nonsense") == nil)
  }

  @Test func aLineIsOneLineWithTimeLevelAndCategory() {
    let line = AppLog.format(.info, "run", "first\nsecond", at: Date(timeIntervalSince1970: 0))
    #expect(line == "1970-01-01T00:00:00.000Z INFO  [run] first / second")
    let long = AppLog.format(.error, "app", String(repeating: "x", count: 5000), at: .now)
    #expect(long.count < 2100 && long.hasSuffix("…"))
  }

  @Test func secretsNeverReachTheLog() {
    let secrets = [
      "GET https://api.example.com/v1?api_key=abcd1234efgh5678&q=1",
      "https://user:hunter2@example.com/x",
      "Authorization: Bearer abcdefghijklmnop12345",
      #"{"x-api-key": "sk-ant-api03-abcdefghijklmnopqrstuv"}"#,
      "password=correct-horse-battery",
      "token ghp_abcdefghijklmnopqrstuvwxyz0123456789",
      "https://api.telegram.org/bot123456789:ABCdefGHIjklMNOpqrsTUVwxyz_0123456/sendMessage",
      "https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXX",
    ]
    for secret in secrets {
      let masked = LogRedaction.mask("Request failed: \(secret)")
      #expect(masked.contains(LogRedaction.placeholder), "\(secret)")
      for word in ["abcd1234efgh5678", "hunter2", "abcdefghijklmnop12345", "sk-ant-api03", "correct-horse", "ghp_abc", "ABCdefGHIjkl", "XXXXXXXXXXXX"] {
        #expect(!masked.contains(word), "\(word) in \(masked)")
      }
    }
    #expect(masked("Request failed: https://api.example.com/v1?q=1") == "Request failed: https://api.example.com/v1?q=1")
    #expect(masked("Run ended: smoke.flow.json, passed") == "Run ended: smoke.flow.json, passed")
  }

  private func masked(_ text: String) -> String { LogRedaction.mask(text) }
}

struct CrashReportTests {
  private let payload = Data(
    """
    {"timeStampBegin":"2026-05-01T08:00:00Z","timeStampEnd":"2026-05-01T09:30:00Z","crashDiagnostics":[{
      "diagnosticMetaData":{"appVersion":"0.1.0","appBuildVersion":"42","osVersion":"macOS 26.0","deviceType":"Mac15,3",
        "platformArchitecture":"arm64","signal":11,"exceptionType":1,"exceptionCode":1,"terminationReason":"Namespace SIGSEGV, Code 0x2"},
      "callStackTree":{"callStacks":[
        {"threadAttributed":false,"callStackRootFrames":[{"binaryName":"libsystem","address":1,"offsetIntoBinaryTextSegment":1}]},
        {"threadAttributed":true,"callStackRootFrames":[{"binaryName":"JEVision","address":4296015872,"offsetIntoBinaryTextSegment":64,
          "subFrames":[{"binaryName":"libsystem_kernel.dylib","address":4300000000,"offsetIntoBinaryTextSegment":8}]}]}]}}]}
    """.utf8)

  @Test func aSummaryNamesTheBuildTheSignalAndTheCrashedThread() throws {
    let summary = try #require(CrashReports.summary(of: payload))
    #expect(summary.contains("- Time: 2026-05-01T09:30:00Z"))
    #expect(summary.contains("- JEVision 0.1.0 (42)"))
    #expect(summary.contains("- System: macOS 26.0, Mac15,3, arm64"))
    #expect(summary.contains("- Signal: 11 (SIGSEGV)"))
    #expect(summary.contains("- Reason: Namespace SIGSEGV, Code 0x2"))
    #expect(summary.contains("0  JEVision  0x100100000 +64\n1  libsystem_kernel.dylib"))
    #expect(!summary.contains("libsystem  0x1"))
  }

  @Test func framesOfALoadedBinaryNameTheirFunctionAndOthersNameTheirBuild() throws {
    // libsystem_malloc stands in for the build that crashed: this process has the same one loaded.
    let malloc = try #require(dlsym(dlopen(nil, RTLD_NOW), "malloc"))
    var info = Dl_info()
    #expect(dladdr(malloc, &info) != 0)
    let start = try #require(info.dli_fbase)
    let uuid = try #require(Symbols.uuid(of: start.assumingMemoryBound(to: mach_header.self)))
    let offset = UnsafeRawPointer(start).distance(to: malloc) + 4
    let other = "00000000-0000-0000-0000-0000000000AB"
    let payload = Data(
      """
      {"crashDiagnostics":[{"callStackTree":{"callStacks":[{"threadAttributed":true,"callStackRootFrames":[
        {"binaryName":"libsystem_malloc.dylib","binaryUUID":"\(uuid.lowercased())","address":16,"offsetIntoBinaryTextSegment":\(offset),
         "subFrames":[{"binaryName":"JEVision","binaryUUID":"\(other)","address":32,"offsetIntoBinaryTextSegment":64}]}]}]}}]}
      """.utf8)
    let summary = try #require(CrashReports.summary(of: payload))
    #expect(summary.contains("0  libsystem_malloc.dylib  0x10 +\(offset)  malloc + 4\n"))
    #expect(summary.contains("1  JEVision  0x20 +64\n"))
    #expect(summary.hasSuffix("dSYM:\n- JEVision \(other)"))
  }

  @Test func theMainThreadStartsInDyldByName() async throws {
    let dyld = try #require(Symbols.dyld)
    let uuid = try #require(Symbols.uuid(of: dyld))
    // The main thread's own stack, not the chain of async calls that `MainActor.run` would show;
    // the test is async so that it runs off the main thread and can wait for it.
    let root = try #require(DispatchQueue.main.sync { Thread.callStackReturnAddresses.last?.uintValue })
    let offset = try #require(UInt64(exactly: Int(bitPattern: UInt(root)) - Int(bitPattern: dyld)))
    #expect(Symbols().function(in: uuid, at: offset)?.hasPrefix("start + ") == true)
  }

  @Test func swiftAndRustSymbolsReadAsTheCodeNamesThem() {
    #expect(Symbols.readable("$sSi") == "Swift.Int")
    #expect(Symbols.readable("_ZN15jevision_runner3run6Runner3run17h0123456789abcdefE") == "jevision_runner::run::Runner::run")
    #expect(Symbols.readable("_ZN4core3ptr46drop_in_place$LT$alloc..vec..Vec$LT$u8$GT$$GT$17h0123456789abcdefE")
      == "core::ptr::drop_in_place<alloc::vec::Vec<u8>>")
    #expect(Symbols.readable("_ZN3app4main28_$u7b$$u7b$closure$u7d$$u7d$17h0123456789abcdefE") == "app::main::{{closure}}")
    #expect(Symbols.readable("objc_msgSend") == "objc_msgSend")
    #expect(Symbols.readable("_ZN3app99tooShortE") == "_ZN3app99tooShortE")
  }

  @Test func aFileWithoutACrashHasNoSummary() {
    #expect(CrashReports.summary(of: Data("{}".utf8)) == nil)
    #expect(CrashReports.summary(of: Data("not json".utf8)) == nil)
    #expect(CrashReports.summary(of: Data(#"{"crashDiagnostics":[]}"#.utf8)) == nil)
  }

  @Test func reportsAreSavedInTheGivenFolderNewestFirstAndOnlyTheLastAreKept() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let reports = CrashReports(folder: folder)
    #expect(reports.all().isEmpty)

    let start = Date(timeIntervalSince1970: 1_800_000_000)
    for number in 0..<(CrashReports.kept + 3) {
      try reports.save(payload, crashedAt: start.addingTimeInterval(Double(number) * 60))
    }
    let kept = reports.all()
    #expect(kept.count == CrashReports.kept)
    #expect(kept.first?.date == start.addingTimeInterval(Double(CrashReports.kept + 2) * 60))
    #expect(kept.last?.date == start.addingTimeInterval(3 * 60))
    #expect(kept.allSatisfy { $0.url.deletingLastPathComponent().standardizedFileURL == folder.standardizedFileURL })
    #expect(try Data(contentsOf: try #require(kept.first).url) == payload)
  }

  @Test func twoCrashesAtTheSameTimeAreBothKept() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let reports = CrashReports(folder: folder)
    let date = Date(timeIntervalSince1970: 1_800_000_000)
    let first = try reports.save(payload, crashedAt: date)
    let second = try reports.save(payload, crashedAt: date)
    #expect(first != second)
    #expect(reports.all().count == 2)
  }

  @Test func otherFilesInTheFolderAreNotReports() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    try Data("x".utf8).write(to: folder.appending(path: "notes.txt"))
    try Data("{}".utf8).write(to: folder.appending(path: "other.json"))
    #expect(CrashReports(folder: folder).all().isEmpty)
  }
}

struct GuideTests {
  @Test func theExampleAndTheGuideAreWhereTheBuildScriptTakesThemFrom() {
    #expect(FileManager.default.fileExists(atPath: repository.appending(path: "examples/textedit/\(ExampleProject.firstTest)").path))
    #expect(FileManager.default.fileExists(atPath: repository.appending(path: "docs/vm.html").path))
  }

  private func bundle(in folder: URL, docs: Bool) throws -> Bundle {
    let resources = folder.appending(path: "Test.app/Contents/Resources", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: resources.appending(path: "docs"), withIntermediateDirectories: true)
    if docs { try Data("<html></html>".utf8).write(to: resources.appending(path: "docs/vm.html")) }
    return try #require(Bundle(url: folder.appending(path: "Test.app")))
  }

  @Test func withoutABundleTheExampleIsMissingAndTheGuideIsOnline() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let bundle = try bundle(in: folder, docs: false)
    #expect(ExampleProject.bundled(in: bundle) == nil)
    #expect(VirtualMachineGuide.url(in: bundle).scheme == "https")
  }

  @Test func insideTheAppTheGuideOpensFromDisk() throws {
    let folder = temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = VirtualMachineGuide.url(in: try bundle(in: folder, docs: true))
    #expect(url.isFileURL && url.lastPathComponent == "vm.html")
  }
}
