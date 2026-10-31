import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import XCTest

@testable import RigelRoomKit

/// The socket's wire format, decoded from frames a real agentchattr server
/// pushed. `ios/tools/ws-frames` recorded them from a throwaway room on the
/// same recipe `FixturesTests` describes for the REST payloads — one seeded
/// user line, two registered agents — so these tests fail when the room's
/// push format moves, which is the only useful way to test a contract with
/// another program.
final class StreamTests: XCTestCase {

    /// Every frame of a captured file, decoded. The files are one frame per
    /// line, which is how the tool writes them.
    private func frames(_ name: String) throws -> [RoomEvent] {
        try fixtureText(name)
            .split(separator: "\n", omittingEmptySubsequences: true)
            .map { RoomEvent.decode(Data($0.utf8)) }
    }

    private func messages(_ events: [RoomEvent]) -> [RoomMessage] {
        events.flatMap { event -> [RoomMessage] in
            if case .history(let batch) = event { return batch }
            if case .message(let one) = event { return [one] }
            return []
        }
    }

    // MARK: - The snapshot the room sends on connect

    func testTheSnapshotsFirstFrameIsTheRoomIdentity() throws {
        let events = try frames("ws_snapshot.jsonl")
        guard case .settings(let settings)? = events.first else {
            return XCTFail("the first frame was \(String(describing: events.first))")
        }
        XCTAssertEqual(settings.title, "agentchattr")
        XCTAssertEqual(settings.username, "user")
        XCTAssertEqual(settings.channels, ["general"])
    }

    func testTheSnapshotCarriesTheLogAndThenSaysItIsDone() throws {
        let events = try frames("ws_snapshot.jsonl")
        let history = messages(events)
        XCTAssertFalse(history.isEmpty)
        // The room sorts history by timestamp; a phone renders it in the order
        // it arrived, so that order has to be the id order.
        XCTAssertEqual(history.map(\.id), history.map(\.id).sorted())
        // The seeded line and both agents' lines, the same three the REST
        // fixture holds — the two transports have to agree.
        XCTAssertEqual(history.map(\.sender).prefix(3), ["user", "claude", "codex"])
        XCTAssertEqual(history.first?.channel, "general")
        // …and the room narrates joins and leaves into the same history.
        XCTAssertTrue(history.contains(where: \.isNarration))

        let complete = try XCTUnwrap(events.firstIndex(of: .historyComplete))
        let status = try XCTUnwrap(events.firstIndex { event in
            if case .status = event { return true }
            return false
        })
        XCTAssertLessThan(complete, status, "the roster arrives after the history")
        XCTAssertTrue(events.contains(.historyComplete))
    }

    /// The snapshot carries the room's whole repertoire, and this client still
    /// shows only some of it. What it does not draw has to be a named,
    /// deliberate `.ignored` rather than a frame it could not read — jobs and
    /// rules are no longer on that list: `WorkTests` covers them.
    func testTheFramesThisClientIgnoresAreNamedNotSwallowed() throws {
        let events = try frames("ws_snapshot.jsonl")
        let ignored = events.compactMap { event -> String? in
            if case .ignored(let type) = event { return type }
            return nil
        }
        for type in ["agents", "base_colors", "todos", "hats", "schedules"] {
            XCTAssertTrue(ignored.contains(type), "\(type) should be ignored by name")
        }
        XCTAssertFalse(ignored.contains(""), "a frame this client cannot read at all got captured")
    }

    // MARK: - What the room pushes while it is open

    func testAPushedLineDecodesWithItsSenderAndText() throws {
        let events = try frames("ws_push.jsonl")
        // Everything after the snapshot is what the room pushed while the
        // socket was open; the lines before it are the history replayed on
        // connect, which the other test covers.
        let pushed = events.drop { $0 != .historyComplete }.dropFirst()
        let line = try XCTUnwrap(
            pushed.compactMap { event -> RoomMessage? in
                if case .message(let message) = event { return message }
                return nil
            }.first
        )
        XCTAssertEqual(line.sender, "claude")
        XCTAssertEqual(line.text, "Posting while the socket is open.")
        XCTAssertEqual(line.type, "chat")
        XCTAssertEqual(line.channel, "general")
        // The instant is real and sub-second; only the value changes when the
        // fixture is recaptured, so this is about the shape.
        XCTAssertGreaterThan(line.timestamp, 1_700_000_000)
        XCTAssertNotEqual(line.timestamp.rounded(), line.timestamp)
    }

