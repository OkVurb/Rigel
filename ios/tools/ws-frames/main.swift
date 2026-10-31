// Dump the frames a room pushes over `/ws`, one raw JSON frame per line, so
// the streaming tests can decode payloads a real server sent — the same reason
// `capture-fixtures.py` exists for the REST endpoints.
//
// It compiles against the kit's own sources on purpose: the address parsing and
// the frame decoding below are the shipping code, so a capture run also checks
// that the decoder agrees with the server that just spoke to it. stderr says
// what this client made of each frame; stdout is only the fixture.
//
//   swiftc -O ios/tools/ws-frames/main.swift \
//          ios/RigelPhone/Sources/RigelRoomKit/*.swift -o /tmp/ws-frames
//
//   /tmp/ws-frames 192.168.4.126:8311 "$TOKEN" 3 > ws_snapshot.jsonl
//
// To catch a frame the room only sends while the socket is open — a new line,
// or a roster change — run it in the background with a longer window and post
// something to the room while it waits.
//
// usage: ws-frames <room address> <token> [seconds]

import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

let arguments = CommandLine.arguments
guard arguments.count >= 3, let address = RoomAddress(raw: arguments[1]) else {
    FileHandle.standardError.write(
        Data("usage: ws-frames <room address> <token> [seconds]\n".utf8)
    )
    exit(2)
}
let token = arguments[2]
let seconds = arguments.count > 3 ? (Double(arguments[3]) ?? 3) : 3
guard let url = address.webSocketURL(path: "/ws", query: [("token", token)]) else {
    FileHandle.standardError.write(Data("could not build a socket URL from \(arguments[1])\n".utf8))
    exit(2)
}

/// What the shipping client makes of a frame. `ignored` frames are the room's
/// other business (todos, rules, schedules); the name after the colon is the
/// type it carried, or empty when the frame could not be read at all.
func kind(of frame: String) -> String {
    switch RoomEvent.decode(Data(frame.utf8)) {
    case .settings: return "settings"
    case .status: return "status"
    case .history(let batch): return "history(\(batch.count))"
    case .historyComplete: return "history_complete"
    case .message(let message): return "message(\(message.sender) #\(message.id))"
    case .delete(let ids): return "delete(\(ids.count))"
    case .clear(let channel): return "clear(\(channel ?? "whole room"))"
    case .ignored(let type): return "ignored:\(type)"
    }
}

func out(_ text: String) {
    FileHandle.standardOutput.write(Data((text + "\n").utf8))
}

func note(_ text: String) {
    FileHandle.standardError.write(Data((text + "\n").utf8))
}

let socket = URLSession.shared.webSocketTask(with: url)
socket.resume()
let finished = DispatchSemaphore(value: 0)

func receive() {
    socket.receive { result in
        switch result {
        case .success(let message):
            let frame: String
            switch message {
            case .string(let text): frame = text
            case .data(let data): frame = String(decoding: data, as: UTF8.self)
            @unknown default: frame = ""
            }
            out(frame)
            note(kind(of: frame))
            receive()
        case .failure(let error):
            note("socket ended: \(error)")
            finished.signal()
        }
    }
}

note("connected to \(address.webSocketURL(path: "/ws", query: [])?.absoluteString ?? "?") (token omitted)")
receive()
DispatchQueue.global().asyncAfter(deadline: .now() + seconds) { finished.signal() }
finished.wait()
socket.cancel(with: .normalClosure, reason: nil)
note("done after \(seconds)s")
