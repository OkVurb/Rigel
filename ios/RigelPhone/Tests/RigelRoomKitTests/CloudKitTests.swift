import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import XCTest

@testable import RigelRoomKit

/// The phone's half of Rigel Cloud: the Firestore shapes `rigel cloud publish`
/// writes, and the sentences Firebase's error codes become.
final class CloudKitTests: XCTestCase {

    private let project = FirebaseProject(
        id: "rigel-cloud-test",
        apiKey: "web-key"
    )!

    func testAProjectNeedsAnIdAndAKey() {
        XCTAssertNil(FirebaseProject(id: " ", apiKey: "key"))
        XCTAssertNil(FirebaseProject(id: "rigel/cloud", apiKey: "key"))
        XCTAssertNil(FirebaseProject(id: "rigel", apiKey: ""))
        XCTAssertEqual(project.id, "rigel-cloud-test")
    }

    func testAProjectRemembersItsAppCheckEndpoint() throws {
        let url = try XCTUnwrap(URL(string: "https://example.com/app-check"))
        let project = try XCTUnwrap(
            FirebaseProject(id: "rigel-cloud-test", apiKey: "web-key", appCheckURL: url)
        )
        XCTAssertEqual(project.appCheckURL, url)
        XCTAssertNotEqual(project, self.project)
        XCTAssertNil(
            FirebaseProject(
                id: "rigel-cloud-test",
                apiKey: "web-key",
                appCheckURL: URL(string: "ftp://example.com/app-check")
            )
        )
    }

    /// The token is fetched once, reused until it is close to expiry, and refreshed before it
    /// runs out. The fake clock keeps the test off the wall clock.
    func testAppCheckTokensAreCachedAndRefreshedBeforeExpiry() async throws {
        let clock = TestClock(Date(timeIntervalSince1970: 1_760_961_600))
        let calls = TestCounter()
        let endpoint = try XCTUnwrap(URL(string: "https://example.com/app-check"))
        let tokens = AppCheckTokens(
            url: endpoint,
            now: { clock.now },
            fetch: { _ in
                let call = calls.bump()
                let expires = Int(clock.now.timeIntervalSince1970) + 3_600
                let body = #"{"token":"app-check-\#(call)","expires_at":\#(expires)}"#
                let response = try XCTUnwrap(
                    HTTPURLResponse(
                        url: endpoint,
                        statusCode: 200,
                        httpVersion: nil,
                        headerFields: nil
                    )
                )
                return (Data(body.utf8), response)
            }
        )

        let first = try await tokens.token()
        XCTAssertEqual(first, "app-check-1")
        let second = try await tokens.token()
        XCTAssertEqual(second, "app-check-1")
        XCTAssertEqual(calls.value, 1, "two reads share one cached token")

        clock.now = clock.now.addingTimeInterval(3_600 - 30)
        let refreshed = try await tokens.token()
        XCTAssertEqual(refreshed, "app-check-2")
        XCTAssertEqual(calls.value, 2, "a token inside the margin is not reused")
    }

