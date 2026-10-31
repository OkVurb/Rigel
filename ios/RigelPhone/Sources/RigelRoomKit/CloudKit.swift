import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// The Firebase project sessions were published to: the same id and public web
/// API key the desktop's Settings → Cloud holds.
///
/// The key identifies the project; it does not authorise a read. The Firestore
/// rules, which require `request.auth.uid == uid`, decide who may see a
/// document.
public struct FirebaseProject: Equatable, Sendable {
    public let id: String
    public let apiKey: String
    /// The same attestation endpoint the desktop uses, when the project enforces App Check.
    public let appCheckURL: URL?
    let appCheck: AppCheckTokens?
    public let identityBase: URL
    public let tokenBase: URL
    public let firestoreBase: URL

    public init?(
        id: String,
        apiKey: String,
        appCheckURL: URL? = nil,
        identityBase: URL = URL(string: "https://identitytoolkit.googleapis.com")!,
        tokenBase: URL = URL(string: "https://securetoken.googleapis.com")!,
        firestoreBase: URL = URL(string: "https://firestore.googleapis.com")!
    ) {
        let id = id.trimmingCharacters(in: .whitespacesAndNewlines)
        let apiKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !id.isEmpty, !apiKey.isEmpty, !id.contains("/") else { return nil }
        if let appCheckURL {
            let scheme = appCheckURL.scheme?.lowercased() ?? ""
            guard scheme == "http" || scheme == "https" else { return nil }
        }
        self.id = id
        self.apiKey = apiKey
        self.appCheckURL = appCheckURL
        self.appCheck = appCheckURL.map { AppCheckTokens(url: $0) }
        self.identityBase = identityBase
        self.tokenBase = tokenBase
        self.firestoreBase = firestoreBase
    }

    public static func == (lhs: FirebaseProject, rhs: FirebaseProject) -> Bool {
        lhs.id == rhs.id
            && lhs.apiKey == rhs.apiKey
            && lhs.appCheckURL == rhs.appCheckURL
            && lhs.identityBase == rhs.identityBase
            && lhs.tokenBase == rhs.tokenBase
            && lhs.firestoreBase == rhs.firestoreBase
    }
}

/// A signed-in Firebase account. The refresh token is the thing worth keeping;
/// the ID token is short-lived and re-derived from it at launch.
public struct FirebaseAccount: Equatable, Sendable {
    public let uid: String
    public let email: String
    public let projectID: String
    public let idToken: String
    public let refreshToken: String
    public let expiresAt: Date

    public init(
        uid: String,
        email: String,
        projectID: String,
        idToken: String,
        refreshToken: String,
        expiresAt: Date
    ) {
        self.uid = uid
        self.email = email
        self.projectID = projectID
        self.idToken = idToken
        self.refreshToken = refreshToken
        self.expiresAt = expiresAt
    }

    /// A token this close to expiry is not worth sending.
    public var isFresh: Bool {
        expiresAt.timeIntervalSinceNow > 30
    }
}

/// The published sessions the phone can list, from `users/{uid}/sessions`.
public struct CloudSessionSummary: Identifiable, Equatable, Sendable {
    public let id: String
    public let title: String
    public let project: String
    public let status: String
    public let createdAt: Int64
    public let endedAt: Int64?
    public let spendMicros: Int64
    public let turnCount: Int
    public let agentCount: Int

    public init(
        id: String,
        title: String,
        project: String,
        status: String,
        createdAt: Int64,
        endedAt: Int64?,
        spendMicros: Int64,
        turnCount: Int,
        agentCount: Int
    ) {
        self.id = id
        self.title = title
        self.project = project
        self.status = status
        self.createdAt = createdAt
        self.endedAt = endedAt
        self.spendMicros = spendMicros
        self.turnCount = turnCount
        self.agentCount = agentCount
    }

    public var created: Date { Date(timeIntervalSince1970: Double(createdAt) / 1_000) }
}

/// One published turn: the prompt a person typed and the assistant's own words.
/// Tool calls, file changes and command output are never part of this shape.
public struct CloudTurn: Identifiable, Equatable, Sendable {
    public let id: String
    public let order: Int
    public let prompt: String
    public let reply: String
    public let role: String?
    public let harness: String?
    public let model: String?
    public let startedAt: Int64
    public let completedAt: Int64?
    public let costMicros: Int64
    public let subagent: Bool
    public let truncated: Bool

