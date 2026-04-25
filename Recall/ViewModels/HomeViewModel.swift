import Foundation
import Combine

@MainActor
final class HomeViewModel: ObservableObject {
    @Published var searchQuery: String = ""
    @Published var isSearchFocused: Bool = false
    @Published private(set) var displayedMemories: [Memory] = []

    private let store = MemoryStore.shared
    private var cancellables = Set<AnyCancellable>()

    init() {
        store.$memories
            .combineLatest($searchQuery)
            .map { memories, query in
                query.trimmingCharacters(in: .whitespaces).isEmpty
                    ? memories
                    : memories.filter {
                        $0.title.localizedCaseInsensitiveContains(query)
                        || ($0.content?.localizedCaseInsensitiveContains(query) ?? false)
                        || ($0.note?.localizedCaseInsensitiveContains(query) ?? false)
                        || ($0.source?.localizedCaseInsensitiveContains(query) ?? false)
                      }
            }
            .receive(on: RunLoop.main)
            .assign(to: &$displayedMemories)
    }

    func reloadFromDisk() {
        store.reload()
    }

    func delete(_ memory: Memory) {
        store.delete(memory)
    }
}
