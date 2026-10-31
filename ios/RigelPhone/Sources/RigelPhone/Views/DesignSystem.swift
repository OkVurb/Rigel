import SwiftUI
import RigelRoomKit

/// The app's one background: a cold night sky with one warm light in it.
///
/// It is drawn rather than shipped as an image on purpose — it scales to every
/// phone, costs no bundle bytes, and a slow drift keeps the screen from
/// feeling like a static screenshot.
struct AuroraBackground: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var drift: CGFloat = 0

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [Theme.midnight, Theme.background, Color.black],
                startPoint: .top,
                endPoint: .bottom
            )
            RadialGradient(
                colors: [Theme.accent.opacity(0.20), Theme.accent.opacity(0.05), .clear],
                center: UnitPoint(x: 0.84, y: 0.06 + drift * 0.03),
                startRadius: 8,
                endRadius: 460
            )
            RadialGradient(
                colors: [Theme.cool.opacity(0.14), .clear],
                center: UnitPoint(x: 0.08, y: 0.66 - drift * 0.03),
                startRadius: 4,
                endRadius: 400
            )
            RadialGradient(
                colors: [Theme.violet.opacity(0.07), .clear],
                center: UnitPoint(x: 0.55, y: 1.02),
                startRadius: 4,
                endRadius: 320
            )
            StarField()
        }
        .ignoresSafeArea()
        .onAppear {
            // A slow drift, unless the phone has asked for still images.
            guard !reduceMotion else { return }
            withAnimation(.easeInOut(duration: 16).repeatForever(autoreverses: true)) {
                drift = 1
            }
        }
    }
}

/// Fifty-odd faint stars, fixed by a seed so the sky is the same sky on every
/// launch and every screenshot.
private struct StarField: View {
    private static let stars: [(x: CGFloat, y: CGFloat, r: CGFloat, alpha: Double)] = {
        var seed: UInt64 = 0x5EED_1B17
        func next() -> CGFloat {
            seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
            return CGFloat((seed >> 33) % 1_000) / 1_000
        }
        return (0..<58).map { _ in
            let x = next()
            let y = next() * 0.98
            let radius = 0.7 + next() * 1.5
            let alpha = 0.08 + Double(next()) * 0.30
            return (x, y, radius, alpha)
        }
    }()

    var body: some View {
        Canvas { context, size in
            for star in Self.stars {
                let rect = CGRect(x: star.x * size.width, y: star.y * size.height, width: star.r * 2, height: star.r * 2)
                context.fill(Path(ellipseIn: rect), with: .color(.white.opacity(star.alpha)))
            }
        }
        .blendMode(.screen)
        .allowsHitTesting(false)
    }
}

/// The star from Rigel's logo, exactly: four points on the axes with concave
/// quadratic edges, traced from the desktop's `ui/src/mark.tsx` so the phone's
/// mark and the desktop's are one glyph rather than two drawings that agree.
struct StarShape: Shape {
    func path(in rect: CGRect) -> Path {
        let unit = min(rect.width, rect.height) / 24
        let originX = rect.midX - 12 * unit
        let originY = rect.midY - 12 * unit

        func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: originX + x * unit, y: originY + y * unit)
        }

        var path = Path()
        path.move(to: point(12, 4))
        path.addQuadCurve(to: point(20, 12), control: point(12.9, 11.1))
        path.addQuadCurve(to: point(12, 20), control: point(12.9, 12.9))
        path.addQuadCurve(to: point(4, 12), control: point(11.1, 12.9))
        path.addQuadCurve(to: point(12, 4), control: point(11.1, 11.1))
        path.closeSubpath()
        return path
    }
}

