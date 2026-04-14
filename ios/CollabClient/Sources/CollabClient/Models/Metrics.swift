import Foundation

struct Metrics: Codable {
    let totalMessages: Int?
    let messagesLastHour: Int?
    let activeWorkers: Int?
    let sseSubscribers: Int?
    let uptimeSecs: Int?

    enum CodingKeys: String, CodingKey {
        case totalMessages    = "messages_total"
        case messagesLastHour = "messages_last_hour"
        case activeWorkers    = "active_workers"
        case sseSubscribers   = "sse_subscribers"
        case uptimeSecs       = "uptime_secs"
    }

    var uptimeFormatted: String {
        guard let s = uptimeSecs else { return "—" }
        let h = s / 3600
        let m = (s % 3600) / 60
        if h > 0 { return "\(h)h \(m)m" }
        return "\(m)m"
    }
}
