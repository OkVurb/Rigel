import Foundation

/// Where a room lives, parsed from whatever a person types into the connect
/// screen: `192.168.1.42`, `192.168.1.42:8300`, `studio.local`, or the whole
/// `http://192.168.1.42:8300` URL the room prints at startup.
public struct RoomAddress: Hashable, Sendable {
    /// agentchattr's own default (`server.port` in its `config.toml`).
    public static let defaultPort = 8300
    /// What a TLS front end serves on: `tailscale serve` puts the room's
    /// loopback port on the tailnet's name over HTTPS, and that name takes no
    /// port of its own.
    public static let defaultSecurePort = 443

    /// `http` for a room reached directly on the network, `https` for one
    /// behind a TLS front end such as `tailscale serve`.
    public let scheme: String
    public let host: String
    public let port: Int

    public init(host: String, port: Int = RoomAddress.defaultPort, scheme: String = "http") {
        self.host = host
        self.port = port
        self.scheme = scheme.lowercased() == "https" ? "https" : "http"
    }

    /// The port a scheme implies when none was typed.
    public static func defaultPort(for scheme: String) -> Int {
        scheme == "https" ? defaultSecurePort : defaultPort
    }

    /// `nil` for anything that cannot be a host — the connect screen turns
    /// that into one plain sentence rather than a half-built request.
    public init?(raw: String) {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return nil }

        var scheme = "http"
        // A pasted URL keeps only its authority, but its scheme decides both
        // the default port and whether the client speaks TLS.
        if let mark = text.range(of: "://") {
            let name = text[..<mark.lowerBound].lowercased()
            guard name == "http" || name == "https" else { return nil }
            scheme = name
            text = String(text[mark.upperBound...])
        }
        if let cut = text.firstIndex(where: { $0 == "/" || $0 == "?" || $0 == "#" }) {
            text = String(text[..<cut])
        }
        // Some keyboards/autocorrect add a trailing dot.
        while text.hasSuffix(".") { text.removeLast() }
        guard !text.isEmpty else { return nil }

        var host = text
        var port = RoomAddress.defaultPort(for: scheme)
        var bracketed = false

        if text.hasPrefix("[") {
            // A bracketed IPv6 literal, optionally followed by `:port`.
            bracketed = true
            guard let closing = text.firstIndex(of: "]") else { return nil }
            host = String(text[text.index(after: text.startIndex)..<closing])
            let rest = text[text.index(after: closing)...]
            if rest.isEmpty {
                // keep the default port
            } else if rest.hasPrefix(":"), let parsed = Int(rest.dropFirst()) {
                port = parsed
            } else {
                return nil
            }
        } else if let colon = text.lastIndex(of: ":") {
            let tail = text[text.index(after: colon)...]
            if !tail.isEmpty, tail.allSatisfy(\.isNumber) {
                host = String(text[..<colon])
                port = Int(tail) ?? port
            }
        }

        guard !host.isEmpty, !host.contains(where: \.isWhitespace) else { return nil }
        // A colon left in the host is either a typo (`host:port`) or an
        // unbracketed IPv6 literal, which is ambiguous; the room's own URL
        // form brackets those.
        guard bracketed || !host.contains(":") else { return nil }
        guard (1...65535).contains(port) else { return nil }
        self.init(host: host, port: port, scheme: scheme)
    }

    /// `host` when the port is the one the scheme implies, `host:port`
    /// otherwise — with `https://` in front when the connection is TLS, since
    /// that is the tailnet path rather than the plain room.
    public var displayName: String {
        let name = host.contains(":") ? "[\(host)]" : host
        let shown = port == RoomAddress.defaultPort(for: scheme) ? name : "\(name):\(port)"
        return scheme == "https" ? "https://\(shown)" : shown
    }

    /// Plain HTTP for a room on the network, HTTPS behind a TLS front end; see
    /// SECURITY.md's LAN-bind warning and the README's note about trusted
    /// networks.
    public var baseURL: URL? {
        var components = URLComponents()
        components.scheme = scheme
        components.host = host
        // 443 is the only port that belongs left out. The room's 8300 has to be
        // spelled out, because plain http would otherwise default to 80.
        if !(scheme == "https" && port == 443) {
            components.port = port
        }
        return components.url
    }

    /// The addresses to try, in the order they were handed over: fields left
    /// blank and text that cannot be an address are dropped, and an address
    /// named twice is only tried once.
    ///
    /// This is the order rule for a phone that knows a room by two names — its
    /// LAN address at home and its `tailscale serve` name away — where only one
    /// of them is reachable at any moment.
    public static func candidates(from texts: [String]) -> [RoomAddress] {
        var seen: Set<RoomAddress> = []
        var ordered: [RoomAddress] = []
        for text in texts {
            guard let address = RoomAddress(raw: text), !seen.contains(address) else { continue }
            seen.insert(address)
            ordered.append(address)
        }
        return ordered
    }

    /// The same address spoken as a socket: `ws` in front of a room reached on
    /// the network, `wss` in front of one behind the TLS front end. The token
    /// travels in the query string, which is how the room's own UI authenticates
    /// its `/ws` connection.
    public func webSocketURL(path: String, query: [(String, String)] = []) -> URL? {
        guard let plain = url(path: path, query: query),
              var components = URLComponents(url: plain, resolvingAgainstBaseURL: false)
        else { return nil }
        components.scheme = scheme == "https" ? "wss" : "ws"
        return components.url
    }

    /// A URL for one API path plus query items.
    public func url(path: String, query: [(String, String)] = []) -> URL? {
        guard let base = baseURL else { return nil }
        let root = base.absoluteString.hasSuffix("/")
            ? String(base.absoluteString.dropLast())
            : base.absoluteString
        let suffix = path.hasPrefix("/") ? path : "/" + path
        guard var components = URLComponents(string: root + suffix) else { return nil }
        if !query.isEmpty {
            components.queryItems = query.map { URLQueryItem(name: $0.0, value: $0.1) }
        }
        return components.url
    }
}
