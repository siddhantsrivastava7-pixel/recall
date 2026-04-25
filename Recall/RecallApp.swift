import SwiftUI

@main
struct RecallApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .onReceive(
                    NotificationCenter.default.publisher(
                        for: UIApplication.willEnterForegroundNotification
                    )
                ) { _ in
                    MemoryStore.shared.reload()
                    if UserDefaults(suiteName: AppConstants.appGroupID)?
                        .bool(forKey: AppConstants.autoTransferKey) == true
                    {
                        Task { await SyncService.shared.flushPending() }
                    }
                }
        }
    }
}