/// The logo's star on transparency — no plate, no ring, just the glyph in the
/// brand amber over a soft bloom, so it sits on the aurora rather than in a
/// box. `glow` is the resting brightness; `ChargeStar` turns it up while the
/// star gathers power.
struct StarMark: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var size: CGFloat = 72
    var animated: Bool = true
    var glow: Double = 1
    @State private var breathe = false

    var body: some View {
        ZStack {
            Circle()
                .fill(
                    RadialGradient(
                        colors: [
                            Theme.accent.opacity(0.28 * glow),
                            Theme.accent.opacity(0.06 * glow),
                            .clear,
                        ],
                        center: .center,
                        startRadius: 0,
                        endRadius: size * 0.95
                    )
                )
                .frame(width: size * 1.7, height: size * 1.7)
                .scaleEffect(breathe ? 1.04 : 0.96)

            StarShape()
                .fill(Theme.accent)
                .frame(width: size, height: size)
                .shadow(color: Theme.accent.opacity(0.40 * glow), radius: size * 0.16)
                .shadow(color: Theme.accent.opacity(0.15 * glow), radius: size * 0.40)
        }
        .frame(width: size * 1.7, height: size * 1.7)
        .onAppear {
            guard animated, !reduceMotion else { return }
            withAnimation(.easeInOut(duration: 4.2).repeatForever(autoreverses: true)) {
                breathe = true
            }
        }
        .accessibilityHidden(true)
    }
}

/// The star as a control: hold it and it draws power — the glow climbs, a ring
/// closes around the glyph, the star swells — then it bursts and opens the
/// room's settings. A tap still opens them directly, so the gesture is a
/// flourish and not the only way in.
struct ChargeStar: View {
    var size: CGFloat = 26
    var hold: Double = 0.95
    let action: () -> Void

    @State private var charge: Double = 0
    @State private var firing = false
    @State private var burst: Double = 0
    @State private var chargeTask: Task<Void, Never>?
    @State private var burstTask: Task<Void, Never>?
    @State private var pressedAt: Date?

    var body: some View {
        ZStack {
            if burst > 0 {
                BurstRings(progress: burst)
                    .frame(width: size * 2.4, height: size * 2.4)
                    .allowsHitTesting(false)
            }

            StarMark(size: size, animated: false, glow: 0.55 + charge * 1.8)
                .scaleEffect(firing ? 0.25 : 1 + charge * 0.22)
                .opacity(firing ? 0 : 1)

            Circle()
                .trim(from: 0, to: charge)
                .stroke(
                    Theme.accentGradient,
                    style: StrokeStyle(lineWidth: max(1.4, size * 0.06), lineCap: .round)
                )
                .rotationEffect(.degrees(-90))
                .frame(width: size * 1.62, height: size * 1.62)
                .opacity(firing ? 0 : min(1, charge * 1.6))
        }
        .frame(width: size * 1.75, height: size * 1.75)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in press() }
                .onEnded { _ in release() }
        )
        .sensoryFeedback(.impact(weight: .heavy), trigger: firing)
        .sensoryFeedback(.impact(weight: .light), trigger: charge > 0.5)
        .accessibilityElement()
        .accessibilityLabel("Room settings")
        .accessibilityHint("Opens the room's settings. Hold to charge the star first.")
        .accessibilityAddTraits(.isButton)
        .accessibilityAction { action() }
        .onDisappear {
            chargeTask?.cancel()
            burstTask?.cancel()
        }
    }

    private func press() {
        guard chargeTask == nil, !firing else { return }
        pressedAt = Date()
        charge = 0
        burst = 0
        firing = false
        chargeTask = Task { @MainActor in
            let steps = max(24, Int(hold * 60))
            for step in 1...steps {
                if Task.isCancelled { return }
                try? await Task.sleep(nanoseconds: 16_666_666)
                if Task.isCancelled { return }
                charge = min(1, Double(step) / Double(steps))
            }
            fire()
        }
    }

    private func release() {
        let held = Date().timeIntervalSince(pressedAt ?? Date())
        pressedAt = nil
        guard !firing else { return }
        if charge >= 1 { return }
        chargeTask?.cancel()
        chargeTask = nil
        withAnimation(.spring(response: 0.4, dampingFraction: 0.72)) { charge = 0 }
        if held < 0.22 { action() }
    }

    private func fire() {
        chargeTask = nil
        guard !firing else { return }
        withAnimation(.easeOut(duration: 0.18)) { firing = true }
        withAnimation(.easeOut(duration: 0.55)) { burst = 1 }
        burstTask = Task { @MainActor in
            try? await Task.sleep(nanoseconds: 360_000_000)
            action()
            try? await Task.sleep(nanoseconds: 320_000_000)
            withAnimation(.easeOut(duration: 0.25)) {
                firing = false
                burst = 0
                charge = 0
            }
        }
    }
}

