import SwiftUI

/// Room details, channel switching, and the two ways out — reconnect, and
/// forget entirely (which clears the keychain entry too).
struct RoomSettingsSheet: View {
    @Environment(RoomModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var showingCloud = false

    var body: some View {
        ZStack {
            AuroraBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    header
                    details
                    channelSection
                    cloudSection
                    actions
                    about
                }
                .padding(22)
                .padding(.bottom, 20)
            }
            .scrollIndicators(.hidden)
        }
        .presentationBackground(Theme.background)
        .presentationDragIndicator(.visible)
        .sheet(isPresented: $showingCloud) {
            CloudSessionsView()
        }
    }

    private var header: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                Eyebrow("connected")
                Text("Room")
                    .font(Type.display(28, .bold))
                    .foregroundStyle(Theme.ink)
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

    private var details: some View {
        GlassCard(radius: 20) {
            VStack(alignment: .leading, spacing: 12) {
                row("Address", model.connectedAddress)
                if !model.alternateText.isEmpty {
                    row("Also reachable at", model.alternateText)
                }
                row("Agents online", String(model.agentCount))
                if !model.roomSubtitle.isEmpty {
                    row("Note", model.roomSubtitle)
                }
            }
            .padding(16)
        }
    }

    private func row(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Eyebrow(label)
            Text(value)
                .font(Type.body(13.5))
                .foregroundStyle(Theme.ink)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var channelSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            Eyebrow("channel")
            FlowRow(spacing: 8) {
                ForEach(model.channels, id: \.self) { name in
                    Button {
                        model.select(channel: name)
                    } label: {
                        SelectChip(title: "#\(name)", selected: name == model.channel)
                    }
                    .buttonStyle(PressableStyle())
                }
            }
        }
    }

    /// The cloud does not need the room: it is here as well so a session
    /// published earlier is reachable without disconnecting first.
    private var cloudSection: some View {
        Button {
            showingCloud = true
        } label: {
            HStack(spacing: 11) {
                Image(systemName: "cloud")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.cool)
                    .frame(width: 30, height: 30)
                    .background(Theme.cool.opacity(0.12), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                VStack(alignment: .leading, spacing: 2) {
                    Text("Published sessions")
                        .font(Type.body(14, .semibold))
                        .foregroundStyle(Theme.ink)
                    Text("Transcripts sent to your Firebase project")
                        .font(Type.body(11))
                        .foregroundStyle(Theme.inkFaint)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(Theme.inkFaint)
            }
            .padding(13)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.panel.opacity(0.55), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .strokeBorder(Theme.panelEdge)
            )
        }
        .buttonStyle(PressableStyle())
    }

    private var actions: some View {
        VStack(spacing: 10) {
            Button {
                Task { await model.reconnect() }
                dismiss()
            } label: {
                Label("Reconnect", systemImage: "arrow.clockwise")
                    .font(Type.body(15, .semibold))
                    .foregroundStyle(Theme.background)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 14)
                    .background(Theme.accentGradient, in: Capsule())
            }
            .buttonStyle(PressableStyle())

            HStack(spacing: 10) {
                Button {
                    model.disconnect()
                    dismiss()
                } label: {
                    Text("Disconnect")
                        .font(Type.body(14, .medium))
                        .foregroundStyle(Theme.inkMuted)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 13)
                        .background(Theme.panel.opacity(0.6), in: Capsule())
                        .overlay(Capsule().strokeBorder(Theme.panelEdge))
                }
                .buttonStyle(PressableStyle())

                Button {
                    model.forget()
                    dismiss()
                } label: {
                    Text("Forget")
                        .font(Type.body(14, .medium))
                        .foregroundStyle(Theme.problem)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 13)
                        .background(Theme.problem.opacity(0.10), in: Capsule())
                        .overlay(Capsule().strokeBorder(Theme.problem.opacity(0.25)))
                }
                .buttonStyle(PressableStyle())
            }
        }
    }

    private var about: some View {
        GlassCard(radius: 18) {
            VStack(alignment: .leading, spacing: 7) {
                Eyebrow("about")
                Text(
                    "Rigel 0.3.0 for iPhone — the room on your computer, on your phone: the timeline, "
                        + "the board and the rule book, live. It can say one line at a time, answer a "
                        + "decision card and put photos or files into the room, each write confirmed "
                        + "first. Starting agents, moving jobs and activating rules stay on the desktop."
                )
                .font(Type.body(12))
                .foregroundStyle(Theme.inkFaint)
                .fixedSize(horizontal: false, vertical: true)
            }
            .padding(15)
        }
    }
}

/// A wrapping row of chips — SwiftUI has no flow layout of its own and a
/// horizontal scroller hides options off the edge.
struct FlowRow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var widest: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                y += rowHeight + spacing
                x = 0
                rowHeight = 0
            }
            x += size.width + spacing
            widest = max(widest, x - spacing)
            rowHeight = max(rowHeight, size.height)
        }
        return CGSize(width: proposal.width ?? widest, height: y + rowHeight)
    }

    func placeSubviews(
        in bounds: CGRect,
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX {
                y += rowHeight + spacing
                x = bounds.minX
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), anchor: .topLeading, proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}
