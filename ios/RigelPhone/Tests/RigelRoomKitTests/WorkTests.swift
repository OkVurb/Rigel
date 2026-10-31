import Foundation
import XCTest

@testable import RigelRoomKit

/// The room's working state — the board, the rule book, and the decision cards
/// agents wait on.
///
/// The snapshot tests read the frame a real room pushed on connect; the rest
/// build frames from the shapes `jobs.py`, `rules.py` and `store.add` write,
/// because the captured room happened to be empty. The decoding is lenient by
/// design, so the cases that matter are the ones where a field is *missing*:
/// that is the shape an older room produces, and the phone has to render it
/// anyway.
final class WorkTests: XCTestCase {

    private func frames(_ name: String) throws -> [RoomEvent] {
        try fixtureText(name)
            .split(separator: "\n", omittingEmptySubsequences: true)
            .map { RoomEvent.decode(Data($0.utf8)) }
    }

    // MARK: - The collected frames, from a real room

    func testTheSnapshotCarriesTheBoardAndTheRules() throws {
        let events = try frames("ws_snapshot.jsonl")
        let boards = events.compactMap { event -> [RoomJob]? in
            if case .jobs(let jobs) = event { return jobs }
            return nil
        }
        let ruleBooks = events.compactMap { event -> [RoomRule]? in
            if case .rules(let rules) = event { return rules }
            return nil
        }
        XCTAssertEqual(boards.count, 1, "one board per snapshot")
        XCTAssertEqual(ruleBooks.count, 1, "one rule book per snapshot")
        // The captured room had neither. The point of this test is that the
        // frames are *collections*, not `ignored`: an empty board still has to
        // draw as a board.
        XCTAssertTrue(boards[0].isEmpty)
        XCTAssertTrue(ruleBooks[0].isEmpty)
    }

    // MARK: - One job

    func testAJobDecodesEveryFieldTheBoardDraws() throws {
        let frame = """
        {"type":"jobs","data":[{"id":3,"uid":"j-3","type":"task","title":"Fix the flaky test",\
        "body":"It fails one run in five.","status":"open","channel":"general",\
        "created_by":"user","assignee":"claude","anchor_msg_id":12,\
        "messages":[{"id":0,"uid":"m-0","sender":"user","text":"Please look","time":"10:00:00",\
        "timestamp":1700000000.0,"attachments":[]}],"created_at":1700000000.0,\
        "updated_at":1700000100.0,"sort_order":2}]}
        """
        let event = RoomEvent.decode(Data(frame.utf8))
        guard case .jobs(let jobs) = event else {
            return XCTFail("decoded as \(event)")
        }
        let job = try XCTUnwrap(jobs.first)
        XCTAssertEqual(job.id, 3)
        XCTAssertEqual(job.title, "Fix the flaky test")
        XCTAssertEqual(job.body, "It fails one run in five.")
        XCTAssertEqual(job.status, "open")
        XCTAssertEqual(job.lane, "open")
        XCTAssertEqual(job.assignee, "claude")
        XCTAssertEqual(job.createdBy, "user")
        XCTAssertEqual(job.sortOrder, 2)
        XCTAssertEqual(job.messages.count, 1)
        XCTAssertEqual(job.messages.first?.sender, "user")
        XCTAssertEqual(job.messages.first?.text, "Please look")
        // A job message with no `type` is chat, not a decode failure.
        XCTAssertEqual(job.messages.first?.type, "chat")
    }

    func testAnOlderJobWithoutTheOptionalFieldsStillDraws() throws {
        let frame = """
        {"type":"jobs","data":[{"id":1,"title":"Only what matters"}]}
        """
        guard case .jobs(let jobs) = RoomEvent.decode(Data(frame.utf8)) else {
            return XCTFail("a minimal job did not decode")
        }
        let job = try XCTUnwrap(jobs.first)
        XCTAssertEqual(job.lane, "open")
        XCTAssertEqual(job.channel, "general")
        XCTAssertTrue(job.messages.isEmpty)
        XCTAssertEqual(job.assignee, "")
    }

    func testAnInventedLaneBecomesOpen() throws {
        let frame = """
        {"type":"jobs","data":[{"id":1,"title":"x","status":"blocked"}]}
        """
        guard case .jobs(let jobs) = RoomEvent.decode(Data(frame.utf8)) else {
            return XCTFail("did not decode")
        }
        XCTAssertEqual(jobs[0].status, "blocked")
        XCTAssertEqual(jobs[0].lane, "open", "unknown lanes are work nobody has done")
    }

    func testOneJobChangingArrivesAsAJobEvent() throws {
        let frame = """
        {"type":"job","action":"update","data":{"id":7,"title":"Ship it","status":"done",\
        "channel":"general","created_by":"user","assignee":"","body":"","messages":[],\
        "created_at":1.0,"updated_at":2.0,"sort_order":1}}
        """
        guard case .job(let job) = RoomEvent.decode(Data(frame.utf8)) else {
            return XCTFail("decoded as \(RoomEvent.decode(Data(frame.utf8)))")
        }
        XCTAssertEqual(job.id, 7)
        XCTAssertEqual(job.status, "done")
        XCTAssertEqual(job.lane, "done")
    }

    // MARK: - The rule book