    func testABrokenAppCheckEndpointIsASentenceNotAToken() async throws {
        let endpoint = try XCTUnwrap(URL(string: "https://example.com/app-check"))
        let tokens = AppCheckTokens(
            url: endpoint,
            now: { Date(timeIntervalSince1970: 1_760_961_600) },
            fetch: { _ in
                let response = try XCTUnwrap(
                    HTTPURLResponse(
                        url: endpoint,
                        statusCode: 200,
                        httpVersion: nil,
                        headerFields: nil
                    )
                )
                return (Data(#"{"token":""}"#.utf8), response)
            }
        )

        do {
            _ = try await tokens.token()
            XCTFail("an empty token must not be returned")
        } catch let error as CloudError {
            XCTAssertTrue(
                error.errorDescription?.contains("App Check") == true,
                "\(error.errorDescription ?? "")"
            )
        }
    }

    func testASessionDocumentDecodesFromFirestoresValueWrappers() throws {
        let document: [String: Any] = [
            "name": "projects/p/databases/(default)/documents/users/uid/sessions/s-1",
            "fields": [
                "id": ["stringValue": "s-1"],
                "title": ["stringValue": "Fix the build"],
                "project": ["stringValue": "rigel"],
                "status": ["stringValue": "idle"],
                "created_at": ["integerValue": "1760961600000"],
                "ended_at": ["nullValue": NSNull()],
                "spend_micros": ["integerValue": "32000"],
                "turn_count": ["integerValue": "4"],
                "agent_count": ["integerValue": "2"],
            ],
        ]

        let session = try XCTUnwrap(Firestore.session(document))
        XCTAssertEqual(session.id, "s-1")
        XCTAssertEqual(session.title, "Fix the build")
        XCTAssertEqual(session.project, "rigel")
        XCTAssertEqual(session.status, "idle")
        XCTAssertEqual(session.createdAt, 1_760_961_600_000)
        XCTAssertNil(session.endedAt)
        XCTAssertEqual(session.spendMicros, 32_000)
        XCTAssertEqual(session.turnCount, 4)
        XCTAssertEqual(session.agentCount, 2)
        XCTAssertEqual(session.created.timeIntervalSince1970, 1_760_961_600)
    }

    func testATurnCarriesTheConversationAndSaysWhetherItWasClipped() throws {
        let document: [String: Any] = [
            "fields": [
                "id": ["stringValue": "t-1"],
                "order": ["integerValue": "3"],
                "prompt": ["stringValue": "why is the build slow?"],
                "reply": ["stringValue": "It compiles twice."],
                "role": ["stringValue": "implementer"],
                "harness": ["stringValue": "native"],
                "model": ["stringValue": "gpt-5.6"],
                "started_at": ["integerValue": "1760961600000"],
                "cost_micros": ["integerValue": "7500"],
                "subagent": ["booleanValue": false],
                "truncated": ["booleanValue": true],
            ],
        ]

        let turn = try XCTUnwrap(Firestore.turn(document))
        XCTAssertEqual(turn.id, "t-1")
        XCTAssertEqual(turn.order, 3)
        XCTAssertEqual(turn.prompt, "why is the build slow?")
        XCTAssertEqual(turn.reply, "It compiles twice.")
        XCTAssertEqual(turn.role, "implementer")
        XCTAssertEqual(turn.model, "gpt-5.6")
        XCTAssertEqual(turn.costMicros, 7_500)
        XCTAssertFalse(turn.subagent)
        XCTAssertTrue(turn.truncated)
        XCTAssertNil(turn.completedAt)
    }

    /// A document with no id is not guessed at: it is skipped, the same way
    /// the desktop skips an unreadable row.
    func testADocumentWithoutAnIdIsSkipped() {
        XCTAssertNil(Firestore.session(["fields": ["title": ["stringValue": "x"]]]))
        XCTAssertNil(Firestore.turn(["fields": ["prompt": ["stringValue": "x"]]]))
    }

    func testFirebaseErrorsBecomeSentences() {
        func message(_ code: String) -> String {
            let body = Data(#"{"error":{"message":"\#(code)"}}"#.utf8)
            return FirebaseAuth.message(from: body)
        }

        XCTAssertEqual(message("EMAIL_NOT_FOUND"), "the email or password is wrong")
        XCTAssertEqual(
            message("INVALID_LOGIN_CREDENTIALS"),
            "the email or password is wrong"
        )
        XCTAssertEqual(message("TOO_MANY_ATTEMPTS_TRY_LATER"), "too many attempts — wait a minute and try again")
        XCTAssertEqual(
            message("PERMISSION_DENIED"),
            "permission denied: deploy firestore.rules to this project"
        )
        XCTAssertEqual(
            message("WEAK_PASSWORD : Password should be at least 6 characters"),
            "Firebase needs a password of at least 6 characters"
        )
        XCTAssertEqual(
            message("App Check token is invalid."),
            "App Check rejected this request: check the attestation endpoint and the project's settings (see docs/cloud.md)"
        )
        XCTAssertEqual(FirebaseAuth.message(from: Data("not json".utf8)), "")
    }

    /// The query the phone sends: one orderBy per collection, both indexed by
    /// default, and no composite index to deploy by hand.
    func testListURLsOrderByTheFieldTheDesktopWrites() throws {
        let client = CloudSessions(project: project)
        let account = FirebaseAccount(
            uid: "uid-1",
            email: "ada@example.com",
            projectID: project.id,
            idToken: "id-1",
            refreshToken: "refresh-1",
            expiresAt: Date().addingTimeInterval(3_600)
        )

        let sessions = client.sessionsURL(account: account, limit: 50)
        XCTAssertEqual(
            sessions?.absoluteString,
            "https://firestore.googleapis.com/v1/projects/rigel-cloud-test/databases/(default)/documents/users/uid-1/sessions?pageSize=50&orderBy=created_at%20desc"
        )

        let turns = client.turnsURL(sessionID: "s-1", account: account, limit: 300)
        XCTAssertEqual(
            turns?.absoluteString,
            "https://firestore.googleapis.com/v1/projects/rigel-cloud-test/databases/(default)/documents/users/uid-1/sessions/s-1/turns?pageSize=300&orderBy=order"
        )
        XCTAssertNil(client.turnsURL(sessionID: "a/b", account: account, limit: 300))
    }

    func testAnotherProjectTokenIsNotSent() {
        let client = CloudSessions(project: project)
        let foreign = FirebaseAccount(
            uid: "uid-1",
            email: "ada@example.com",
            projectID: "some-other-project",
            idToken: "id-1",
            refreshToken: "refresh-1",
            expiresAt: Date().addingTimeInterval(3_600)
        )
        XCTAssertNil(client.sessionsURL(account: foreign, limit: 50))
    }
}

/// A clock the test can move. `@unchecked Sendable` because the lock, not the compiler, keeps
/// the value safe through the provider's closures.
private final class TestClock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Date

    init(_ value: Date) {
        self.value = value
    }

    var now: Date {
        get {
            lock.lock()
            defer { lock.unlock() }
            return value
        }
        set {
            lock.lock()
            defer { lock.unlock() }
            value = newValue
        }
    }
}

private final class TestCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0

    /// The number this call is, starting at one.
    func bump() -> Int {
        lock.lock()
        defer { lock.unlock() }
        count += 1
        return count
    }

    var value: Int {
        lock.lock()
        defer { lock.unlock() }
        return count
    }
}
