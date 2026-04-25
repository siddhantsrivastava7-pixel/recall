import Foundation
import UniformTypeIdentifiers
import SwiftUI

@MainActor
final class ShareViewModel: ObservableObject {
    @Published var title: String = ""
    @Published var urlString: String = ""
    @Published var previewText: String = ""
    @Published var note: String = ""
    @Published var isSaving: Bool = false
    @Published var didSave: Bool = false
    @Published var source: String = ""

    private let store = MemoryStore.shared
    private let autoTransfer: Bool

    init() {
        autoTransfer = UserDefaults(suiteName: AppConstants.appGroupID)?
            .bool(forKey: AppConstants.autoTransferKey) ?? false
    }

    // MARK: - Load shared content from NSExtensionContext

    func loadItems(from context: NSExtensionContext?) async {
        guard let items = context?.inputItems as? [NSExtensionItem] else { return }

        for item in items {
            for provider in (item.attachments ?? []) {
                if provider.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
                    await loadURL(from: provider)
                } else if provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
                    await loadText(from: provider)
                }
            }
        }

        if title.isEmpty, !urlString.isEmpty {
            title = urlTitle(from: urlString)
        }
        if source.isEmpty, let host = URL(string: urlString)?.host {
            source = host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
        }
    }

    private func loadURL(from provider: NSItemProvider) async {
        await withCheckedContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.url.identifier) { item, _ in
                if let url = item as? URL {
                    Task { @MainActor in
                        self.urlString = url.absoluteString
                        if self.title.isEmpty {
                            self.title = self.urlTitle(from: url.absoluteString)
                        }
                    }
                }
                continuation.resume()
            }
        }
    }

    private func loadText(from provider: NSItemProvider) async {
        await withCheckedContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.plainText.identifier) { item, _ in
                if let text = item as? String {
                    Task { @MainActor in
                        if self.title.isEmpty {
                            self.title = String(text.prefix(120))
                        } else {
                            self.previewText = String(text.prefix(240))
                        }
                    }
                }
                continuation.resume()
            }
        }
    }

    private func urlTitle(from urlString: String) -> String {
        guard let url = URL(string: urlString) else { return urlString }
        let path = url.lastPathComponent
            .replacingOccurrences(of: "-", with: " ")
            .replacingOccurrences(of: "_", with: " ")
        return path.isEmpty ? (url.host ?? urlString) : path
    }

    // MARK: - Save

    func save() {
        guard !isSaving else { return }
        isSaving = true

        let memory = Memory(
            title: title.trimmingCharacters(in: .whitespacesAndNewlines),
            content: previewText.isEmpty ? nil : previewText,
            url: urlString.isEmpty ? nil : urlString,
            source: source,
            note: note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : note
        )

        store.add(memory)

        if autoTransfer, let config = PairingService.shared.config {
            Task {
                let ok = await SyncService.shared.push(memory, config: config)
                if ok { store.markSynced(memory.id) }
                await MainActor.run {
                    isSaving = false
                    didSave = true
                }
            }
        } else {
            isSaving = false
            didSave = true
        }
    }
}
