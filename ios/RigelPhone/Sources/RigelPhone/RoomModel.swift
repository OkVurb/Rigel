import Foundation
import Observation
import RigelRoomKit

/// Everything the screens read: the connection, the roster, the stream.
///
/// The model is the only place that knows about persistence, the room's socket
/// and its token dance; views call `connect`, `refresh` and `disconnect` and
/// render whatever lands. Updates arrive over the room's own `/ws` socket — the
/// room pushes a snapshot on connect and every new line as it is said — rather
/// than a poll, so a line appears when it happens instead of up to two seconds
/// later.
@MainActor
@Observable
final class RoomModel {
    enum Phase: Equatable {
        case idle
        case connecting
        case live
    }

    /// Bound to the connect screen's fields and kept across attempts.
    ///
    /// A room has two names in practice — the LAN address at home and the
    /// `tailscale serve` name away — and only one of them is reachable at any
    /// moment, so both are remembered and both are tried; see `remember`.
    var addressText: String = ""
    var alternateText: String = ""
    var tokenText: String = ""

    private(set) var phase: Phase = .idle
    /// A connect attempt failed; shown on the connect screen.
    private(set) var connectError: String?
    /// The live connection is up but its socket is between attempts; shown as
    /// a banner while the room stays on screen with its last data.
    private(set) var streamError: String?

    private(set) var connectedAddress: String = ""
    private(set) var roomTitle: String = "Room"
    private(set) var roomSubtitle: String = ""
    private(set) var username: String = "user"
    private(set) var channels: [String] = ["general"]
    private(set) var status: RoomStatus?
    private(set) var lastSync: Date?
    var channel: String = "general"

    /// The room's board, newest change last. The socket sends the whole board
    /// on connect and one job per change after that, so this is never fetched
    /// behind the person's back once the room is live.
    private(set) var jobs: [RoomJob] = []
    /// The rule book, in the room's own order.
    private(set) var rules: [RoomRule] = []
    /// A decision card waiting on the confirmation step: non-nil is the card
    /// being up, and nothing is written until `confirmDecision`.
    private(set) var pendingDecision: DecisionAnswer?
    /// The card whose answer is in flight, by message id.
    private(set) var resolvingDecision: Int?
    /// Why an answer did not land; shown under the timeline.
    private(set) var decisionError: String?

    /// The lanes the board draws, in board order.
    var jobsByLane: [JobLane] {
        RoomJob.lanes.compactMap { lane in
            let inLane = jobs
                .filter { $0.lane == lane }
                .sorted { ($0.sortOrder, $0.id) < ($1.sortOrder, $1.id) }
            return inLane.isEmpty ? nil : JobLane(lane: lane, jobs: inLane)
        }
    }

    /// The rules the room is currently feeding every agent, first.
    var rulesByStatus: [RuleStatusGroup] {
        let order = ["active", "pending", "draft", "archived"]
        let known = order.compactMap { status -> RuleStatusGroup? in
            let group = rules.filter { $0.status == status }
            return group.isEmpty ? nil : RuleStatusGroup(status: status, rules: group)
        }
        let others = Dictionary(grouping: rules.filter { !order.contains($0.status) }, by: \.status)
            .sorted { $0.key < $1.key }
            .map { RuleStatusGroup(status: $0.key, rules: $0.value) }
        return known + others
    }

    /// The composer's text. Kept across a failed send, a reconnect and a
    /// background round trip, because a phone that drops what was typed is a
    /// phone that loses work.
    var draft: String = ""
    /// A line waiting for the confirmation card: non-nil is the card being up,
    /// and nothing is written to the room until `confirmSend`.
    private(set) var pendingSend: RoomLine?
    /// The last confirmed send that did not go; shown under the composer.
    private(set) var sendError: String?
    private(set) var isSending = false

    /// Files uploaded for the line being composed. They are already in the
    /// room — the drain happens on Send — so they survive a draft being
    /// rewritten, and removing one only removes it from this line.
    private(set) var pendingAttachments: [RoomAttachment] = []
    /// Why the last upload did not land, or why a picked file was not offered.
    private(set) var uploadError: String?
    private(set) var isUploading = false

