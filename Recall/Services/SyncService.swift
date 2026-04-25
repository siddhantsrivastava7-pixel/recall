import Foundation

// MARK: - SyncService
// One-way phone → desktop transfer via HTTPS relay.
// Failures are silent; memories stay marked unsynced for retry.

actor SyncService {
    static let shared = SyncService()

    private let session: URLSession = {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 15
        return URLSession(configuration: config)
    }()

    private init() {}

    // Send a single memory. Returns true if successful.
    func push(_ memory: Memory, config: PairingConfig) async -> Bool {
        guard let url = URL(string: config.endpoint + "/api/push-memory") else { return false }

        let payload = MemorySyncPayload(
            device_id: config.device_id,
            pairing_secret: config.pairing_secret,
            payload: memory.toPayload()
        )

        guard let body = try? JSONEncoder().encode(payload) else { return false }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = body

        do {
            let (_, response) = try await session.data(for: request)
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            return (200..<300).contains(status)
        } catch {
            return false
        }
    }

    // Flush all unsynced memories. Updates store on success.
    func flushPending() async {
        let (config, pending): (PairingConfig?, [Memory]) = await MainActor.run {
            (PairingService.shared.config, MemoryStore.shared.unsynced)
        }
        guard let config else { return }

        await withTaskGroup(of: (UUID, Bool).self) { group in
            for memory in pending {
                group.addTask {
                    let ok = await self.push(memory, config: config)
                    return (memory.id, ok)
                }
            }
            for await (id, ok) in group where ok {
                await MainActor.run { MemoryStore.shared.markSynced(id) }
            }
        }
    }
}
