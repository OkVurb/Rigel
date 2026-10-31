import SwiftUI
import RigelRoomKit

/// The room's standing orders: what every agent is told before it starts.
///
/// Read-only on purpose. Activating a rule changes what every agent does next,
/// for every job, until somebody turns it off — that is a decision the desktop
/// makes, where the proposal and the agent it came from are both on screen.
struct RulesView: View {
    @Environment(RoomModel.self) private var model
    @State private var appeared = false

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 20) {
                header
                if model.rules.isEmpty {
                    emptyState
                } else {
                    ForEach(model.rulesByStatus) { group in
                        section(group.status, rules: group.rules)
                    }
                    footer
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 10)
            .padding(.bottom, 26)
        }
        .scrollIndicators(.hidden)
        .onAppear {
            withAnimation(.spring(response: 0.6, dampingFraction: 0.86)) { appeared = true }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            Eyebrow("standing orders")
            Text("Rules")
                .font(Type.display(30, .bold))
                .foregroundStyle(Theme.wordmark)

            let active = model.rules.filter(\.isActive).count
            let pending = model.rules.filter { $0.status == "pending" }.count
            HStack(spacing: 8) {
                stat("\(active)", "in effect", Theme.online)
                stat("\(pending)", "waiting", Theme.accent)
                Spacer(minLength: 0)
            }
        }
    }

    private func stat(_ value: String, _ label: String, _ tint: Color) -> some View {
        HStack(spacing: 7) {
            Text(value)
                .font(Type.display(16, .bold))
                .foregroundStyle(tint)
            Text(label)
                .font(Type.label(10, .medium))
                .foregroundStyle(Theme.inkFaint)
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 7)
        .background(Theme.panel.opacity(0.6), in: Capsule())
        .overlay(Capsule().strokeBorder(tint.opacity(0.22)))
    }

    private func section(_ status: String, rules: [RoomRule]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Circle()
                    .fill(tint(for: status))
                    .frame(width: 7, height: 7)
                    .shadow(color: tint(for: status).opacity(0.7), radius: 5)
                Text(status.uppercased())
                    .font(Type.label(10.5, .bold))
                    .tracking(1.6)
                    .foregroundStyle(Theme.inkMuted)
                Rectangle().fill(Theme.hairline).frame(height: 1)
            }

            ForEach(rules) { rule in
                RuleCard(rule: rule)
                    .opacity(appeared ? 1 : 0)
                    .offset(y: appeared ? 0 : 14)
                    .animation(.spring(response: 0.55, dampingFraction: 0.86), value: appeared)
            }
        }
    }

    private func tint(for status: String) -> Color {
        switch status {
        case "active": return Theme.online
        case "pending": return Theme.accent
        case "archived": return Theme.inkFaint
        default: return Theme.violet
        }
    }

    private var footer: some View {
        GlassCard(radius: 18) {
            VStack(alignment: .leading, spacing: 6) {
                Eyebrow("why this is read-only")
                Text(
                    "Active rules are injected into every agent's context before it starts a turn, "
                        + "for every job. Proposing, activating and archiving one stay on the desktop, "
                        + "where the proposal and the agent that made it are both in view."
                )
                .font(Type.body(12.5))
                .foregroundStyle(Theme.inkFaint)
                .fixedSize(horizontal: false, vertical: true)
            }
            .padding(15)
        }
    }

    private var emptyState: some View {
        VStack(alignment: .center, spacing: 12) {
            StarMark(size: 42, animated: false)
                .opacity(0.7)
            Text("No standing orders")
                .font(Type.display(17, .semibold))
                .foregroundStyle(Theme.ink)
            Text(
                "When an agent suggests something worth remembering — a test to run, a file not to "
                    + "touch — the room files it here as a proposal, and the desktop decides whether "
                    + "every agent should be told it from now on."
            )
            .font(Type.body(13))
            .foregroundStyle(Theme.inkFaint)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 36)
        .padding(.horizontal, 12)
    }
}

private struct RuleCard: View {
    let rule: RoomRule

    private var tint: Color {
        if rule.isActive { return Theme.online }
        switch rule.status {
        case "pending": return Theme.accent
        case "archived": return Theme.inkFaint
        default: return Theme.violet
        }
    }

    var body: some View {
        GlassCard(radius: 18, glow: rule.isActive ? Theme.online.opacity(0.22) : nil) {
            VStack(alignment: .leading, spacing: 9) {
                HStack(spacing: 8) {
                    HStack(spacing: 5) {
                        Circle().fill(tint).frame(width: 5, height: 5)
                        Text(rule.status.uppercased())
                            .font(Type.label(9, .bold))
                            .tracking(1.1)
                            .foregroundStyle(tint)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(tint.opacity(0.12), in: Capsule())

                    if !rule.author.isEmpty {
                        Text("from \(rule.author)")
                            .font(Type.label(9.5))
                            .foregroundStyle(Theme.inkFaint)
                    }
                    Spacer(minLength: 0)
                }

                Text(rule.text)
                    .font(Type.body(14.5, rule.isActive ? .medium : .regular))
                    .foregroundStyle(rule.isActive ? Theme.ink : Theme.inkMuted)
                    .fixedSize(horizontal: false, vertical: true)

                if !rule.reason.isEmpty {
                    Text(rule.reason)
                        .font(Type.body(12))
                        .foregroundStyle(Theme.inkFaint)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(15)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
