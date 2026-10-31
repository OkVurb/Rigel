import SwiftUI

/// Rigel's palette, extended for the phone.
///
/// The desktop's two anchors are kept — near-black `#0b0c0e` and the amber
/// `#e9a23b` — and the phone adds the depth the desktop does not have room for:
/// a cold midnight behind the amber, one cool accent for channels and lanes,
/// and a raised surface for glass.
enum Theme {
    // The room, in layers.
    static let background = Color(hex: 0x05070B)
    static let midnight = Color(hex: 0x0A0E17)
    static let panel = Color(hex: 0x101725)
    static let panelHigh = Color(hex: 0x162033)
    static let panelEdge = Color.white.opacity(0.075)
    static let hairline = Color.white.opacity(0.055)

    // Ink.
    static let ink = Color(white: 0.955)
    static let inkMuted = Color(white: 0.66)
    static let inkFaint = Color(white: 0.43)

    // Light.
    static let accent = Color(hex: 0xF2A93B)
    static let accentBright = Color(hex: 0xFFD68F)
    static let accentDeep = Color(hex: 0xC97C12)
    static let cool = Color(hex: 0x74A9FF)
    static let violet = Color(hex: 0xA78BFA)
    static let online = Color(hex: 0x5BD08A)
    static let problem = Color(hex: 0xFF7A66)

    // Gradients that several screens share.
    static let accentGradient = LinearGradient(
        colors: [accentBright, accent, accentDeep],
        startPoint: .topLeading,
        endPoint: .bottomTrailing
    )
    static let wordmark = LinearGradient(
        colors: [Color.white, accentBright, accent],
        startPoint: .top,
        endPoint: .bottom
    )

    /// A lane's own colour, so the board is readable before it is read.
    static func laneTint(_ lane: String) -> Color {
        switch lane {
        case "done": return online
        case "archived": return inkFaint
        default: return accent
        }
    }
}

/// The two type voices: rounded for things that speak, monospaced for things
/// that label. Mixing them is the whole hierarchy — every eyebrow, count and
/// timestamp is mono, every title and message is rounded.
enum Type {
    static func display(_ size: CGFloat, _ weight: Font.Weight = .semibold) -> Font {
        .system(size: size, weight: weight, design: .rounded)
    }

    static func body(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .rounded)
    }

    static func label(_ size: CGFloat = 10.5, _ weight: Font.Weight = .semibold) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }
}

extension Color {
    /// `#rrggbb` — the shape the room stores agent colours in, with a fallback
    /// for anything else (a hand-edited config can hold a word like `red`).
    init(hex: UInt32) {
        let red = Double((hex >> 16) & 0xff) / 255
        let green = Double((hex >> 8) & 0xff) / 255
        let blue = Double(hex & 0xff) / 255
        self.init(red: red, green: green, blue: blue)
    }

    init(hex: String, fallback: Color = Theme.inkMuted) {
        var text = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("#") { text.removeFirst() }
        guard text.count == 6, let value = UInt32(text, radix: 16) else {
            self = fallback
            return
        }
        self.init(hex: value)
    }
}