    /// The selected channel's timeline, newest last. The socket carries every
    /// channel, so this is a slice of what the model holds rather than a fetch.
    var messages: [RoomMessage] { byChannel[channel] ?? [] }

    private var client: RoomClient?
    private var streamTask: Task<Void, Never>?
    /// The socket of the moment, when there is one. A line goes here and
    /// nowhere else, so this being nil is what "the room is not connected"
    /// means for the composer.
    private var connection: RoomConnection?

    /// Lines held per channel. A phone does not need the whole history of a
    /// busy room, and the room is free to hand one out.
    private static let retainedPerChannel = 400

    private var byChannel: [String: [RoomMessage]] = [:]
    /// While a snapshot is on its way, frames land here and replace
    /// `byChannel` only when the room says it is done — so a reconnect never
    /// blanks the timeline it is about to refill.
    private var incoming: [String: [RoomMessage]] = [:]
    private var isSnapshotting = false

    private var didRestore = false

    private static let addressKey = "rigel.phone.room.address"
    private static let alternateKey = "rigel.phone.room.alternate-address"
    private static let tokenAccount = "room-token"

    var isConnected: Bool { phase == .live }
    var isBusy: Bool { phase == .connecting }
    var agentCount: Int { status?.agents.count ?? 0 }

    /// Where this room's files are served from, for the views that draw them.
    var attachmentBase: URL? { client?.address.baseURL }

    // MARK: - Launch and connection

    /// Reconnect to the saved room on launch. Runs once; a failure leaves the
    /// connect screen filled in with the reason.
    func restore() async {
        guard !didRestore else { return }
        didRestore = true
        addressText = UserDefaults.standard.string(forKey: Self.addressKey) ?? ""
        alternateText = UserDefaults.standard.string(forKey: Self.alternateKey) ?? ""
        tokenText = KeychainStore.read(account: Self.tokenAccount) ?? ""
        guard !addressText.isEmpty || !alternateText.isEmpty else { return }
        await connect()
    }

    func connect() async {
        stopStream()
        client = nil
        connectError = nil
        streamError = nil
        // Both of the room's names, in the order they were remembered; blank
        // fields and anything that cannot be an address are dropped.
        let candidates = RoomAddress.candidates(from: [addressText, alternateText])
        guard !candidates.isEmpty else {
            phase = .idle
            connectError = RoomError.badAddress.errorDescription
            return
        }
        phase = .connecting
        do {
            let found = try await RoomClient.firstAnswering(
                candidates,
                token: RoomToken.normalized(tokenText)
            )
            if !found.settings.channels.contains(channel) {
                channel = found.settings.channels.first ?? "general"
            }
            // One pass of the read endpoints so the room is on screen the
            // moment the connect screen goes away. The socket then re-sends
            // exactly this as its snapshot, which is the same data arriving
            // twice on purpose: nothing has to be on screen that the room and
            // the socket disagree about.
            let roster = try await found.client.status()
            let recent = try await found.client.messages(limit: 200, channel: channel)

            client = found.client
            tokenText = found.client.token
            remember(found.client.address)
            apply(settings: found.settings, roster: roster, recent: recent)
            phase = .live
            KeychainStore.save(found.client.token, account: Self.tokenAccount)
            openStream()
            // The socket will send both of these too; fetching them now only
            // means the Board and Rules tabs have something in them during the
            // second before the snapshot lands. A room too old to answer either
            // endpoint leaves them empty rather than failing the connection.
            await loadWork()
        } catch {
            client = nil
            phase = .idle
            connectError = Self.describe(error)
        }
    }

    /// The settings sheet's Reconnect: a live room gets a new socket; a dead
    /// one goes through the full connect.
    func reconnect() async {
        if isConnected {
            refresh()
            return
        }
        await connect()
    }

    func disconnect() {
        stopStream()
        client = nil
        phase = .idle
        connectError = nil
        streamError = nil
        sendError = nil
    }

