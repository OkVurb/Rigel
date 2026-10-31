import SwiftUI
import RigelRoomKit

/// The room's board: every job, lane by lane.
///
/// Read-only by design. The room's own UI moves a job between lanes by
/// dragging it, and that writes `sort_order` for the whole lane — a gesture
/// that would be a guess about somebody's backlog when it comes from a phone.
/// What a phone is good at is answering the decision a job is blocked on, and
/// that happens on the card in the timeline.
struct BoardView: View {
    @Environment(RoomModel.self) private var model
    @State private var selected: RoomJob?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 20) {
                header
                if model.jobs.isEmpty {
                    emptyState
                } else {
                    ForEach(model.jobsByLane) { group in
                        lane(group.lane, jobs: group.jobs)
                            .transition(.opacity)
                    }
                    footer
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 10)
            .padding(.bottom, 26)
        }
        .scrollIndicators(.hidden)
        .sheet(item: $selected) { job in
            JobSheet(job: job)
        }
    }

    // MARK: - Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            Eyebrow("the room's work")
            Text("Board")
                .font(Type.display(30, .bold))
                .foregroundStyle(Theme.wordmark)

            HStack(spacing: 8) {
                ForEach(RoomJob.lanes, id: \.self) { lane in
                    let count = model.jobs.filter { $0.lane == lane }.count
                    HStack(spacing: 6) {
                        Circle()
                            .fill(Theme.laneTint(lane))
                            .frame(width: 6, height: 6)
                        Text("\(count) \(lane)")
                            .font(Type.label(10, .semibold))
                            .foregroundStyle(count == 0 ? Theme.inkFaint : Theme.inkMuted)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(Theme.panel.opacity(0.6), in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.panelEdge))
                }
                Spacer(minLength: 0)
            }
        }
    }

    // MARK: - Lanes

    private func lane(_ name: String, jobs: [RoomJob]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Circle()
                    .fill(Theme.laneTint(name))
                    .frame(width: 7, height: 7)
                    .shadow(color: Theme.laneTint(name).opacity(0.8), radius: 6)
                Text(name.uppercased())
                    .font(Type.label(10.5, .bold))
                    .tracking(1.6)
                    .foregroundStyle(Theme.inkMuted)
                Text(String(jobs.count))
                    .font(Type.label(9.5, .semibold))
                    .foregroundStyle(Theme.inkFaint)
                Rectangle()
                    .fill(Theme.hairline)
                    .frame(height: 1)
            }

            ForEach(jobs) { job in
                Button {
                    selected = job
                } label: {
                    JobCard(job: job)
                }
                .buttonStyle(PressableStyle(scale: 0.985))
            }
        }
    }

    private var footer: some View {
        GlassCard(radius: 18) {
            HStack(spacing: 10) {
                Image(systemName: "hand.point.up.left")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.inkFaint)
                Text("Moving a job and starting an agent stay on the desktop. Answering what a job is blocked on happens in the timeline.")
                    .font(Type.body(12))
                    .foregroundStyle(Theme.inkFaint)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(14)
        }
    }

    private var emptyState: some View {
        VStack(alignment: .center, spacing: 12) {
            StarMark(size: 42, animated: false)
                .opacity(0.7)
            Text("The board is empty")
                .font(Type.display(17, .semibold))
                .foregroundStyle(Theme.ink)
            Text(
                "A job is a bounded piece of work with its own thread — usually one an agent opened "
                    + "for itself, or one a proposal in the timeline turned into. They appear here the "
                    + "moment the room creates one."
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

/// One job in a lane: what it is, who owns it, and how much has been said.
struct JobCard: View {
    let job: RoomJob
    private var tint: Color { Theme.laneTint(job.lane) }

    var body: some View {
        GlassCard(radius: 18, glow: job.lane == "open" ? tint.opacity(0.35) : nil) {
            HStack(alignment: .center, spacing: 12) {
                RoundedRectangle(cornerRadius: 2, style: .continuous)
                    .fill(LinearGradient(colors: [tint, tint.opacity(0.35)], startPoint: .top, endPoint: .bottom))
                    .frame(width: 3)
                    .frame(maxHeight: .infinity)

                VStack(alignment: .leading, spacing: 7) {
                    Text(job.title)
                        .font(Type.body(15, .semibold))
                        .foregroundStyle(Theme.ink)
                        .multilineTextAlignment(.leading)
                        .fixedSize(horizontal: false, vertical: true)

                    HStack(spacing: 10) {
                        Text("#\(job.channel)")
                        if !job.assignee.isEmpty {
                            Label(job.assignee, systemImage: "person.fill")
                        }
                        if !job.messages.isEmpty {
                            Label(String(job.messages.count), systemImage: "text.bubble.fill")
                        }
                    }
                    .font(Type.label(9.5))
                    .foregroundStyle(Theme.inkFaint)
                    .labelStyle(.titleAndIcon)
                }

                Spacer(minLength: 0)

                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(Theme.inkFaint)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 13)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// One job, whole: the brief, who it belongs to, and its conversation.
struct JobSheet: View {
    let job: RoomJob
    @Environment(RoomModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack {
            AuroraBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    VStack(alignment: .leading, spacing: 8) {
                        Eyebrow("#\(job.channel) · job \(job.id)")
                        Text(job.title)
                            .font(Type.display(26, .bold))
                            .foregroundStyle(Theme.ink)
                            .fixedSize(horizontal: false, vertical: true)
                        HStack(spacing: 8) {
                            LaneChip(lane: job.lane)
                            if !job.assignee.isEmpty {
                                Text(job.assignee)
                                    .font(Type.label(10, .semibold))
                                    .foregroundStyle(Theme.inkMuted)
                            }
                        }
                    }

                    if !job.body.isEmpty {
                        GlassCard(radius: 18) {
                            Text(job.body)
                                .font(Type.body(14.5))
                                .foregroundStyle(Theme.ink.opacity(0.92))
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(15)
                        }
                    }

                    VStack(alignment: .leading, spacing: 10) {
                        Eyebrow("conversation")
                        if job.messages.isEmpty {
                            VStack(spacing: 9) {
                                StarMark(size: 30, animated: false)
                                    .opacity(0.55)
                                Text("Nothing said about this one yet.")
                                    .font(Type.body(12.5))
                                    .foregroundStyle(Theme.inkFaint)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 18)
                        }
                        ForEach(job.messages) { message in
                            messageRow(message)
                        }
                    }

                    if job.createdAt > 0 {
                        Eyebrow(
                            "opened \(Date(timeIntervalSince1970: job.createdAt).formatted(date: .abbreviated, time: .shortened))"
                        )
                    }
                }
                .padding(22)
                .padding(.bottom, 10)
            }
            .scrollIndicators(.hidden)
        }
        .presentationBackground(Theme.background)
        .presentationDragIndicator(.visible)
        .presentationDetents([.medium, .large])
        .overlay(alignment: .topTrailing) {
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(Theme.inkMuted)
                    .frame(width: 32, height: 32)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .buttonStyle(PressableStyle())
            .padding(16)
            .accessibilityLabel("Close")
        }
    }

    /// One line of a job's thread, in the timeline's voice: the speaker's own
    /// coin, its colour on the name, and a bubble tinted with it. The sheet
    /// used to draw flat grey boxes here, which read as a different app.
    private func messageRow(_ message: RoomJobMessage) -> some View {
        let agent = agent(speaking: message.sender)
        let tint = agent.map { Color(hex: $0.color) } ?? Theme.cool
        return HStack(alignment: .top, spacing: 9) {
            if let agent {
                AgentAvatar(agent: agent, size: 26)
            } else {
                ZStack {
                    Circle().fill(Theme.panelHigh)
                    Text(String(message.sender.prefix(1)).uppercased())
                        .font(Type.body(11, .bold))
                        .foregroundStyle(Theme.inkMuted)
                }
                .frame(width: 26, height: 26)
                .overlay(Circle().strokeBorder(Theme.panelEdge))
            }

            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 7) {
                    Text(message.sender)
                        .font(Type.label(10, .bold))
                        .tracking(0.7)
                        .foregroundStyle(tint)
                    Text(message.time)
                        .font(Type.label(9))
                        .foregroundStyle(Theme.inkFaint)
                }
                Text(message.text)
                    .font(Type.body(14))
                    .foregroundStyle(Theme.ink)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background {
                RoundedRectangle(cornerRadius: 15, style: .continuous)
                    .fill(
                        LinearGradient(
                            colors: [tint.opacity(0.12), Theme.panel.opacity(0.68)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
            }
            .overlay {
                RoundedRectangle(cornerRadius: 15, style: .continuous)
                    .strokeBorder(tint.opacity(0.22), lineWidth: 1)
            }
        }
    }

    private func agent(speaking sender: String) -> AgentPresence? {
        (model.status?.agents ?? []).first { $0.name == sender || $0.label == sender }
    }
}

/// A job's lane, as a chip.
struct LaneChip: View {
    let lane: String

    var body: some View {
        Text(lane.uppercased())
            .font(Type.label(9.5, .bold))
            .tracking(1.1)
            .foregroundStyle(tint)
            .padding(.horizontal, 9)
            .padding(.vertical, 4)
            .background(tint.opacity(0.14), in: Capsule())
            .overlay(Capsule().strokeBorder(tint.opacity(0.35)))
    }

    private var tint: Color { Theme.laneTint(lane) }
}
