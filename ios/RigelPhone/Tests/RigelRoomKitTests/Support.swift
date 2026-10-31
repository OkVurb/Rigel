import Foundation
import XCTest

/// A captured fixture could not be found, which is a broken test bundle rather
/// than a failed assertion.
enum FixtureError: Error {
    case missing(String)
    case notUTF8(String)
}

/// One captured payload from `ios/tools/capture-fixtures.py`, by file name.
///
/// The resource bundle differs between a macOS `swift test`, a Linux one and an
/// Xcode run, so this tries the layouts a `.copy("Fixtures")` resource can end
/// up in — including beside the test file itself, which is what a `swift test`
/// run usually leaves alone — and reports a clear error when none of them
/// holds the file.
func fixture(_ name: String) throws -> Data {
    let candidates = [
        Bundle.module.url(forResource: name, withExtension: nil, subdirectory: "Fixtures"),
        Bundle.module.resourceURL?.appendingPathComponent("Fixtures").appendingPathComponent(name),
        Bundle.module.bundleURL.appendingPathComponent("Fixtures").appendingPathComponent(name),
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent("Fixtures")
            .appendingPathComponent(name),
    ]
    for case let url? in candidates where FileManager.default.fileExists(atPath: url.path) {
        return try Data(contentsOf: url)
    }
    throw FixtureError.missing(name)
}

/// A fixture read as text, for the one payload that is HTML.
func fixtureText(_ name: String) throws -> String {
    let data = try fixture(name)
    guard let text = String(data: data, encoding: .utf8) else {
        throw FixtureError.notUTF8(name)
    }
    return text
}
