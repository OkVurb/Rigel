import Foundation
import SwiftUI
import RigelRoomKit

/// Rigel Cloud on the phone: sign in to the same Firebase project the desktop
/// publishes to, list its sessions, read one transcript.
///
/// The screen has three states and no navigation stack, because it is presented
/// as a sheet from either the connect screen or the room's settings: the
/// account form, the session list, and one session's turns.
struct CloudSessionsView: View {
    @Environment(CloudModel.self) private var cloud
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack {
            AuroraBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    header
                    if cloud.selected != nil {
                        transcript
                    } else if cloud.isSignedIn {
                        list
                    } else {
                        form
                    }
                    if let problem = cloud.error {
                        NoticeBanner(title: problem)
                    }
                }
                .padding(22)
                .padding(.bottom, 24)
            }
            .scrollIndicators(.hidden)
        }
        .presentationBackground(Theme.background)
        .presentationDragIndicator(.visible)
        .task { await cloud.restore() }
    }

    private var header: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                Eyebrow("rigel cloud")
                Text(cloud.selected == nil ? "Sessions" : (cloud.selected?.title ?? "Session"))
                    .font(Type.display(28, .bold))
                    .foregroundStyle(Theme.ink)
                    .lineLimit(2)
                if let account = cloud.account {
                    Text(account.email)
                        .font(Type.body(12))
                        .foregroundStyle(Theme.inkFaint)
                }
            }
            Spacer()
            Button("Done") { dismiss() }
                .font(Type.body(14, .semibold))
                .foregroundStyle(Theme.accent)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(Theme.accent.opacity(0.12), in: Capsule())
                .buttonStyle(PressableStyle())
        }
    }

    /* ---------------------------------- sign in ---------------------------------- */

    private var form: some View {
        @Bindable var cloud = cloud
        return VStack(alignment: .leading, spacing: 16) {
            Text(
                "Sign in to the Firebase project your computer publishes to. The project id and "
                    + "web API key are under Firebase Project settings; the key is public by design. "
                    + "If the project enforces App Check, use the same attestation endpoint the "
                    + "desktop has."
            )
            .font(Type.body(12.5))
            .foregroundStyle(Theme.inkMuted)
            .fixedSize(horizontal: false, vertical: true)

            GlassCard(radius: 20) {
                VStack(alignment: .leading, spacing: 14) {
                    field("Project id", text: $cloud.projectText, prompt: "rigel-cloud-4f2a1")
                    field("Web API key", text: $cloud.keyText, prompt: "AIza…")
                    field(
                        "App Check URL (optional)",
                        text: $cloud.appCheckText,
                        prompt: "https://…/app-check"
                    )
                    FadingRule()
                    emailField($cloud.emailText)
                    secureField("Password", text: $cloud.passwordText)
                }
                .padding(16)
            }

            Button {
                Task { await cloud.signIn() }
            } label: {
                HStack(spacing: 9) {
                    if cloud.loading {
                        ProgressView().controlSize(.small).tint(Theme.background)
                    } else {
                        Image(systemName: "cloud")
                            .font(.system(size: 13, weight: .bold))
                    }
                    Text(cloud.loading ? "Signing in…" : "Sign in and load")
                }
            }
            .buttonStyle(PrimaryButtonStyle())
            .disabled(cloud.loading)

            HStack(alignment: .top, spacing: 7) {
                Image(systemName: "lock")
                    .font(.system(size: 10, weight: .semibold))
                    .padding(.top, 1)
                Text(
                    "The password is used once and not stored. The refresh token goes to the "
                        + "keychain, and Firestore's rules only let this account read its own tree."
                )
                .font(Type.body(11.5))
                .fixedSize(horizontal: false, vertical: true)
            }
            .foregroundStyle(Theme.inkFaint)
        }
    }

    private func field(_ label: String, text: Binding<String>, prompt: String) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Eyebrow(label)
            TextField(prompt, text: text)
                .plainRoomEntry()
                .font(Type.body(14))
                .foregroundStyle(Theme.ink)
                .tint(Theme.accent)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(Theme.background.opacity(0.55), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(Theme.panelEdge)
                )
        }
    }

    private func emailField(_ text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Eyebrow("Email")
            TextField("you@example.com", text: text)
                .emailEntry()
                .font(Type.body(14))
                .foregroundStyle(Theme.ink)
                .tint(Theme.accent)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(Theme.background.opacity(0.55), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(Theme.panelEdge)
                )
        }
    }

    private func secureField(_ label: String, text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Eyebrow(label)
            SecureField("••••••••", text: text)
                .font(Type.body(14))
                .foregroundStyle(Theme.ink)
                .tint(Theme.accent)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(Theme.background.opacity(0.55), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(Theme.panelEdge)
                )
        }
    }

    /* ----------------------------------- list ----------------------------------- */

    private var list: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Button {
                    Task { await cloud.loadSessions() }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                        .font(Type.body(12.5, .semibold))
                        .foregroundStyle(Theme.inkMuted)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(Theme.panel.opacity(0.6), in: Capsule())
                        .overlay(Capsule().strokeBorder(Theme.panelEdge))
                }
                .buttonStyle(PressableStyle())
                .disabled(cloud.loading)

                Spacer()

                Button("Sign out") { cloud.signOut() }
                    .font(Type.body(12.5, .semibold))
                    .foregroundStyle(Theme.problem)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(Theme.problem.opacity(0.10), in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.problem.opacity(0.25)))
                    .buttonStyle(PressableStyle())
            }

            if cloud.loading && cloud.sessions.isEmpty {
                HStack {
                    Spacer()
                    ProgressView().tint(Theme.accent)
                    Spacer()
                }
                .padding(.vertical, 24)
            } else if cloud.sessions.isEmpty {
                GlassCard(radius: 20) {
                    VStack(alignment: .leading, spacing: 8) {
                        Eyebrow("nothing yet")
                        Text(
                            "No session has been published from this account. On the computer, open "
                                + "a session and press “Publish to phone”."
                        )
                        .font(Type.body(12.5))
                        .foregroundStyle(Theme.inkMuted)
                        .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(16)
                }
            } else {
                ForEach(cloud.sessions) { session in
                    Button {
                        Task { await cloud.open(session) }
                    } label: {
                        sessionRow(session)
                    }
                    .buttonStyle(PressableStyle())
                }
            }
        }
    }

    private func sessionRow(_ session: CloudSessionSummary) -> some View {
        GlassCard(radius: 18) {
            VStack(alignment: .leading, spacing: 7) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(session.title)
                        .font(Type.body(15, .semibold))
                        .foregroundStyle(Theme.ink)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                    Spacer(minLength: 0)
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundStyle(Theme.inkFaint)
                }
                HStack(spacing: 7) {
                    Text(session.project.isEmpty ? "unknown project" : session.project)
                        .font(Type.label(9.5, .semibold))
                        .foregroundStyle(Theme.cool)
                    Text("·")
                        .foregroundStyle(Theme.inkFaint)
                    Text(session.created.formatted(date: .abbreviated, time: .shortened))
                        .font(Type.label(9.5))
                        .foregroundStyle(Theme.inkFaint)
                }
                Text(
                    "\(session.turnCount) turn\(session.turnCount == 1 ? "" : "s") · "
                        + "\(money(session.spendMicros)) · "
                        + "\(session.agentCount) agent\(session.agentCount == 1 ? "" : "s")"
                )
                .font(Type.label(9.5))
                .foregroundStyle(Theme.inkFaint)
            }
            .padding(15)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /* -------------------------------- transcript -------------------------------- */

    private var transcript: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                Button {
                    cloud.closeSession()
                } label: {
                    Label("All sessions", systemImage: "chevron.left")
                        .font(Type.body(12.5, .semibold))
                        .foregroundStyle(Theme.accent)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(Theme.accent.opacity(0.12), in: Capsule())
                }
                .buttonStyle(PressableStyle())
                Spacer()
            }

            if let session = cloud.selected {
                HStack(spacing: 7) {
                    Text(session.project.isEmpty ? "unknown project" : session.project)
                        .font(Type.label(9.5, .semibold))
                        .foregroundStyle(Theme.cool)
                    Text("·")
                        .foregroundStyle(Theme.inkFaint)
                    Text(session.created.formatted(date: .abbreviated, time: .shortened))
                        .font(Type.label(9.5))
                        .foregroundStyle(Theme.inkFaint)
                }
            }

            if cloud.loading && cloud.turns.isEmpty {
                HStack {
                    Spacer()
                    ProgressView().tint(Theme.accent)
                    Spacer()
                }
                .padding(.vertical, 24)
            } else if cloud.turns.isEmpty {
                Text("This session has no published turns.")
                    .font(Type.body(12.5))
                    .foregroundStyle(Theme.inkMuted)
            } else {
                ForEach(cloud.turns) { turn in
                    CloudTurnCard(turn: turn)
                }
                HStack(alignment: .top, spacing: 7) {
                    Image(systemName: "scissors")
                        .font(.system(size: 10, weight: .semibold))
                        .padding(.top, 1)
                    Text(
                        "Only the prompt and the assistant's own words are published. Tool calls, "
                            + "command output and file changes stay on the computer."
                    )
                    .font(Type.body(11))
                    .fixedSize(horizontal: false, vertical: true)
                }
                .foregroundStyle(Theme.inkFaint)
            }
        }
    }

    private func money(_ micros: Int64) -> String {
        String(format: "$%.4f", Double(micros) / 1_000_000)
    }
}

