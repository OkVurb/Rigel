import Foundation

/// One job on the room's board: a bounded piece of work with its own thread.
///
/// The room keeps these in `jobs.json` and pushes the whole board on connect
/// (`{"type": "jobs"}`), then one job per change (`{"type": "job", "action":
/// …}`). Statuses are the room's own three lanes: `open`, `done`, `archived`.
public struct RoomJob: Decodable, Identifiable, Hashable, Sendable {
    public let id: Int
    public let title: String
    /// The brief, as written. Usually empty.
    public let body: String
    /// `open`, `done` or `archived` — the lanes the board draws.
    public let status: String
    public let channel: String
    public let createdBy: String
    /// The agent the job is addressed to; frequently empty.
    public let assignee: String
    public let messages: [RoomJobMessage]
    public let createdAt: Double
    public let updatedAt: Double
    public let sortOrder: Int

    public init(
        id: Int,
        title: String,
        body: String = "",
        status: String = "open",
        channel: String = "general",
        createdBy: String = "",
        assignee: String = "",
        messages: [RoomJobMessage] = [],
        createdAt: Double = 0,
        updatedAt: Double = 0,
        sortOrder: Int = 0
    ) {
        self.id = id
        self.title = title
        self.body = body
        self.status = status
        self.channel = channel
        self.createdBy = createdBy
        self.assignee = assignee
        self.messages = messages
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.sortOrder = sortOrder
    }

    enum CodingKeys: String, CodingKey {
        case id, title, body, status, channel, assignee, messages
        case createdBy = "created_by"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case sortOrder = "sort_order"
    }

    /// Lenient like the rest of the kit: a job stored by an older room lacks
    /// fields today's server always writes, and a phone that refuses to show
    /// the board over one of them would be worse than one that shows it.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(Int.self, forKey: .id)
        title = (try? container.decodeIfPresent(String.self, forKey: .title)) ?? "Untitled job"
        body = (try? container.decodeIfPresent(String.self, forKey: .body)) ?? ""
        status = (try? container.decodeIfPresent(String.self, forKey: .status)) ?? "open"
        channel = (try? container.decodeIfPresent(String.self, forKey: .channel)) ?? "general"
        createdBy = (try? container.decodeIfPresent(String.self, forKey: .createdBy)) ?? ""
        assignee = (try? container.decodeIfPresent(String.self, forKey: .assignee)) ?? ""
        messages = (try? container.decodeIfPresent([RoomJobMessage].self, forKey: .messages)) ?? []
        createdAt = (try? container.decodeIfPresent(Double.self, forKey: .createdAt)) ?? 0
        updatedAt = (try? container.decodeIfPresent(Double.self, forKey: .updatedAt)) ?? 0
        sortOrder = (try? container.decodeIfPresent(Int.self, forKey: .sortOrder)) ?? 0
    }

    /// The lanes a board has, in the order they are drawn.
    public static let lanes = ["open", "done", "archived"]

    /// The lane this job belongs in, even if the room invented a fourth one:
    /// an unknown status is work nobody has done yet.
    public var lane: String {
        Self.lanes.contains(status) ? status : "open"
    }
}

/// One line in a job's own thread. Separate from `RoomMessage`: the board's
/// conversations are per job, not per channel, and carry no metadata.
public struct RoomJobMessage: Decodable, Hashable, Sendable, Identifiable {
    /// The index within the job — the room numbers these from zero.
    public let id: Int
    public let sender: String
    public let text: String
    public let time: String
    public let timestamp: Double
    public let type: String

    public init(
        id: Int,
        sender: String,
        text: String,
        time: String = "",
        timestamp: Double = 0,
        type: String = "chat"
    ) {
        self.id = id
        self.sender = sender
        self.text = text
        self.time = time
        self.timestamp = timestamp
        self.type = type
    }

    enum CodingKeys: String, CodingKey {
        case id, sender, text, time, timestamp, type
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = (try? container.decodeIfPresent(Int.self, forKey: .id)) ?? 0
        sender = (try? container.decodeIfPresent(String.self, forKey: .sender)) ?? "?"
        text = (try? container.decodeIfPresent(String.self, forKey: .text)) ?? ""
        time = (try? container.decodeIfPresent(String.self, forKey: .time)) ?? ""
        timestamp = (try? container.decodeIfPresent(Double.self, forKey: .timestamp)) ?? 0
        type = (try? container.decodeIfPresent(String.self, forKey: .type)) ?? "chat"
    }
}

/// One of the room's standing orders.
///
/// A rule is proposed, then activated — `pending`, `active`, `draft` and
/// `archived` are the states the room uses. Active rules are injected into
/// every agent's context, which is why this screen is read-only: activating
/// one changes what every agent does next, and that decision stays where the
/// proposal was made.
public struct RoomRule: Decodable, Identifiable, Hashable, Sendable {
    public let id: Int
    public let text: String
    public let author: String
    public let reason: String
    public let status: String
    public let createdAt: Double

    public init(
        id: Int,
        text: String,
        author: String = "",
        reason: String = "",
        status: String = "pending",
        createdAt: Double = 0
    ) {
        self.id = id
        self.text = text
        self.author = author
        self.reason = reason
        self.status = status
        self.createdAt = createdAt
    }

    enum CodingKeys: String, CodingKey {
        case id, text, author, reason, status
        case createdAt = "created_at"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = (try? container.decodeIfPresent(Int.self, forKey: .id)) ?? 0
        text = (try? container.decodeIfPresent(String.self, forKey: .text)) ?? ""
        author = (try? container.decodeIfPresent(String.self, forKey: .author)) ?? ""
        reason = (try? container.decodeIfPresent(String.self, forKey: .reason)) ?? ""
        status = (try? container.decodeIfPresent(String.self, forKey: .status)) ?? "pending"
        createdAt = (try? container.decodeIfPresent(Double.self, forKey: .createdAt)) ?? 0
    }

    /// Whether the room is currently feeding this rule to every agent.
    public var isActive: Bool { status == "active" }
}

/// A decision an agent is waiting on, carried in a message's `metadata`.
///
/// The room posts these as `type: "decision"` with a fixed set of `choices`
/// (`yes`/`no`, usually). Answering one writes `@sender <choice>` back into the
/// channel, so this is the one place a phone can move work forward — and the
/// reason every answer is confirmed before it is sent.
public struct RoomDecision: Decodable, Hashable, Sendable {
    public let choices: [String]
    public let resolved: Bool
    public let chosen: String?

    public init(choices: [String], resolved: Bool = false, chosen: String? = nil) {
        self.choices = choices
        self.resolved = resolved
        self.chosen = chosen
    }

    enum CodingKeys: String, CodingKey {
        case choices, resolved, chosen
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        choices = (try? container.decodeIfPresent([String].self, forKey: .choices)) ?? []
        resolved = (try? container.decodeIfPresent(Bool.self, forKey: .resolved)) ?? false
        chosen = try? container.decodeIfPresent(String.self, forKey: .chosen)
    }

    /// A card with no choices is prose about a decision, not one to answer.
    public var isAnswerable: Bool { !resolved && !choices.isEmpty }
}