/// The little shockwave the charged star leaves behind: two rings and a ring
/// of sparks, all fading as they fly outward.
private struct BurstRings: View {
    var progress: Double

    var body: some View {
        ZStack {
            Circle()
                .strokeBorder(Theme.accentBright.opacity((1 - progress) * 0.75), lineWidth: 1.5)
                .scaleEffect(0.35 + progress * 1.05)
            Circle()
                .strokeBorder(Theme.accent.opacity((1 - progress) * 0.40), lineWidth: 1)
                .scaleEffect(0.2 + progress * 1.55)
            ForEach(0..<10, id: \.self) { index in
                Capsule()
                    .fill(Theme.accentBright.opacity((1 - progress) * 0.85))
                    .frame(width: 1.6, height: 8)
                    .offset(y: -14 - progress * 22)
                    .rotationEffect(.degrees(Double(index) * 36))
                    .scaleEffect(1 - progress * 0.35)
            }
        }
    }
}

/// A translucent panel with a light-catching edge — the one surface every
/// screen puts its content on.
struct GlassCard<Content: View>: View {
    var radius: CGFloat = 22
    var glow: Color? = nil
    @ViewBuilder var content: Content

    var body: some View {
        content
            .background {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .fill(.ultraThinMaterial)
                    .overlay {
                        RoundedRectangle(cornerRadius: radius, style: .continuous)
                            .fill(Theme.panel.opacity(0.55))
                    }
            }
            .overlay {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(
                        LinearGradient(
                            colors: [Color.white.opacity(0.16), Color.white.opacity(0.03)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        ),
                        lineWidth: 1
                    )
            }
            .shadow(color: .black.opacity(0.45), radius: 18, y: 10)
            .shadow(color: (glow ?? .clear).opacity(0.28), radius: 22)
    }
}

/// A small mono label: the voice every eyebrow, count and timestamp uses.
struct Eyebrow: View {
    private let text: String
    private let tint: Color
    private let size: CGFloat

    init(_ text: String, tint: Color = Theme.inkFaint, size: CGFloat = 10.5) {
        self.text = text
        self.tint = tint
        self.size = size
    }

    var body: some View {
        Text(text.uppercased())
            .font(Type.label(size))
            .tracking(1.5)
            .foregroundStyle(tint)
    }
}

/// The live dot, with the pulse that makes "live" mean something.
struct PulseDot: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var color: Color = Theme.online
    @State private var pulse = false

    var body: some View {
        ZStack {
            Circle()
                .fill(color.opacity(0.30))
                .frame(width: 16, height: 16)
                .scaleEffect(pulse ? 1.25 : 0.55)
                .opacity(pulse ? 0.05 : 0.8)
            Circle()
                .fill(color)
                .frame(width: 7, height: 7)
                .shadow(color: color.opacity(0.9), radius: 6)
        }
        .frame(width: 16, height: 16)
        .onAppear {
            guard !reduceMotion else { return }
            withAnimation(.easeOut(duration: 1.9).repeatForever(autoreverses: false)) {
                pulse = true
            }
        }
        .accessibilityHidden(true)
    }
}

/// Press feedback used by every tappable card and chip.
struct PressableStyle: ButtonStyle {
    var scale: CGFloat = 0.97
    var dim: Double = 0.9

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? scale : 1)
            .opacity(configuration.isPressed ? dim : 1)
            .animation(.spring(response: 0.26, dampingFraction: 0.72), value: configuration.isPressed)
    }
}

/// The one filled button: amber gradient, dark label, a warm shadow.
struct PrimaryButtonStyle: ButtonStyle {
    var height: CGFloat = 52

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Type.body(16, .semibold))
            .foregroundStyle(Theme.background)
            .frame(maxWidth: .infinity, minHeight: height)
            .background(Theme.accentGradient, in: Capsule())
            .shadow(color: Theme.accent.opacity(configuration.isPressed ? 0.20 : 0.42), radius: 18, y: 8)
            .scaleEffect(configuration.isPressed ? 0.975 : 1)
            .animation(.spring(response: 0.3, dampingFraction: 0.7), value: configuration.isPressed)
    }
}

