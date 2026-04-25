import Foundation
import Combine

// MARK: - MemoryStore
// Lightweight JSON-backed store using the App Group shared container.
// Both the main app and Share Extension read/write this same file.

@MainActor
final class MemoryStore: ObservableObject {
    static let shared = MemoryStore()

    @Published private(set) var memories: [Memory] = []

    private let fileURL: URL

    private init() {
        let container = FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: AppConstants.appGroupID)
            ?? FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        fileURL = container.appendingPathComponent("memories.json")
        load()
    }

    // MARK: - CRUD

    func add(_ memory: Memory) {
        memories.insert(memory, at: 0)
        persist()
    }

    func update(_ memory: Memory) {
        guard let idx = memories.firstIndex(where: { $0.id == memory.id }) else { return }
        memories[idx] = memory
        persist()
    }

    func delete(_ memory: Memory) {
        memories.removeAll { $0.id == memory.id }
        persist()
    }

    func markSynced(_ id: UUID) {
        guard let idx = memories.firstIndex(where: { $0.id == id }) else { return }
        memories[idx].synced = true
        persist()
    }

    var unsynced: [Memory] {
        memories.filter { !$0.synced }
    }

    // MARK: - Search

    func search(query: String) -> [Memory] {
        guard !query.trimmingCharacters(in: .whitespaces).isEmpty else { return memories }
        let q = query.lowercased()
        return memories.filter {
            $0.title.lowercased().contains(q)
            || ($0.content?.lowercased().contains(q) ?? false)
            || ($0.note?.lowercased().contains(q) ?? false)
            || ($0.source?.lowercased().contains(q) ?? false)
        }
    }

    // MARK: - Persistence

    func reload() { load() }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL) else { return }
        let decoded = try? JSONDecoder().decode([Memory].self, from: data)
        memories = decoded ?? []
    }

    private func persist() {
        guard let data = try? JSONEncoder().encode(memories) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }
}
