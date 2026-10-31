import Foundation
import XCTest

@testable import RigelRoomKit

/// What a person types into the connect screen, and what the client builds
/// from it. Every case here is something a keyboard on a phone actually
/// produces: autocorrect dots, a pasted banner, a URL from the room's own
/// startup printout.
final class RoomAddressTests: XCTestCase {

    func testBareHostUsesTheRoomsDefaultPort() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "192.168.1.42"))
        XCTAssertEqual(address.host, "192.168.1.42")
        XCTAssertEqual(address.port, 8300)
        XCTAssertEqual(address.displayName, "192.168.1.42")
        XCTAssertEqual(address.baseURL?.absoluteString, "http://192.168.1.42:8300")
    }

    func testAnExplicitPortWins() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "192.168.1.42:8311"))
        XCTAssertEqual(address.port, 8311)
        XCTAssertEqual(address.displayName, "192.168.1.42:8311")
    }

    func testAPastedURLKeepsOnlyItsAuthority() throws {
        let address = try XCTUnwrap(RoomAddress(raw: " http://192.168.1.42:8311/ "))
        XCTAssertEqual(address.host, "192.168.1.42")
        XCTAssertEqual(address.port, 8311)
        XCTAssertEqual(address.baseURL?.absoluteString, "http://192.168.1.42:8311")

        let withPath = try XCTUnwrap(RoomAddress(raw: "http://rigel.local:8300/chat?room=1#x"))
        XCTAssertEqual(withPath.host, "rigel.local")
        XCTAssertEqual(withPath.port, 8300)
    }

    /// The room prints `http://<tailnet-name>:8300`; Tailscale and mDNS names
    /// carry dots and dashes and nothing else unusual.
    func testHostNamesSurvive() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "workstation.tail1a2b3.ts.net"))
        XCTAssertEqual(address.host, "workstation.tail1a2b3.ts.net")
        XCTAssertEqual(address.baseURL?.absoluteString, "http://workstation.tail1a2b3.ts.net:8300")
    }

    /// `tailscale serve` puts the room's loopback port on the tailnet name over
    /// HTTPS, and that name takes no port of its own.
    func testATailnetNameIsHTTPSOnFourFourThree() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "https://workstation.tail1a2b3.ts.net"))
        XCTAssertEqual(address.scheme, "https")
        XCTAssertEqual(address.host, "workstation.tail1a2b3.ts.net")
        XCTAssertEqual(address.port, 443)
        XCTAssertEqual(address.displayName, "https://workstation.tail1a2b3.ts.net")
        XCTAssertEqual(address.baseURL?.absoluteString, "https://workstation.tail1a2b3.ts.net")
    }

    func testAnExplicitPortOnAnHTTPSAddressIsKept() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "https://workstation.tail1a2b3.ts.net:8443"))
        XCTAssertEqual(address.port, 8443)
        XCTAssertEqual(address.displayName, "https://workstation.tail1a2b3.ts.net:8443")
        XCTAssertEqual(address.baseURL?.absoluteString, "https://workstation.tail1a2b3.ts.net:8443")
    }

    /// A TLS front end can sit in front of a LAN address too; the port the room
    /// actually runs on is then worth spelling out.
    func testHTTPSKeepsTheRoomsOwnPortWhenItIsNamed() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "https://192.168.1.42:8300"))
        XCTAssertEqual(address.scheme, "https")
        XCTAssertEqual(address.port, 8300)
        XCTAssertEqual(address.displayName, "https://192.168.1.42:8300")
    }

    /// The escape hatch for a name that is not a name: the tailnet IP.
    func testAnHttpsAddressWithoutASchemeStaysPlainHTTP() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "100.101.102.103"))
        XCTAssertEqual(address.scheme, "http")
        XCTAssertEqual(address.port, 8300)
    }

    func testTrailingDotFromAutocorrectIsTrimmed() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "studio.local."))
        XCTAssertEqual(address.host, "studio.local")
    }

    func testIPv6LiteralKeepsItsBracketsWhenShown() throws {
        let address = try XCTUnwrap(RoomAddress(raw: "[fe80::1]:8311"))
        XCTAssertEqual(address.host, "fe80::1")
        XCTAssertEqual(address.port, 8311)
        XCTAssertEqual(address.displayName, "[fe80::1]:8311")
    }

    /// A room the phone knows by two names: the order they were given in is the
    /// order they are tried, a name written twice is only tried once, and
    /// anything that cannot be an address is not a mistake worth a round trip.
    func testCandidateAddressesKeepOrderAndDropWhatCannotBeUsed() {
        let ordered = RoomAddress.candidates(from: [
            "192.168.1.42:8300",
            "   ",
            "not an address",
            "192.168.1.42",
            "https://workstation.tail1a2b3.ts.net",
        ])
        XCTAssertEqual(
            ordered.map(\.displayName),
            ["192.168.1.42", "https://workstation.tail1a2b3.ts.net"]
        )

        // Away from home it is the other way round, and that order is what the
        // phone tries first.
        let away = RoomAddress.candidates(from: [
            "https://workstation.tail1a2b3.ts.net",
            "192.168.1.42",
        ])
        XCTAssertEqual(away.map(\.scheme), ["https", "http"])

        XCTAssertTrue(RoomAddress.candidates(from: []).isEmpty)
        XCTAssertTrue(RoomAddress.candidates(from: ["", "  ", "host:0"]).isEmpty)
    }

    func testAnythingThatCannotBeARoomIsRejected() {
        for raw in ["", "   ", "with space", "host:0", "host:70000", "ftp://host",
                    "host:port", "[fe80::1", "192.168.1.42:"] {
            XCTAssertNil(RoomAddress(raw: raw), "\(raw.isEmpty ? "(empty)" : raw) should not parse")
        }
    }

    func testURLsCarryTheQueryTheClientSends() throws {
        let address = RoomAddress(host: "192.168.1.42", port: 8311)
        let url = try XCTUnwrap(
            address.url(path: "/api/messages", query: [("since_id", "2"), ("limit", "50")])
        )
        XCTAssertEqual(url.absoluteString, "http://192.168.1.42:8311/api/messages?since_id=2&limit=50")
        XCTAssertEqual(address.url(path: "/api/settings")?.absoluteString, "http://192.168.1.42:8311/api/settings")
    }
}

