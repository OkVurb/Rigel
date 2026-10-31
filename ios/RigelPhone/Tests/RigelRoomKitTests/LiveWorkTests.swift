import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import XCTest

@testable import RigelRoomKit

/// The two writes and the two reads this client added, against a room that is
/// actually running — the board, the rule book, and answering a decision card.
///
/// Reads need only the address; answering writes into the room, so it sits
/// behind the same gate as the send probe:
///
/// ```bash
/// RIGEL_ROOM_BASE=http://192.168.4.126:8323 RIGEL_ROOM_TOKEN=<token> \
///   RIGEL_ROOM_LIVE_PUSH=1 swift test --filter LiveWorkTests
/// ```
///
/// The room under test is a throwaway copy with a decision card seeded into its
/// log, because nothing on the HTTP surface creates one: agents make these
/// through the room's own tools.
final class LiveWorkTests: XCTestCase {

    private var environment: [String: String] { ProcessInfo.processInfo.environment }

    private func live() throws -> (RoomClient, RoomAddress) {
        let token = environment["RIGEL_ROOM_TOKEN"] ?? ""
        // Skip before unwrapping: a plain `swift test` has no room to point at,
        // and that is a skipped test rather than a broken bundle.
        try XCTSkipUnless(
            !token.isEmpty && environment["RIGEL_ROOM_BASE"] != nil,
            "set RIGEL_ROOM_BASE and RIGEL_ROOM_TOKEN to run against a live room"
        )
        let address = try XCTUnwrap(environment["RIGEL_ROOM_BASE"].flatMap(RoomAddress.init(raw:)))
        return (RoomClient(address: address, token: token), address)
    }

    private func writable() throws {
        try XCTSkipUnless(
            environment["RIGEL_ROOM_LIVE_PUSH"] == "1",
            "set RIGEL_ROOM_LIVE_PUSH=1 to answer a card in the room under test"
        )
    }

    func testTheBoardAndTheRuleBookAnswer() async throws {
        let (client, _) = try live()
        let jobs = try await client.jobs()
        let rules = try await client.rules()
        // Empty is a real answer for both; what is being proved here is that
        // `/api/jobs` and `/api/rules` decode into the shapes the screens draw.
        XCTAssertTrue(jobs.allSatisfy { !$0.title.isEmpty })
        XCTAssertTrue(rules.allSatisfy { !$0.text.isEmpty })
    }

    func testAnsweringADecisionCardResolvesItInTheRoom() async throws {
        let (client, _) = try live()
        try writable()

        let history = try await client.messages(limit: 200, channel: "general")
        guard let card = history.last(where: { $0.isOpenDecision }) else {
            throw XCTSkip("the room under test has no open decision card to answer")
        }
        let choice = try XCTUnwrap(card.decision?.choices.first)

        try await client.resolveDecision(messageID: card.id, choice: choice)

        // The room is the record: ask it again rather than trusting the POST's
        // 200. Its own reply is `message_update`, which the phone applies too.
        var resolved: RoomMessage?
        for _ in 0..<20 {
            let fresh = try await client.messages(limit: 200, channel: "general")
            if let found = fresh.first(where: { $0.id == card.id }), found.decision?.resolved == true {
                resolved = found
                break
            }
            try await Task.sleep(for: .milliseconds(150))
        }
        let settled = try XCTUnwrap(resolved, "the room never reported the card resolved")
        XCTAssertEqual(settled.decision?.chosen, choice)

        // …and the room posted the answer as a line, tagged at whoever asked.
        let after = try await client.messages(sinceID: card.id, limit: 200, channel: "general")
        XCTAssertTrue(
            after.contains { $0.text.contains("@\(card.sender)") && $0.text.contains(choice) },
            "the room did not post the chosen answer into the channel"
        )
    }
}
