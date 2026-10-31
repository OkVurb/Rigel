import PhotosUI
import SwiftUI
import UniformTypeIdentifiers
import RigelRoomKit
#if os(iOS)
import UIKit
#endif

/// The live room: who is online, what was said, and the two writes this app is
/// allowed — a confirmed line, and a confirmed answer to a decision card.
struct RoomView: View {
    @Environment(RoomModel.self) private var model
    @State private var showingSettings = false
    @State private var pickingPhotos = false
    @State private var pickingFiles = false
    @State private var photoItem: PhotosPickerItem?

    var body: some View {
        @Bindable var model = model
        VStack(spacing: 0) {
            header
            if model.channels.count > 1 {
                channels
            }
            presence
            timeline
            banners
            composer(draft: $model.draft)
        }
        .sheet(isPresented: $showingSettings) {
            RoomSettingsSheet()
        }
        .sheet(item: decisionConfirmation) { answer in
            DecisionConfirmation(answer: answer)
        }
    }

    // MARK: - Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Eyebrow("the room")
                    Text(model.roomTitle)
                        .font(Type.display(30, .bold))
                        .foregroundStyle(Theme.wordmark)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                    Text(model.roomSubtitle.isEmpty ? model.connectedAddress : model.roomSubtitle)
                        .font(Type.label(10.5))
                        .foregroundStyle(Theme.inkFaint)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                statusPill
                // The mark is the settings control: a tap opens the room's
                // settings, holding charges the star until it bursts.
                ChargeStar(size: 24) { showingSettings = true }
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 10)
        .padding(.bottom, 14)
    }

    private var statusPill: some View {
        HStack(spacing: 6) {
            PulseDot(color: model.streamError == nil ? Theme.online : Theme.accent)
            Text(model.streamError == nil ? "live" : "reconnecting")
                .font(Type.label(10, .semibold))
                .foregroundStyle(model.streamError == nil ? Theme.inkMuted : Theme.accent)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(Theme.panelEdge))
        .accessibilityLabel(model.streamError == nil ? "Live" : "Reconnecting")
    }

    private var channels: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(model.channels, id: \.self) { name in
                    Button {
                        withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                            model.select(channel: name)
                        }
                    } label: {
                        SelectChip(
                            title: "#\(name)",
                            selected: name == model.channel,
                            tint: Theme.accent
                        )
                    }
                    .buttonStyle(PressableStyle())
                }
            }
            .padding(.horizontal, 20)
        }
        .padding(.bottom, 12)
    }

    // MARK: - Presence

    private var presence: some View {
        let agents = model.status?.agents ?? []
        return HStack(spacing: 10) {
            if agents.isEmpty {
                Text("No agents in the room")
                    .font(Type.body(12))
                    .foregroundStyle(Theme.inkFaint)
            } else {
                HStack(spacing: -9) {
                    ForEach(agents.prefix(6)) { agent in
                        AgentAvatar(agent: agent, size: 30)
                            .background(Circle().fill(Theme.background))
                    }
                }
                VStack(alignment: .leading, spacing: 1) {
                    Text("\(agents.filter(\.available).count) of \(agents.count) online")
                        .font(Type.body(12, .semibold))
                        .foregroundStyle(Theme.inkMuted)
                    Text(agents.filter(\.available).map(\.label).prefix(3).joined(separator: " · "))
                        .font(Type.label(9.5))
                        .foregroundStyle(Theme.inkFaint)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
            if model.status?.paused == true {
                Text("PAUSED")
                    .font(Type.label(9.5, .bold))
                    .tracking(1.2)
                    .foregroundStyle(Theme.problem)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background(Theme.problem.opacity(0.12), in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.problem.opacity(0.35)))
            }
        }
        .padding(.horizontal, 20)
        .padding(.bottom, 12)
    }

    // MARK: - Timeline

    private var timeline: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if model.messages.isEmpty {
                        emptyState
                    }
                    ForEach(model.messages) { message in
                        MessageRow(
                            message: message,
                            ownName: model.username,
                            agents: model.status?.agents ?? [],
                            attachmentBase: model.attachmentBase
                        )
                        .id(message.id)
                        .transition(
                            .asymmetric(
                                insertion: .opacity.combined(with: .move(edge: .bottom)),
                                removal: .opacity
                            )
                        )
                    }
                }
                .padding(.horizontal, 18)
                .padding(.top, 6)
                .padding(.bottom, 18)
            }
            .scrollIndicators(.hidden)
            .defaultScrollAnchor(.bottom)
            .animation(.spring(response: 0.4, dampingFraction: 0.85), value: model.messages.last?.id)
            .onChange(of: model.messages.last?.id) { _, _ in
                guard let last = model.messages.last?.id else { return }
                withAnimation(.easeOut(duration: 0.2)) {
                    proxy.scrollTo(last, anchor: .bottom)
                }
            }
        }
    }

    private var emptyState: some View {
        VStack(alignment: .center, spacing: 12) {
            StarMark(size: 44, animated: false)
                .opacity(0.75)
            Text("Nothing in #\(model.channel) yet")
                .font(Type.display(17, .semibold))
                .foregroundStyle(Theme.ink)
            Text("Lines appear here as the room says them. Mention an agent to hand it work.")
                .font(Type.body(13))
                .foregroundStyle(Theme.inkFaint)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 44)
        .padding(.horizontal, 24)
    }

    // MARK: - Banners

    @ViewBuilder
    private var banners: some View {
        let problems = [
            model.streamError,
            model.sendError,
            model.decisionError,
            model.uploadError,
        ].compactMap { $0 }

        if let first = problems.first {
            NoticeBanner(title: first)
                .padding(.horizontal, 18)
                .padding(.bottom, 8)
                .transition(.opacity.combined(with: .move(edge: .bottom)))
        }
    }

    // MARK: - Composing

    private func composer(draft: Binding<String>) -> some View {
        VStack(spacing: 8) {
            if model.isUploading || !model.pendingAttachments.isEmpty {
                attachmentTray
            }

            HStack(alignment: .bottom, spacing: 8) {
                attachButton

                TextField("Message #\(model.channel) — @claude to hand it work", text: draft, axis: .vertical)
                    .lineLimit(1...5)
                    .font(Type.body(15))
                    .foregroundStyle(Theme.ink)
                    .tint(Theme.accent)
                    .padding(.vertical, 8)

                Button {
                    model.proposeSend()
                } label: {
                    Group {
                        if model.isSending {
                            ProgressView().controlSize(.small).tint(Theme.background)
                        } else {
                            Image(systemName: "arrow.up")
                                .font(.system(size: 16, weight: .bold))
                        }
                    }
                    .foregroundStyle(Theme.background)
                    .frame(width: 40, height: 40)
                    .background(
                        model.canSend ? AnyShapeStyle(Theme.accentGradient) : AnyShapeStyle(Theme.panelHigh),
                        in: Circle()
                    )
                    .shadow(color: model.canSend ? Theme.accent.opacity(0.4) : .clear, radius: 12, y: 4)
                }
                .buttonStyle(PressableStyle(scale: 0.92))
                .disabled(!model.canSend)
                .accessibilityLabel("Send. You will be asked to confirm the line first.")
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .background {
                RoundedRectangle(cornerRadius: 26, style: .continuous)
                    .fill(.ultraThinMaterial)
                    .overlay {
                        RoundedRectangle(cornerRadius: 26, style: .continuous)
                            .fill(Theme.panel.opacity(0.5))
                    }
            }
            .overlay {
                RoundedRectangle(cornerRadius: 26, style: .continuous)
                    .strokeBorder(Theme.panelEdge, lineWidth: 1)
            }
            .shadow(color: .black.opacity(0.4), radius: 16, y: 8)
        }
        .padding(.horizontal, 18)
        .padding(.bottom, 10)
        .sheet(item: confirmation) { item in
            SendConfirmation(line: item.line)
        }
        .photosPicker(isPresented: $pickingPhotos, selection: $photoItem, matching: .images)
        .onChange(of: photoItem) { _, item in
            handlePhoto(item)
        }
        .fileImporter(
            isPresented: $pickingFiles,
            allowedContentTypes: [.item],
            allowsMultipleSelection: true
        ) { result in
            handleFiles(result)
        }
    }

    private var attachButton: some View {
        Menu {
            Button {
                pickingPhotos = true
            } label: {
                Label("Photo library", systemImage: "photo.on.rectangle")
            }
            Button {
                pickingFiles = true
            } label: {
                Label("Choose a file", systemImage: "folder")
            }
        } label: {
            Image(systemName: "paperclip")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(Theme.inkMuted)
                .frame(width: 34, height: 34)
                .background(Circle().fill(Theme.panel.opacity(0.7)))
                .overlay(Circle().strokeBorder(Theme.panelEdge))
        }
        .disabled(model.isUploading)
        .accessibilityLabel("Attach a photo or a file")
    }

    /// The files already in the room, waiting for the line: a thumbnail or a
    /// document tile, the name, and a way to take it back off.
    private var attachmentTray: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(model.pendingAttachments) { attachment in
                    HStack(spacing: 7) {
                        if attachment.isImage {
                            AsyncImage(url: model.url(for: attachment)) { phase in
                                if case .success(let image) = phase {
                                    image.resizable().scaledToFill()
                                } else {
                                    Image(systemName: "photo")
                                        .font(.system(size: 10))
                                        .foregroundStyle(Theme.inkFaint)
                                }
                            }
                            .frame(width: 26, height: 26)
                            .clipShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
                        } else {
                            Image(systemName: "doc.fill")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(Theme.cool)
                                .frame(width: 26, height: 26)
                                .background(
                                    Theme.cool.opacity(0.12),
                                    in: RoundedRectangle(cornerRadius: 7, style: .continuous)
                                )
                        }

                        Text(attachment.name)
                            .font(Type.body(11.5, .medium))
                            .foregroundStyle(Theme.inkMuted)
                            .lineLimit(1)
                            .frame(maxWidth: 120, alignment: .leading)

                        Button {
                            model.discard(attachment)
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 8, weight: .bold))
                                .foregroundStyle(Theme.inkFaint)
                                .frame(width: 18, height: 18)
                        }
                        .buttonStyle(PressableStyle())
                        .accessibilityLabel("Remove \(attachment.name)")
                    }
                    .padding(.leading, 6)
                    .padding(.trailing, 5)
                    .padding(.vertical, 4)
                    .background(Theme.panel.opacity(0.78), in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.panelEdge))
                }

                if model.isUploading {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.mini).tint(Theme.accent)
                        Text("Uploading…")
                            .font(Type.label(9.5))
                            .foregroundStyle(Theme.inkFaint)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(Theme.panel.opacity(0.78), in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.panelEdge))
                }
            }
            .padding(.horizontal, 2)
            .padding(.vertical, 1)
        }
    }

    /// A photo is re-encoded as JPEG before it leaves the phone: the picker can
    /// hand back HEIC, which the room does not store.
    private func handlePhoto(_ item: PhotosPickerItem?) {
        guard let item else { return }
        photoItem = nil
        Task {
            guard let data = try? await item.loadTransferable(type: Data.self) else {
                model.refuseUpload("That photo could not be read.")
                return
            }
            #if os(iOS)
            guard let image = UIImage(data: data),
                  let jpeg = image.jpegData(compressionQuality: 0.86)
            else {
                model.refuseUpload("That photo could not be read.")
                return
            }
            await model.upload(
                jpeg,
                filename: "photo-\(Int(Date().timeIntervalSince1970)).jpg",
                mimeType: "image/jpeg"
            )
            #else
            await model.upload(data, filename: "photo.jpg", mimeType: "image/jpeg")
            #endif
        }
    }

    private func handleFiles(_ result: Result<[URL], Error>) {
        switch result {
        case .failure(let error):
            model.refuseUpload(error.localizedDescription)
        case .success(let urls):
            Task {
                for url in urls.prefix(6) {
                    let scoped = url.startAccessingSecurityScopedResource()
                    defer {
                        if scoped { url.stopAccessingSecurityScopedResource() }
                    }
                    let ext = url.pathExtension.lowercased()
                    guard UploadRules.extensions.contains(ext) else {
                        model.refuseUpload(
                            "The room does not store .\(ext.isEmpty ? "?" : ext) files."
                        )
                        continue
                    }
                    do {
                        let data = try Data(contentsOf: url)
                        guard data.count <= UploadRules.maxBytes else {
                            model.refuseUpload("\(url.lastPathComponent) is larger than 10 MB.")
                            continue
                        }
                        let mime = UTType(filenameExtension: ext)?.preferredMIMEType
                            ?? "application/octet-stream"
                        await model.upload(
                            data,
                            filename: url.lastPathComponent,
                            mimeType: mime
                        )
                    } catch {
                        model.refuseUpload(error.localizedDescription)
                    }
                }
            }
        }
    }

    /// The confirmation card's item, and the identity a sheet wants: a line is
    /// its channel and its text. Dismissing the card any way at all — the
    /// Cancel button, a downward swipe — cancels the send, because a card that
    /// is gone is not a send.
    private var confirmation: Binding<ConfirmSend?> {
        Binding(
            get: { model.pendingSend.map(ConfirmSend.init) },
            set: { if $0 == nil { model.cancelSend() } }
        )
    }

    private var decisionConfirmation: Binding<DecisionAnswer?> {
        Binding(
            get: { model.pendingDecision },
            set: { if $0 == nil { model.cancelDecision() } }
        )
    }
}

