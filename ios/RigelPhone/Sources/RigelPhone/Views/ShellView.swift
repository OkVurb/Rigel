import SwiftUI

/// The app once a room is live: three surfaces over one room, one floating
/// control between them.
///
/// A hand-built bar rather than `TabView` for two reasons: the timeline's
/// composer has to sit above it without a second stacked bar, and the bar is
/// part of the visual language — glass, amber pill, spring — rather than a
/// system chrome the rest of the app has to match.
struct ShellView: View {
    @Environment(RoomModel.self) private var model
    @State private var tab: Tab = .chat

    enum Tab: String, CaseIterable, Identifiable {
        case chat
        case board
        case rules

        var id: String { rawValue }

        var title: String {
            switch self {
            case .chat: return "Room"
            case .board: return "Board"
            case .rules: return "Rules"
            }
        }

        var icon: String {
            switch self {
            case .chat: return "bubble.left.and.text.bubble.right"
            case .board: return "square.stack.3d.up"
            case .rules: return "text.book.closed"
            }
        }
    }

    var body: some View {
        ZStack {
            AuroraBackground()
            content
                .id(tab)
                .transition(.opacity)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            bar
        }
        .sensoryFeedback(.selection, trigger: tab)
        .animation(.easeInOut(duration: 0.18), value: tab)
    }

    @ViewBuilder
    private var content: some View {
        switch tab {
        case .chat:
            RoomView()
        case .board:
            BoardView()
        case .rules:
            RulesView()
        }
    }

    private var openJobs: Int {
        model.jobs.filter { $0.lane == "open" }.count
    }

    private var bar: some View {
        HStack(spacing: 4) {
            ForEach(Tab.allCases) { item in
                Button {
                    tab = item
                } label: {
                    HStack(spacing: 7) {
                        Image(systemName: item.icon)
                            .font(.system(size: 14, weight: .semibold))
                        if tab == item {
                            Text(item.title)
                                .font(Type.body(13, .semibold))
                                .transition(.opacity)
                        }
                        if item == .board, openJobs > 0 {
                            Text("\(openJobs)")
                                .font(Type.label(10, .bold))
                                .padding(.horizontal, 5)
                                .padding(.vertical, 2)
                                .background(
                                    (tab == item ? Theme.background.opacity(0.18) : Theme.accent.opacity(0.18)),
                                    in: Capsule()
                                )
                        }
                    }
                    .foregroundStyle(tab == item ? Theme.background : Theme.inkMuted)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 11)
                    .background {
                        if tab == item {
                            Capsule()
                                .fill(Theme.accentGradient)
                                .shadow(color: Theme.accent.opacity(0.45), radius: 14, y: 5)
                        }
                    }
                    .contentShape(Capsule())
                }
                .buttonStyle(PressableStyle(scale: 0.95))
                .accessibilityLabel(item.title)
            }
        }
        .padding(6)
        .background {
            Capsule()
                .fill(.ultraThinMaterial)
                .overlay { Capsule().fill(Theme.panel.opacity(0.55)) }
        }
        .overlay {
            Capsule().strokeBorder(
                LinearGradient(
                    colors: [Color.white.opacity(0.18), Color.white.opacity(0.03)],
                    startPoint: .top,
                    endPoint: .bottom
                ),
                lineWidth: 1
            )
        }
        .shadow(color: .black.opacity(0.55), radius: 22, y: 12)
        .padding(.horizontal, 28)
        .padding(.top, 6)
        .padding(.bottom, 4)
    }
}
