import Foundation
import EventKit
import AVFoundation
import CoreGraphics

// Read-only EventKit bridge. Only the explicit `connect` verb requests access.
// No event is saved and no calendar data is inspected by `status` or `permissions`.
@main struct CalendarBridge {
    static func send(_ value: Any) {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) else { exit(1) }
        FileHandle.standardOutput.write(data)
    }
    static func status() -> [String: Any] {
        let auth = EKEventStore.authorizationStatus(for: .event)
        return ["available":true,"connected":auth == .fullAccess,"source":"apple-calendar",
                "message": auth == .fullAccess ? "Connected to Apple Calendar. Tuck only reads upcoming events." : "Connect Apple Calendar to read upcoming events. You can also import an ICS file."]
    }
    @MainActor static func main() async {
        let args = Array(CommandLine.arguments.dropFirst())
        switch args.first {
        case "permissions":
            let mic = AVCaptureDevice.authorizationStatus(for: .audio)
            let value = mic == .authorized ? "authorized" : mic == .notDetermined ? "notDetermined" : mic == .denied ? "denied" : "restricted"
            send(["microphone":value,"system":CGPreflightScreenCaptureAccess()])
        case "request-microphone":
            let allowed = await AVCaptureDevice.requestAccess(for: .audio)
            send(["granted":allowed])
        case "status": send(status())
        case "connect":
            do { _ = try await EKEventStore().requestFullAccessToEvents(); send(status()) }
            catch { send(["available":true,"connected":false,"source":"apple-calendar","message":"Calendar permission was not granted. Allow Tuck in System Settings → Privacy & Security → Calendars, or import an ICS file."]) }
        case "events":
            guard EKEventStore.authorizationStatus(for: .event) == .fullAccess else { send(["error":"Connect Apple Calendar first."]); return }
            let formatter = ISO8601DateFormatter()
            guard args.count == 3, let from = formatter.date(from:args[1]), let to = formatter.date(from:args[2]), to > from, to.timeIntervalSince(from) <= 31*86400 else { send(["error":"Choose a date range of at most 31 days."]); return }
            let store = EKEventStore()
            let predicate = store.predicateForEvents(withStart:from,end:to,calendars:nil)
            let events = store.events(matching:predicate).sorted { $0.startDate < $1.startDate }.prefix(250).map { event -> [String:Any] in
                let url = event.url?.absoluteString ?? ""
                let join = [url,event.location ?? "",event.notes ?? ""].compactMap { text -> String? in
                    guard let detector = try? NSDataDetector(types:NSTextCheckingResult.CheckingType.link.rawValue) else {return nil}
                    for match in detector.matches(in:text,range:NSRange(text.startIndex...,in:text)) {
                        if let link = match.url, ["https","http"].contains(link.scheme?.lowercased() ?? "") { return link.absoluteString }
                    }; return nil
                }.first ?? ""
                return ["id":"apple-calendar:\(event.calendarItemIdentifier):\(formatter.string(from:event.startDate))","source":"apple-calendar","title":event.title ?? "Untitled event","start":formatter.string(from:event.startDate),"end":formatter.string(from:event.endDate),"joinUrl":join,"attendees":(event.attendees ?? []).prefix(100).map { $0.name ?? $0.url.absoluteString }]
            }
            send(events)
        default: send(["error":"Unknown calendar command."])
        }
    }
}
