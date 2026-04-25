import Foundation
import Combine

@MainActor
final class SettingsViewModel: ObservableObject {
    @Published var autoTransferEnabled: Bool {
        didSet { UserDefaults(suiteName: AppConstants.appGroupID)?.set(autoTransferEnabled, forKey: AppConstants.autoTransferKey) }
    }
    @Published var isSyncing: Bool = false
    @Published var lastSyncMessage: String = ""

    let pairing = PairingService.shared
    private let store = MemoryStore.shared

    init() {
        let defaults = UserDefaults(suiteName: AppConstants.appGroupID)
        autoTransferEnabled = defaults?.bool(forKey: AppConstants.autoTransferKey) ?? false
    }

    func sendAllPending() {
        guard !isSyncing else { return }
        isSyncing = true
        lastSyncMessage = ""
        Task {
            await SyncService.shared.flushPending()
            isSyncing = false
            let pending = store.unsynced.count
            lastSyncMessage = pending == 0 ? "All memories sent." : "\(pending) memories failed — will retry."
        }
    }

    func unpair() {
        pairing.unpair()
    }
}