/// What the room will keep, mirrored from `ALLOWED_UPLOAD_EXTS` in
/// `agentchattr/app.py`. Checked before the bytes are read, so a file the room
/// would refuse is a sentence rather than a wasted upload.
private enum UploadRules {
    static let extensions: Set<String> = [
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg",
        "pdf", "txt", "md", "markdown", "csv", "json", "yaml", "yml", "log", "zip",
    ]
    static let maxBytes = 10 * 1024 * 1024
}

/// The line, with the identity `sheet(item:)` needs.
private struct ConfirmSend: Identifiable {
    let line: RoomLine
    var id: String { line.channel + "\u{1}" + line.text }
}

// MARK: - Confirmation: a line

/// The confirmation step: the line exactly as the room will store it, where it
/// lands, and the boundary this app keeps.
private struct SendConfirmation: View {
    let line: RoomLine
    @Environment(RoomModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack {
            AuroraBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    VStack(alignment: .leading, spacing: 6) {
                        Eyebrow("sending to #\(line.channel)")
                        Text(line.text.isEmpty ? "Send these files?" : "Say this?")
                            .font(Type.display(26, .bold))
                            .foregroundStyle(Theme.ink)
                    }

                    if !line.text.isEmpty {
                        GlassCard(radius: 20) {
                            Text(line.text)
                                .font(Type.body(16))
                                .foregroundStyle(Theme.ink)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(16)
                        }
                    }

                    if !line.attachments.isEmpty {
                        AttachmentList(attachments: line.attachments, base: model.attachmentBase)
                    }

                    Text(
                        "It goes to #\(line.channel) in \(model.roomTitle), where the room's agents "
                            + "can see it and answer it. Nothing else is written from here: a decision "
                            + "is answered on its own card, never as prose."
                    )
                    .font(Type.body(13))
                    .foregroundStyle(Theme.inkMuted)
                    .fixedSize(horizontal: false, vertical: true)

                    Button {
                        Task {
                            await model.confirmSend()
                            dismiss()
                        }
                    } label: {
                        HStack(spacing: 8) {
                            if model.isSending {
                                ProgressView().controlSize(.small).tint(Theme.background)
                            }
                            Text(model.isSending ? "Sending…" : "Send to the room")
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(model.isSending)

                    Button("Cancel") { dismiss() }
                        .font(Type.body(15, .medium))
                        .foregroundStyle(Theme.inkMuted)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .padding(22)
            }
        }
        .presentationBackground(Theme.background)
        .presentationDragIndicator(.visible)
    }
}

// MARK: - Confirmation: a decision

/// The second write in this app: answering the card an agent is blocked on.
///
/// It shows the card's own words, the choice in the room's spelling, and where
/// the answer will land — the room posts `@sender <choice>` in the card's
/// channel, which is what wakes the agent that asked.
private struct DecisionConfirmation: View {
    let answer: DecisionAnswer
    @Environment(RoomModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack {
            AuroraBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    VStack(alignment: .leading, spacing: 6) {
                        Eyebrow("\(answer.message.sender) is waiting")
                        Text("Answer this card?")
                            .font(Type.display(26, .bold))
                            .foregroundStyle(Theme.ink)
                    }

                    GlassCard(radius: 20, glow: Theme.accent) {
                        Text(answer.message.text)
                            .font(Type.body(15.5))
                            .foregroundStyle(Theme.ink)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(16)
                    }

                    VStack(alignment: .leading, spacing: 8) {
                        Eyebrow("your answer")
                        Text(answer.choice)
                            .font(Type.display(30, .bold))
                            .foregroundStyle(Theme.accentGradient)
                    }

                    Text(
                        "The room records this on the card and posts “@\(answer.message.sender) "
                            + "\(answer.choice)” in #\(answer.message.channel). That line is what lets "
                            + "the agent carry on, and every client will see it."
                    )
                    .font(Type.body(13))
                    .foregroundStyle(Theme.inkMuted)
                    .fixedSize(horizontal: false, vertical: true)

                    Button {
                        Task {
                            await model.confirmDecision()
                            dismiss()
                        }
                    } label: {
                        HStack(spacing: 8) {
                            if model.resolvingDecision == answer.message.id {
                                ProgressView().controlSize(.small).tint(Theme.background)
                            }
                            Text(
                                model.resolvingDecision == answer.message.id
                                    ? "Answering…"
                                    : "Answer \(answer.choice)"
                            )
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(model.resolvingDecision != nil)

                    Button("Cancel") { dismiss() }
                        .font(Type.body(15, .medium))
                        .foregroundStyle(Theme.inkMuted)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .padding(22)
            }
        }
        .presentationBackground(Theme.background)
        .presentationDragIndicator(.visible)
    }
}
