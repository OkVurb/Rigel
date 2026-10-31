import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// A reader for one room: settings, the roster, history, and the landing page
/// the token comes from.
///
/// The room is agentchattr, the chat server Rigel's Chat tab drives. This
/// speaks the endpoints the desktop speaks (`/api/settings`, `/api/status`,
/// `/api/messages`, `/api/jobs`, `/api/rules`) with the same `x-session-token`
/// header. A typed line is not one of them: the room takes that as a frame on
/// its own socket, which is `RoomConnection.send`.
///
/// Exactly one write goes over HTTP — `resolveDecision`. That is the room's
/// design, not a shortcut: an answer to a decision card has to be checked
/// against the card's own choices and recorded on it, so the room does it in
/// one place rather than trusting whatever prose a client types.
public final class RoomClient: @unchecked Sendable {
    public let address: RoomAddress
    public let token: String
    private let session: URLSession

    /// One session for every client, and deliberately never invalidated.
    ///
    /// Two reasons. The practical one: Foundation on Linux aborts the process
    /// when a `URLSession` that has carried TLS traffic is deallocated — a
    /// libcurl self-reference surfaces as "_MultiHandle deallocated with
    /// non-zero retain count", which is a bug in swift-corelibs-foundation and
    /// uncatchable from here. A shared session is never deallocated, so it
    /// cannot fire. The ordinary one: one pool for every room means reconnects
    /// and room switches reuse connections instead of building a session each
    /// time, which is what `URLSession.shared` gives callers anyway.
    private static let sharedSession: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 8
        configuration.timeoutIntervalForResource = 20
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: configuration)
    }()

    /// The client is immutable; the only reference type it holds is
    /// `URLSession`, which Foundation documents as thread-safe — hence the
    /// unchecked conformance.
    public init(address: RoomAddress, token: String) {
        self.address = address
        self.token = token
        self.session = Self.sharedSession
    }

    /// The room's title, channels and configured username. This doubles as the
    /// connect check: it is the cheapest endpoint that proves both the address
    /// and the token.
    public func settings() async throws -> RoomSettings {
        try decode(RoomSettings.self, from: await get("/api/settings"))
    }

    /// The roster and pause state.
    public func status() async throws -> RoomStatus {
        try decode(RoomStatus.self, from: await get("/api/status"))
    }

    /// Recent history, or everything newer than `sinceID`.
    ///
    /// The live timeline does not come from here — that is the socket
    /// (`RoomStream`). This is what the connect screen waits for, so the room
    /// is on screen the moment it goes away, and what any caller wants when it
    /// has no socket open.
    public func messages(
        sinceID: Int = 0,
        limit: Int = 50,
        channel: String? = nil
    ) async throws -> [RoomMessage] {
        var query: [(String, String)] = []
        if sinceID > 0 { query.append(("since_id", String(sinceID))) }
        query.append(("limit", String(limit)))
        if let channel, !channel.isEmpty { query.append(("channel", channel)) }
        let data = try await get("/api/messages", query: query)
        do {
            return try JSONDecoder().decode([RoomMessage].self, from: data)
        } catch {
            throw Self.decodeFailure(data, error)
        }
    }

    /// The room's board. The socket sends the same collection on connect; this
    /// is what a screen reads when it wants the board before the snapshot has
    /// finished arriving.
    public func jobs() async throws -> [RoomJob] {
        try decode([RoomJob].self, from: await get("/api/jobs"))
    }

    /// The room's standing orders. Read-only on this client: activating a rule
    /// changes what every agent does next.
    public func rules() async throws -> [RoomRule] {
        try decode([RoomRule].self, from: await get("/api/rules"))
    }

    /// Answer a decision card.
    ///
    /// The room validates the choice against the card's own list, marks the
    /// card resolved, posts `@sender <choice>` in the card's channel and then
    /// broadcasts the updated card. The screen does not draw the answer before
    /// that broadcast lands: this only reports whether the answer was taken.
    public func resolveDecision(messageID: Int, choice: String) async throws {
        _ = try await post(
            "/api/messages/\(messageID)/resolve_decision",
            body: ["choice": choice]
        )
    }

    /// Copy one file into the room and answer with what it stored: the name the
    /// person gave it, and the path it is served at.
    ///
    /// `/api/upload` takes multipart form data in one field, so this is the one
    /// place the client builds a body by hand. The room decides what it will
    /// keep — photos, and the documents listed beside `ALLOWED_UPLOAD_EXTS` in
    /// `agentchattr/app.py` — and refuses anything else with a sentence this
    /// surfaces to the composer.
    public func upload(
        _ bytes: Data,
        filename: String,
        mimeType: String
    ) async throws -> RoomAttachment {
        guard let url = address.url(path: "/api/upload") else { throw RoomError.badAddress }

        let boundary = "rigel-\(UUID().uuidString)"
        let safeName = filename
            .replacingOccurrences(of: "\"", with: "")
            .replacingOccurrences(of: "\r", with: "")
            .replacingOccurrences(of: "\n", with: "")

        var body = Data()
        func append(_ text: String) { body.append(Data(text.utf8)) }
        append("--\(boundary)\r\n")
        append("Content-Disposition: form-data; name=\"file\"; filename=\"\(safeName)\"\r\n")
        append("Content-Type: \(mimeType)\r\n\r\n")
        body.append(bytes)
        append("\r\n--\(boundary)--\r\n")

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        // The client's session times out at eight seconds, which is right for
        // a read and wrong for ten megabytes over a phone hotspot.
        request.timeoutInterval = 60
        request.setValue(
            "multipart/form-data; boundary=\(boundary)",
            forHTTPHeaderField: "Content-Type"
        )
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if !token.isEmpty {
            request.setValue(token, forHTTPHeaderField: "x-session-token")
        }
        request.httpBody = body

        let (reply, response) = try await perform(request)
        switch response.statusCode {
        case 200...299:
            return try decode(RoomAttachment.self, from: reply)
        case 401, 403:
            throw RoomError.unauthorized
        default:
            if let message = Self.errorMessage(from: reply) {
                throw RoomError.server(message)
            }
            throw RoomError.http(response.statusCode)
        }
    }

    /// The room's landing page, fetched without a token — the page agentchattr
    /// injects `window.__SESSION_TOKEN__` into.
    public func landingPage() async throws -> String {
        let data = try await get("/", authenticated: false)
        return String(decoding: data, as: UTF8.self)
    }

    /// Read the token out of the landing page and prove it before trusting it:
    /// `/api/settings` must answer through it, so a page that merely contains
    /// the marker cannot pass itself off as a room.
    public static func adoptToken(address: RoomAddress) async throws -> String {
        let page = try await RoomClient(address: address, token: "").landingPage()
        guard let found = RoomToken.scan(page) else { throw RoomError.notARoom }
        _ = try await RoomClient(address: address, token: found).settings()
        return found
    }

    /// The first of these addresses that answers, with a token the room
    /// accepts — a client and its settings, ready to read from.
    ///
    /// This is the whole point of a phone that knows its room by two names: at
    /// home the LAN address answers and the tailnet name is a detour, away from
    /// home the reverse, and the phone should not need to be told which it is.
    /// Addresses are tried in order; the first that answers wins. A blank
    /// token is read from that address's own landing page, and a token the room
    /// refuses is re-adopted the same way, because every room restart mints a
    /// new one — nothing is trusted without the settings call that follows.
    ///
    /// When nothing answers, the error names every address that was tried.
    public static func firstAnswering(
        _ addresses: [RoomAddress],
        token: String = ""
    ) async throws -> (client: RoomClient, settings: RoomSettings) {
        var failures: [Error] = []
        for address in addresses {
            do {
                return try await open(address, token: token)
            } catch RoomError.unauthorized where !token.isEmpty {
                // The room exists and said no: the token is stale rather than
                // wrong about which room this is, so ask the room for the
                // current one and give this address one more go.
                do {
                    return try await open(address, token: "")
                } catch {
                    failures.append(error)
                }
            } catch {
                failures.append(error)
            }
        }
        throw RoomError.noneAnswered(failures, at: addresses)
    }

    /// One address, one attempt: adopt a token when none was given, then prove
    /// both the address and the token with the cheapest endpoint that answers
    /// with something this client needs anyway.
    private static func open(
        _ address: RoomAddress,
        token: String
    ) async throws -> (client: RoomClient, settings: RoomSettings) {
        let resolved = token.isEmpty ? try await adoptToken(address: address) : token
        let client = RoomClient(address: address, token: resolved)
        return (client, try await client.settings())
    }

    // MARK: - Transport

    private func get(
        _ path: String,
        query: [(String, String)] = [],
        authenticated: Bool = true
    ) async throws -> Data {
        guard let url = address.url(path: path, query: query) else {
            throw RoomError.badAddress
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.cachePolicy = .reloadIgnoringLocalCacheData
        if authenticated, !token.isEmpty {
            request.setValue(token, forHTTPHeaderField: "x-session-token")
        }

        let (data, response) = try await perform(request)
        switch response.statusCode {
        case 200...299:
            return data
        case 401, 403:
            throw RoomError.unauthorized
        default:
            if let message = Self.errorMessage(from: data) {
                throw RoomError.server(message)
            }
            throw RoomError.http(response.statusCode)
        }
    }

    /// The one write this client makes over HTTP. The body is JSON, which is
    /// what the room's `resolve_decision` reads; the same token header as every
    /// read, so a room that says `403` here means the token went stale.
    private func post(_ path: String, body: [String: String]) async throws -> Data {
        guard let url = address.url(path: path) else { throw RoomError.badAddress }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if !token.isEmpty {
            request.setValue(token, forHTTPHeaderField: "x-session-token")
        }
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (data, response) = try await perform(request)
        switch response.statusCode {
        case 200...299:
            return data
        case 401, 403:
            throw RoomError.unauthorized
        default:
            if let message = Self.errorMessage(from: data) {
                throw RoomError.server(message)
            }
            throw RoomError.http(response.statusCode)
        }
    }

    private func perform(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        do {
            return try await withCheckedThrowingContinuation { continuation in
                let task = session.dataTask(with: request) { data, response, error in
                    if let error {
                        continuation.resume(throwing: error)
                        return
                    }
                    guard let http = response as? HTTPURLResponse, let data else {
                        continuation.resume(throwing: RoomError.notARoom)
                        return
                    }
                    continuation.resume(returning: (data, http))
                }
                task.resume()
            }
        } catch let error as URLError {
            throw RoomError.unreachable(Self.unreachableMessage(error, address: address))
        }
    }

    private func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        do {
            return try JSONDecoder().decode(type, from: data)
        } catch {
            throw Self.decodeFailure(data, error)
        }
    }

    /// Turn a decoding failure into the most specific error the body supports:
    /// an HTML page is `.notARoom`, a room-style error body is `.server`,
    /// anything else is `.decoding`.
    static func decodeFailure(_ data: Data, _ error: Error) -> RoomError {
        if let text = String(data: data, encoding: .utf8) {
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.hasPrefix("<") { return .notARoom }
            if let message = Self.errorMessage(from: data) { return .server(message) }
        }
        return .decoding(String(describing: error))
    }

    /// The room's own `{"error": "…"}` sentence, when the body carries one.
    public static func errorMessage(from data: Data) -> String? {
        struct Body: Decodable { let error: String? }
        guard let body = try? JSONDecoder().decode(Body.self, from: data) else { return nil }
        return body.error
    }

    /// URLError codes in the words a person on a couch can act on.
    static func unreachableMessage(_ error: URLError, address: RoomAddress) -> String {
        switch error.code {
        case .cannotConnectToHost, .cannotFindHost, .networkConnectionLost:
            return "Nothing answered at \(address.displayName). Is the room running, and is this phone on the same network (or signed in to the tailnet)?"
        case .timedOut:
            if address.scheme == "https" {
                return "\(address.displayName) did not answer in time. Check the tailnet name, and that the computer is still serving the room with `tailscale serve`."
            }
            return "\(address.displayName) did not answer in time. Check the address, and that the room was started with --allow-network."
        case .notConnectedToInternet:
            return "This phone has no network connection."
        default:
            // -1022: App Transport Security refused the plain-HTTP load.
            if error.code.rawValue == -1022 {
                return "iOS blocked the plain-HTTP connection to \(address.displayName). This build should allow local networking; please report it."
            }
            return "Could not reach \(address.displayName): \(error.localizedDescription)"
        }
    }
}
