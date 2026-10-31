import SwiftUI
import RigelRoomKit

/// First screen: the mark, the address, the optional token, and the honest note
/// about what binding a room to the network means.
struct ConnectView: View {
    @Environment(RoomModel.self) private var model
    @State private var showingCloud = false

    var body: some View {
        @Bindable var model = model
        ZStack {
            AuroraBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    hero
                    fields(
                        address: $model.addressText,
                        alternate: $model.alternateText,
                        token: $model.tokenText
                    )
                    if let problem = model.connectError {
                        NoticeBanner(title: problem)
                    }
                    connectButton
                    cloudButton
                    steps
                }
                .padding(22)
                .padding(.bottom, 24)
            }
            .scrollIndicators(.hidden)
        }
        .sheet(isPresented: $showingCloud) {
            CloudSessionsView()
        }
    }

    private var hero: some View {
        VStack(alignment: .leading, spacing: 10) {
            StarMark(size: 62)
                .padding(.bottom, 2)
            Text("Rigel")
                .font(Type.display(44, .bold))
                .foregroundStyle(Theme.wordmark)
                .shadow(color: Theme.accent.opacity(0.25), radius: 24)
            Text("The room, the board and the rule book from your computer — live on this phone.")
                .font(Type.body(15))
                .foregroundStyle(Theme.inkMuted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.top, 8)
    }

    /// The cloud is independent of any room: a published session is readable
    /// even when the computer is off, which is the whole point of sending it.
    private var cloudButton: some View {
        Button {
            showingCloud = true
        } label: {
            HStack(spacing: 9) {
                Image(systemName: "cloud")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.cool)
                Text("Sessions published from your computer")
                    .font(Type.body(12.5, .medium))
                    .foregroundStyle(Theme.inkMuted)
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(Theme.inkFaint)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(Theme.panel.opacity(0.45), in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.panelEdge))
        }
        .buttonStyle(PressableStyle())
        .accessibilityHint("Opens the sessions published to your Firebase project")
    }

    /// The three fields take `Binding`s rather than the model: `@Bindable`'s
    /// `$model` projection only exists in the view's `body`, and a helper that
    /// wants to write through the model has to be handed the bindings — the
    /// same shape as the room's composer.
    private func fields(
        address: Binding<String>,
        alternate: Binding<String>,
        token: Binding<String>
    ) -> some View {
        GlassCard(radius: 22) {
            VStack(alignment: .leading, spacing: 16) {
                field("Room address", text: address, prompt: "192.168.1.42:8300 or https://host.ts.net")
                field("If that one does not answer", text: alternate, prompt: "optional — the room's other name")
                field("Session token", text: token, prompt: "optional — fetched from the room when blank")

                HStack(alignment: .top, spacing: 7) {
                    Image(systemName: "moon.stars")
                        .font(.system(size: 10, weight: .semibold))
                        .padding(.top, 1)
                    Text(
                        "One room usually has two names: its LAN address at home and its tailnet name "
                            + "away. Both are kept, and whichever answers leads the next attempt."
                    )
                    .font(Type.body(11.5))
                    .fixedSize(horizontal: false, vertical: true)
                }
                .foregroundStyle(Theme.inkFaint)
            }
            .padding(16)
        }
    }

    private func field(_ label: String, text: Binding<String>, prompt: String) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Eyebrow(label)
            TextField(prompt, text: text)
                .plainRoomEntry()
                .font(Type.body(14.5))
                .foregroundStyle(Theme.ink)
                .tint(Theme.accent)
                .padding(.horizontal, 12)
                .padding(.vertical, 11)
                .background(Theme.background.opacity(0.55), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(Theme.panelEdge)
                )
        }
    }

    private var connectButton: some View {
        Button {
            Task { await model.connect() }
        } label: {
            HStack(spacing: 9) {
                if model.isBusy {
                    ProgressView().controlSize(.small).tint(Theme.background)
                } else {
                    Image(systemName: "arrow.right")
                        .font(.system(size: 13, weight: .bold))
                }
                Text(model.isBusy ? "Connecting…" : "Connect")
            }
        }
        .buttonStyle(PrimaryButtonStyle())
        // Either field is enough to connect: the model tries whichever names
        // the person gave it.
        .disabled(
            model.isBusy
                || RoomAddress.candidates(from: [model.addressText, model.alternateText]).isEmpty
        )
        .opacity(
            RoomAddress.candidates(from: [model.addressText, model.alternateText]).isEmpty ? 0.5 : 1
        )
    }

    private var steps: some View {
        GlassCard(radius: 20) {
            VStack(alignment: .leading, spacing: 14) {
                Eyebrow("on the computer")
                step("1", "Start the room: Rigel's Chat tab, or agentchattr with server.host set to this machine's address and --allow-network.")
                step("2", "It prints “Web UI: http://<ip>:8300” and a “Session token: …” line. Type that address here.")
                step("3", "Both devices on the same Wi-Fi. iOS asks for Local Network access on the first connect — choose Allow.")
                step("4", "Away from home: run `tailscale serve --bg 8300` on the computer, which leaves the room on 127.0.0.1, and put the https://…ts.net name it prints in the second field here.")
                FadingRule()
                Text(
                    "A network-bound room is plain HTTP, for a trusted network only: anyone on it "
                        + "can read the room and its token. The tailnet name is TLS, and the room never "
                        + "leaves loopback."
                )
                .font(Type.body(11.5))
                .foregroundStyle(Theme.inkFaint)
                .fixedSize(horizontal: false, vertical: true)
            }
            .padding(16)
        }
    }

    private func step(_ number: String, _ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(number)
                .font(Type.label(10, .bold))
                .foregroundStyle(Theme.background)
                .frame(width: 18, height: 18)
                .background(Theme.accentGradient, in: Circle())
            Text(text)
                .font(Type.body(12))
                .foregroundStyle(Theme.inkMuted)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
