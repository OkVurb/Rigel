import SwiftUI
import RigelRoomKit

/// One line in the stream.
///
/// The room is one shared channel rather than a two-party chat, so the shape
/// is a forum bubble: the speaker and time above it, the line in a panel, and
/// `you` set apart on the right in amber so your own words are findable while
/// scrolling. An agent's own colour carries through to its name and ring.
struct MessageRow: View {
    let message: RoomMessage
    let ownName: String
    let agents: [AgentPresence]
    /// Where the room's `/uploads/…` paths resolve from. Optional because a
    /// row drawn outside a live room (a preview) has nothing to resolve them
    /// against and should simply not draw files.
    var attachmentBase: URL? = nil

    private var isOwn: Bool { message.sender == ownName }
    private var agent: AgentPresence? {
        agents.first { $0.name == message.sender || $0.label == message.sender }
    }

    var body: some View {
        if message.isNarration {
            narration
        } else {
            bubble
        }
    }

    private var narration: some View {
        HStack(spacing: 10) {
            Spacer(minLength: 0)
            Text(message.text.isEmpty ? "\(message.sender) \(message.type)" : message.text)
                .font(Type.label(9.5))
                .foregroundStyle(Theme.inkFaint)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(Theme.panel.opacity(0.45), in: Capsule())
            Spacer(minLength: 0)
        }
        .padding(.vertical, 2)
    }

    private var bubble: some View {
        HStack(alignment: .top, spacing: 10) {
            if isOwn {
                Spacer(minLength: 44)
            } else {
                avatar
            }

            VStack(alignment: isOwn ? .trailing : .leading, spacing: 5) {
                HStack(spacing: 7) {
                    Text(isOwn ? "you" : message.sender)
                        .font(Type.label(10, .bold))
                        .tracking(0.8)
                        .foregroundStyle(nameTint)
                    Text(message.time)
                        .font(Type.label(9))
                        .foregroundStyle(Theme.inkFaint)
                }

                VStack(alignment: .leading, spacing: 10) {
                    if !message.text.isEmpty {
                        Text(message.text)
                            .font(Type.body(15))
                            .foregroundStyle(Theme.ink)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }

                    if !message.attachments.isEmpty {
                        AttachmentList(attachments: message.attachments, base: attachmentBase)
                    }

                    if let decision = message.decision {
                        DecisionCard(message: message, decision: decision)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 11)
                .background {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .fill(bubbleFill)
                }
                .overlay {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .strokeBorder(bubbleEdge, lineWidth: 1)
                }
                .shadow(color: .black.opacity(0.22), radius: 10, y: 5)
            }

            if !isOwn {
                Spacer(minLength: 30)
            }
        }
    }

    private var avatar: some View {
        Group {
            if let agent {
                AgentAvatar(agent: agent, size: 30)
            } else {
                ZStack {
                    Circle().fill(Theme.panelHigh)
                    Text(String(message.sender.prefix(1)).uppercased())
                        .font(Type.body(12, .bold))
                        .foregroundStyle(Theme.inkMuted)
                }
                .frame(width: 30, height: 30)
                .overlay(Circle().strokeBorder(Theme.panelEdge))
            }
        }
        .padding(.top, 16)
    }

    private var nameTint: Color {
        if isOwn { return Theme.accent }
        if let agent { return Color(hex: agent.color) }
        if message.sender == "system" { return Theme.inkFaint }
        return Theme.cool
    }

    private var bubbleFill: AnyShapeStyle {
        if isOwn {
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Theme.accent.opacity(0.26), Theme.accent.opacity(0.10)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        }
        if let agent {
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(hex: agent.color).opacity(0.13), Theme.panel.opacity(0.72)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        }
        return AnyShapeStyle(Theme.panel.opacity(0.72))
    }

    private var bubbleEdge: AnyShapeStyle {
        if isOwn { return AnyShapeStyle(Theme.accent.opacity(0.38)) }
        if let agent { return AnyShapeStyle(Color(hex: agent.color).opacity(0.22)) }
        return AnyShapeStyle(Theme.panelEdge)
    }
}

/// The files on a line: photos as thumbnails, documents as chips, both opening
/// the room's own copy. A phone is not going to preview a PDF inside a chat
/// bubble honestly, and the room already serves the file, so a link is the
/// right amount of browser.
struct AttachmentList: View {
    let attachments: [RoomAttachment]
    let base: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(attachments) { attachment in
                if let link = url(for: attachment) {
                    if attachment.isImage {
                        image(attachment, link: link)
                    } else {
                        file(attachment, link: link)
                    }
                }
            }
        }
    }

    private func url(for attachment: RoomAttachment) -> URL? {
        guard let base else { return nil }
        return URL(string: attachment.url, relativeTo: base)?.absoluteURL
    }

    private func image(_ attachment: RoomAttachment, link: URL) -> some View {
        Link(destination: link) {
            AsyncImage(url: link) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                case .failure:
                    ZStack {
                        Theme.panelHigh
                        Image(systemName: "photo")
                            .font(.system(size: 18))
                            .foregroundStyle(Theme.inkFaint)
                    }
                default:
                    ZStack {
                        Theme.panelHigh
                        ProgressView().controlSize(.small).tint(Theme.accent)
                    }
                }
            }
            .frame(width: 210, height: 140)
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Theme.panelEdge)
            }
            .overlay(alignment: .bottomLeading) {
                Text(attachment.name)
                    .font(Type.label(8.5, .semibold))
                    .foregroundStyle(Theme.ink)
                    .lineLimit(1)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(.ultraThinMaterial, in: Capsule())
                    .padding(7)
            }
        }
        .buttonStyle(PressableStyle(scale: 0.98))
        .accessibilityLabel("Photo \(attachment.name)")
    }

    private func file(_ attachment: RoomAttachment, link: URL) -> some View {
        Link(destination: link) {
            HStack(spacing: 9) {
                Image(systemName: icon(for: attachment))
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.cool)
                    .frame(width: 28, height: 28)
                    .background(Theme.cool.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

                VStack(alignment: .leading, spacing: 2) {
                    Text(attachment.name)
                        .font(Type.body(12.5, .semibold))
                        .foregroundStyle(Theme.ink)
                        .lineLimit(1)
                    Text(kind(for: attachment))
                        .font(Type.label(8.5))
                        .foregroundStyle(Theme.inkFaint)
                }
                Spacer(minLength: 0)
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(Theme.inkFaint)
            }
            .padding(9)
            .frame(maxWidth: 250, alignment: .leading)
            .background(Theme.background.opacity(0.38), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Theme.panelEdge)
            }
        }
        .buttonStyle(PressableStyle(scale: 0.98))
        .accessibilityLabel("File \(attachment.name)")
    }

    private func icon(for attachment: RoomAttachment) -> String {
        switch URL(fileURLWithPath: attachment.url).pathExtension.lowercased() {
        case "zip": return "archivebox.fill"
        case "pdf": return "doc.richtext.fill"
        case "csv", "json", "yaml", "yml": return "tablecells.fill"
        default: return "doc.text.fill"
        }
    }

    private func kind(for attachment: RoomAttachment) -> String {
        let ext = URL(fileURLWithPath: attachment.url).pathExtension.lowercased()
        return ext.isEmpty ? "FILE" : ext.uppercased()
    }
}