    public init(
        id: String,
        order: Int,
        prompt: String,
        reply: String,
        role: String?,
        harness: String?,
        model: String?,
        startedAt: Int64,
        completedAt: Int64?,
        costMicros: Int64,
        subagent: Bool,
        truncated: Bool
    ) {
        self.id = id
        self.order = order
        self.prompt = prompt
        self.reply = reply
        self.role = role
        self.harness = harness
        self.model = model
        self.startedAt = startedAt
        self.completedAt = completedAt
        self.costMicros = costMicros
        self.subagent = subagent
        self.truncated = truncated
    }

    public var started: Date { Date(timeIntervalSince1970: Double(startedAt) / 1_000) }
}

/// A Firebase answer the screens can show without translating JSON themselves.
public enum CloudError: LocalizedError, Equatable {
    case badProject
    case signedOut
    case badResponse(String)
    case firebase(String)
    case http(Int, String)
    case appCheck(String)

    public var errorDescription: String? {
        switch self {
        case .badProject:
            return "That project id or web API key does not look right."
        case .signedOut:
            return "Sign in to read the published sessions."
        case .badResponse(let detail):
            return "Firebase answered something unexpected: \(detail)"
        case .firebase(let message):
            return message
        case .http(let status, let message):
            return message.isEmpty ? "Firebase answered HTTP \(status)." : message
        case .appCheck(let detail):
            return "App Check could not attest this phone: \(detail)"
        }
    }
}

/// The phone's half of a custom App Check provider: one short-lived token from the endpoint the
/// desktop also uses, cached until shortly before it expires.
///
/// The endpoint answers `{"token": "…", "expires_at": <unix seconds>}`. Concurrent callers share
/// one request, so a burst of Firestore reads does not stampede it. Tokens stay in memory only.
public actor AppCheckTokens {
    private struct Cached: Sendable {
        let token: String
        let expiresAt: Date
    }

    private let url: URL
    private let fetch: @Sendable (URL) async throws -> (Data, HTTPURLResponse)
    private let now: @Sendable () -> Date
    private var cached: Cached?
    private var inFlight: Task<Cached, Error>?

    public init(url: URL, session: URLSession = .shared) {
        self.init(url: url, now: { Date() }) { url in
            let (data, response) = try await session.data(from: url)
            guard let http = response as? HTTPURLResponse else {
                throw CloudError.appCheck("the endpoint gave no HTTP response")
            }
            return (data, http)
        }
    }

    /// For the tests: a clock and a fetch that do not touch the network.
    init(
        url: URL,
        now: @escaping @Sendable () -> Date,
        fetch: @escaping @Sendable (URL) async throws -> (Data, HTTPURLResponse)
    ) {
        self.url = url
        self.now = now
        self.fetch = fetch
    }

    /// A token good for at least another minute.
    public func token() async throws -> String {
        if let cached, cached.expiresAt.timeIntervalSince(now()) > 60 {
            return cached.token
        }
        if let inFlight {
            return try await inFlight.value.token
        }
        let task = Task { try await self.fetchToken() }
        inFlight = task
        do {
            let fresh = try await task.value
            cached = fresh
            inFlight = nil
            return fresh.token
        } catch {
            inFlight = nil
            throw error
        }
    }

    private func fetchToken() async throws -> Cached {
        let (data, response) = try await fetch(url)
        guard (200..<300).contains(response.statusCode) else {
            throw CloudError.appCheck("the attestation endpoint answered HTTP \(response.statusCode)")
        }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let token = object["token"] as? String,
              !token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let seconds = object["expires_at"] as? NSNumber
        else {
            throw CloudError.appCheck(
                "the endpoint must answer with a token and expires_at"
            )
        }
        let expiresAt = Date(timeIntervalSince1970: seconds.doubleValue)
        guard expiresAt > now() else {
            throw CloudError.appCheck("the endpoint returned a token that has already expired")
        }
        guard expiresAt.timeIntervalSince(now()) <= 7 * 24 * 60 * 60 else {
            throw CloudError.appCheck("expires_at must be Unix seconds, not milliseconds")
        }
        return Cached(token: token, expiresAt: expiresAt)
    }
}