    func forget() {
        disconnect()
        addressText = ""
        alternateText = ""
        tokenText = ""
        connectedAddress = ""
        roomTitle = "Room"
        roomSubtitle = ""
        username = "user"
        channels = ["general"]
        channel = "general"
        byChannel = [:]
        incoming = [:]
        isSnapshotting = false
        status = nil
        lastSync = nil
        jobs = []
        rules = []
        pendingDecision = nil
        decisionError = nil
        resolvingDecision = nil
        draft = ""
        pendingSend = nil
        sendError = nil
        isSending = false
        pendingAttachments = []
        uploadError = nil
        isUploading = false
        connection = nil
        UserDefaults.standard.removeObject(forKey: Self.addressKey)
        UserDefaults.standard.removeObject(forKey: Self.alternateKey)
        KeychainStore.delete(account: Self.tokenAccount)
    }

    /// Switching channels is local: the socket carries every channel, so there
    /// is nothing to fetch and nothing to wait for.
    func select(channel name: String) {
        guard name != channel else { return }
        channel = name
    }

    // MARK: - Saying one line

    /// Whether the composer's Send has anything to do: there has to be a socket,
    /// because that is where a line goes, and the line has to say something —
    /// text or a file. While the room is between connections the banner above
    /// the composer is the reason.
    var canSend: Bool {
        connection != nil
            && !isSending
            && !isUploading
            && RoomLine(draft: draft, channel: channel, attachments: pendingAttachments) != nil
    }

    /// The composer's Send: stage the line for the confirmation card.
    ///
    /// Nothing is written here. The confirmation is the point of the step — a
    /// slipped thumb should not be able to say something in a room full of
    /// agents, and a blank draft is not a line at all.
    func proposeSend() {
        sendError = nil
        uploadError = nil
        pendingSend = RoomLine(draft: draft, channel: channel, attachments: pendingAttachments)
    }

    func cancelSend() {
        pendingSend = nil
    }

    /// The absolute URL for an attachment, so a view can load it without
    /// knowing which of the room's two names this phone is using.
    func url(for attachment: RoomAttachment) -> URL? {
        client?.address.url(path: attachment.url)
    }

    /// The confirmation card's Send, and the only path in this app that writes
    /// to the room. A line and nothing else: approvals are not reachable from
    /// here.
    ///
    /// The draft is cleared only once the line is on its way, and the line
    /// itself is not drawn by this model — it appears when the room echoes it
    /// back on the socket, which is how the timeline stays the room's record of
    /// what was said rather than the phone's guess at it.
    func confirmSend() async {
        guard let line = pendingSend else { return }
        pendingSend = nil
        guard let connection else {
            sendError = RoomError.notConnected.errorDescription
            return
        }
        isSending = true
        defer { isSending = false }
        do {
            try await connection.send(line)
            draft = ""
            pendingAttachments = []
            sendError = nil
        } catch {
            sendError = Self.describe(error)
        }
    }

    // MARK: - Files

    /// Copy one picked file into the room and add it to the line being
    /// composed. Nothing is said here: the line is still a draft, and the bytes
    /// have simply moved to the room's own uploads directory.
    func upload(_ bytes: Data, filename: String, mimeType: String) async {
        guard let client else {
            uploadError = RoomError.notConnected.errorDescription
            return
        }
        isUploading = true
        defer { isUploading = false }
        do {
            let attachment = try await client.upload(
                bytes,
                filename: filename,
                mimeType: mimeType
            )
            if !pendingAttachments.contains(where: { $0.url == attachment.url }) {
                pendingAttachments.append(attachment)
            }
            uploadError = nil
        } catch {
            uploadError = Self.describe(error)
        }
    }

    /// Take one file back off the line. The room keeps its copy; this only
    /// stops it being sent.
    func discard(_ attachment: RoomAttachment) {
        pendingAttachments.removeAll { $0.url == attachment.url }
    }

    /// A file the composer refused before uploading it at all.
    func refuseUpload(_ reason: String) {
        uploadError = reason
    }

    // MARK: - Answering a decision