extension View {
    /// Email entry for the cloud form: no autocapitalisation or smart quotes,
    /// and the keyboard that carries `@`.
    @ViewBuilder
    func emailEntry() -> some View {
        #if os(iOS)
        self
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .keyboardType(.emailAddress)
            .textContentType(.emailAddress)
        #else
        self.autocorrectionDisabled()
        #endif
    }
}

/// One published exchanged: what was asked, and what the assistant said.
private struct CloudTurnCard: View {
    let turn: CloudTurn

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if !turn.prompt.isEmpty {
                HStack {
                    Spacer(minLength: 28)
                    VStack(alignment: .leading, spacing: 5) {
                        Eyebrow("you", tint: Theme.accent)
                        Text(turn.prompt)
                            .font(Type.body(14))
                            .foregroundStyle(Theme.ink)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.horizontal, 13)
                    .padding(.vertical, 10)
                    .background(
                        LinearGradient(
                            colors: [Theme.accent.opacity(0.24), Theme.accent.opacity(0.09)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        ),
                        in: RoundedRectangle(cornerRadius: 16, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .strokeBorder(Theme.accent.opacity(0.35))
                    )
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    Eyebrow(turn.role ?? turn.harness ?? "assistant", tint: Theme.cool)
                    Spacer(minLength: 0)
                    if turn.subagent {
                        Text("subagent")
                            .font(Type.label(8.5, .semibold))
                            .foregroundStyle(Theme.violet)
                    }
                    Text(turn.started.formatted(date: .omitted, time: .shortened))
                        .font(Type.label(8.5))
                        .foregroundStyle(Theme.inkFaint)
                }

                if turn.reply.isEmpty {
                    Text("No reply was published for this turn.")
                        .font(Type.body(12.5))
                        .italic()
                        .foregroundStyle(Theme.inkFaint)
                } else {
                    Text(turn.reply)
                        .font(Type.body(14.5))
                        .foregroundStyle(Theme.ink)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }

                HStack(spacing: 7) {
                    if let model = turn.model, !model.isEmpty {
                        Text(model)
                    }
                    if turn.costMicros > 0 {
                        Text(String(format: "$%.4f", Double(turn.costMicros) / 1_000_000))
                    }
                    if turn.truncated {
                        Text("clipped for the phone")
                            .foregroundStyle(Theme.accent)
                    }
                }
                .font(Type.label(8.5))
                .foregroundStyle(Theme.inkFaint)
            }
            .padding(13)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.panel.opacity(0.58), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .strokeBorder(Theme.panelEdge)
            )
        }
    }
}
