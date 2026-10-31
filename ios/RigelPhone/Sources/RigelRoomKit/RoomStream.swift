import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// One frame from the room's `/ws` socket, decoded into the shapes a phone
/// renders.
///
/// The board and the standing orders have frames of their own, because the
/// room treats them as state rather than conversation: `jobs`/`rules` carry
/// the whole collection on connect, and `job`/`rule` carry one item changing.
///
/// Anything else the room sends — typing indicators, todos, hats, schedules,
/// and whatever a later room invents — arrives as `.ignored` rather than as an
/// error. A client that tears its socket down because the server learned a new
/// trick would drop the chat on the floor, so the frames are tolerant here and
/// the *decoding is held to account by fixtures captured from a real room*
/// (`StreamTests`).
public enum RoomEvent: Sendable, Equatable {
    /// The room's identity and channels; the first frame of every connection,
    /// and again whenever the desktop renames the room or edits a channel.
    case settings(RoomSettings)
    /// The roster and the pause state: sent after the history, and after every
    /// agent registers, leaves, or changes state.
    case status(RoomStatus)
    /// A batch of saved history. The room sends these before
    /// `historyComplete`, and they carry *every* channel — the timeline on
    /// screen is one channel's slice of them.
    case history([RoomMessage])
    /// The saved history is done; frames after this are new lines.
    case historyComplete
    /// One new line, as it was said.
    case message(RoomMessage)
    /// Lines the desktop deleted, by id.
    case delete([Int])
    /// `#channel` was cleared — or the whole room, when the frame names none.
    case clear(channel: String?)
    /// The whole board, as `/api/jobs` returns it: sent after the history on
    /// every connection.
    case jobs([RoomJob])
    /// One job created or changed. `action` is the room's word for why
    /// (`create`, `update`, `message`, `message_delete`) and is not needed to
    /// place it: the id is.
    case job(RoomJob)
    /// The whole rule book.
    case rules([RoomRule])
    /// One rule proposed, activated, archived or edited.
    case rule(RoomRule)
    /// An existing line changed under us — most often a decision card that
    /// somebody else (the desktop, another phone) just answered.
    case messageUpdate(RoomMessage)
    /// A frame this client has nothing to do with; the type is kept so tests
    /// can tell "ignored on purpose" from "ignored because unreadable".
    case ignored(String)

    /// Decode one frame. Never throws: see the note on the enum.
    public static func decode(_ data: Data) -> RoomEvent {
        (try? JSONDecoder().decode(RoomEvent.self, from: data)) ?? .ignored("")
    }
}

extension RoomEvent: Decodable {
    private enum Keys: String, CodingKey {
        case type, data, messages, ids, channel, message
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: Keys.self)
        let type = (try? container.decode(String.self, forKey: .type)) ?? ""
        switch type {
        case "settings":
            self = (try? container.decode(RoomSettings.self, forKey: .data))
                .map(RoomEvent.settings) ?? .ignored(type)
        case "status":
            self = (try? container.decode(RoomStatus.self, forKey: .data))
                .map(RoomEvent.status) ?? .ignored(type)
        case "message":
            self = (try? container.decode(RoomMessage.self, forKey: .data))
                .map(RoomEvent.message) ?? .ignored(type)
        case "history":
            self = (try? container.decode([RoomMessage].self, forKey: .messages))
                .map(RoomEvent.history) ?? .ignored(type)
        case "history_complete":
            self = .historyComplete
        case "delete":
            self = (try? container.decode([Int].self, forKey: .ids))
                .map(RoomEvent.delete) ?? .ignored(type)
        case "clear":
            // `{"type": "clear"}` with no channel means the whole room.
            self = .clear(channel: try? container.decodeIfPresent(String.self, forKey: .channel))
        case "jobs":
            self = (try? container.decode([RoomJob].self, forKey: .data))
                .map(RoomEvent.jobs) ?? .ignored(type)
        case "job":
            self = (try? container.decode(RoomJob.self, forKey: .data))
                .map(RoomEvent.job) ?? .ignored(type)
        case "rules":
            self = (try? container.decode([RoomRule].self, forKey: .data))
                .map(RoomEvent.rules) ?? .ignored(type)
        case "rule":
            self = (try? container.decode(RoomRule.self, forKey: .data))
                .map(RoomEvent.rule) ?? .ignored(type)
        case "message_update":
            self = (try? container.decode(RoomMessage.self, forKey: .message))
                .map(RoomEvent.messageUpdate) ?? .ignored(type)
        default:
            self = .ignored(type)
        }
    }
}

/// A room's channel, before it is open: `connect()` hands back the connection,
/// which is what reads the room's frames and what says the one line this client
/// is allowed to say.
///
/// One instance is one attempt: a connection ends when the room closes it, so
/// the reconnection policy lives in the caller.
public final class RoomStream: @unchecked Sendable {
    public let address: RoomAddress
    public let token: String

