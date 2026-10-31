// swift-tools-version: 6.0

import Foundation
import PackageDescription

// `swift test` plans every target in the package, and the app target needs
// SwiftUI and Security, which exist only on Apple platforms — but this app is
// built by xtool *on Linux*, so the target has to stay in the manifest for that
// build to work. The one case that needs it gone is running the tests here,
// where the wire format is verified against real captured payloads:
//
//     RIGEL_LINUX_TEST=1 swift test
//
// Every other build — `xtool dev`, `swift build --triple <ios-triple>`, and a
// plain `swift test` on macOS — sees an ordinary xtool project: one library
// product, the app, which is the shape xtool requires.
let appTargetIncluded = ProcessInfo.processInfo.environment["RIGEL_LINUX_TEST"] != "1"

var products: [Product] = [
    .library(
        name: appTargetIncluded ? "RigelPhone" : "RigelRoomKit",
        targets: [appTargetIncluded ? "RigelPhone" : "RigelRoomKit"]
    ),
]

var targets: [Target] = [
    // The room's wire format and HTTP client, deliberately free of SwiftUI and
    // of Apple-only frameworks. Linux can build and test this target, which is
    // where the contract is verified.
    .target(
        name: "RigelRoomKit"
    ),
    .testTarget(
        name: "RigelRoomKitTests",
        dependencies: ["RigelRoomKit"],
        resources: [.copy("Fixtures")]
    ),
]

if appTargetIncluded {
    // The app itself: SwiftUI and platform storage only.
    targets.insert(
        .target(
            name: "RigelPhone",
            dependencies: ["RigelRoomKit"]
        ),
        at: 0
    )
}

let package = Package(
    name: "RigelPhone",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: products,
    targets: targets
)
