import Foundation

/// Everything that can go wrong talking to a room, in the words the phone
/// should show. The server's own error bodies (`{"error": "…"}`) are surfaced
/// verbatim through `.server` so a version mismatch reads as the room's
/// sentence, not ours.
public enum RoomError: Error, Equatable, Sendable {
    /// The typed address could not be parsed.
    case badAddress
    /// The request never got an answer; the payload is a ready-to-show line.
    case unreachable(String)
    /// 401/403 — a missing or rejected session token.
    case unauthorized
    /// A line was typed while the room had no socket of ours to say it on: the
    /// phone is between connections, so nothing was written.
    case notConnected
    /// Something answered, but not like a room server.
    case notARoom
    /// Any other HTTP status.
    case http(Int)
    /// The answer was JSON that did not match the room's shape.
    case decoding(String)
    /// The room's own `{"error": "…"}` text.
    case server(String)
}

extension RoomError {
    /// What to say when none of the addresses a client tried answered.
    ///
    /// A room that *did* answer and refused — a rejected token, or something
    /// that is not a room at all — is something a person can act on, so that
    /// failure wins and reaches them unchanged. When every attempt merely
    /// failed to reach anything, the sentence names all of them, because
    /// "which of my two addresses did it try?" is the first thing to wonder.
    public static func noneAnswered(_ failures: [Error], at addresses: [RoomAddress]) -> RoomError {
        for failure in failures {
            if let room = failure as? RoomError, !room.isUnreachable { return room }
        }
        guard let list = Self.sentenceList(addresses.map(\.displayName)) else {
            return .unreachable("There is no room to try: type an address.")
        }
        return .unreachable(
            "Nothing answered at \(list). Is the room running, and is this phone on the "
                + "same network (or signed in to the tailnet)?"
        )
    }

    /// True when the room was never reached — the one failure a second address
    /// can fix, as opposed to a room that answered and said no.
    var isUnreachable: Bool {
        if case .unreachable = self { return true }
        return false
    }

    /// `a`, `a or b`, `a, b or c`: the way a person would list them aloud.
    private static func sentenceList(_ names: [String]) -> String? {
        switch names.count {
        case 0: return nil
        case 1: return names[0]
        default: return names.dropLast().joined(separator: ", ") + " or " + names[names.count - 1]
        }
    }
}

extension RoomError: LocalizedError {
    public var errorDescription: String? {
        switch self {
        case .badAddress:
            return "That does not look like an address. Try something like 192.168.1.42:8300."
        case .unreachable(let detail):
            return detail
        case .unauthorized:
            return "The room refused the token. Copy it again from the “Session token:” line the room prints, or leave the field blank to fetch it from the room."
        case .notConnected:
            return "The room is not connected right now, so nothing was sent. Your line is still here — send it again once it reconnects."
        case .notARoom:
            return "Something answered, but it is not a Rigel room server."
        case .http(let status):
            return "The room answered with HTTP \(status)."
        case .decoding(let detail):
            return "The room's answer could not be read: \(detail)"
        case .server(let message):
            return message
        }
    }
}