/// Attach the project's App Check token to a Firebase request, when App Check is configured.
private func authorizeWithAppCheck(
    _ request: inout URLRequest,
    project: FirebaseProject
) async throws {
    guard let appCheck = project.appCheck else { return }
    request.setValue(try await appCheck.token(), forHTTPHeaderField: "X-Firebase-AppCheck")
}

/// Email/password sign-in and the refresh the phone needs on every launch.
///
/// The password is used once and not stored; only the refresh token is.
public struct FirebaseAuth: Sendable {
    public let project: FirebaseProject
    private let session: URLSession

    public init(project: FirebaseProject, session: URLSession = .shared) {
        self.project = project
        self.session = session
    }

    public func signIn(email: String, password: String) async throws -> FirebaseAccount {
        let email = email.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !email.isEmpty, !password.isEmpty else {
            throw CloudError.firebase("an email address and password are required")
        }
        var request = URLRequest(url: project.identityBase.appendingPathComponent("v1/accounts:signInWithPassword"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = try JSONSerialization.data(withJSONObject: [
            "email": email,
            "password": password,
            "returnSecureToken": true,
        ])
        request.url = request.url.flatMap { url in
            var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
            components?.queryItems = [URLQueryItem(name: "key", value: project.apiKey)]
            return components?.url
        }

        let data = try await send(request)
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let uid = object["localId"] as? String,
              let idToken = object["idToken"] as? String,
              let refreshToken = object["refreshToken"] as? String
        else {
            throw CloudError.badResponse("sign-in returned no token")
        }
        let seconds = Double((object["expiresIn"] as? String) ?? "") ?? 3_600
        return FirebaseAccount(
            uid: uid,
            email: (object["email"] as? String) ?? email,
            projectID: project.id,
            idToken: idToken,
            refreshToken: refreshToken,
            expiresAt: Date().addingTimeInterval(seconds)
        )
    }

    /// Exchange the stored refresh token for a fresh ID token. Firebase rotates
    /// the refresh token, so the caller has to keep the new one.
    public func refresh(_ account: FirebaseAccount) async throws -> FirebaseAccount {
        var components = URLComponents(
            url: project.tokenBase.appendingPathComponent("v1/token"),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = [URLQueryItem(name: "key", value: project.apiKey)]
        guard let url = components?.url else {
            throw CloudError.badProject
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "content-type")
        let body = [
            "grant_type=refresh_token",
            "refresh_token=\(percentEncoded(account.refreshToken))",
        ].joined(separator: "&")
        request.httpBody = Data(body.utf8)

        let data = try await send(request)
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let uid = object["user_id"] as? String,
              let idToken = object["id_token"] as? String,
              let refreshToken = object["refresh_token"] as? String
        else {
            throw CloudError.badResponse("refresh returned no token")
        }
        let seconds = Double((object["expires_in"] as? String) ?? "") ?? 3_600
        return FirebaseAccount(
            uid: uid,
            email: account.email,
            projectID: project.id,
            idToken: idToken,
            refreshToken: refreshToken,
            expiresAt: Date().addingTimeInterval(seconds)
        )
    }

    private func send(_ request: URLRequest) async throws -> Data {
        var request = request
        try await authorizeWithAppCheck(&request, project: project)
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw CloudError.http(0, error.localizedDescription)
        }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            throw CloudError.http(status, Self.message(from: data))
        }
        return data
    }

    /// Firebase's error envelope, turned into the sentence the desktop uses.
    static func message(from data: Data) -> String {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let error = object["error"] as? [String: Any],
              let code = error["message"] as? String
        else {
            return ""
        }
        if code.lowercased().contains("app check") {
            return "App Check rejected this request: check the attestation endpoint and the project's settings (see docs/cloud.md)"
        }
        switch code.split(separator: " ").first.map(String.init) ?? code {
        case "EMAIL_NOT_FOUND", "INVALID_PASSWORD", "INVALID_LOGIN_CREDENTIALS":
            return "the email or password is wrong"
        case "EMAIL_EXISTS":
            return "that email already has an account"
        case "USER_DISABLED":
            return "that account is disabled"
        case "TOO_MANY_ATTEMPTS_TRY_LATER":
            return "too many attempts — wait a minute and try again"
        case "OPERATION_NOT_ALLOWED":
            return "email/password sign-in is disabled in this Firebase project"
        case "WEAK_PASSWORD":
            return "Firebase needs a password of at least 6 characters"
        case "PERMISSION_DENIED":
            return "permission denied: deploy firestore.rules to this project"
        case "NOT_FOUND":
            return "the session was not published, or the project id is wrong"
        default:
            return code
        }
    }
}

/// The read half: list the account's published sessions and one session's turns.
///
/// The documents are the ones `rigel cloud publish` writes; the desktop's
/// `docs/cloud.md` is the contract. A missing document is an empty list, not an
/// error — a new account has published nothing yet.
public struct CloudSessions: Sendable {
    public let project: FirebaseProject
    private let session: URLSession

