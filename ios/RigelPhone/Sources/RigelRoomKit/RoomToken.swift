import Foundation

/// The session token the room injects into its own landing page
/// (`window.__SESSION_TOKEN__="…"`).
///
/// This mirrors `adopt_token` in `crates/rigel-core/src/agent/agentchat.rs`:
/// a room started outside Rigel (or by the desktop app itself) exposes its
/// token to the page it serves, and a client that can read that page can join
/// without being handed the token by hand. The value is only trusted after
/// `/api/settings` answers through it — see `RoomClient.adoptToken`.
public enum RoomToken {
    /// Scans for the first plausible token on the page, not just the first
    /// occurrence: a page may mention the marker in prose before the script.
    public static func scan(_ html: String) -> String? {
        var remainder = html[...]
        while let marker = remainder.range(of: "__SESSION_TOKEN__") {
            let after = remainder[marker.upperBound...]
            if let open = after.firstIndex(of: "\""),
               let close = after[after.index(after: open)...].firstIndex(of: "\"")
            {
                let value = String(after[after.index(after: open)..<close])
                if isPlausible(value) { return value }
            }
            remainder = after
        }
        return nil
    }

    /// The vendor's own shape: 32–128 hex characters. `room.py` mints
    /// `secrets.token_hex(32)`, and the desktop's regex accepts the same range.
    static func isPlausible(_ value: String) -> Bool {
        (32...128).contains(value.count) && value.allSatisfy(\.isHexDigit)
    }

    /// Clean whatever was pasted into the token field: surrounding whitespace,
    /// quotes, a leading `Session token:` label, or the whole token line from
    /// the room's banner.
    public static func normalized(_ raw: String) -> String {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if let colon = text.range(of: "Session token:") {
            text = String(text[colon.upperBound...])
        }
        text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        text = text.trimmingCharacters(in: CharacterSet(charactersIn: "\"'`"))
        // A token pasted as part of a query string (`?token=…`).
        if let query = text.range(of: "token=") {
            text = String(text[query.upperBound...])
            if let cut = text.firstIndex(where: { $0 == "&" || $0 == "#" || $0.isWhitespace }) {
                text = String(text[..<cut])
            }
        }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
