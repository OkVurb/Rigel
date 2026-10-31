import Foundation
import XCTest

@testable import RigelRoomKit

/// What a draft on the phone becomes: the rule this client and the room share,
/// and the frame the room's own page sends. Nothing here needs a server — the
/// live half is next door in `StreamTests`, because a frame that is right and a
/// socket that never opens are different failures, and only one of them can be
/// proved without a room.
final class RoomLineTests: XCTestCase {

    /// The room checks a line for emptiness before it stores it, so the phone
    /// refuses one earlier: a confirmation card about nothing is worse than a
    /// Send button that does nothing.
    func testABlankDraftIsNotALine() {
        for draft in ["", "   ", "\n", " \t\n ", "\r\n"] {
            XCTAssertNil(RoomLine(draft: draft, channel: "general"), draft.debugDescription)
        }
    }

    /// The room strips the edges itself, so the phone does the same; the inside
    /// is kept exactly, because the room stores real newlines and a line this
    /// client quietly rewrapped would not be the line the person wrote.
    func testTrimmingTakesTheEdgesAndKeepsTheMiddle() throws {
        let plain = try XCTUnwrap(RoomLine(draft: "  morning, both  ", channel: "general"))
        XCTAssertEqual(plain.text, "morning, both")
        XCTAssertEqual(plain.channel, "general")

        let multi = try XCTUnwrap(
            RoomLine(draft: "\nlook at storage.rs\n\n```\njournal_mode = WAL\n```\n", channel: "general")
        )
        XCTAssertTrue(multi.text.hasPrefix("look at"), multi.text)
        XCTAssertTrue(multi.text.contains("\n\n```\n"), multi.text)
        XCTAssertTrue(multi.text.hasSuffix("```"), multi.text)
    }

    func testAnEmptyChannelFallsBackToTheRoomsDefault() throws {
        let line = try XCTUnwrap(RoomLine(draft: "hello", channel: ""))
        XCTAssertEqual(line.channel, "general")
    }

    /// The frame is the room's own send shape — `{"type": "message", …}`, the
    /// one its web UI writes — and it names no `sender`. The room attributes a
    /// line to the session that sent it, and this is why nothing from this app
    /// can write as an agent.
    func testTheFrameIsTheRoomsSendShapeAndNamesNoSender() throws {
        let line = try XCTUnwrap(RoomLine(draft: "look at storage.rs:120", channel: "general"))
        let json = try XCTUnwrap(
            JSONSerialization.jsonObject(with: try JSONEncoder().encode(line)) as? [String: String]
        )
        XCTAssertEqual(
            json,
            ["type": "message", "text": "look at storage.rs:120", "channel": "general"]
        )
    }

    /// Quotes and backslashes are JSON's to escape and the room's to unescape; a
    /// phone that mangled either would be saying something else.
    func testAwkwardTextSurvivesTheFrame() throws {
        let awkward = "he said \"stop\" \\ then left — see `a\\b`"
        let line = try XCTUnwrap(RoomLine(draft: awkward, channel: "general"))
        struct Frame: Decodable { let text: String }
        let decoded = try JSONDecoder().decode(Frame.self, from: try JSONEncoder().encode(line))
        XCTAssertEqual(decoded.text, awkward)
    }
}