/// The token field accepts more than a bare token, because people paste
/// whatever they copied: the banner line, a quoted string, a whole URL.
final class RoomTokenTests: XCTestCase {

    func testNormalizingAcceptsTheShapesPeoplePaste() {
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        XCTAssertEqual(RoomToken.normalized("  \(token)  "), token)
        XCTAssertEqual(RoomToken.normalized("Session token: \(token)"), token)
        XCTAssertEqual(RoomToken.normalized("\"\(token)\""), token)
        XCTAssertEqual(RoomToken.normalized("http://192.168.1.42:8300/?token=\(token)&x=1"), token)
    }

    func testScanPrefersTheTokenOverAProseMention() throws {
        let token = String(repeating: "ab12cd34", count: 8)
        let page = """
        <p>The __SESSION_TOKEN__ marker is documented below.</p>
        <script>window.__SESSION_TOKEN__="\(token)";</script>
        """
        XCTAssertEqual(RoomToken.scan(page), token)
    }

    func testScanSkipsAValueThatCannotBeAToken() throws {
        let token = String(repeating: "9f", count: 32)
        let page = """
        <script>window.__SESSION_TOKEN__="too-short";</script>
        <script>window.__SESSION_TOKEN__="\(token)";</script>
        """
        XCTAssertEqual(RoomToken.scan(page), token)
    }

    func testScanFindsNothingOnAPageWithoutTheMarker() {
        XCTAssertNil(RoomToken.scan("<html><body>hello</body></html>"))
    }

    func testPlausibilityMatchesTheServersOwnShape() {
        // agentchattr mints 32 bytes of hex; the desktop accepts 32–128
        // hex characters. Anything else is not a token.
        XCTAssertTrue(RoomToken.isPlausible(String(repeating: "a", count: 32)))
        XCTAssertTrue(RoomToken.isPlausible(String(repeating: "f", count: 128)))
        XCTAssertFalse(RoomToken.isPlausible(String(repeating: "a", count: 31)))
        XCTAssertFalse(RoomToken.isPlausible(String(repeating: "a", count: 129)))
        XCTAssertFalse(RoomToken.isPlausible(String(repeating: "z", count: 64)))
    }
}