    /// An agent registering pushes a roster with it in, which is what makes the
    /// pills above the timeline live rather than a poll behind reality.
    func testARegistrationPushesARosterWithTheAgentInIt() throws {
        let events = try frames("ws_push.jsonl")
        let rosters = events.compactMap { event -> RoomStatus? in
            if case .status(let roster) = event { return roster }
            return nil
        }
        let occupied = try XCTUnwrap(rosters.first { !$0.agents.isEmpty })
        XCTAssertEqual(occupied.agents.map(\.name), ["claude"])
        XCTAssertEqual(occupied.agents.map(\.label), ["Claude"])
        XCTAssertEqual(occupied.agents.map(\.color), ["#da7756"])
        XCTAssertTrue(occupied.agents.allSatisfy { $0.available && !$0.busy })
        XCTAssertFalse(occupied.paused)
    }

    // MARK: - Frames that were not captured

    /// `delete` and `clear` are the desktop's own actions — only a client that
    /// writes can provoke them — so their shapes come from the room's source
    /// rather than from a capture.
    func testDesktopDeletionsAndClearsDecode() {
        XCTAssertEqual(
            RoomEvent.decode(Data(#"{"type": "delete", "ids": [4, 5]}"#.utf8)),
            .delete([4, 5])
        )
        XCTAssertEqual(
            RoomEvent.decode(Data(#"{"type": "clear", "channel": "general"}"#.utf8)),
            .clear(channel: "general")
        )
        XCTAssertEqual(
            RoomEvent.decode(Data(#"{"type": "clear"}"#.utf8)),
            .clear(channel: nil),
            "a clear with no channel is the whole room"
        )
    }

    /// A frame this client cannot read has to cost a line of chat nothing: the
    /// socket is one long-lived connection, and a phone that drops it because
    /// the room learned a new trick loses the conversation.
    func testAnUnreadableFrameIsIgnoredRatherThanFatal() {
        XCTAssertEqual(
            RoomEvent.decode(Data(#"{"type": "recipe", "data": {}}"#.utf8)),
            .ignored("recipe"),
            "a frame type from a newer room"
        )
        XCTAssertEqual(
            RoomEvent.decode(Data(#"{"type": "message", "data": {"id": "one"}}"#.utf8)),
            .ignored("message"),
            "a known type whose payload no longer fits"
        )
        XCTAssertEqual(RoomEvent.decode(Data("not json at all".utf8)), .ignored(""))
    }

    // MARK: - The socket URL

    func testTheSocketURLSpeaksTheRoomsOwnScheme() throws {
        let plain = RoomAddress(host: "192.168.1.42", port: 8311)
        XCTAssertEqual(
            plain.webSocketURL(path: "/ws", query: [("token", "abc123")])?.absoluteString,
            "ws://192.168.1.42:8311/ws?token=abc123"
        )
        let served = RoomAddress(host: "workstation.tail1a2b3.ts.net", port: 443, scheme: "https")
        XCTAssertEqual(
            served.webSocketURL(path: "/ws", query: [("token", "abc123")])?.absoluteString,
            "wss://workstation.tail1a2b3.ts.net/ws?token=abc123"
        )
    }
}

/// The tests that open a real socket, skipped unless they are pointed at a
/// room:
///
///   RIGEL_ROOM_BASE=http://192.168.4.126:8311 RIGEL_ROOM_TOKEN=<token> \
///     swift test --filter LiveStreamTests
///
/// The fixtures prove decoding; these prove the transport — the upgrade
/// handshake, the token in the query string, and that a room really does push
/// its snapshot to this client. Over a tailnet name
/// (`RIGEL_ROOM_BASE=https://<device>.<tailnet>.ts.net`) they also prove it
/// through `tailscale serve` and TLS, which is the route the phone uses when it
/// is away from home.
///
/// The push test writes to the room, so it is behind a second switch: a room
/// someone is using should not have a probe line appear in it unannounced.
final class LiveStreamTests: XCTestCase {

    private var address: RoomAddress? {
        ProcessInfo.processInfo.environment["RIGEL_ROOM_BASE"].flatMap(RoomAddress.init(raw:))
    }

    private var token: String {
        ProcessInfo.processInfo.environment["RIGEL_ROOM_TOKEN"] ?? ""
    }

    /// The point of remembering two addresses: the one that answers is used
    /// even when it is the second. A closed loopback port stands in for the LAN
    /// address the phone has walked away from.
    func testTheSecondAddressIsUsedWhenTheFirstDoesNotAnswer() async throws {
        try XCTSkipUnless(
            address != nil && !token.isEmpty,
            "set RIGEL_ROOM_BASE and RIGEL_ROOM_TOKEN to run against a live room"
        )
        let live = try XCTUnwrap(address)
        let abandoned = RoomAddress(host: "127.0.0.1", port: 1)

        let found = try await RoomClient.firstAnswering([abandoned, live], token: token)

        XCTAssertEqual(found.client.address, live)
        XCTAssertFalse(found.settings.title.isEmpty)
    }

    func testTheSocketDeliversTheRoomsSnapshot() async throws {
        try XCTSkipUnless(
            address != nil && !token.isEmpty,
            "set RIGEL_ROOM_BASE and RIGEL_ROOM_TOKEN to run against a live room"
        )
        let address = try XCTUnwrap(address)

        let stream = RoomStream(address: address, token: token)
        let seen = await withTaskGroup(of: [String: Bool].self) { group -> [String: Bool] in
            group.addTask { await Self.watch(stream, for: nil) }
            group.addTask {
                try? await Task.sleep(for: .seconds(20))
                return ["timed out": true]
            }
            let first = await group.next() ?? [:]
            group.cancelAll()
            return first
        }

        XCTAssertEqual(seen["settings"], true, "no settings frame: \(seen)")
        XCTAssertEqual(seen["history_complete"], true, "the snapshot never completed: \(seen)")
        XCTAssertEqual(seen["status"], true, "no roster in the snapshot: \(seen)")
        XCTAssertNil(seen["failed"], "the socket failed: \(seen)")
    }

    /// A line typed on the computer, arriving on the socket. This is the whole
    /// point of the stream: the phone sees it when it happens, not on the next
    /// tick of a poll.
    func testALinePostedWhileConnectedArrivesOnTheSocket() async throws {
        try XCTSkipUnless(
            address != nil && !token.isEmpty,
            "set RIGEL_ROOM_BASE and RIGEL_ROOM_TOKEN to run against a live room"
        )
        let address = try XCTUnwrap(address)
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["RIGEL_ROOM_LIVE_PUSH"] == "1",
            "set RIGEL_ROOM_LIVE_PUSH=1 to post a probe line into the room under test"
        )

        let token = self.token
        let marker = "Rigel streaming probe \(UUID().uuidString.prefix(8))"
        let stream = RoomStream(address: address, token: token)
        let arrived = await withTaskGroup(of: Bool.self) { group -> Bool in
            group.addTask { await Self.watch(stream, for: marker)["line"] == true }
            group.addTask {
                // Give the socket a moment to be connected and past its
                // snapshot, then post the line the way the room's own page
                // does: a second socket, the same session token.
                try? await Task.sleep(for: .seconds(2))
                try? await Self.post(marker, to: address, token: token)
                return false
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(20))
                return false
            }
            var found = false
            for await outcome in group where outcome {
                found = true
                group.cancelAll()
                break
            }
            return found
        }
        XCTAssertTrue(arrived, "\(marker) never arrived on the socket")
    }

    // MARK: - Helpers

    /// Read a socket until the snapshot is complete and — when a line is
    /// expected — until that line shows up. Returns what it saw, so a timeout
    /// says which part was missing.
    private static func watch(_ stream: RoomStream, for marker: String?) async -> [String: Bool] {
        var seen: [String: Bool] = [:]
        do {
            for try await event in stream.events() {
                switch event {
                case .settings:
                    seen["settings"] = true
                case .historyComplete:
                    seen["history_complete"] = true
                case .status:
                    seen["status"] = true
                case .message(let message):
                    if let marker, message.text == marker {
                        seen["line"] = true
                        return seen
                    }
                default:
                    break
                }
                // Waiting for the roster too: the room sends it after the
                // history, and it is the last frame of the snapshot.
                if marker == nil, seen.count == 3 { return seen }
            }
        } catch {
            seen["failed"] = true
            seen["error"] = true
        }
        return seen
    }

    /// Send one line from a socket of its own, the way the room's own page and
    /// the desktop do. This test is about a line arriving from *somewhere else*,
    /// so it deliberately does not use the phone's send path — that one is
    /// proved on its own in `LiveSendTests`.
    private static func post(_ text: String, to address: RoomAddress, token: String) async throws {
        let url = try XCTUnwrap(address.webSocketURL(path: "/ws", query: [("token", token)]))
        let socket = URLSession.shared.webSocketTask(with: url)
        socket.resume()
        let frame = #"{"type": "message", "text": "\#(text)", "channel": "general"}"#
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            socket.send(.string(frame)) { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume()
                }
            }
        }
        socket.cancel(with: .normalClosure, reason: nil)
    }
}

/// The phone's one remote action, against a real room: `RoomConnection.send`
/// writes the line on the socket the reader already holds, the room echoes the
/// stored line back down that same connection, and the room's own log carries
/// it afterwards. It writes, so it waits behind the same switch the reader's
/// probe does.
///
///   RIGEL_ROOM_LIVE_PUSH=1 RIGEL_ROOM_BASE=http://192.168.4.126:8311 \
///     RIGEL_ROOM_TOKEN=<token> swift test --filter LiveSendTests
final class LiveSendTests: XCTestCase {

    private var address: RoomAddress? {
        ProcessInfo.processInfo.environment["RIGEL_ROOM_BASE"].flatMap(RoomAddress.init(raw:))
    }

    private var token: String {
        ProcessInfo.processInfo.environment["RIGEL_ROOM_TOKEN"] ?? ""
    }

    func testALineSentFromThePhoneIsEchoedBackAndStored() async throws {
        try XCTSkipUnless(
            address != nil && !token.isEmpty,
            "set RIGEL_ROOM_BASE and RIGEL_ROOM_TOKEN to run against a live room"
        )
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["RIGEL_ROOM_LIVE_PUSH"] == "1",
            "set RIGEL_ROOM_LIVE_PUSH=1 to post a probe line into the room under test"
        )
        let address = try XCTUnwrap(address)
        let marker = "Rigel send probe \(UUID().uuidString.prefix(8))"
        let line = try XCTUnwrap(RoomLine(draft: marker, channel: "general"))

        let connection = try RoomStream(address: address, token: token).connect()
        let echoed = await withTaskGroup(of: Bool.self) { group -> Bool in
            group.addTask {
                do {
                    for try await event in connection.events() {
                        if case .message(let message) = event, message.text == marker {
                            return true
                        }
                    }
                } catch {
                    return false
                }
                return false
            }
            group.addTask {
                // The room reads from this socket only once its snapshot is on
                // its way, so the write waits a moment rather than racing the
                // frames ahead of it.
                try? await Task.sleep(for: .seconds(2))
                try? await connection.send(line)
                return false
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(20))
                return false
            }
            var found = false
            for await outcome in group where outcome {
                found = true
                group.cancelAll()
                break
            }
            return found
        }
        XCTAssertTrue(echoed, "\(marker) never came back on the socket it was written to")

        // The echo proves the socket path; the log proves the room stored the
        // line rather than a client believing its own frame.
        let stored = try await RoomClient(address: address, token: token).messages(limit: 50)
        XCTAssertTrue(stored.contains { $0.text == marker }, "the room's log has no \(marker)")
    }
}