    public init(project: FirebaseProject, session: URLSession = .shared) {
        self.project = project
        self.session = session
    }

    public func sessions(for account: FirebaseAccount, limit: Int = 50) async throws -> [CloudSessionSummary] {
        let documents = try await list(
            path: "users/\(account.uid)/sessions",
            orderBy: "created_at desc",
            limit: limit,
            account: account
        )
        return documents.compactMap(Firestore.session)
            .sorted { $0.createdAt > $1.createdAt }
    }

    public func turns(for sessionID: String, account: FirebaseAccount, limit: Int = 300) async throws -> [CloudTurn] {
        guard !sessionID.isEmpty, !sessionID.contains("/") else {
            throw CloudError.badResponse("a session id cannot contain a slash")
        }
        var turns: [CloudTurn] = []
        var pageToken: String?
        var pages = 0
        repeat {
            let page = try await turnsPage(
                sessionID: sessionID,
                account: account,
                limit: limit,
                pageToken: pageToken
            )
            turns.append(contentsOf: page.documents.compactMap(Firestore.turn))
            pageToken = page.nextPageToken
            pages += 1
        } while pageToken != nil && pages < 5
        return turns.sorted { ($0.order, $0.id) < ($1.order, $1.id) }
    }

    private func list(
        path: String,
        orderBy: String,
        limit: Int,
        account: FirebaseAccount
    ) async throws -> [[String: Any]] {
        var pageToken: String?
        var documents: [[String: Any]] = []
        var pages = 0
        repeat {
            let page = try await page(
                path: path,
                orderBy: orderBy,
                limit: limit,
                pageToken: pageToken,
                account: account
            )
            documents.append(contentsOf: page.documents)
            pageToken = page.nextPageToken
            pages += 1
        } while pageToken != nil && pages < 5
        return documents
    }

    private func turnsPage(
        sessionID: String,
        account: FirebaseAccount,
        limit: Int,
        pageToken: String?
    ) async throws -> (documents: [[String: Any]], nextPageToken: String?) {
        try await page(
            path: "users/\(account.uid)/sessions/\(sessionID)/turns",
            orderBy: "order",
            limit: limit,
            pageToken: pageToken,
            account: account
        )
    }

    /// The sessions list a phone refreshes on pull. Internal for the URL test.
    func sessionsURL(account: FirebaseAccount, limit: Int, pageToken: String? = nil) -> URL? {
        guard account.projectID == project.id else { return nil }
        return pageURL(
            path: "users/\(account.uid)/sessions",
            orderBy: "created_at desc",
            limit: limit,
            pageToken: pageToken
        )
    }

    /// One session's turns, in publication order.
    func turnsURL(
        sessionID: String,
        account: FirebaseAccount,
        limit: Int,
        pageToken: String? = nil
    ) -> URL? {
        guard account.projectID == project.id, !sessionID.isEmpty, !sessionID.contains("/") else {
            return nil
        }
        return pageURL(
            path: "users/\(account.uid)/sessions/\(sessionID)/turns",
            orderBy: "order",
            limit: limit,
            pageToken: pageToken
        )
    }

