import Foundation
import Observation
import RigelRoomKit

/// The phone's Rigel Cloud account: the same Firebase project the desktop
/// publishes to, and the sessions it can read.
///
/// Only two things are kept between launches: the project id and public web API
/// key in `UserDefaults` (neither is a secret), and the Firebase refresh token in
/// the keychain. The password is used for one sign-in and never stored.
@MainActor
@Observable
final class CloudModel {
    var projectText: String = ""
    var keyText: String = ""
    /// The custom App Check provider's endpoint, when the project enforces App Check.
    var appCheckText: String = ""
    var emailText: String = ""
    var passwordText: String = ""

    private(set) var account: FirebaseAccount?
    private(set) var sessions: [CloudSessionSummary] = []
    private(set) var selected: CloudSessionSummary?
    private(set) var turns: [CloudTurn] = []
    private(set) var loading = false
    private(set) var error: String?

    private var didRestore = false
    private static let projectKey = "rigel.phone.cloud.project"
    private static let apiKeyKey = "rigel.phone.cloud.api-key"
    private static let appCheckURLKey = "rigel.phone.cloud.app-check-url"
    private static let accountKey = "firebase-session"

    var isSignedIn: Bool { account != nil }

    /// A project and account that survived a relaunch. The refresh token is
    /// exchanged for a fresh ID token here, so the session list can be fetched
    /// without asking for the password again.
    func restore() async {
        guard !didRestore else { return }
        didRestore = true
        let defaults = UserDefaults.standard
        projectText = defaults.string(forKey: Self.projectKey) ?? ""
        keyText = defaults.string(forKey: Self.apiKeyKey) ?? ""
        appCheckText = defaults.string(forKey: Self.appCheckURLKey) ?? ""
        guard let text = KeychainStore.read(account: Self.accountKey),
              let data = text.data(using: .utf8),
              let stored = try? JSONDecoder().decode(StoredAccount.self, from: data)
        else { return }
        account = stored.account
        guard let project = FirebaseProject(id: projectText, apiKey: keyText),
              project.id == stored.account.projectID
        else {
            account = nil
            return
        }
        if !stored.account.isFresh {
            do {
                let fresh = try await FirebaseAuth(project: project).refresh(stored.account)
                account = fresh
                keep(fresh)
            } catch {
                // A revoked or rotated-away token means a fresh sign-in, not a
                // screen full of errors at launch.
                account = nil
                KeychainStore.delete(account: Self.accountKey)
                return
            }
        }
        await loadSessions()
    }

    /// Save the project, sign in, and list what is published. Signing in again
    /// is also how a wrong project id is corrected.
    func signIn() async {
        let appCheckURL = Self.appCheckURL(from: appCheckText)
        if !appCheckText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
           appCheckURL == nil
        {
            error = "The App Check URL must start with http:// or https://."
            return
        }
        guard let project = FirebaseProject(
            id: projectText,
            apiKey: keyText,
            appCheckURL: appCheckURL
        ) else {
            error = "Enter the Firebase project id and its web API key."
            return
        }
        let email = emailText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !email.isEmpty, !passwordText.isEmpty else {
            error = "Enter the email address and password for the account."
            return
        }
        loading = true
        error = nil
        defer { loading = false }
        do {
            let signedIn = try await FirebaseAuth(project: project).signIn(
                email: email,
                password: passwordText
            )
            passwordText = ""
            account = signedIn
            keep(signedIn)
            UserDefaults.standard.set(project.id, forKey: Self.projectKey)
            UserDefaults.standard.set(project.apiKey, forKey: Self.apiKeyKey)
            UserDefaults.standard.set(
                appCheckText.trimmingCharacters(in: .whitespacesAndNewlines),
                forKey: Self.appCheckURLKey
            )
            sessions = try await CloudSessions(project: project).sessions(for: signedIn)
        } catch {
            self.error = Self.sentence(error)
        }
    }

    func signOut() {
        account = nil
        sessions = []
        selected = nil
        turns = []
        error = nil
        passwordText = ""
        KeychainStore.delete(account: Self.accountKey)
    }

    /// Pull the list again. A no-op while signed out or already busy.
    func loadSessions() async {
        guard let account, let project = currentProject else { return }
        guard !loading else { return }
        loading = true
        error = nil
        defer { loading = false }
        do {
            let fresh = try await refreshed(account, project: project)
            sessions = try await CloudSessions(project: project).sessions(for: fresh)
        } catch {
            self.error = Self.sentence(error)
        }
    }

    func open(_ session: CloudSessionSummary) async {
        guard let project = currentProject else { return }
        selected = session
        turns = []
        error = nil
        loading = true
        defer { loading = false }
        do {
            guard let account else { return }
            let fresh = try await refreshed(account, project: project)
            turns = try await CloudSessions(project: project).turns(
                for: session.id,
                account: fresh
            )
        } catch {
            self.error = Self.sentence(error)
        }
    }

    func closeSession() {
        selected = nil
        turns = []
        error = nil
    }

    private var currentProject: FirebaseProject? {
        FirebaseProject(
            id: projectText,
            apiKey: keyText,
            appCheckURL: Self.appCheckURL(from: appCheckText)
        )
    }

    private static func appCheckURL(from text: String) -> URL? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              let url = URL(string: trimmed),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https"
        else { return nil }
        return url
    }

    /// Firebase ID tokens live an hour; a phone opened the next day has to
    /// exchange its refresh token before the first request.
    private func refreshed(
        _ account: FirebaseAccount,
        project: FirebaseProject
    ) async throws -> FirebaseAccount {
        guard !account.isFresh else { return account }
        let fresh = try await FirebaseAuth(project: project).refresh(account)
        self.account = fresh
        keep(fresh)
        return fresh
    }

    private func keep(_ account: FirebaseAccount) {
        let stored = StoredAccount(account: account)
        guard let data = try? JSONEncoder().encode(stored),
              let text = String(data: data, encoding: .utf8)
        else { return }
        KeychainStore.save(text, account: Self.accountKey)
    }

    private static func sentence(_ error: Error) -> String {
        (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
    }
}

/// The keychain blob. The ID token is included so a relaunch within the hour
/// does not need a refresh; it is still short-lived and overwritten on every
/// refresh.
private struct StoredAccount: Codable {
    var uid: String
    var email: String
    var projectID: String
    var idToken: String
    var refreshToken: String
    var expiresAt: Date

    init(account: FirebaseAccount) {
        uid = account.uid
        email = account.email
        projectID = account.projectID
        idToken = account.idToken
        refreshToken = account.refreshToken
        expiresAt = account.expiresAt
    }

    var account: FirebaseAccount {
        FirebaseAccount(
            uid: uid,
            email: email,
            projectID: projectID,
            idToken: idToken,
            refreshToken: refreshToken,
            expiresAt: expiresAt
        )
    }
}