/// An agent, as a coin: its own colour, its first letter, a ring when it is in.
struct AgentAvatar: View {
    let agent: AgentPresence
    var size: CGFloat = 30

    var body: some View {
        ZStack {
            Circle()
                .fill(
                    LinearGradient(
                        colors: [Color(hex: agent.color).opacity(0.95), Color(hex: agent.color).opacity(0.55)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
            Text(initial)
                .font(Type.body(size * 0.42, .bold))
                .foregroundStyle(Theme.background.opacity(0.85))
        }
        .frame(width: size, height: size)
        .overlay {
            Circle().strokeBorder(
                agent.available ? Color(hex: agent.color).opacity(0.95) : Color.white.opacity(0.10),
                lineWidth: 1.5
            )
        }
        .overlay {
            if agent.busy {
                Circle().strokeBorder(Theme.accentBright.opacity(0.9), lineWidth: 1.5)
                    .rotationEffect(.degrees(-40))
                    .mask(alignment: .top) { Rectangle().frame(height: size / 2) }
            }
        }
        .shadow(color: Color(hex: agent.color).opacity(agent.available ? 0.45 : 0), radius: 8)
        .opacity(agent.available ? 1 : 0.45)
        .accessibilityLabel("\(agent.label), \(agent.available ? (agent.busy ? "busy" : "online") : "offline")")
    }

    private var initial: String {
        let name = agent.label.isEmpty ? agent.name : agent.label
        return String(name.prefix(1)).uppercased()
    }
}

/// A pill that can be selected: channels, lanes, filters.
struct SelectChip: View {
    let title: String
    var selected: Bool
    var tint: Color = Theme.accent
    var leading: String? = nil

    var body: some View {
        HStack(spacing: 6) {
            if let leading {
                Image(systemName: leading).font(.system(size: 11, weight: .semibold))
            }
            Text(title)
                .font(Type.body(13, selected ? .semibold : .medium))
                .lineLimit(1)
        }
        .foregroundStyle(selected ? Theme.background : Theme.inkMuted)
        .padding(.horizontal, 13)
        .padding(.vertical, 8)
        .background {
            if selected {
                Capsule().fill(tint)
            } else {
                Capsule().fill(Theme.panel.opacity(0.75))
            }
        }
        .overlay {
            Capsule().strokeBorder(selected ? Color.clear : Theme.panelEdge, lineWidth: 1)
        }
        .contentShape(Capsule())
    }
}

/// A hairline that fades at both ends, used to separate sections without a box.
struct FadingRule: View {
    var body: some View {
        LinearGradient(
            colors: [.clear, Color.white.opacity(0.12), .clear],
            startPoint: .leading,
            endPoint: .trailing
        )
        .frame(height: 1)
    }
}

/// A problem, said in one voice everywhere.
///
/// A refused address, a dropped stream and a line that would not send are the
/// same kind of thing to the person holding the phone, so they get the same
/// surface: a tinted glass strip with an icon, an optional second line, and
/// nothing that looks tappable. Before this, each screen grew its own version
/// and two of them were bare coloured text.
struct NoticeBanner: View {
    var tint: Color = Theme.problem
    var icon: String = "exclamationmark.triangle.fill"
    let title: String
    var detail: String? = nil

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(tint.opacity(0.18))
                Image(systemName: icon)
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(tint)
            }
            .frame(width: 24, height: 24)

            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(Type.body(12.5, .semibold))
                    .foregroundStyle(tint)
                    .fixedSize(horizontal: false, vertical: true)
                if let detail {
                    Text(detail)
                        .font(Type.body(11.5))
                        .foregroundStyle(Theme.inkMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .background {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(.ultraThinMaterial)
                .overlay {
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .fill(tint.opacity(0.08))
                }
        }
        .overlay {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(tint.opacity(0.28), lineWidth: 1)
        }
        .accessibilityElement(children: .combine)
    }
}