/// A decision an agent is waiting on, drawn where it was asked.
///
/// The choices are the room's own strings and are sent back verbatim: the room
/// validates them against the card, so echoing its words is the only thing that
/// can work. Tapping one does not answer it — it stages the answer for the
/// confirmation card, because the room will post the choice into the channel
/// and every agent will act on it.
struct DecisionCard: View {
    let message: RoomMessage
    let decision: RoomDecision
    @Environment(RoomModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 11) {
            HStack(spacing: 7) {
                ZStack {
                    Circle().fill((decision.resolved ? Theme.online : Theme.accent).opacity(0.18))
                    Image(systemName: decision.resolved ? "checkmark" : "questionmark")
                        .font(.system(size: 9, weight: .black))
                        .foregroundStyle(decision.resolved ? Theme.online : Theme.accent)
                }
                .frame(width: 18, height: 18)

                Eyebrow(
                    decision.resolved ? "answered" : "decision · waiting on you",
                    tint: decision.resolved ? Theme.online : Theme.accent
                )
                Spacer(minLength: 0)
            }

            if decision.resolved {
                Text(decision.chosen.flatMap { $0.isEmpty ? nil : $0 } ?? "resolved")
                    .font(Type.body(15, .semibold))
                    .foregroundStyle(Theme.ink)
            } else {
                HStack(spacing: 9) {
                    ForEach(decision.choices, id: \.self) { choice in
                        Button {
                            model.stage(decision: message, choice: choice)
                        } label: {
                            Text(choice)
                                .font(Type.body(14, .bold))
                                .foregroundStyle(Theme.background)
                                .padding(.horizontal, 18)
                                .padding(.vertical, 9)
                                .background(Theme.accentGradient, in: Capsule())
                                .shadow(color: Theme.accent.opacity(0.4), radius: 10, y: 4)
                        }
                        .buttonStyle(PressableStyle(scale: 0.93))
                        .disabled(model.resolvingDecision != nil)
                        .opacity(model.resolvingDecision == nil ? 1 : 0.5)
                    }
                    if model.resolvingDecision == message.id {
                        ProgressView().controlSize(.small).tint(Theme.accent)
                    }
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Theme.background.opacity(0.55))
        }
        .overlay {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(
                    (decision.resolved ? Theme.online : Theme.accent).opacity(decision.resolved ? 0.25 : 0.42),
                    lineWidth: 1
                )
        }
    }
}
