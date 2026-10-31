import Foundation
import XCTest

@testable import RigelRoomKit

/// The room's wire format, decoded from payloads a real agentchattr server
/// sent. `ios/tools/capture-fixtures.py` recorded them from a throwaway room
/// with two agents registered, so these tests fail when the server's shape
/// moves — which is the only useful way to test a contract with another
/// program. Hand-written JSON would prove the decoders agree with themselves.
final class FixturesTests: XCTestCase {

    func testSettingsCarryTheRoomIdentity() throws {
        let settings = try JSONDecoder().decode(RoomSettings.self, from: try fixture("settings.json"))
        XCTAssertEqual(settings.title, "agentchattr")
        XCTAssertEqual(settings.subtitle, "")
        XCTAssertEqual(settings.username, "user")
        XCTAssertEqual(settings.channels, ["general"])
        // The payload also carries `font`, `history_limit`, `contrast` and
        // `custom_roles`, none of which this client uses. Decoding it at all is
        // the assertion that unknown fields are tolerated.
    }

    func testRosterListsBothAgentsAndThePauseState() throws {
        let status = try JSONDecoder().decode(RoomStatus.self, from: try fixture("roster.json"))
        XCTAssertFalse(status.paused)
        XCTAssertEqual(status.agents.map(\.name), ["claude", "codex"])
        XCTAssertEqual(status.agents.map(\.label), ["Claude", "Codex"])
        XCTAssertEqual(status.agents.map(\.color), ["#da7756", "#10a37f"])
        XCTAssertTrue(status.agents.allSatisfy { $0.available && !$0.busy })
    }

    func testHistoryDecodesTheWholeLog() throws {
        let messages = try JSONDecoder().decode([RoomMessage].self, from: try fixture("messages.json"))
        XCTAssertEqual(messages.map(\.id), [1, 2, 3])
        XCTAssertEqual(messages.map(\.sender), ["user", "claude", "codex"])
        XCTAssertEqual(
            messages.first?.text,
            "Morning - can you two look at the storage layer before lunch?"
        )
        XCTAssertEqual(messages.first?.channel, "general")
        XCTAssertEqual(messages.first?.time, "09:06:40")
        XCTAssertFalse(messages.contains(where: \.isNarration))
    }

    /// The room stores real newlines and a fenced block; a phone that renders
    /// codex's line as one long paragraph is wrong, so the newlines have to
    /// survive decoding intact.
    func testHistoryKeepsRealNewlinesAndFences() throws {
        let messages = try JSONDecoder().decode([RoomMessage].self, from: try fixture("messages.json"))
        let codex = try XCTUnwrap(messages.last)
        XCTAssertTrue(codex.text.contains("\n\n```\n"))
        XCTAssertTrue(codex.text.contains("journal_mode = WAL"))
        // The instant is a real Double with sub-second precision, and it sorts
        // after the seeded line from the user. Only the value changes when the
        // fixtures are recaptured, so the assertion is about the shape.
        XCTAssertGreaterThan(codex.timestamp, 1_700_000_000)
        XCTAssertGreaterThan(codex.timestamp, messages[0].timestamp)
        XCTAssertNotEqual(codex.timestamp.rounded(), codex.timestamp)
    }

    func testTheCursorAnswersWithOnlyTheNewerLines() throws {
        let messages = try JSONDecoder().decode(
            [RoomMessage].self,
            from: try fixture("messages_since.json")
        )
        XCTAssertEqual(messages.map(\.id), [3])
        XCTAssertEqual(messages.first?.sender, "codex")
    }

    /// A rejected token is answered with the room's own sentence. It has to
    /// reach the person unchanged: it names the thing they have to fix.
    func testARejectedTokenKeepsTheRoomsOwnSentence() throws {
        let data = try fixture("room_error.json")
        let sentence = "forbidden: invalid or missing session token"
        XCTAssertEqual(RoomClient.errorMessage(from: data), sentence)
        XCTAssertEqual(RoomClient.decodeFailure(data, FixtureError.missing("none")), .server(sentence))
    }

    func testTheLandingPageCarriesATokenThisClientScansOut() throws {
        let html = try fixtureText("landing_script.html")
        let token = try XCTUnwrap(RoomToken.scan(html))
        XCTAssertEqual(token.count, 64)
        XCTAssertTrue(token.allSatisfy(\.isHexDigit))
        XCTAssertTrue(RoomToken.isPlausible(token))
        // The capture redacts the live token before writing the fixture, so a
        // committed fixture can never carry a usable secret.
        XCTAssertEqual(token, String(repeating: "0123456789abcdef", count: 4))
    }
}
