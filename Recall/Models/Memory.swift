import Foundation

// MARK: - Memory Model

struct Memory: Identifiable, Codable, Equatable, Hashable {
    var id: UUID
    var title: String
    var content: String?
    var url: String?
    var source: String?
    var createdAt: Date
    var note: String?
    var synced: Bool

    init(
        id: UUID = UUID(),
        title: String,
        content: String? = nil,
        url: String? = nil,
        source: String? = nil,
        createdAt: Date = Date(),
        note: String? = nil,
        synced: Bool = false
    ) {
        self.id = id
        self.title = title
        self.content = content
        self.url = url
        self.source = source
        self.createdAt = createdAt
        self.note = note
        self.synced = synced
    }

    // MARK: - Derived helpers

    var displaySource: String {
        if let source = source, !source.isEmpty { return source }
        if let urlString = url, let host = URL(string: urlString)?.host {
            return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
        }
        return "Unknown"
    }

    var sourceIcon: String {
        guard let urlString = url, let host = URL(string: urlString)?.host?.lowercased() else {
            return "note.text"
        }
        if host.contains("youtube") || host.contains("youtu.be") { return "play.rectangle" }
        if host.contains("twitter") || host.contains("x.com")    { return "bird" }
        if host.contains("github")                                 { return "chevron.left.forwardslash.chevron.right" }
        if host.contains("medium")                                 { return "m.circle" }
        if host.contains("reddit")                                 { return "bubble.left.and.bubble.right" }
        return "link"
    }

    var relativeDate: String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        return formatter.localizedString(for: createdAt, relativeTo: Date())
    }
}

// MARK: - Sync Payload

struct MemorySyncPayload: Codable {
    let device_id: String
    let pairing_secret: String
    let payload: MemoryPayload
}

struct MemoryPayload: Codable {
    let id: String
    let title: String
    let content: String?
    let url: String?
    let source: String?
    let createdAt: String
    let note: String?
}

extension Memory {
    func toPayload() -> MemoryPayload {
        let iso = ISO8601DateFormatter()
        return MemoryPayload(
            id: id.uuidString,
            title: title,
            content: content,
            url: url,
            source: source,
            createdAt: iso.string(from: createdAt),
            note: note
        )
    }
}
