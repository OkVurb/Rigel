import SwiftUI

@main
struct RigelPhoneApp: App {
    @State private var model = RoomModel()
    @State private var cloud = CloudModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .environment(cloud)
                .preferredColorScheme(.dark)
        }
    }
}