    private func pageURL(
        path: String,
        orderBy: String,
        limit: Int,
        pageToken: String?
    ) -> URL? {
        let base = project.firestoreBase.appendingPathComponent(
            "v1/projects/\(project.id)/databases/(default)/documents/\(path)"
        )
        guard var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else {
            return nil
        }
        var items = [
            URLQueryItem(name: "pageSize", value: String(limit)),
            URLQueryItem(name: "orderBy", value: orderBy),
        ]
        if let pageToken {
            items.append(URLQueryItem(name: "pageToken", value: pageToken))
        }
        components.queryItems = items
        return components.url
    }

    private func page(
        path: String,
        orderBy: String,
        limit: Int,
        pageToken: String?,
        account: FirebaseAccount
    ) async throws -> (documents: [[String: Any]], nextPageToken: String?) {
        guard account.projectID == project.id else {
            throw CloudError.signedOut
        }
        guard let url = pageURL(path: path, orderBy: orderBy, limit: limit, pageToken: pageToken) else {
            throw CloudError.badProject
        }

        var request = URLRequest(url: url)
        request.setValue("Bearer \(account.idToken)", forHTTPHeaderField: "authorization")
        try await authorizeWithAppCheck(&request, project: project)
        let (data, response) = try await session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            throw CloudError.http(status, FirebaseAuth.message(from: data))
        }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw CloudError.badResponse("the session list was not JSON")
        }
        let documents = object["documents"] as? [[String: Any]] ?? []
        return (documents, object["nextPageToken"] as? String)
    }
}

/// Firestore's typed wrapper, flattened just enough for the two documents this
/// client reads. Every value is `{"stringValue": …}` or similar; a missing field
/// is simply absent.
enum Firestore {
    static func session(_ document: [String: Any]) -> CloudSessionSummary? {
        let fields = document["fields"] as? [String: Any] ?? [:]
        guard let id = string(fields, "id"), let title = string(fields, "title") else {
            return nil
        }
        return CloudSessionSummary(
            id: id,
            title: title,
            project: string(fields, "project") ?? "",
            status: string(fields, "status") ?? "idle",
            createdAt: int(fields, "created_at") ?? 0,
            endedAt: int(fields, "ended_at"),
            spendMicros: int(fields, "spend_micros") ?? 0,
            turnCount: Int(int(fields, "turn_count") ?? 0),
            agentCount: Int(int(fields, "agent_count") ?? 0)
        )
    }

    static func turn(_ document: [String: Any]) -> CloudTurn? {
        let fields = document["fields"] as? [String: Any] ?? [:]
        guard let id = string(fields, "id") else { return nil }
        return CloudTurn(
            id: id,
            order: Int(int(fields, "order") ?? 0),
            prompt: string(fields, "prompt") ?? "",
            reply: string(fields, "reply") ?? "",
            role: string(fields, "role"),
            harness: string(fields, "harness"),
            model: string(fields, "model"),
            startedAt: int(fields, "started_at") ?? 0,
            completedAt: int(fields, "completed_at"),
            costMicros: int(fields, "cost_micros") ?? 0,
            subagent: bool(fields, "subagent") ?? false,
            truncated: bool(fields, "truncated") ?? false
        )
    }

    static func string(_ fields: [String: Any], _ key: String) -> String? {
        (fields[key] as? [String: Any])?["stringValue"] as? String
    }

    static func int(_ fields: [String: Any], _ key: String) -> Int64? {
        guard let value = (fields[key] as? [String: Any])?["integerValue"] else { return nil }
        if let text = value as? String {
            return Int64(text)
        }
        if let number = value as? NSNumber {
            return number.int64Value
        }
        return nil
    }

    static func bool(_ fields: [String: Any], _ key: String) -> Bool? {
        (fields[key] as? [String: Any])?["booleanValue"] as? Bool
    }
}

/// `application/x-www-form-urlencoded` for the one refresh request. The refresh
/// token is base64url-ish, but `+`, `/` and `=` are still possible in a base64
/// token and would change its meaning in a body.
private func percentEncoded(_ value: String) -> String {
    var allowed = CharacterSet.alphanumerics
    allowed.insert(charactersIn: "-._~")
    return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
}
