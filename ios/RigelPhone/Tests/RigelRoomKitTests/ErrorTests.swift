import Foundation
import XCTest

@testable import RigelRoomKit

/// A stand-in for "the decoder threw", since `decodeFailure` only uses the
/// error for its message.
private struct SampleDecodingError: Error {}

/// Every failure has to arrive as a sentence a person on a couch can act on.
/// The room's own error text wins when there is one; ours has to name the
/// address, and say which of the three likely causes it is.
final class RoomErrorTests: XCTestCase {

    func testAnHTMLPageMeansSomethingElseAnswered() {
        let page = Data("<html><title>hello</title></html>".utf8)
        XCTAssertEqual(RoomClient.decodeFailure(page, SampleDecodingError()), .notARoom)
    }

    func testJSONThatIsNotARoomErrorIsAReadFailure() {
        let failure = RoomClient.decodeFailure(Data("[]".utf8), SampleDecodingError())
        guard case .decoding = failure else {
            return XCTFail("expected .decoding, got \(failure)")
        }
    }

    func testARefusedConnectionNamesTheAddressAndTheLikelyCause() {
        let address = RoomAddress(host: "192.168.1.42")
        let message = RoomClient.unreachableMessage(URLError(.cannotConnectToHost), address: address)
        XCTAssertTrue(message.contains("192.168.1.42"), message)
        XCTAssertTrue(message.contains("same network"), message)
    }

    func testATimeoutMentionsTheLanBind() {
        let address = RoomAddress(host: "rigel.local")
        let message = RoomClient.unreachableMessage(URLError(.timedOut), address: address)
        XCTAssertTrue(message.contains("rigel.local"), message)
        XCTAssertTrue(message.contains("--allow-network"), message)
    }

    /// On a tailnet address, a timeout is much more likely to be the front end
    /// than the LAN bind, so the sentence points there instead.
    func testATimeoutOnATailnetNamePointsAtTheFrontEnd() {
        let address = RoomAddress(host: "workstation.tail1a2b3.ts.net", port: 443, scheme: "https")
        let message = RoomClient.unreachableMessage(URLError(.timedOut), address: address)
        XCTAssertTrue(message.contains("tailscale serve"), message)
    }

    func testAnOfflinePhoneIsToldSo() {
        let message = RoomClient.unreachableMessage(
            URLError(.notConnectedToInternet),
            address: RoomAddress(host: "192.168.1.42")
        )
        XCTAssertTrue(message.contains("no network connection"), message)
    }

    func testPlainHTTPRefusedByIOSIsCalledOut() {
        // `URLError.Code(rawValue:)` is failable in swift-corelibs-foundation
        // and non-optional in the Darwin overlay; the code under test only
        // reads the number.
        #if os(Linux)
        let code = URLError.Code(rawValue: -1022)!
        #else
        let code = URLError.Code(rawValue: -1022)
        #endif
        let blocked = URLError(code)
        let message = RoomClient.unreachableMessage(blocked, address: RoomAddress(host: "192.168.1.42"))
        XCTAssertTrue(message.contains("plain-HTTP"), message)
    }

    /// A line typed while the room is between connections: nothing was written,
    /// and the sentence has to say both things — what happened, and that the
    /// draft is still on screen to send again.
    func testALineWithNoSocketSaysSoAndKeepsTheDraft() throws {
        let sentence = try XCTUnwrap(RoomError.notConnected.errorDescription)
        XCTAssertTrue(sentence.contains("not connected"), sentence)
        XCTAssertTrue(sentence.contains("still here"), sentence)
    }

    func testTheUnauthorizedSentencePointsAtTheTokenLine() throws {
        let sentence = try XCTUnwrap(RoomError.unauthorized.errorDescription)
        XCTAssertTrue(sentence.contains("Session token:"), sentence)
        XCTAssertEqual(RoomError.http(500).errorDescription, "The room answered with HTTP 500.")
        XCTAssertEqual(RoomError.server("forbidden").errorDescription, "forbidden")
        XCTAssertTrue(try XCTUnwrap(RoomError.badAddress.errorDescription).contains("192.168.1.42:8300"))
    }
}

/// A phone that knows its room by two names — its LAN address at home, its
/// tailnet name away — has to say what it tried when neither answers, and has
/// to keep the room's own refusal when one of them answered and said no.
final class RoomAttemptTests: XCTestCase {

