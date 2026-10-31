import Foundation

/// One file the room is holding for a line.
///
/// The room stores attachments as a name and a room-relative path
/// (`/uploads/1a2b3c4d.png`); the absolute URL is built against whichever
/// address this phone is connected through, so a LAN name and a tailnet name
/// both work without the room having to know which one the phone used.
public struct RoomAttachment: Codable, Hashable, Sendable, Identifiable {
    public let name: String
    public let url: String

    public var id: String { url }

    public init(name: String, url: String) {
        self.name = name
        self.url = url
    }

    private enum CodingKeys: String, CodingKey {
        case name, url
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = (try? container.decodeIfPresent(String.self, forKey: .name)) ?? "file"
        url = (try? container.decodeIfPresent(String.self, forKey: .url)) ?? ""
    }

    /// Whether the room will serve this as something an image view can show.
    public var isImage: Bool {
        Self.imageExtensions.contains(URL(fileURLWithPath: url).pathExtension.lowercased())
    }

    /// The file's absolute URL on `address`.
    public func absoluteURL(on address: RoomAddress) -> URL? {
        guard let base = address.baseURL else { return nil }
        return URL(string: url, relativeTo: base)?.absoluteURL
    }

    static let imageExtensions: Set<String> = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]
}

/// One line from the room's log, as `/api/messages` returns it.
///
/// Decoding is lenient on purpose: the room's log is an append-only JSONL file
/// whose older lines predate fields today's server always writes. A phone that
/// refuses to render history because one line lacks `type` would be worse than
/// one that shows it.
public struct RoomMessage: Decodable, Identifiable, Hashable, Sendable {
    public let id: Int
    public let sender: String
    public let text: String
    /// `chat` for anything a person or agent typed; `join`, `leave` and
    /// `system` are the room narrating itself.
    public let type: String
    public let timestamp: Double
    /// `HH:MM:SS` as the server stored it; agentchattr owns the clock.
    public let time: String
    public let channel: String
    public let replyTo: Int?
    /// The decision card this message carries, when it is one. The room stores
    /// it in `metadata`, alongside keys this client has no use for.
    public let decision: RoomDecision?
    /// Files posted with the line — photos, and the documents the room stores.
    public let attachments: [RoomAttachment]

    public init(
        id: Int,
        sender: String,
        text: String,
        type: String = "chat",
        timestamp: Double = 0,
        time: String = "",
        channel: String = "general",
        replyTo: Int? = nil,
        decision: RoomDecision? = nil,
        attachments: [RoomAttachment] = []
    ) {
        self.id = id
        self.sender = sender
        self.text = text
        self.type = type
        self.timestamp = timestamp
        self.time = time
        self.channel = channel
        self.replyTo = replyTo
        self.decision = decision
        self.attachments = attachments
    }

    enum CodingKeys: String, CodingKey {
        case id, sender, text, type, timestamp, time, channel, metadata, attachments
        case replyTo = "reply_to"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(Int.self, forKey: .id)
        sender = (try? container.decodeIfPresent(String.self, forKey: .sender)) ?? "?"
        text = (try? container.decodeIfPresent(String.self, forKey: .text)) ?? ""
        type = (try? container.decodeIfPresent(String.self, forKey: .type)) ?? "chat"
        timestamp = (try? container.decodeIfPresent(Double.self, forKey: .timestamp)) ?? 0
        time = (try? container.decodeIfPresent(String.self, forKey: .time)) ?? ""
        channel = (try? container.decodeIfPresent(String.self, forKey: .channel)) ?? "general"
        replyTo = try? container.decodeIfPresent(Int.self, forKey: .replyTo)
        let metadata = try? container.decodeIfPresent(RoomDecision.self, forKey: .metadata)
        decision = type == "decision" ? metadata : nil
        attachments = (try? container.decodeIfPresent([RoomAttachment].self, forKey: .attachments)) ?? []
    }

    /// This line with its decision answered, for the moment between the POST
    /// landing and the room broadcasting `message_update`. The room's frame is
    /// still what the timeline settles on; this only keeps the buttons from
    /// inviting a second answer to a question that is already answered.
    public func answered(_ choice: String) -> RoomMessage {
        guard let decision else { return self }
        return RoomMessage(
            id: id,
            sender: sender,
            text: text,
            type: type,
            timestamp: timestamp,
            time: time,
            channel: channel,
            replyTo: replyTo,
            decision: RoomDecision(choices: decision.choices, resolved: true, chosen: choice),
            attachments: attachments
        )
    }

    /// The room narrating itself (`join`, `leave`, `system`) rather than a
    /// person or an agent talking.
    public var isNarration: Bool {
        Self.narrationTypes.contains(type)
    }