    func testRulesDecodeTheirStatus() throws {
        let frame = """
        {"type":"rules","data":[{"id":1,"uid":"r-1","text":"Always run the tests",\
        "author":"claude","reason":"Last change broke the build","status":"active",\
        "created_at":1700000000.0},{"id":2,"text":"Never touch main","status":"pending"}]}
        """
        guard case .rules(let rules) = RoomEvent.decode(Data(frame.utf8)) else {
            return XCTFail("did not decode")
        }
        XCTAssertEqual(rules.count, 2)
        XCTAssertTrue(rules[0].isActive)
        XCTAssertEqual(rules[0].author, "claude")
        XCTAssertFalse(rules[1].isActive)
        XCTAssertEqual(rules[1].reason, "")
    }

    func testOneRuleChangingArrivesAsARuleEvent() throws {
        let frame = """
        {"type":"rule","action":"activate","data":{"id":4,"text":"Ask before deleting",\
        "author":"user","reason":"","status":"active","created_at":3.0}}
        """
        guard case .rule(let rule) = RoomEvent.decode(Data(frame.utf8)) else {
            return XCTFail("did not decode")
        }
        XCTAssertEqual(rule.id, 4)
        XCTAssertTrue(rule.isActive)
    }

    // MARK: - Decisions inside the timeline

    private func decisionMessage(_ json: String) throws -> RoomMessage {
        try JSONDecoder().decode(RoomMessage.self, from: Data(json.utf8))
    }

    func testADecisionCardCarriesItsChoices() throws {
        let message = try decisionMessage(
            """
            {"id":9,"sender":"claude","text":"Run the migration?","type":"decision",\
            "timestamp":1700000000.0,"time":"10:00:00","channel":"general",\
            "metadata":{"choices":["yes","no"],"resolved":false}}
            """
        )
        XCTAssertTrue(message.isOpenDecision)
        XCTAssertEqual(message.decision?.choices, ["yes", "no"])
        XCTAssertFalse(message.decision?.resolved ?? true)
    }

    func testAResolvedDecisionIsNotOpen() throws {
        let message = try decisionMessage(
            """
            {"id":9,"sender":"claude","text":"Run the migration?","type":"decision",\
            "metadata":{"choices":["yes","no"],"resolved":true,"chosen":"yes"}}
            """
        )
        XCTAssertFalse(message.isOpenDecision)
        XCTAssertEqual(message.decision?.chosen, "yes")
    }

    func testMetadataOnAnOrdinaryLineIsNotADecision() throws {
        let message = try decisionMessage(
            """
            {"id":10,"sender":"claude","text":"Done.","type":"chat",\
            "metadata":{"choices":["yes","no"],"resolved":false}}
            """
        )
        XCTAssertNil(message.decision)
        XCTAssertFalse(message.isOpenDecision)
    }

    func testAnsweringMarksTheCardResolvedWithoutWaitingForTheRoom() throws {
        let message = try decisionMessage(
            """
            {"id":9,"sender":"claude","text":"Run the migration?","type":"decision",\
            "metadata":{"choices":["yes","no"],"resolved":false}}
            """
        )
        let answered = message.answered("yes")
        XCTAssertEqual(answered.id, message.id, "it is the same card, not a new line")
        XCTAssertFalse(answered.isOpenDecision)
        XCTAssertEqual(answered.decision?.chosen, "yes")
        // The original is untouched: the room's copy is still the truth until
        // its `message_update` says otherwise.
        XCTAssertTrue(message.isOpenDecision)
    }

    func testAnsweringALineThatIsNotADecisionIsItself() throws {
        let message = try decisionMessage("{\"id\":1,\"sender\":\"user\",\"text\":\"hi\"}")
        XCTAssertEqual(message.answered("yes"), message)
    }

    // MARK: - The message_update frame

    func testTheRoomBroadcastingAResolvedCardIsAMessageUpdate() throws {
        let frame = """
        {"type":"message_update","message":{"id":9,"sender":"claude","text":"Run the migration?",\
        "type":"decision","timestamp":1700000000.0,"time":"10:00:00","channel":"general",\
        "metadata":{"choices":["yes","no"],"resolved":true,"chosen":"no"}}}
        """
        guard case .messageUpdate(let message) = RoomEvent.decode(Data(frame.utf8)) else {
            return XCTFail("decoded as \(RoomEvent.decode(Data(frame.utf8)))")
        }
        XCTAssertEqual(message.id, 9)
        XCTAssertFalse(message.isOpenDecision)
        XCTAssertEqual(message.decision?.chosen, "no")
    }

    // MARK: - The frames that are still somebody else's business

    func testUnreadFramesAreStillIgnoredByName() throws {
        let events = try frames("ws_snapshot.jsonl")
        let ignored = events.compactMap { event -> String? in
            if case .ignored(let type) = event { return type }
            return nil
        }
        for type in ["agents", "base_colors", "todos", "hats", "schedules"] {
            XCTAssertTrue(ignored.contains(type), "\(type) should still be ignored by name")
        }
        XCTAssertFalse(ignored.contains("jobs"), "the board is read now")
        XCTAssertFalse(ignored.contains("rules"), "the rule book is read now")
        XCTAssertFalse(ignored.contains(""), "a frame this client cannot read at all")
    }
}
