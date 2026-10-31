import SwiftUI

/// The whole app: the connect screen until a room is live, the room after.
struct RootView: View {
    @Environment(RoomModel.self) private var model
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        ZStack {
            Theme.background.ignoresSafeArea()
            if model.isConnected {
                ShellView()
            } else {
                ConnectView()
            }
        }
        .task { await model.restore() }
        .onChange(of: scenePhase) { _, phase in
            switch phase {
            case .active:
                model.resume()
            case .inactive, .background:
                model.pause()
            @unknown default:
                break
            }
        }
    }
}

extension View {
    /// Text entry for hosts and tokens: no autocapitalization, no smart
    /// quotes, the raw keyboard where the platform has one. iOS-only
    /// modifiers are compiled out elsewhere so the package builds on macOS.
    @ViewBuilder
    func plainRoomEntry() -> some View {
        #if os(iOS)
        self
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .keyboardType(.URL)
        #else
        self.autocorrectionDisabled()
        #endif
    }
}