    /// Streams get their own session. The client's shared one is tuned for
    /// fetches — an 8 s request timeout and a 20 s limit on the whole
    /// resource — and both would cut a socket that is meant to sit open for
    /// hours. Like that one, it is never invalidated: Foundation on Linux
    /// aborts the process when a session that has carried TLS is deallocated,
    /// and a static session is not deallocated.
    private static let sharedSession: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 15
        configuration.timeoutIntervalForResource = 60 * 60 * 24 * 7
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: configuration)
    }()

    public init(address: RoomAddress, token: String) {
        self.address = address
        self.token = token
    }

    /// Open the room's channel: the socket now, so a line can be said while it
    /// is being read.
    ///
    /// This never answers with a frame of its own. The room refuses the one
    /// thing it can refuse here — a token it does not know — by accepting the
    /// socket and closing it at once (code 4003), which surfaces on the first
    /// read of `events()`, with no frames before it.
    public func connect() throws -> RoomConnection {
        guard let url = address.webSocketURL(path: "/ws", query: [("token", token)]) else {
            throw RoomError.badAddress
        }
        let socket = Self.sharedSession.webSocketTask(with: url)
        socket.resume()
        return RoomConnection(address: address, socket: socket)
    }

    /// One connection's frames, for a reader that will not write: the sequence
    /// ends when the room closes the socket. An address that cannot be a socket
    /// URL ends it with `.badAddress` rather than throwing at the call, which
    /// is what callers that only read have always had.
    public func events() -> AsyncThrowingStream<RoomEvent, Error> {
        do {
            return try connect().events()
        } catch {
            return AsyncThrowingStream { continuation in continuation.finish(throwing: error) }
        }
    }

    /// A socket that ends is not an HTTP failure, but the sentence lands in the
    /// same banner above a timeline that stays on screen while the client
    /// retries, so it is phrased for that.
    static func dropped(_ error: Error, at address: RoomAddress) -> RoomError {
        guard let urlError = error as? URLError else {
            return .unreachable("Lost the connection to \(address.displayName). Reconnecting…")
        }
        return .unreachable(RoomClient.unreachableMessage(urlError, address: address))
    }
}

/// One open connection to a room: the frames it pushes, and the one line this
/// client will say.
///
/// This is the room's own web-UI channel. The token goes in the query string,
/// and a wrong one is accepted and then closed at once (code 4003). On connect
/// the room pushes a snapshot — settings, the registry, the frames this client
/// ignores, saved history in batches, then `history_complete`, then the roster
/// — and after that every new line, deletion, clear and roster change as it
/// happens. That snapshot is why a reconnect does not need to replay anything:
/// the room re-sends the whole room.
///
/// Reading and writing share the socket on purpose. A line written on a socket
/// the room is already reading costs one frame; a *second* connection would be
/// answered with the whole history again, on a phone, once per line. The room
/// echoes the line it stored back down this same socket, which is where the
/// timeline gets it — nothing here is shown optimistically.
///
/// `send` says one line and nothing else. Answering a decision card is an HTTP
/// call (`RoomClient.resolveDecision`), not a frame, because the room records
/// the answer on the card before it says anything — so this client never
/// writes an approval as chat prose it merely hopes was taken.
public struct RoomConnection: @unchecked Sendable {
    let address: RoomAddress
    /// The socket, resumed by `RoomStream.connect()` and cancelled when
    /// `events()` ends. `URLSessionTask` is not `Sendable`, hence the unchecked
    /// conformance: the task goes to one pump and to a writer at a time, which
    /// is what a socket is for.
    let socket: URLSessionWebSocketTask

    /// The socket's frames, in order: the snapshot, then every line, deletion,
    /// clear and roster change as it happens. The sequence ends when the room
    /// closes it, and throws when it cannot be opened or drops. Cancelling the
    /// consuming task closes the socket — which is what a phone going to the
    /// background should do. One connection reads once.
    public func events() -> AsyncThrowingStream<RoomEvent, Error> {
        let address = self.address
        let socket = self.socket
        return AsyncThrowingStream { continuation in
            let pump = Task {
                do {
                    while !Task.isCancelled {
                        let text = try await Self.nextText(from: socket)
                        continuation.yield(RoomEvent.decode(Data(text.utf8)))
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: RoomStream.dropped(error, at: address))
                }
            }
            continuation.onTermination = { _ in
                pump.cancel()
                socket.cancel(with: .goingAway, reason: nil)
            }
        }
    }

    /// Say one line: the room's own frame, on the socket this connection is.
    ///
    /// The write lands in the room's receive loop, which is what makes a live
    /// reader the precondition: a socket the room has already closed fails here
    /// rather than pretending, and the stored line comes back through
    /// `events()` as the room echoes it to everyone — this connection included.
    public func send(_ line: RoomLine) async throws {
        let text: String
        do {
            text = String(decoding: try JSONEncoder().encode(line), as: UTF8.self)
        } catch {
            throw RoomError.decoding("The line could not be written as a frame.")
        }
        do {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                socket.send(.string(text)) { error in
                    if let error {
                        continuation.resume(throwing: error)
                    } else {
                        continuation.resume()
                    }
                }
            }
        } catch {
            throw Self.unsent(error, at: address)
        }
    }

    /// The next text frame. `URLSession`'s async `receive()` is not in
    /// swift-corelibs-foundation, so this bridges the completion-handler form,
    /// which both platforms have.
    private static func nextText(from socket: URLSessionWebSocketTask) async throws -> String {
        let message = try await withCheckedThrowingContinuation { continuation in
            socket.receive { continuation.resume(with: $0) }
        }
        switch message {
        case .string(let text):
            return text
        case .data(let data):
            return String(decoding: data, as: UTF8.self)
        @unknown default:
            return ""
        }
    }

    /// Why a line did not go: the sentence the reader's dropped socket gets,
    /// because it is the same failure — the room stopped answering on our
    /// connection — plus one for a socket that ends without being a URL error.
    static func unsent(_ error: Error, at address: RoomAddress) -> RoomError {
        guard let urlError = error as? URLError else {
            return .unreachable("Lost the connection to \(address.displayName) before the line was sent.")
        }
        return .unreachable(RoomClient.unreachableMessage(urlError, address: address))
    }
}