    /// Stage an answer to a card the room is waiting on.
    ///
    /// The card is the confirmation step, for the same reason a line has one:
    /// the room records the choice on the card, posts `@sender <choice>` into
    /// the channel and tells every client, so a slipped thumb would move work
    /// forward with nobody having read what they agreed to.
    func stage(decision message: RoomMessage, choice: String) {
        guard message.isOpenDecision else { return }
        decisionError = nil
        pendingDecision = DecisionAnswer(message: message, choice: choice)
    }

    func cancelDecision() {
        pendingDecision = nil
    }

    /// The confirmation card's Send, and the only other write in this app.
    ///
    /// It goes over HTTP rather than the socket because the room has to check
    /// the choice against the card and mark it resolved before it says
    /// anything — `RoomClient.resolveDecision`. The room then broadcasts the
    /// updated card, which is what the timeline settles on; `settle` only stops
    /// the buttons inviting a second answer while that is in flight.
    func confirmDecision() async {
        guard let answer = pendingDecision, let client else { return }
        pendingDecision = nil
        resolvingDecision = answer.message.id
        defer { resolvingDecision = nil }
        do {
            try await client.resolveDecision(messageID: answer.message.id, choice: answer.choice)
            decisionError = nil
            settle(answer.message.id, as: answer.choice)
        } catch {
            decisionError = Self.describe(error)
        }
    }

    // MARK: - Foreground / background

    /// Leaving the foreground closes the socket; the last data stays visible.
    func pause() {
        stopStream()
    }

    /// Coming back opens a fresh socket, and the room re-sends its snapshot.
    func resume() {
        guard isConnected else { return }
        refresh()
    }

    // MARK: - Reading

    /// A fresh look at the room: one new socket, one new snapshot. What is on
    /// screen stays there until the snapshot completes.
    func refresh() {
        guard isConnected else { return }
        openStream()
    }

    // MARK: - The socket

    /// One socket at a time, for as long as this room is connected.
    ///
    /// The loop is the whole reconnection policy: read the socket until it
    /// ends, adopt a fresh token if the room has restarted, wait, try again.
    /// Everything the screens see is applied by `apply`, so a dropped socket
    /// costs the timeline nothing.
    private func openStream() {
        stopStream()
        guard client != nil else { return }
        streamTask = Task { [weak self] in
            var failures = 0
            while !Task.isCancelled {
                guard let self, let client = self.client else { return }
                guard let connection = try? RoomStream(
                    address: client.address,
                    token: client.token
                ).connect() else {
                    // The address was proved by `/api/settings` a moment ago,
                    // so this is the connect screen's business, not the loop's.
                    self.streamError = RoomError.badAddress.errorDescription
                    return
                }
                self.connection = connection
                let lived = await self.consume(connection)
                self.connection = nil
                if Task.isCancelled { return }
                failures = lived ? 0 : failures + 1
                // Every room restart mints a new token. Adopting it here is
                // what lets the phone follow a restart by itself; while the
                // token is still good this costs one landing page and one
                // settings call per attempt, and while the room is down it
                // costs nothing that answers.
                await self.recoverToken()
                if Task.isCancelled { return }
                // A socket that never answered may mean the phone has moved:
                // out of the house the LAN name stops answering and the
                // tailnet name is the one that works. Ask both again.
                if !lived {
                    await self.relocate()
                    if Task.isCancelled { return }
                }
                let wait = Self.backoff(failures)
                try? await Task.sleep(for: .seconds(wait))
            }
        }
    }

    /// Read one connection to its end, handing every frame to `apply`. Returns
    /// whether the room got as far as answering: a socket that delivered
    /// frames is worth reopening at once, one that never answered is not.
    @discardableResult
    private func consume(_ connection: RoomConnection) async -> Bool {
        // A fresh connection means a fresh snapshot; frames are collected
        // separately until the room says the history is complete.
        incoming = [:]
        isSnapshotting = true
        var lived = false
        do {
            for try await event in connection.events() {
                if Task.isCancelled { return lived }
                lived = true
                apply(event)
            }
        } catch {
            if Task.isCancelled { return lived }
            streamError = Self.describe(error)
        }
        return lived
    }

    private func stopStream() {
        streamTask?.cancel()
        streamTask = nil
        connection = nil
    }