    /// A card that wants an answer from whoever is reading.
    public var isOpenDecision: Bool {
        decision?.isAnswerable ?? false
    }

    static let narrationTypes: Set<String> = ["join", "leave", "system", "agent_renamed"]
}

/// One agent in the roster. `/api/status` keys these by name.
public struct AgentPresence: Hashable, Identifiable, Sendable {
    public let name: String
    public let label: String
    public let color: String
    public let available: Bool
    public let busy: Bool
    public let role: String?

    public var id: String { name }

    public init(
        name: String,
        label: String,
        color: String,
        available: Bool,
        busy: Bool,
        role: String?
    ) {
        self.name = name
        self.label = label
        self.color = color
        self.available = available
        self.busy = busy
        self.role = role
    }
}

/// The roster plus the room's pause state, as `/api/status` returns them.
///
/// The server's shape is a dictionary keyed by agent name, with `paused`
/// mixed in beside the agents, so this decodes key by key and keeps only the
/// values that really look like an agent.
public struct RoomStatus: Decodable, Sendable, Equatable {
    public let paused: Bool
    public let agents: [AgentPresence]

    public init(paused: Bool, agents: [AgentPresence]) {
        self.paused = paused
        self.agents = agents
    }

    private struct AnyKey: CodingKey {
        var stringValue: String
        var intValue: Int?
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) {
            self.intValue = intValue
            self.stringValue = String(intValue)
        }
    }

    private struct Presence: Decodable {
        let label: String?
        let color: String?
        let available: Bool?
        let busy: Bool?
        let role: String?
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: AnyKey.self)
        var paused = false
        var agents: [AgentPresence] = []
        for key in container.allKeys {
            if key.stringValue == "paused" {
                paused = (try? container.decode(Bool.self, forKey: key)) ?? false
                continue
            }
            guard let presence = try? container.decode(Presence.self, forKey: key) else { continue }
            agents.append(
                AgentPresence(
                    name: key.stringValue,
                    label: presence.label ?? key.stringValue,
                    color: presence.color ?? "#888888",
                    available: presence.available ?? false,
                    busy: presence.busy ?? false,
                    role: presence.role
                )
            )
        }
        self.paused = paused
        self.agents = agents.sorted { $0.name < $1.name }
    }
}

/// One line, ready to be said: the text as the room will store it, and the
/// channel it goes to.
///
/// A draft becomes a line only when it says something — the room drops blank
/// lines, so the phone refuses them before opening anything — and everything
/// else is kept as typed, because the room stores real newlines and a line the
/// phone quietly rewrapped would not be the line the person wrote.
///
/// This is also the frame the room's own page sends, and it carries no
/// `sender`: the room attributes a line to the session that sent it, and this
/// client will not write as anybody else. That is why nothing here can post as
/// an agent.
public struct RoomLine: Encodable, Equatable, Sendable {
    public let text: String
    public let channel: String
    /// Already uploaded: a line carries the room's own paths, never file bytes.
    public let attachments: [RoomAttachment]

    /// `nil` when the line says nothing: no text, no files.
    public init?(draft: String, channel: String, attachments: [RoomAttachment] = []) {
        let trimmed = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !attachments.isEmpty else { return nil }
        text = trimmed
        self.channel = channel.isEmpty ? "general" : channel
        self.attachments = attachments
    }

    private enum CodingKeys: String, CodingKey {
        case type, text, channel, attachments
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode("message", forKey: .type)
        try container.encode(text, forKey: .text)
        try container.encode(channel, forKey: .channel)
        if !attachments.isEmpty {
            try container.encode(attachments, forKey: .attachments)
        }
    }
}

/// `/api/settings`: what the room calls itself and the channels it carries.
public struct RoomSettings: Decodable, Sendable, Equatable {
    /// The room's title — `agentchattr` unless it was renamed.
    public let title: String
    /// Shown beside the title; an empty string hides it.
    public let subtitle: String
    /// The channels this room carries; `general` always exists.
    public let channels: [String]
    /// What the room calls the person at the keyboard (`user` unless renamed).
    public let username: String

    public init(title: String, subtitle: String, channels: [String], username: String) {
        self.title = title
        self.subtitle = subtitle
        self.channels = channels
        self.username = username
    }

    enum CodingKeys: String, CodingKey {
        case title, subtitle, channels, username
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        title = (try? container.decodeIfPresent(String.self, forKey: .title)) ?? "agentchattr"
        subtitle = (try? container.decodeIfPresent(String.self, forKey: .subtitle)) ?? ""
        username = (try? container.decodeIfPresent(String.self, forKey: .username)) ?? "user"
        let names = (try? container.decodeIfPresent([String].self, forKey: .channels)) ?? ["general"]
        channels = names.isEmpty ? ["general"] : names
    }
}