    func testNeitherAddressAnsweringNamesBoth() throws {
        let addresses = [
            RoomAddress(host: "192.168.4.126"),
            RoomAddress(host: "rigel.tail5fbfdb.ts.net", port: 443, scheme: "https"),
        ]
        let failure = RoomError.noneAnswered(
            addresses.map { _ in RoomError.unreachable("nothing answered") },
            at: addresses
        )
        let sentence = try XCTUnwrap(failure.errorDescription)
        XCTAssertTrue(sentence.contains("192.168.4.126"), sentence)
        XCTAssertTrue(sentence.contains("https://rigel.tail5fbfdb.ts.net"), sentence)
        XCTAssertTrue(sentence.contains(" or "), sentence)
        XCTAssertTrue(sentence.contains("tailnet"), sentence)
    }

    /// A room that answered and refused the token is not a reachability
    /// problem, and its sentence — the one that names what to fix — has to
    /// survive the fallback.
    func testARefusedTokenBeatsTheReachabilitySentence() {
        let addresses = [RoomAddress(host: "192.168.4.126"), RoomAddress(host: "10.0.0.9")]
        let failures: [Error] = [
            RoomError.unreachable("nothing answered"),
            RoomError.unauthorized,
        ]
        let failure = RoomError.noneAnswered(failures, at: addresses)
        XCTAssertEqual(failure, .unauthorized)
        XCTAssertEqual(failure.errorDescription, RoomError.unauthorized.errorDescription)
    }

    func testASingleAddressIsNamedOnItsOwn() throws {
        let address = RoomAddress(host: "192.168.4.126")
        let failure = RoomError.noneAnswered([RoomError.unreachable("nothing")], at: [address])
        let sentence = try XCTUnwrap(failure.errorDescription)
        XCTAssertTrue(sentence.contains("192.168.4.126"), sentence)
        XCTAssertFalse(sentence.contains(" or "), sentence)
    }

    /// The probe itself, against two loopback ports nothing can be listening
    /// on: every address is attempted, and the failure names them all rather
    /// than only whichever one happened to fail last.
    func testTheProbeTriesEveryAddressAndNamesThemAll() async {
        let dead = [
            RoomAddress(host: "127.0.0.1", port: 1),
            RoomAddress(host: "127.0.0.1", port: 2),
        ]
        do {
            _ = try await RoomClient.firstAnswering(dead, token: "not-a-real-token")
            XCTFail("something answered on 127.0.0.1:1 and 127.0.0.1:2")
        } catch let failure as RoomError {
            let sentence = failure.errorDescription ?? ""
            XCTAssertTrue(sentence.contains("127.0.0.1:1"), sentence)
            XCTAssertTrue(sentence.contains("127.0.0.1:2"), sentence)
        } catch {
            XCTFail("unexpected failure: \(error)")
        }
    }
}

/// One test that talks to a real room, skipped unless it is pointed at one:
///
///   RIGEL_ROOM_BASE=http://127.0.0.1:8311 RIGEL_ROOM_TOKEN=<token> swift test
///
/// It exists because the fixtures prove decoding but not transport — TLS
/// settings, header names, query encoding and redirects only show up against a
/// live server.
final class LiveRoomTests: XCTestCase {

    func testARealRoomAnswersSettingsStatusAndMessages() async throws {
        let environment = ProcessInfo.processInfo.environment
        let token = environment["RIGEL_ROOM_TOKEN"] ?? ""
        let address = environment["RIGEL_ROOM_BASE"].flatMap(RoomAddress.init(raw:))
        try XCTSkipUnless(
            address != nil && !token.isEmpty,
            "set RIGEL_ROOM_BASE and RIGEL_ROOM_TOKEN to run against a live room"
        )
        let room = RoomClient(address: try XCTUnwrap(address), token: token)

        let settings = try await room.settings()
        XCTAssertFalse(settings.title.isEmpty)
        XCTAssertTrue(settings.channels.contains("general"))

        let status = try await room.status()
        XCTAssertEqual(status.agents.map(\.name), status.agents.map(\.name).sorted())

        let messages = try await room.messages(sinceID: 0, limit: 50)
        XCTAssertEqual(messages.map(\.id), messages.map(\.id).sorted())

        // The cursor is exclusive: asking from the last id returns nothing new.
        // A room nobody has spoken in has no messages at all, which is a valid
        // answer rather than a failure.
        if let last = messages.last?.id {
            let after = try await room.messages(sinceID: last, limit: 50)
            XCTAssertTrue(after.isEmpty, "since_id=\(last) returned \(after.map(\.id))")
        }

        // A wrong token is refused, and the refusal is the room's own sentence.
        let impostor = RoomClient(address: try XCTUnwrap(address), token: "not-the-token")
        do {
            _ = try await impostor.settings()
            XCTFail("the room accepted a wrong token")
        } catch let error as RoomError {
            XCTAssertEqual(error, .unauthorized)
        }
        // Nothing to tear down: every client shares the kit's one session, for
        // the reason spelled out on `RoomClient.sharedSession`.
    }
}