    /// Reconnect waits: a second at first, because a room restart or a Wi-Fi
    /// flicker clears in about that, then longer, so a room that is down for
    /// the evening is not hammered by a phone in a pocket.
    private static func backoff(_ failures: Int) -> Double {
        let steps: [Double] = [1, 2, 4, 8, 15]
        return steps[min(max(failures, 1), steps.count) - 1]
    }

    // MARK: - Applying frames

    private func apply(_ event: RoomEvent) {
        switch event {
        case .settings(let settings):
            roomTitle = settings.title
            roomSubtitle = settings.subtitle
            username = settings.username
            channels = settings.channels
            if !channels.contains(channel) {
                channel = channels.first ?? "general"
            }
        case .status(let roster):
            status = roster
        case .history(let batch):
            if isSnapshotting {
                Self.merge(batch, into: &incoming)
            } else {
                Self.merge(batch, into: &byChannel)
            }
            lastSync = Date()
        case .historyComplete:
            if isSnapshotting {
                byChannel = incoming
                incoming = [:]
                isSnapshotting = false
            }
            lastSync = Date()
            streamError = nil
        case .message(let message):
            Self.merge([message], into: &byChannel)
            // A line that lands while the snapshot is still arriving belongs in
            // both places: on screen now, and in the snapshot that is about to
            // replace what is on screen.
            if isSnapshotting {
                Self.merge([message], into: &incoming)
            }
            lastSync = Date()
        case .delete(let ids):
            remove(ids)
        case .clear(let name):
            clear(name)
        case .jobs(let board):
            jobs = board.sorted { ($0.sortOrder, $0.id) < ($1.sortOrder, $1.id) }
        case .job(let job):
            upsert(job: job)
        case .rules(let book):
            rules = book
        case .rule(let rule):
            upsert(rule: rule)
        case .messageUpdate(let message):
            // A line that changed under us — most often a decision card
            // answered on the desktop. The room's copy replaces ours.
            Self.merge([message], into: &byChannel)
            if isSnapshotting {
                Self.merge([message], into: &incoming)
            }
            lastSync = Date()
        case .ignored:
            // Typing indicators, todos, hats, schedules and sessions — the
            // room's other business, none of which this screen shows.
            break
        }
    }

    private func apply(settings: RoomSettings, roster: RoomStatus, recent: [RoomMessage]) {
        roomTitle = settings.title
        roomSubtitle = settings.subtitle
        username = settings.username
        channels = settings.channels
        status = roster
        byChannel = [:]
        Self.merge(recent, into: &byChannel)
        lastSync = Date()
    }

    /// Fold one batch of lines into a channel's list, newest last. Duplicates
    /// by id are replaced rather than doubled — a line can arrive twice across
    /// a reconnect — and the tail is trimmed to what a phone should hold.
    private static func merge(_ batch: [RoomMessage], into stored: inout [String: [RoomMessage]]) {
        guard !batch.isEmpty else { return }
        var touched: Set<String> = []
        for message in batch {
            var list = stored[message.channel, default: []]
            if let index = list.firstIndex(where: { $0.id == message.id }) {
                list[index] = message
            } else {
                list.append(message)
            }
            stored[message.channel] = list
            touched.insert(message.channel)
        }
        for name in touched {
            var list = stored[name, default: []]
            list.sort { $0.id < $1.id }
            if list.count > retainedPerChannel {
                list.removeFirst(list.count - retainedPerChannel)
            }
            stored[name] = list
        }
    }

    /// Fold one changed job into the board: replace by id, or append when the
    /// room added one. The board is small, so this stays a linear scan.
    private func upsert(job: RoomJob) {
        if let index = jobs.firstIndex(where: { $0.id == job.id }) {
            jobs[index] = job
        } else {
            jobs.append(job)
        }
        jobs.sort { ($0.sortOrder, $0.id) < ($1.sortOrder, $1.id) }
    }

    private func upsert(rule: RoomRule) {
        if let index = rules.firstIndex(where: { $0.id == rule.id }) {
            rules[index] = rule
        } else {
            rules.append(rule)
        }
    }

    /// The board and the rule book, for the second between going live and the
    /// socket's snapshot. Both are optional to the connection.
    private func loadWork() async {
        guard let client else { return }
        async let board = client.jobs()
        async let book = client.rules()
        if let loaded = try? await board { jobs = loaded }
        if let loaded = try? await book { rules = loaded }
    }

    /// Mark a card answered, in both the timeline and the snapshot being built.
    private func settle(_ messageID: Int, as choice: String) {
        for (name, list) in byChannel {
            guard let index = list.firstIndex(where: { $0.id == messageID }) else { continue }
            byChannel[name]?[index] = list[index].answered(choice)
        }
        for (name, list) in incoming {
            guard let index = list.firstIndex(where: { $0.id == messageID }) else { continue }
            incoming[name]?[index] = list[index].answered(choice)
        }
    }

    private func remove(_ ids: [Int]) {
        guard !ids.isEmpty else { return }
        let doomed = Set(ids)
        for (name, list) in byChannel {
            byChannel[name] = list.filter { !doomed.contains($0.id) }
        }
        for (name, list) in incoming {
            incoming[name] = list.filter { !doomed.contains($0.id) }
        }
    }

    private func clear(_ name: String?) {
        guard let name else {
            byChannel = [:]
            incoming = [:]
            return
        }
        byChannel[name] = []
        incoming[name] = []
    }

    /// Ask every remembered address again and move to whichever answers now.
    ///
    /// Walking out of the house with the app open is the case this exists for:
    /// the address that used to answer is not the one that does any more, and
    /// nothing else in the model would notice — the socket would simply keep
    /// retrying a network the phone has left.
    private func relocate() async {
        let candidates = RoomAddress.candidates(from: [addressText, alternateText])
        guard candidates.count > 1 else { return }
        guard let found = try? await RoomClient.firstAnswering(
            candidates,
            token: RoomToken.normalized(tokenText)
        ) else { return }
        guard found.client.address != client?.address else { return }
        client = found.client
        tokenText = found.client.token
        remember(found.client.address)
        KeychainStore.save(found.client.token, account: Self.tokenAccount)
        streamError = nil
    }

    /// Whichever address answered leads the next attempt and is the one the
    /// screens name; the other stays behind it. At home that leaves the LAN
    /// name first and the tailnet name second, away from home it swaps — so the
    /// slow attempt against a network the phone has left is made once, not on
    /// every launch.
    private func remember(_ address: RoomAddress) {
        let others = RoomAddress.candidates(from: [addressText, alternateText])
            .filter { $0 != address }
        addressText = address.displayName
        alternateText = others.first?.displayName ?? ""
        connectedAddress = address.displayName
        UserDefaults.standard.set(addressText, forKey: Self.addressKey)
        UserDefaults.standard.set(alternateText, forKey: Self.alternateKey)
    }

    /// Re-read the token from the room's landing page after a restart.
    private func recoverToken() async {
        guard let address = RoomAddress(raw: connectedAddress) ?? RoomAddress(raw: addressText) else {
            return
        }
        guard let token = try? await RoomClient.adoptToken(address: address) else { return }
        guard token != client?.token else { return }
        client = RoomClient(address: address, token: token)
        tokenText = token
        KeychainStore.save(token, account: Self.tokenAccount)
    }

    private static func describe(_ error: Error) -> String {
        if let room = error as? RoomError, let text = room.errorDescription {
            return text
        }
        return error.localizedDescription
    }
}

/// One lane of the board, with the identity a `ForEach` wants.
struct JobLane: Identifiable, Equatable {
    let lane: String
    let jobs: [RoomJob]
    var id: String { lane }
}

/// One status of the rule book, with the identity a `ForEach` wants.
struct RuleStatusGroup: Identifiable, Equatable {
    let status: String
    let rules: [RoomRule]
    var id: String { status }
}

/// A staged answer to a decision card, with the identity `sheet(item:)` wants.
///
/// It carries the whole card rather than its id so the confirmation can show
/// what is being answered without looking it up again, and so a card deleted
/// between staging and confirming fails on the room's side instead of on ours.
struct DecisionAnswer: Identifiable, Equatable {
    let message: RoomMessage
    let choice: String
    var id: Int { message.id }
}
